use std::path::Path;

use chrono::Utc;
use uuid::Uuid;

use crate::domain::Repository;
use crate::git;

pub fn detect(local_path: &Path) -> Result<Repository, String> {
    if !local_path.exists() {
        return Err(format!("path does not exist: {}", local_path.display()));
    }
    let canonical = local_path
        .canonicalize()
        .map_err(|error| format!("invalid path: {error}"))?;
    if !canonical.is_dir() {
        return Err(format!("path is not a directory: {}", canonical.display()));
    }
    if !git::is_git_repository(&canonical) {
        return Err(format!(
            "{} is not a git repository",
            canonical.display()
        ));
    }
    let root = git::toplevel(&canonical)?;
    let local_path = root.to_string_lossy().replace('\\', "/");
    let name = root
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "repository".to_string());
    let remote_url = git::remote_url(&root).unwrap_or_default();
    let default_branch = git::default_branch(&root).unwrap_or_else(|| "main".to_string());
    let now = Utc::now().to_rfc3339();

    Ok(Repository {
        id: Uuid::new_v4().to_string(),
        name,
        local_path,
        remote_url,
        default_branch,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn detects_repository_from_subdirectory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("myrepo");
        fs::create_dir(&root).unwrap();
        init_repo(&root);
        let nested = root.join("src");
        fs::create_dir(&nested).unwrap();

        let repo = detect(&nested).unwrap();
        assert_eq!(repo.name, "myrepo");
        assert_eq!(repo.default_branch, "main");
        assert!(repo.remote_url.is_empty());
        assert!(repo.local_path.ends_with("myrepo"));
    }

    #[test]
    fn reads_remote_and_default_branch() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("remote-repo");
        fs::create_dir(&root).unwrap();
        init_repo(&root);
        git(&root, &["remote", "add", "origin", "https://example.com/foo.git"]);
        git(
            &root,
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        git(
            &root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );

        let repo = detect(&root).unwrap();
        assert_eq!(repo.remote_url, "https://example.com/foo.git");
        assert_eq!(repo.default_branch, "main");
    }

    #[test]
    fn rejects_non_git_directory() {
        let temp = tempfile::tempdir().unwrap();
        let plain = temp.path().join("plain");
        fs::create_dir(&plain).unwrap();
        assert!(detect(&plain).is_err());
    }

    #[test]
    fn reports_dirty_then_clean() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("dirty");
        fs::create_dir(&root).unwrap();
        init_repo(&root);

        assert!(git::changed_files(&root).unwrap().is_empty());
        fs::write(root.join("new.txt"), "content").unwrap();
        assert_eq!(git::changed_files(&root).unwrap().len(), 1);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "second"]);
        assert!(git::changed_files(&root).unwrap().is_empty());
    }
}
