use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::{NoaAck, RequestNoaHandshake};
use crate::{
    error::Result,
    repo::{get_current_git_branch, manage_gitignore, Repository},
    server::constant_time_eq,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaHandshakeResponse {
    pub repo_id: String,
    pub current_branch: String,
    pub noa_initialized: bool,
    pub gitignore_updated: bool,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchSelection {
    NewSession(String),
    NewTask(String),
    Existing(String),
    Current,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaAuthResponse {
    pub workspace_id: String,
    pub selected_branch: String,
    pub branch_base: String,
    pub approved: bool,
}

pub fn handle_handshake_request(
    workspace_root: &Path,
    req: &RequestNoaHandshake,
    expected_token: &str,
) -> Result<NoaHandshakeResponse> {
    if !workspace_root.join(".git").exists() {
        anyhow::bail!("workspace has no git repository, noa requires git");
    }

    match &req.token {
        Some(token) if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) => {}
        _ => {
            anyhow::bail!("authentication failed: invalid or missing token");
        }
    }

    let noa_dir = workspace_root.join(".noa");
    let mut noa_initialized = false;
    let mut gitignore_updated = false;

    if noa_dir.exists() {
        let gitignore_path = workspace_root.join(".gitignore");
        if gitignore_path.exists() {
            let content = std::fs::read_to_string(&gitignore_path)?;
            let has_noa = content
                .lines()
                .any(|l| l.trim() == ".noa/" || l.trim() == ".noa");
            if !has_noa {
                manage_gitignore(workspace_root)?;
                gitignore_updated = true;
            }
        } else {
            manage_gitignore(workspace_root)?;
            gitignore_updated = true;
        }
    } else {
        Repository::init(workspace_root)?;
        noa_initialized = true;
        gitignore_updated = true;
    }

    let current_branch = get_current_git_branch(workspace_root)?;

    let repo_id = format!("{}:{}", req.workspace_id, workspace_root.display());

    Ok(NoaHandshakeResponse {
        repo_id,
        current_branch,
        noa_initialized,
        gitignore_updated,
        workspace_id: req.workspace_id.clone(),
    })
}

pub fn handle_auth_request(
    workspace_root: &Path,
    workspace_id: &str,
    selection: &BranchSelection,
    _suggested_branch: &str,
    branch_prefix: &str,
) -> Result<NoaAuthResponse> {
    let base_branch = get_current_git_branch(workspace_root)?;

    let selected_branch = match selection {
        BranchSelection::NewSession(session_id) => {
            validate_git_ref_component(session_id)?;
            let name = format!("{branch_prefix}{session_id}");
            create_git_branch(workspace_root, &name, &base_branch)?;
            name
        }
        BranchSelection::NewTask(task_name) => {
            validate_git_ref_component(task_name)?;
            let name = format!("{branch_prefix}{task_name}");
            create_git_branch(workspace_root, &name, &base_branch)?;
            name
        }
        BranchSelection::Existing(branch) => {
            validate_git_ref_name(branch)?;
            checkout_git_branch(workspace_root, branch)?;
            branch.clone()
        }
        BranchSelection::Current => base_branch.clone(),
    };

    tracing::info!(
        "auth decision: workspace_root={} selection={:?} branch={}",
        workspace_root.display(),
        selection,
        base_branch
    );

    Ok(NoaAuthResponse {
        workspace_id: workspace_id.to_string(),
        selected_branch,
        branch_base: base_branch,
        approved: true,
    })
}

fn validate_git_ref_component(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 128 {
        anyhow::bail!(
            "invalid ref component: length must be 1-128, got {}",
            name.len()
        );
    }
    if name.contains('\0') || name.contains('\n') || name.contains('\r') {
        anyhow::bail!("invalid ref component: contains control characters");
    }
    if name.contains("..") || name.contains('~') || name.contains('^') || name.contains(':') {
        anyhow::bail!("invalid ref component: contains forbidden characters");
    }
    if name.starts_with('.') || name.starts_with('-') || name.ends_with('.') {
        anyhow::bail!("invalid ref component: invalid start/end character");
    }
    if name
        .contains(|c: char| c.is_ascii_control() || c == ' ' || c == '\\' || c == '[' || c == '?')
    {
        anyhow::bail!("invalid ref component: contains forbidden characters");
    }
    Ok(())
}

fn validate_git_ref_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("empty ref name");
    }
    for component in name.split('/') {
        validate_git_ref_component(component)?;
    }
    Ok(())
}

pub fn handle_ready(workspace_id: &str, branch: &str, _snapshot_id: &str) -> Result<NoaAck> {
    tracing::info!("Noa workspace {} ready on branch {}", workspace_id, branch);
    Ok(NoaAck {
        ok: true,
        message: format!("workspace {workspace_id} ready"),
    })
}

