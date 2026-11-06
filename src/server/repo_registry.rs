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

    /// Validates an update payload (description length + branch shape).
    fn validate_payload(name: &str, description: Option<&str>, default_branch: &str) -> Result<()> {
        Self::validate(name)?;
        if let Some(d) = description {
            if d.len() > 512 {
                anyhow::bail!("description must be at most 512 bytes");
            }
        }
        // Branch shape: git ref rules (informational field) — no control
        // chars, no leading dash, no "..", no spaces.
        if default_branch.is_empty()
            || default_branch.starts_with('-')
            || default_branch.contains("..")
            || default_branch.contains('\u{0}')
            || default_branch.contains(' ')
            || default_branch.starts_with('/')
            || default_branch.ends_with('/')
        {
            anyhow::bail!("invalid default_branch {default_branch:?}");
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
        Self::validate_payload(name, description.as_deref(), default_branch)?;
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        let description = description.clone();
        let default_branch = default_branch.to_string();
        let origin = origin.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            let entry = {
                let mut table = txn.open_table(REPOS)?;
                let existing: Option<Vec<u8>> =
                    table.get(name.as_str())?.map(|g| g.value().to_vec());
                // origin AND created_us are preserved on update —
                // created_us means "at registration" (R2 S2).
                let (origin, created_us) = match &existing {
                    Some(bytes) => {
                        let old: RepoEntry = rmp_serde::from_slice(bytes)?;
                        (old.origin, old.created_us)
                    }
                    None => (
                        origin,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as i64,
                    ),
                };
                let entry = RepoEntry {
                    name: name.clone(),
                    description,
                    default_branch,
                    origin,
                    created_us,
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
    let reg = &state.repo_registry;
    match reg.list().await {
        Ok(repos) => (StatusCode::OK, Json(json!({ "repos": repos }))).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

fn json_err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

pub async fn get_repo(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let reg = &state.repo_registry;
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
    let reg = &state.repo_registry;
    match reg
        .register(&name, description, &default_branch, "manual")
        .await
    {
        Ok(entry) => (StatusCode::CREATED, repo_json(entry)).into_response(),
        // Validation failures (client fault) → 400; DB/storage errors → 500.
        Err(e)
            if e.to_string().contains("must match")
                || e.to_string().contains("must be at most")
                || e.to_string().contains("invalid")
                || e.to_string().contains("at least") =>
        {
            json_err(StatusCode::BAD_REQUEST, &e.to_string())
        }
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_repo(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    // Existence first: a ghost name must answer 404, not a 409 about PRs of
    // a repo the catalog denies (POST /api/v1/prs accepts any repo string,
    // so such orphan PRs are creatable and must not reshape this answer).
    match state.repo_registry.get(&name).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return json_err(StatusCode::NOT_FOUND, &format!("repo '{name}' not found"));
        }
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
    // Guard (W2b review): deleting the catalog row orphans any open PRs —
    // merge_pr never consults the catalog, so those tickets stay merge-able
    // throughout; what the delete costs is their ADDRESSABILITY (the scoped
    // view 404s while the flat route still serves them) and it lets a
    // later re-registration of the same name silently adopt them. Refuse
    // while open PRs exist; closing them is the operator's call (close every
    // open PR, then delete — the allow path stays reachable).
    let prs = match crate::server::pr_store::PrStore::new(Arc::clone(&state.db)) {
        Ok(store) => store.list(Some(&name), None, Some("open")).await,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    match prs {
        Ok(open) if !open.is_empty() => {
            return json_err(
                StatusCode::CONFLICT,
                &format!(
                    "repo '{name}' has {} open PR(s); close or merge them first",
                    open.len()
                ),
            );
        }
        Ok(_) => {}
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
    let reg = &state.repo_registry;
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
