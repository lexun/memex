//! Worktree management via vibetree
//!
//! Manages git worktrees for isolated worker environments.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::{CortexError, Result};

/// Information about a worktree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub path: PathBuf,
    pub is_main: bool,
}

/// Manages worktrees via vibetree CLI
pub struct WorktreeManager {
    /// Root repository path
    repo_root: PathBuf,
}

impl WorktreeManager {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Create a new worktree for a branch
    pub fn create(&self, branch: &str, base: Option<&str>) -> Result<WorktreeInfo> {
        info!("Creating worktree for branch: {}", branch);

        let mut cmd = Command::new("vibetree");
        cmd.arg("add").arg(branch);
        cmd.current_dir(&self.repo_root);

        if let Some(base_branch) = base {
            cmd.arg("--base").arg(base_branch);
        }

        let output = cmd.output().map_err(|e| {
            CortexError::VibetreeFailed(format!("Failed to run vibetree: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CortexError::VibetreeFailed(format!(
                "vibetree add failed: {}",
                stderr
            )));
        }

        // Parse the output to get the worktree path
        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!("vibetree output: {}", stdout);

        // Get worktree info
        let worktrees = self.list()?;
        worktrees
            .into_iter()
            .find(|w| w.branch == branch)
            .ok_or_else(|| {
                CortexError::WorktreeError(format!(
                    "Worktree created but not found in list: {}",
                    branch
                ))
            })
    }

    /// List all worktrees
    pub fn list(&self) -> Result<Vec<WorktreeInfo>> {
        let mut cmd = Command::new("vibetree");
        cmd.arg("list").arg("--json");
        cmd.current_dir(&self.repo_root);

        let output = cmd.output().map_err(|e| {
            CortexError::VibetreeFailed(format!("Failed to run vibetree: {}", e))
        })?;

        if !output.status.success() {
            // Vibetree might not support --json yet, fall back to git
            return self.list_via_git();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Try to parse JSON output
        serde_json::from_str(&stdout).map_err(|e| {
            CortexError::VibetreeFailed(format!("Failed to parse vibetree output: {}", e))
        })
    }

    /// Fallback: list worktrees via git directly
    fn list_via_git(&self) -> Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .arg("worktree")
            .arg("list")
            .arg("--porcelain")
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| CortexError::WorktreeError(format!("git worktree failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;

        for line in stdout.lines() {
            if line.starts_with("worktree ") {
                // Save previous worktree if complete
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    let is_main = path == self.repo_root;
                    worktrees.push(WorktreeInfo {
                        branch,
                        path,
                        is_main,
                    });
                }
                current_path = Some(PathBuf::from(line.strip_prefix("worktree ").unwrap()));
            } else if line.starts_with("branch refs/heads/") {
                current_branch =
                    Some(line.strip_prefix("branch refs/heads/").unwrap().to_string());
            } else if line.starts_with("HEAD ") {
                // Detached HEAD - use commit hash as "branch"
                if current_branch.is_none() {
                    current_branch = Some(line.strip_prefix("HEAD ").unwrap()[..8].to_string());
                }
            }
        }

        // Don't forget the last one
        if let (Some(path), Some(branch)) = (current_path, current_branch) {
            let is_main = path == self.repo_root;
            worktrees.push(WorktreeInfo {
                branch,
                path,
                is_main,
            });
        }

        Ok(worktrees)
    }

    /// Delete a worktree
    pub fn delete(&self, branch: &str) -> Result<()> {
        info!("Deleting worktree for branch: {}", branch);

        let mut cmd = Command::new("vibetree");
        cmd.arg("remove").arg(branch);
        cmd.current_dir(&self.repo_root);

        let output = cmd.output().map_err(|e| {
            CortexError::VibetreeFailed(format!("Failed to run vibetree: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CortexError::VibetreeFailed(format!(
                "vibetree remove failed: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Get path to a specific worktree
    pub fn get_path(&self, branch: &str) -> Result<PathBuf> {
        let worktrees = self.list()?;
        worktrees
            .into_iter()
            .find(|w| w.branch == branch)
            .map(|w| w.path)
            .ok_or_else(|| CortexError::WorktreeError(format!("Worktree not found: {}", branch)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worktree_manager_creation() {
        let manager = WorktreeManager::new("/tmp/test-repo");
        assert_eq!(manager.repo_root, PathBuf::from("/tmp/test-repo"));
    }
}
