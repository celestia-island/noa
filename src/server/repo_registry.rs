//! Repo registry (W2b) — repos become a first-class entity: a redb-backed
//! catalog keyed by repo name (the same name clients use for
//! `git clone /git/<repo>` and the REST API `/api/v1/repos/{name}`).
//!
//! Deliberate scope: metadata + discovery only. Object/ref/workspace
//! stores stay global (the `mirror/<job>` soft namespace is the storage
//! layout); per-repo store sharding is a later slice if volume demands it.
//! Mirror-job upserts auto-register the repo entry so the catalog and the
//! mirror stay in lockstep.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use redb::{ReadableDatabase, ReadableTable, TableDefinition};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::error::Result;
use crate::server::handlers::AppState;

const REPOS: TableDefinition<&str, &[u8]> = TableDefinition::new("repos");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    /// Repo name = clone URL segment = mirror job name.
    /// `^[a-z0-9][a-z0-9-]{0,63}$`.
    pub name: String,
    /// Human description (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default branch advertised for clones (informational; upload-pack
    /// serves whatever HEAD the scratch clone carries).
    #[serde(default = "default_branch")]
    pub default_branch: String,
    /// How this repo came to exist: `mirror` (auto-registered from a
    /// mirror job) or `manual`.
    #[serde(default = "default_origin")]
    pub origin: String,
    /// Unix microseconds at registration.
    pub created_us: i64,
}

fn default_branch() -> String {
    "master".to_string()
}

fn default_origin() -> String {
    "manual".to_string()
}

#[derive(Clone)]
pub struct RepoRegistry {
    db: Arc<redb::Database>,
}

impl RepoRegistry {
    pub fn new(db: Arc<redb::Database>) -> Result<Self> {
        let store = RepoRegistry { db };
        {
            let txn = store.db.begin_write()?;
            txn.open_table(REPOS)?;
            txn.commit()?;
        }
        Ok(store)
    }

    fn validate(name: &str) -> Result<()> {
        let mut chars = name.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !first_ok
            || name.len() > 64
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            anyhow::bail!("repo name must match ^[a-z0-9][a-z0-9-]{{0,63}}$");
        }
        Ok(())
    }

    /// Registers or updates a repo entry. `origin` is set on insert and
    /// preserved on update.
    pub async fn register(
        &self,
        name: &str,
        description: Option<String>,
        default_branch: &str,
        origin: &str,
    ) -> Result<RepoEntry> {
        Self::validate(name)?;
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        let description = description.clone();
        let default_branch = default_branch.to_string();
        let origin = origin.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            let entry = {
                let mut table = txn.open_table(REPOS)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as i64;
                let existing: Option<Vec<u8>> =
                    table.get(name.as_str())?.map(|g| g.value().to_vec());
                let origin = if let Some(bytes) = &existing {
                    let old: RepoEntry = rmp_serde::from_slice(bytes)?;
                    old.origin
                } else {
                    origin
                };
                let entry = RepoEntry {
                    name: name.clone(),
                    description,
                    default_branch,
                    origin,
                    created_us: now,
                };
                let data = rmp_serde::to_vec_named(&entry)?;
                table.insert(name.as_str(), data.as_slice())?;
                entry
            };
            txn.commit()?;
            Ok(entry)
        })
        .await?
    }

    pub async fn get(&self, name: &str) -> Result<Option<RepoEntry>> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(REPOS)?;
            match table.get(name.as_str())? {
                Some(guard) => Ok(Some(rmp_serde::from_slice(guard.value())?)),
                None => Ok(None),
            }
        })
        .await?
    }

    pub async fn list(&self) -> Result<Vec<RepoEntry>> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(REPOS)?;
            let mut out: Vec<RepoEntry> = Vec::new();
            for row in table.iter()? {
                let (_, v) = row?;
                out.push(rmp_serde::from_slice(v.value())?);
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(out)
        })
        .await?
    }

    pub async fn delete(&self, name: &str) -> Result<bool> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            let removed = {
                let mut table = txn.open_table(REPOS)?;
                let x = table.remove(name.as_str())?.is_some();
                x
            };
            txn.commit()?;
            Ok(removed)
        })
        .await?
    }
}

// --- HTTP handlers (W2b) ---

fn repo_json(entry: RepoEntry) -> Json<serde_json::Value> {
    Json(json!({
        "name": entry.name,
        "description": entry.description,
        "default_branch": entry.default_branch,
        "origin": entry.origin,
        "created_us": entry.created_us,
    }))
}

pub async fn list_repos(State(state): State<AppState>) -> Response {
    let reg = match RepoRegistry::new(Arc::clone(&state.db)) {
        Ok(r) => r,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    match reg.list().await {
        Ok(repos) => (StatusCode::OK, Json(json!({ "repos": repos }))).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

fn json_err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

pub async fn get_repo(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let reg = match RepoRegistry::new(Arc::clone(&state.db)) {
        Ok(r) => r,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    match reg.get(&name).await {
        Ok(Some(e)) => repo_json(e).into_response(),
        Ok(None) => json_err(StatusCode::NOT_FOUND, &format!("repo '{name}' not found")),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn register_repo(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = body
        .get("description")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let default_branch = body
        .get("default_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("master")
        .to_string();
    let reg = match RepoRegistry::new(Arc::clone(&state.db)) {
        Ok(r) => r,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    match reg
        .register(&name, description, &default_branch, "manual")
        .await
    {
        Ok(entry) => (StatusCode::CREATED, repo_json(entry)).into_response(),
        Err(e) => json_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

pub async fn delete_repo(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let reg = match RepoRegistry::new(Arc::clone(&state.db)) {
        Ok(r) => r,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    match reg.delete(&name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => json_err(StatusCode::NOT_FOUND, &format!("repo '{name}' not found")),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn registry() -> (tempfile::TempDir, RepoRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(dir.path().join("t.redb"))
                .unwrap(),
        );
        (dir, RepoRegistry::new(db).unwrap())
    }

    #[tokio::test]
    async fn test_register_get_list_delete() {
        let (_d, reg) = registry().await;
        let e = reg
            .register("noa", Some("the VCS itself".into()), "master", "manual")
            .await
            .unwrap();
        assert_eq!(e.name, "noa");
        assert_eq!(e.origin, "manual");
        assert!(reg.get("noa").await.unwrap().is_some());

        reg.register("devtools", None, "master", "mirror")
            .await
            .unwrap();
        let all = reg.list().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "devtools");

        assert!(reg.delete("noa").await.unwrap());
        assert!(reg.get("noa").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_register_preserves_origin_on_update() {
        let (_d, reg) = registry().await;
        reg.register("x", None, "master", "mirror").await.unwrap();
        let e = reg
            .register("x", Some("updated".into()), "master", "manual")
            .await
            .unwrap();
        assert_eq!(e.origin, "mirror", "origin survives an update");
        assert_eq!(e.description, Some("updated".into()));
    }

    #[tokio::test]
    async fn test_register_rejects_bad_names() {
        let (_d, reg) = registry().await;
        for bad in ["", "UPPER", "has/slash", "has dot", "-lead", "a..b"] {
            assert!(
                reg.register(bad, None, "master", "manual").await.is_err(),
                "expected rejection of {bad:?}"
            );
        }
    }
}
