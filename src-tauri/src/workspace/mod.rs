#![allow(dead_code)]

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    pub root: PathBuf,
}

impl WorkspaceManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_root(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    pub fn workspace_path(&self, task_ref: &str) -> PathBuf {
        self.root.join(task_ref)
    }
}
