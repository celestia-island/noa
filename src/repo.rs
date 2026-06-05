use std::path::{Path, PathBuf};

use redb::Database;

use crate::config::RepoConfig;
use crate::error::{NoaError, Result};

pub const NOA_DIR_NAME: &str = ".noa";
pub const DB_NAME: &str = "noa.redb";
pub const AGENT_LOGS_DIR: &str = "agent-logs";
pub const HEAD_FILE: &str = "HEAD";
pub const ORIG_HEAD_FILE: &str = "ORIG_HEAD";

pub struct Repository {
    pub root: PathBuf,
    pub noa_dir: PathBuf,
    pub db: Database,
    pub config: RepoConfig,
}

impl Repository {
    pub fn init(path: &Path) -> Result<Self> {
        let noa_dir = path.join(NOA_DIR_NAME);

        if noa_dir.exists() {
            return Err(NoaError::RepoAlreadyExists(
                noa_dir.display().to_string(),
            ));
        }

        std::fs::create_dir_all(&noa_dir)?;
        std::fs::create_dir_all(noa_dir.join(AGENT_LOGS_DIR))?;

        let config = RepoConfig::default();
        config.save_to_dir(&noa_dir)?;

        std::fs::write(noa_dir.join(HEAD_FILE), "default\n")?;

        let db = Self::open_db(&noa_dir)?;
        Self::init_tables(&db)?;

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
            return Err(NoaError::RepoNotFound(
                noa_dir.display().to_string(),
            ));
        }

        Self::validate(&noa_dir)?;

        let config = RepoConfig::load_from_dir(&noa_dir)?;
        let db = Self::open_db(&noa_dir)?;

        Ok(Repository {
            root: path.to_path_buf(),
            noa_dir,
            db,
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
            return Err(NoaError::InvalidRepo(
                "missing noa.redb".to_string(),
            ));
        }
        if !noa_dir.join(AGENT_LOGS_DIR).exists() {
            return Err(NoaError::InvalidRepo(
                "missing agent-logs/ directory".to_string(),
            ));
        }
        if !noa_dir.join("config").exists() {
            return Err(NoaError::InvalidRepo(
                "missing config file".to_string(),
            ));
        }
        Ok(())
    }

    fn open_db(noa_dir: &Path) -> Result<Database> {
        let db_path = noa_dir.join(DB_NAME);
        Database::builder()
            .create(&db_path)
            .map_err(|e| NoaError::Redb(e.to_string()))
    }

    fn init_tables(db: &Database) -> Result<()> {
        let write_txn = db
            .begin_write()
            .map_err(|e| NoaError::Redb(e.to_string()))?;

        {
            let _ = write_txn.open_table(
                redb::TableDefinition::<&[u8], &[u8]>::new("blobs"),
            );
            let _ = write_txn.open_table(
                redb::TableDefinition::<&[u8], &[u8]>::new("trees"),
            );
            let _ = write_txn.open_table(
                redb::TableDefinition::<&str, &[u8]>::new("snapshots"),
            );
            let _ = write_txn.open_table(
                redb::TableDefinition::<&str, &[u8]>::new("workspaces"),
            );
            let _ = write_txn.open_table(
                redb::TableDefinition::<&str, &[u8]>::new("refs"),
            );
        }

        write_txn
            .commit()
            .map_err(|e| NoaError::Redb(e.to_string()))
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

    pub fn save_config(&self) -> Result<()> {
        self.config.save_to_dir(&self.noa_dir)
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
}
