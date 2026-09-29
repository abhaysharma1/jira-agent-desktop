use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{Repository, Workspace, WorkspaceStatus};
use crate::git;

#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    pub root: PathBuf,
}

impl WorkspaceManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn ensure_root(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    pub fn workspace_path(&self, task_id: &str) -> PathBuf {
        self.root.join(sanitize(task_id))
    }

    pub fn create(
        &self,
        repository: &Repository,
        task_id: &str,
        slug: Option<&str>,
    ) -> Result<Workspace, String> {
        self.ensure_root()
            .map_err(|error| format!("failed to create workspace root: {error}"))?;

        let repo_path = Path::new(&repository.local_path);
        if !git::is_git_repository(repo_path) {
            return Err(format!(
                "{} is not a git repository",
                repository.local_path
            ));
        }

        let path = self.workspace_path(task_id);
        if path.exists() {
            return Err(format!("workspace path already exists: {}", path.display()));
        }

        let branch = branch_name(task_id, slug);
        if git::branch_exists(repo_path, &branch) {
            return Err(format!("branch already exists: {branch}"));
        }

        let base = self.base_ref(repo_path, &repository.default_branch)?;
        git::worktree_add(repo_path, &path, &branch, &base)?;

        Ok(Workspace {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            repository_id: repository.id.clone(),
            path: path.to_string_lossy().replace('\\', "/"),
            branch_name: branch,
            created_at: Utc::now().to_rfc3339(),
        })
    }

    pub fn remove(&self, repository: &Repository, workspace: &Workspace) -> Result<(), String> {
        let repo_path = Path::new(&repository.local_path);
        let target = Path::new(&workspace.path);

        if git::is_git_repository(repo_path) {
            let _ = git::worktree_remove(repo_path, target);
            git::worktree_prune(repo_path);
            git::branch_delete(repo_path, &workspace.branch_name);
        }

        if target.exists() {
            std::fs::remove_dir_all(target)
                .map_err(|error| format!("failed to remove {}: {error}", target.display()))?;
        }

        Ok(())
    }

    pub fn status(&self, workspace: &Workspace) -> WorkspaceStatus {
        let target = Path::new(&workspace.path);
        let exists = target.exists();
        let changed_files = if exists {
            git::changed_files(target).unwrap_or_default()
        } else {
            Vec::new()
        };

        WorkspaceStatus {
            task_id: workspace.task_id.clone(),
            path: workspace.path.clone(),
            branch_name: workspace.branch_name.clone(),
            exists,
            is_clean: exists && changed_files.is_empty(),
            changed_files: changed_files.len() as u32,
            current_branch: if exists {
                git::current_branch(target)
            } else {
                None
            },
        }
    }

    fn base_ref(&self, repo_path: &Path, default_branch: &str) -> Result<String, String> {
        if !git::rev_parse_verify(repo_path, "HEAD") {
            return Err(
                "repository has no commits yet; make an initial commit before creating a workspace"
                    .to_string(),
            );
        }
        let remote = format!("refs/remotes/origin/{default_branch}");
        if git::rev_parse_verify(repo_path, &remote) {
            return Ok(format!("origin/{default_branch}"));
        }
        let local = format!("refs/heads/{default_branch}");
        if git::rev_parse_verify(repo_path, &local) {
            return Ok(default_branch.to_string());
        }
        Ok("HEAD".to_string())
    }
}

fn sanitize(value: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            pending_dash = false;
        } else if !out.is_empty() && !pending_dash {
            out.push('-');
            pending_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn branch_name(task_id: &str, slug: Option<&str>) -> String {
    let base = format!("agent/{}", sanitize(task_id));
    match slug.map(|value| sanitize(value).to_lowercase()) {
        Some(slug) if !slug.is_empty() => format!("{base}-{slug}"),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository;
    use std::fs;
    use std::path::Path as StdPath;
    use std::process::Command;

    fn git(dir: &StdPath, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(dir: &StdPath) {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("README.md"), "hello").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "init"]);
    }

    #[test]
    fn creates_and_removes_isolated_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        fs::create_dir(&repo_dir).unwrap();
        init_repo(&repo_dir);

        let repository = repository::detect(&repo_dir).unwrap();
        let manager = WorkspaceManager::new(temp.path().join("workspaces"));

        let workspace = manager
            .create(&repository, "CC-142", Some("Add pagination to Problems API"))
            .unwrap();

        assert_eq!(workspace.branch_name, "agent/CC-142-add-pagination-to-problems-api");
        assert!(Path::new(&workspace.path).exists());
        assert!(git::branch_exists(&repo_dir, &workspace.branch_name));

        fs::write(Path::new(&workspace.path).join("feature.txt"), "x").unwrap();
        assert!(!git::changed_files(Path::new(&workspace.path)).unwrap().is_empty());
        assert!(git::changed_files(&repo_dir).unwrap().is_empty());

        manager.remove(&repository, &workspace).unwrap();
        assert!(!Path::new(&workspace.path).exists());
        assert!(!git::branch_exists(&repo_dir, &workspace.branch_name));
    }

    #[test]
    fn rejects_unborn_repository() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("empty");
        fs::create_dir(&repo_dir).unwrap();
        git(&repo_dir, &["init", "-q", "-b", "main"]);

        let repository = repository::detect(&repo_dir).unwrap();
        let manager = WorkspaceManager::new(temp.path().join("workspaces"));
        assert!(manager.create(&repository, "CC-1", None).is_err());
    }
}
