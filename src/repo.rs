use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use redb::Database;

use crate::{
    config::RepoConfig,
    error::{NoaError, Result},
    log::FileAgentLog,
    object::RedbObjectStore,
    refs::RedbRefStore,
    snapshot::{RedbSnapshotStore, SnapshotId},
    workspace::WorkspaceManager,
};

pub const NOA_DIR_NAME: &str = ".noa";
pub const DB_NAME: &str = "noa.redb";
pub const AGENT_LOGS_DIR: &str = "agent-logs";
pub const SNAPSHOTS_DIR: &str = "snapshots";
pub const HEAD_FILE: &str = "HEAD";
pub const ORIG_HEAD_FILE: &str = "ORIG_HEAD";

pub struct Repository {
    pub root: PathBuf,
    pub noa_dir: PathBuf,
    pub db: Arc<Database>,
    pub config: RepoConfig,
}

impl Repository {
    pub fn init(path: &Path) -> Result<Self> {
        Self::init_with_noa_remote(path, None)
    }

    pub fn init_with_noa_remote(path: &Path, noa_remote: Option<&str>) -> Result<Self> {
        let noa_dir = path.join(NOA_DIR_NAME);

        if noa_dir.exists() {
            return Err(NoaError::RepoAlreadyExists(noa_dir.display().to_string()));
        }

        std::fs::create_dir_all(&noa_dir)?;
        std::fs::create_dir_all(noa_dir.join(AGENT_LOGS_DIR))?;

        let mut config = RepoConfig::default();
        if let Some(url) = noa_remote {
            config.noa_remote = Some(url.to_string());
        }
        config.save_to_dir(&noa_dir)?;

        std::fs::write(noa_dir.join(HEAD_FILE), "default\n")?;

        let db = Self::open_db(&noa_dir)?;
        Self::init_tables(&db)?;

        let db = Arc::new(db);
        Self::create_default_workspace(&db)?;

        manage_gitignore(path);

        if let Some(url) = noa_remote {
            manage_gitattributes(path, url);
        }

        Ok(Repository {
            root: path.to_path_buf(),
            noa_dir,
            db,
            config,
        })
    }

