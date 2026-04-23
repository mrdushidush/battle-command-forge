//! Git workspace management for isolated mission execution.
//!
//! Each mission gets an isolated git repo in `.battlecommand/workspaces/<id>/`.
//! Per-subtask commits provide rollback safety and audit trail.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKSPACES_DIR: &str = ".battlecommand/workspaces";

/// A managed workspace with its own git repo.
pub struct Workspace {
    pub path: PathBuf,
    pub mission_id: String,
}

impl Workspace {
    /// Create a new isolated workspace for a mission.
    pub fn create(mission_id: &str) -> Result<Self> {
        let path = PathBuf::from(WORKSPACES_DIR).join(mission_id);
        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create workspace at {}", path.display()))?;

        // Initialize git repo
        let output = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&path)
            .output();

        match output {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("   git init warning: {}", stderr.trim());
            }
            Err(e) => {
                eprintln!("   git not available: {}", e);
                // Continue without git — workspace still works for file storage
            }
        }

        Ok(Self {
            path,
            mission_id: mission_id.to_string(),
        })
    }

    /// Open an existing workspace.
    pub fn open(mission_id: &str) -> Result<Self> {
        let path = PathBuf::from(WORKSPACES_DIR).join(mission_id);
        if !path.exists() {
            anyhow::bail!("Workspace {} does not exist", mission_id);
        }
        Ok(Self {
            path,
            mission_id: mission_id.to_string(),
        })
    }

    /// Commit all current changes with a message.
    pub fn commit(&self, message: &str) -> Result<String> {
        // Stage all files
        run_git(&self.path, &["add", "-A"])?;

        // Check if there are changes to commit
        let status = run_git(&self.path, &["status", "--porcelain"])?;
        if status.trim().is_empty() {
            return Ok("no changes".to_string());
        }

        // Commit
        run_git(&self.path, &["commit", "-m", message, "--allow-empty"])?;

        // Get commit hash
        let hash = run_git(&self.path, &["rev-parse", "--short", "HEAD"])?;
        Ok(hash.trim().to_string())
    }

    /// Rollback to a specific commit.
    pub fn rollback(&self, commit: &str) -> Result<()> {
        run_git(&self.path, &["checkout", commit, "--", "."])?;
        Ok(())
    }

    /// Get the git log (last N entries).
    pub fn log(&self, count: usize) -> Result<String> {
        let n = format!("-{}", count);
        run_git(&self.path, &["log", "--oneline", &n])
    }

    /// Copy all files from the workspace to an output directory.
    pub fn export_to(&self, output_dir: &Path) -> Result<()> {
        fs::create_dir_all(output_dir)?;
        copy_dir_contents(&self.path, output_dir)?;
        Ok(())
    }
}

/// Generate a mission ID from a prompt.
pub fn mission_id_from_prompt(prompt: &str) -> String {
    let slug: String = prompt
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .chars()
        .take(30)
        .collect();

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    format!("{}_{}", slug.trim_matches('_'), timestamp)
}

/// List all workspaces.
pub fn list_workspaces() -> Result<Vec<String>> {
    let dir = Path::new(WORKSPACES_DIR);
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut workspaces = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            workspaces.push(entry.file_name().to_string_lossy().to_string());
        }
    }

    workspaces.sort();
    Ok(workspaces)
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git {:?}", args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed: {}", args, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Clone a git repository (shallow) to a target directory.
pub fn clone_repo(url: &str, target: &Path) -> Result<()> {
    println!("[REPO] Cloning {} ...", url);
    let output = Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(target)
        .output()
        .context("Failed to run git clone")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }
    println!("[REPO] Cloned to {}", target.display());
    Ok(())
}

pub(crate) fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Skip .git directory
        if src_path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mission_id_from_prompt() {
        let id = mission_id_from_prompt("Build a FastAPI auth endpoint");
        assert!(id.starts_with("build_a_fastapi_auth_endpoint_"));
        assert!(id.len() > 30); // has timestamp
    }

    #[test]
    fn test_list_workspaces_empty() {
        // Should not panic if directory doesn't exist
        let result = list_workspaces();
        assert!(result.is_ok());
    }
}
