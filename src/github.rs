//! GitHub integration via `gh` CLI.
//! Ported from battleclaw-v2.

use anyhow::{bail, Result};
use std::process::Command;

pub struct GitHubOps {
    workspace: String,
}

#[derive(Debug)]
pub struct PrResult {
    pub url: String,
    pub number: u32,
}

impl GitHubOps {
    pub fn new(workspace: &str) -> Self {
        Self {
            workspace: workspace.to_string(),
        }
    }

    pub fn is_available() -> bool {
        Command::new("gh")
            .args(["auth", "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn create_repo(&self, name: &str, private: bool) -> Result<String> {
        let vis = if private { "--private" } else { "--public" };
        let output = Command::new("gh")
            .args([
                "repo",
                "create",
                name,
                vis,
                "--source",
                &self.workspace,
                "--push",
            ])
            .output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            bail!(
                "Failed to create repo: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }
    }

    pub fn add_remote(&self, repo_url: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["remote", "add", "origin", repo_url])
            .current_dir(&self.workspace)
            .output()?;
        if !output.status.success() {
            Command::new("git")
                .args(["remote", "set-url", "origin", repo_url])
                .current_dir(&self.workspace)
                .output()?;
        }
        Ok(())
    }

    pub fn push(&self, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["push", "-u", "origin", branch])
            .current_dir(&self.workspace)
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            bail!("Push failed: {}", String::from_utf8_lossy(&output.stderr))
        }
    }

    pub fn create_pr(&self, title: &str, body: &str, base: &str) -> Result<PrResult> {
        let output = Command::new("gh")
            .args([
                "pr", "create", "--title", title, "--body", body, "--base", base,
            ])
            .current_dir(&self.workspace)
            .output()?;
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let number = url
                .rsplit('/')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            Ok(PrResult { url, number })
        } else {
            bail!(
                "PR creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }
    }

    pub fn status(&self) -> Result<String> {
        let branch = run_git(&self.workspace, &["branch", "--show-current"])?;
        let remote = run_git(&self.workspace, &["remote", "-v"]).unwrap_or_default();
        Ok(format!(
            "Branch: {}\nRemote:\n{}",
            branch.trim(),
            remote.trim()
        ))
    }
}

fn run_git(cwd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gh_availability() {
        // Just verify it doesn't panic
        let _ = GitHubOps::is_available();
    }
}
