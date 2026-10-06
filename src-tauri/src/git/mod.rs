use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::{DiffStats, FileDiff, FileStat};

fn output(dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))
}

pub fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = output(dir, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("git {} failed: {message}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_git_allow_fail(dir: &Path, args: &[&str]) -> Option<String> {
    let output = output(dir, args).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn is_git_repository(path: &Path) -> bool {
    run_git_allow_fail(path, &["rev-parse", "--is-inside-work-tree"])
        .map(|value| value == "true")
        .unwrap_or(false)
}

pub fn toplevel(path: &Path) -> Result<PathBuf, String> {
    let value = run_git(path, &["rev-parse", "--show-toplevel"])
        .map_err(|_| format!("{} is not a git repository", path.display()))?;
    Ok(PathBuf::from(value))
}

pub fn remote_url(path: &Path) -> Option<String> {
    let value = run_git_allow_fail(path, &["remote", "get-url", "origin"])?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn current_branch(path: &Path) -> Option<String> {
    let value = run_git_allow_fail(path, &["branch", "--show-current"])?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn rev_parse_verify(path: &Path, rev: &str) -> bool {
    run_git_allow_fail(path, &["rev-parse", "--verify", "--quiet", rev])
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

pub fn default_branch(path: &Path) -> Option<String> {
    if let Some(sym) = run_git_allow_fail(
        path,
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
    ) {
        if let Some(branch) = sym.strip_prefix("origin/") {
            return Some(branch.to_string());
        }
    }
    for candidate in ["main", "master"] {
        if rev_parse_verify(path, &format!("refs/heads/{candidate}")) {
            return Some(candidate.to_string());
        }
    }
    current_branch(path)
}

pub fn status_porcelain(path: &Path) -> Result<String, String> {
    run_git(path, &["status", "--porcelain"])
}

#[allow(dead_code)]
pub fn diff_check(path: &Path) -> bool {
    run_git_allow_fail(path, &["diff", "--check"]).is_some()
}

pub fn changed_files(path: &Path) -> Result<Vec<String>, String> {
    let raw = status_porcelain(path)?;
    Ok(raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect())
}

pub fn worktree_add(
    repo: &Path,
    target: &Path,
    branch: &str,
    base: &str,
) -> Result<(), String> {
    let target = target.to_string_lossy().to_string();
    run_git(repo, &["worktree", "add", "-b", branch, &target, base]).map(|_| ())
}

pub fn worktree_remove(repo: &Path, target: &Path) -> Result<(), String> {
    let target = target.to_string_lossy().to_string();
    run_git(repo, &["worktree", "remove", "--force", &target]).map(|_| ())
}

pub fn worktree_prune(repo: &Path) {
    let _ = run_git_allow_fail(repo, &["worktree", "prune"]);
}

pub fn branch_delete(repo: &Path, branch: &str) {
    let _ = run_git_allow_fail(repo, &["branch", "-D", branch]);
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    rev_parse_verify(repo, &format!("refs/heads/{branch}"))
}

pub fn push_branch(repo: &Path, branch: &str) -> Result<(), String> {
    run_git(repo, &["push", "-u", "origin", branch]).map(|_| ())
}

pub fn commit_all(path: &Path, message: &str) -> Result<String, String> {
    run_git(path, &["add", "-A"])?;

    if let Err(error) = run_git(path, &["commit", "-m", message]) {
        let identity_missing = error.contains("Please tell me who you are")
            || error.contains("unable to auto-detect email")
            || error.contains("empty ident name")
            || error.contains("Author identity unknown");
        if identity_missing {
            run_git(
                path,
                &[
                    "-c",
                    "user.name=JIRA Agent",
                    "-c",
                    "user.email=jira-agent@localhost",
                    "commit",
                    "-m",
                    message,
                ],
            )?;
        } else {
            return Err(error);
        }
    }

    run_git(path, &["rev-parse", "--short", "HEAD"])
}

pub fn worktree_stats(path: &Path) -> Result<DiffStats, String> {
    let mut files: Vec<FileStat> = Vec::new();

    if let Some(raw) = run_git_allow_fail(path, &["diff", "--numstat"]) {
        for line in raw.lines() {
            let mut parts = line.split('\t');
            if let (Some(additions), Some(deletions), Some(file)) =
                (parts.next(), parts.next(), parts.next())
            {
                files.push(FileStat {
                    path: file.to_string(),
                    additions: additions.parse::<u32>().unwrap_or(0),
                    deletions: deletions.parse::<u32>().unwrap_or(0),
                    status: "modified".to_string(),
                });
            }
        }
    }

    if let Ok(raw) = status_porcelain(path) {
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("?? ") {
                let file = rest.trim();
                let additions = count_lines(&path.join(file)).unwrap_or(0) as u32;
                files.push(FileStat {
                    path: file.to_string(),
                    additions,
                    deletions: 0,
                    status: "added".to_string(),
                });
            }
        }
    }

    let files_changed = files.len() as u32;
    let additions = files.iter().map(|file| file.additions).sum();
    let deletions = files.iter().map(|file| file.deletions).sum();

    Ok(DiffStats {
        files_changed,
        additions,
        deletions,
        files,
    })
}

fn count_lines(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(content.lines().count() as u64)
}

const MAX_DIFF_BYTES: usize = 200 * 1024;

pub fn file_diff(path: &Path, file: &str) -> FileDiff {
    let status = file_status(path, file);
    let (additions, deletions) = file_numstat(path, file);

    let original_raw = if status == "added" {
        String::new()
    } else {
        run_git_allow_fail(path, &["show", &format!("HEAD:{file}")]).unwrap_or_default()
    };
    let modified_bytes = if status == "deleted" {
        Vec::new()
    } else {
        std::fs::read(path.join(file)).unwrap_or_default()
    };

    let binary = original_raw.as_bytes().contains(&0) || modified_bytes.contains(&0);
    let truncated = original_raw.len() > MAX_DIFF_BYTES || modified_bytes.len() > MAX_DIFF_BYTES;

    let original = if binary {
        String::new()
    } else {
        cap_chars(original_raw)
    };
    let modified = if binary {
        String::new()
    } else {
        cap_chars(String::from_utf8_lossy(&modified_bytes).to_string())
    };

    FileDiff {
        path: file.to_string(),
        status,
        additions,
        deletions,
        original,
        modified,
        binary,
        truncated,
    }
}

fn cap_chars(value: String) -> String {
    if value.chars().count() <= MAX_DIFF_BYTES {
        value
    } else {
        value.chars().take(MAX_DIFF_BYTES).collect()
    }
}

fn file_status(path: &Path, file: &str) -> String {
    let exists = path.join(file).exists();
    let in_head = rev_parse_verify(path, &format!("HEAD:{file}"));
    match (exists, in_head) {
        (false, _) => "deleted".to_string(),
        (true, false) => "added".to_string(),
        (true, true) => "modified".to_string(),
    }
}

fn file_numstat(path: &Path, file: &str) -> (u32, u32) {
    if let Some(raw) = run_git_allow_fail(path, &["diff", "HEAD", "--numstat", "--", file]) {
        for line in raw.lines() {
            let mut parts = line.split('\t');
            if let (Some(additions), Some(deletions), Some(name)) =
                (parts.next(), parts.next(), parts.next())
            {
                if name == file {
                    return (
                        additions.parse().unwrap_or(0),
                        deletions.parse().unwrap_or(0),
                    );
                }
            }
        }
    }
    let additions = count_lines(&path.join(file)).unwrap_or(0) as u32;
    (additions, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
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

    #[test]
    fn reports_worktree_stats() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        fs::create_dir(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("tracked.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("tracked.txt"), "one\ntwo\n").unwrap();
        fs::write(dir.join("new.txt"), "a\nb\nc\n").unwrap();

        let stats = worktree_stats(&dir).unwrap();
        assert_eq!(stats.files_changed, 2);
        assert!(stats.files.iter().any(|f| f.path == "tracked.txt" && f.additions == 1));
        assert!(stats.files.iter().any(|f| f.path == "new.txt" && f.status == "added"));
        assert!(stats.additions >= 4);
    }

    #[test]
    fn detects_whitespace_errors() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        fs::create_dir(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("file.txt"), "clean\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        assert!(diff_check(&dir));

        fs::write(dir.join("file.txt"), "clean\ntrailing   \n").unwrap();
        assert!(!diff_check(&dir));
    }

    #[test]
    fn builds_file_diffs() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        fs::create_dir(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("keep.txt"), "one\n").unwrap();
        fs::write(dir.join("gone.txt"), "bye\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("keep.txt"), "one\ntwo\n").unwrap();
        fs::write(dir.join("new.txt"), "a\nb\n").unwrap();
        fs::remove_file(dir.join("gone.txt")).unwrap();

        let modified = file_diff(&dir, "keep.txt");
        assert_eq!(modified.status, "modified");
        assert!(modified.original.contains("one"));
        assert!(modified.modified.contains("two"));

        let added = file_diff(&dir, "new.txt");
        assert_eq!(added.status, "added");
        assert!(added.original.is_empty());
        assert!(added.modified.contains('a'));

        let deleted = file_diff(&dir, "gone.txt");
        assert_eq!(deleted.status, "deleted");
        assert!(deleted.modified.is_empty());
        assert!(deleted.original.contains("bye"));
    }

    #[test]
    fn commits_all_changes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        fs::create_dir(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        let hash = commit_all(&dir, "CC-1: do the thing").unwrap();
        assert!(!hash.is_empty());
        let subject = run_git(&dir, &["log", "-1", "--format=%s"]).unwrap();
        assert_eq!(subject, "CC-1: do the thing");
        assert!(changed_files(&dir).unwrap().is_empty());

        assert!(commit_all(&dir, "nothing").is_err());
    }
}