fn create_git_branch(workspace_root: &Path, name: &str, base: &str) -> Result<()> {
    let existing = std::process::Command::new("git")
        .args(["rev-parse", "--verify", name])
        .current_dir(workspace_root)
        .output();

    if let Ok(output) = existing {
        if output.status.success() {
            checkout_git_branch(workspace_root, name)?;
            return Ok(());
        }
    }

    let output = std::process::Command::new("git")
        .args(["checkout", "-b", name, base])
        .current_dir(workspace_root)
        .output()
        .with_context(|| "git checkout -b failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(format!("git checkout -b failed: {stderr}"));
    }

    Ok(())
}

fn checkout_git_branch(workspace_root: &Path, name: &str) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["checkout", name])
        .current_dir(workspace_root)
        .output()
        .with_context(|| "git checkout failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(format!("git checkout failed: {stderr}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_git_repo(dir: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::fs::write(dir.join("README.md"), "hello").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn test_get_current_git_branch() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let branch = get_current_git_branch(tmp.path()).unwrap();
        assert!(!branch.is_empty());
    }

    const TEST_TOKEN: &str = "test-token-for-tests";

    #[test]
    fn test_handle_handshake_creates_noa() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let req = RequestNoaHandshake {
            workspace_id: "test-ws".to_string(),
            remote_name: "origin".to_string(),
            remote_path: tmp.path().display().to_string(),
            token: Some(TEST_TOKEN.to_string()),
        };
        let resp = handle_handshake_request(tmp.path(), &req, TEST_TOKEN).unwrap();
        assert!(resp.noa_initialized);
        assert!(tmp.path().join(".noa").exists());
    }

    #[test]
    fn test_handle_handshake_idempotent() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let req = RequestNoaHandshake {
            workspace_id: "test-ws".to_string(),
            remote_name: "origin".to_string(),
            remote_path: tmp.path().display().to_string(),
            token: Some(TEST_TOKEN.to_string()),
        };
        handle_handshake_request(tmp.path(), &req, TEST_TOKEN).unwrap();
        let resp2 = handle_handshake_request(tmp.path(), &req, TEST_TOKEN).unwrap();
        assert!(!resp2.noa_initialized);
    }

    #[test]
    fn test_handle_handshake_no_git_fails() {
        let tmp = TempDir::new().unwrap();
        let req = RequestNoaHandshake {
            workspace_id: "test-ws".to_string(),
            remote_name: "origin".to_string(),
            remote_path: tmp.path().display().to_string(),
            token: Some(TEST_TOKEN.to_string()),
        };
        let result = handle_handshake_request(tmp.path(), &req, TEST_TOKEN);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_handshake_bad_token_fails() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let req = RequestNoaHandshake {
            workspace_id: "test-ws".to_string(),
            remote_name: "origin".to_string(),
            remote_path: tmp.path().display().to_string(),
            token: Some("wrong-token".to_string()),
        };
        let result = handle_handshake_request(tmp.path(), &req, TEST_TOKEN);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("authentication failed"));
    }

    #[test]
    fn test_handle_handshake_missing_token_fails() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let req = RequestNoaHandshake {
            workspace_id: "test-ws".to_string(),
            remote_name: "origin".to_string(),
            remote_path: tmp.path().display().to_string(),
            token: None,
        };
        let result = handle_handshake_request(tmp.path(), &req, TEST_TOKEN);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("authentication failed"));
    }

    #[test]
    fn test_create_git_branch() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        create_git_branch(tmp.path(), "entelecheia/agent-test", "master").unwrap();
        let branch = get_current_git_branch(tmp.path()).unwrap();
        assert_eq!(branch, "entelecheia/agent-test");
    }

    #[test]
    fn test_auth_new_session() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let resp = handle_auth_request(
            tmp.path(),
            "test-ws",
            &BranchSelection::NewSession("sess-123".to_string()),
            "entelecheia/agent-sess-123",
            "entelecheia/agent-",
        )
        .unwrap();
        assert_eq!(resp.selected_branch, "entelecheia/agent-sess-123");
        assert!(resp.approved);
    }

    #[test]
    fn test_auth_current_branch() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let current = get_current_git_branch(tmp.path()).unwrap();
        let resp = handle_auth_request(
            tmp.path(),
            "test-ws",
            &BranchSelection::Current,
            "",
            "entelecheia/agent-",
        )
        .unwrap();
        assert_eq!(resp.selected_branch, current);
    }

    #[test]
    fn test_handle_ready() {
        let ack = handle_ready("ws-1", "main", "snap-123").unwrap();
        assert!(ack.ok);
    }
}