    pub fn init_with_remotes(path: &Path, remotes: Vec<crate::config::RemoteConfig>) -> Result<Self> {
        let noa_dir = path.join(NOA_DIR_NAME);

        if noa_dir.exists() {
            return Err(NoaError::RepoAlreadyExists(noa_dir.display().to_string()));
        }

        std::fs::create_dir_all(&noa_dir)?;
        std::fs::create_dir_all(noa_dir.join(AGENT_LOGS_DIR))?;

        let config = RepoConfig {
            name: "default".to_string(),
            remotes,
            noa_remote: None,
            sync: None,
        };
        config.save_to_dir(&noa_dir)?;

        std::fs::write(noa_dir.join(HEAD_FILE), "default\n")?;

        let db = Self::open_db(&noa_dir)?;
        Self::init_tables(&db)?;

        let db = Arc::new(db);
        Self::create_default_workspace(&db)?;

        manage_gitignore(path);

        Ok(Repository {
            root: path.to_path_buf(),
            noa_dir,
            db,
            config,
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        let noa_dir = path.join(NOA_DIR_NAME);

        if !noa_dir.exists() {
            return Err(NoaError::RepoNotFound(noa_dir.display().to_string()));
        }

        Self::validate(&noa_dir)?;

        let config = RepoConfig::load_from_dir(&noa_dir)?;
        let db = Self::open_db(&noa_dir)?;
        Self::init_tables(&db)?;

        Ok(Repository {
            root: path.to_path_buf(),
            noa_dir,
            db: Arc::new(db),
            config,
        })
    }

    pub fn find(from: &Path) -> Result<PathBuf> {
        let mut current = from.to_path_buf();
        loop {
            if current.join(NOA_DIR_NAME).exists() {
                return Ok(current);
            }
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => {
                    return Err(NoaError::RepoNotFound(
                        "reached filesystem root".to_string(),
                    ))
                }
            }
        }
    }

    pub fn exists(path: &Path) -> bool {
        path.join(NOA_DIR_NAME).exists()
    }

    fn validate(noa_dir: &Path) -> Result<()> {
        if !noa_dir.join(DB_NAME).exists() {
            return Err(NoaError::InvalidRepo("missing noa.redb".to_string()));
        }
        if !noa_dir.join(AGENT_LOGS_DIR).exists() {
            return Err(NoaError::InvalidRepo(
                "missing agent-logs/ directory".to_string(),
            ));
        }
        if !noa_dir.join("config").exists() {
            return Err(NoaError::InvalidRepo("missing config file".to_string()));
        }
        Ok(())
    }

    fn open_db(noa_dir: &Path) -> Result<Database> {
        let db_path = noa_dir.join(DB_NAME);
        let mut db = Database::builder()
            .create(&db_path)
            .map_err(|e| NoaError::Redb(e.to_string()))?;
        db.check_integrity()
            .map_err(|e| NoaError::Redb(format!("database integrity check failed: {}", e)))?;
        Ok(db)
    }

    fn init_tables(db: &Database) -> Result<()> {
        let write_txn = db
            .begin_write()
            .map_err(|e| NoaError::Redb(e.to_string()))?;

        {
            let _ = write_txn.open_table(redb::TableDefinition::<&[u8], &[u8]>::new("blobs"));
            let _ = write_txn.open_table(redb::TableDefinition::<&[u8], &[u8]>::new("trees"));
            let _ = write_txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("snapshots"));
            let _ = write_txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("workspaces"));
            let _ = write_txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("refs"));
        }

        write_txn
            .commit()
            .map_err(|e| NoaError::Redb(e.to_string()))
    }

    fn create_default_workspace(db: &Arc<Database>) -> Result<()> {
        let ws = crate::workspace::Workspace {
            name: "default".to_string(),
            head: SnapshotId("noa_empty".to_string()),
            base: SnapshotId("noa_empty".to_string()),
            agent_id: None,
            last_seq: 0,
            created_at: 0,
            updated_at: 0,
        };
        let data = rmp_serde::to_vec(&ws).map_err(|e| NoaError::Serialization(e.to_string()))?;
        let txn = db
            .begin_write()
            .map_err(|e| NoaError::Redb(e.to_string()))?;
        {
            let mut table = txn
                .open_table(redb::TableDefinition::<&str, &[u8]>::new("workspaces"))
                .map_err(|e| NoaError::Redb(e.to_string()))?;
            table
                .insert("default", data.as_slice())
                .map_err(|e| NoaError::Redb(e.to_string()))?;
        }
        txn.commit().map_err(|e| NoaError::Redb(e.to_string()))
    }

    pub fn read_head(&self) -> Result<String> {
        let head_path = self.noa_dir.join(HEAD_FILE);
        let content = std::fs::read_to_string(&head_path)?;
        Ok(content.trim().to_string())
    }

    pub fn write_head(&self, name: &str) -> Result<()> {
        let head_path = self.noa_dir.join(HEAD_FILE);
        std::fs::write(&head_path, format!("{}\n", name))?;
        Ok(())
    }

    pub fn read_orig_head(&self) -> Result<Option<String>> {
        let path = self.noa_dir.join(ORIG_HEAD_FILE);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(Some(content.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    pub fn write_orig_head(&self, name: &str) -> Result<()> {
        let path = self.noa_dir.join(ORIG_HEAD_FILE);
        std::fs::write(&path, format!("{}\n", name))?;
        Ok(())
    }

    pub fn agent_logs_dir(&self) -> PathBuf {
        self.noa_dir.join(AGENT_LOGS_DIR)
    }

    pub fn agent_log_path(&self, workspace: &str) -> PathBuf {
        self.agent_logs_dir().join(format!("{}.log", workspace))
    }

    pub fn save_config(&mut self) -> Result<()> {
        self.config.save_to_dir(&self.noa_dir)
    }

    pub fn object_store(&self) -> Result<RedbObjectStore> {
        RedbObjectStore::new(Arc::clone(&self.db))
    }

    pub fn snapshot_store(&self) -> Result<RedbSnapshotStore> {
        RedbSnapshotStore::new(Arc::clone(&self.db))
    }

    pub fn ref_store(&self) -> Result<RedbRefStore> {
        RedbRefStore::new(Arc::clone(&self.db))
    }

    pub fn workspace_manager(&self) -> Result<WorkspaceManager> {
        WorkspaceManager::new(Arc::clone(&self.db))
    }

    pub fn agent_log(&self, workspace: &str) -> Result<FileAgentLog> {
        let path = self.agent_log_path(workspace);
        FileAgentLog::create(&path)
    }

    pub fn init_for_sync(path: &Path) -> Result<SyncInitResult> {
        let noa_dir = path.join(NOA_DIR_NAME);
        let mut noa_initialized = false;
        let mut gitignore_updated = false;

        if !noa_dir.exists() {
            std::fs::create_dir_all(&noa_dir)?;
            std::fs::create_dir_all(noa_dir.join(AGENT_LOGS_DIR))?;
            std::fs::create_dir_all(noa_dir.join(SNAPSHOTS_DIR))?;

            let config = RepoConfig::default();
            config.save_to_dir(&noa_dir)?;

            std::fs::write(noa_dir.join(HEAD_FILE), "default\n")?;

            let db = Self::open_db(&noa_dir)?;
            Self::init_tables(&db)?;

            let db = Arc::new(db);
            Self::create_default_workspace(&db)?;

            noa_initialized = true;
        }

        let gitignore_path = path.join(".gitignore");
        if gitignore_path.exists() {
            let content = std::fs::read_to_string(&gitignore_path)?;
            let has_noa = content
                .lines()
                .any(|l| l.trim() == ".noa/" || l.trim() == ".noa");
            if !has_noa {
                manage_gitignore(path);
                gitignore_updated = true;
            }
        } else {
            manage_gitignore(path);
            gitignore_updated = true;
        }

        let current_branch = get_current_git_branch(path)?;

        Ok(SyncInitResult {
            repo_id: format!("noa:{}", path.display()),
            current_branch,
            noa_initialized,
            gitignore_updated,
        })
    }
}

pub struct SyncInitResult {
    pub repo_id: String,
    pub current_branch: String,
    pub noa_initialized: bool,
    pub gitignore_updated: bool,
}

pub fn get_current_git_branch(workspace_root: &Path) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .map_err(|e| NoaError::Sync(format!("git rev-parse failed: {}", e)))?;

    if !output.status.success() {
        return Err(NoaError::Sync(
            "failed to determine current git branch".to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn manage_gitignore(root: &Path) {
    let gitignore_path = root.join(".gitignore");

    if !gitignore_path.exists() {
        let content = "# Added by noa \u{2014} keep agent iteration data out of git\n.noa/\n";
        if let Err(e) = std::fs::write(&gitignore_path, content) {
            tracing::warn!("failed to write .gitignore: {}", e);
        }
        return;
    }

    let content = match std::fs::read_to_string(&gitignore_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("failed to read .gitignore: {}", e);
            return;
        }
    };

    for line in content.lines() {
        if line.trim() == ".noa" || line.trim() == ".noa/" {
            return;
        }
    }

    if let Err(e) = std::fs::write(&gitignore_path, format!("{}\n.noa/\n", content.trim_end())) {
        tracing::warn!("failed to update .gitignore: {}", e);
    }
}

pub fn manage_gitattributes(root: &Path, noa_remote_url: &str) {
    let gitattributes_path = root.join(".gitattributes");
    let attr_line = format!("{}/**   noa-remote={}", ".noa", noa_remote_url);

    if !gitattributes_path.exists() {
        let content = format!(
            "# Added by noa \u{2014} specifies where agent iteration data is hosted\n{}\n",
            attr_line
        );
        if let Err(e) = std::fs::write(&gitattributes_path, content) {
            tracing::warn!("failed to write .gitattributes: {}", e);
        }
        return;
    }

    let content = match std::fs::read_to_string(&gitattributes_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("failed to read .gitattributes: {}", e);
            return;
        }
    };

    for line in content.lines() {
        if line.contains("noa-remote=") {
            return;
        }
    }

    if let Err(e) = std::fs::write(
        &gitattributes_path,
        format!(
            "{}\n# Added by noa \u{2014} specifies where agent iteration data is hosted\n{}\n",
            content.trim_end(),
            attr_line
        ),
    ) {
        tracing::warn!("failed to update .gitattributes: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        assert!(repo.noa_dir.exists());
        assert!(repo.noa_dir.join(DB_NAME).exists());
        assert!(repo.noa_dir.join(AGENT_LOGS_DIR).exists());
        assert!(repo.noa_dir.join(HEAD_FILE).exists());
        assert!(repo.noa_dir.join("config").exists());
        assert_eq!(repo.read_head().unwrap(), "default");
    }

    #[test]
    fn test_init_fails_if_exists() {
        let tmp = TempDir::new().unwrap();
        Repository::init(tmp.path()).unwrap();
        let result = Repository::init(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_open_existing() {
        let tmp = TempDir::new().unwrap();
        Repository::init(tmp.path()).unwrap();
        let repo = Repository::open(tmp.path()).unwrap();
        assert_eq!(repo.read_head().unwrap(), "default");
    }

    #[test]
    fn test_open_fails_if_missing() {
        let tmp = TempDir::new().unwrap();
        let result = Repository::open(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_find_repo() {
        let tmp = TempDir::new().unwrap();
        Repository::init(tmp.path()).unwrap();

        let subdir = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        let found = Repository::find(&subdir).unwrap();
        assert_eq!(found, tmp.path());
    }

    #[test]
    fn test_exists() {
        let tmp = TempDir::new().unwrap();
        assert!(!Repository::exists(tmp.path()));
        Repository::init(tmp.path()).unwrap();
        assert!(Repository::exists(tmp.path()));
    }

    #[test]
    fn test_stores_accessible() {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        assert!(repo.object_store().is_ok());
        assert!(repo.snapshot_store().is_ok());
        assert!(repo.ref_store().is_ok());
        assert!(repo.workspace_manager().is_ok());
        assert!(repo.agent_log("default").is_ok());
    }

    #[test]
    fn test_init_creates_gitignore() {
        let tmp = TempDir::new().unwrap();
        Repository::init(tmp.path()).unwrap();
        let gitignore = tmp.path().join(".gitignore");
        assert!(gitignore.exists());
        let content = std::fs::read_to_string(&gitignore).unwrap();
        assert!(content.contains(".noa/"));
    }

    #[test]
    fn test_init_appends_to_existing_gitignore() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "*.log\n").unwrap();
        Repository::init(tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(content.contains("*.log"));
        assert!(content.contains(".noa/"));
    }

    #[test]
    fn test_init_does_not_duplicate_gitignore_entry() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".noa/\n").unwrap();
        let before = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        Repository::init(tmp.path()).unwrap();
        let after = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn test_init_with_noa_remote_creates_gitattributes() {
        let tmp = TempDir::new().unwrap();
        let repo =
            Repository::init_with_noa_remote(tmp.path(), Some("https://noa.example.com/repo"))
                .unwrap();

        let gitattributes = tmp.path().join(".gitattributes");
        assert!(gitattributes.exists());
        let content = std::fs::read_to_string(&gitattributes).unwrap();
        assert!(content.contains("noa-remote=https://noa.example.com/repo"));

        assert_eq!(
            repo.config.noa_remote,
            Some("https://noa.example.com/repo".to_string())
        );
    }

    #[test]
    fn test_init_without_noa_remote_no_gitattributes() {
        let tmp = TempDir::new().unwrap();
        Repository::init(tmp.path()).unwrap();
        assert!(!tmp.path().join(".gitattributes").exists());
    }

    #[test]
    fn test_init_with_noa_remote_appends_to_existing_gitattributes() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitattributes"), "*.bin binary\n").unwrap();
        Repository::init_with_noa_remote(tmp.path(), Some("https://noa.example.com/repo")).unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
        assert!(content.contains("*.bin binary"));
        assert!(content.contains("noa-remote="));
    }
}
