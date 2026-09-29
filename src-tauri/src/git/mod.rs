#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Default)]
pub struct GitService {
    pub repo_path: Option<PathBuf>,
}

impl GitService {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: Some(repo_path.into()),
        }
    }

    pub fn run(&self, args: &[&str]) -> Result<String, String> {
        let repo_path = self
            .repo_path
            .as_ref()
            .ok_or_else(|| "repository path is not set".to_string())?;
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .map_err(|error| format!("failed to run git: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

pub fn is_git_repository(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
