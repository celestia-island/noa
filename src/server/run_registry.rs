//! CI run records (W1b): the forge-side store chest's `ci_ops` backend
//! switch reads. A run = one execution of a check pipeline against a repo
//! head, recorded by the dispatcher (farm runner) or any operator via REST.
//!
//! Deliberately minimal fields: repo, number, head, kind, status,
//! timestamps, actor, summary. Log storage is NOT here (logs live in the
//! dispatch pipeline; the record carries a `log_ref` pointer for
//! consumers).

use axum::{
    extract::{Path, Query, State},
    Json,
};
use redb::{ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};

use super::handlers::AppState;
use crate::error::Result;

const RUNS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("runs");

type ApiResult<T> = Result<Json<T>, (axum::http::StatusCode, Json<serde_json::Value>)>;

fn err_json(msg: impl std::fmt::Display) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    tracing::error!("internal server error: {}", msg);
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "internal server error" })),
    )
}

fn bad_request(msg: impl ToString) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

fn not_found(msg: impl ToString) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

/// One recorded CI run against a repo head.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    /// Repo name from the catalog (the PATH segment owns it — a client
    /// payload's repo field is optional and overwritten by the handler).
    #[serde(default)]
    pub repo: String,
    /// Per-repo sequence, assigned by the server on record.
    #[serde(default)]
    pub number: u64,
    /// The head SHA the run executed against (40-hex).
    pub head_sha: String,
    /// Pipeline kind (e.g. "basic-check" — the trunk post-merge lane).
    pub kind: String,
    /// queued | running | success | failure | cancelled
    pub status: String,
    /// Who recorded it (runner id / operator identity).
    pub actor: String,
    /// Wall-clock microseconds (server fills on record if absent).
    #[serde(default)]
    pub started_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_us: Option<u64>,
    /// Pointer into the dispatch pipeline's log store (opaque to noa).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

const RUN_STATUSES: [&str; 5] = ["queued", "running", "success", "failure", "cancelled"];

fn validate_run(run: &RunRecord) -> Result<(), String> {
    if run.head_sha.len() != 40 || !run.head_sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("head_sha must be 40 hex chars".to_string());
    }
    if run.kind.is_empty() || run.kind.len() > 64 {
        return Err("kind must be 1..=64 chars".to_string());
    }
    if !RUN_STATUSES.contains(&run.status.as_str()) {
        return Err(format!("status must be one of {RUN_STATUSES:?}"));
    }
    Ok(())
}

fn run_key(repo: &str, number: u64) -> String {
    format!("{repo}#{number:020}")
}

/// CI run store. Per-request construction follows the mirror-job routes'
/// pattern (ensure_table is a one-time write txn; caching in AppState is a
/// follow-up shared with the other per-request stores there).
pub struct RunRegistry {
    db: std::sync::Arc<redb::Database>,
}

impl RunRegistry {
    #[must_use]
    pub fn new(db: std::sync::Arc<redb::Database>) -> Self {
        // ensure_table: the read path (next_number/list) cannot open a
        // missing table — create it once per construction (write txn), the
        // same pattern RepoRegistry/PrStore use at construction.
        let db_clone = std::sync::Arc::clone(&db);
        let _ = std::thread::spawn(move || -> anyhow::Result<()> {
            let txn = db_clone.begin_write()?;
            {
                let _table = txn.open_table(RUNS_TABLE)?;
            }
            txn.commit()?;
            Ok(())
        })
        .join();
        Self { db }
    }

    /// Records a run, assigning the next per-repo number.
    pub async fn record(&self, mut run: RunRecord) -> Result<RunRecord> {
        let prefix = format!("{}#", run.repo);
        let db = std::sync::Arc::clone(&self.db);
        let prefix_clone = prefix.clone();
        let max = tokio::task::spawn_blocking(move || -> Result<u64> {
            let txn = db.begin_read()?;
            let table = txn.open_table(RUNS_TABLE)?;
            let mut max = 0u64;
            for row in table.range(prefix_clone.as_str()..)? {
                let (key, _) = row?;
                let key_str = key.value();
                if !key_str.starts_with(&prefix_clone) {
                    break;
                }
                if let Ok(n) = key_str[prefix_clone.len()..].parse::<u64>() {
                    max = max.max(n);
                }
            }
            Ok(max + 1)
        })
        .await
        .map_err(|e| anyhow::anyhow!("runs next join: {e}"))??;
        run.number = max;

        let key = run_key(&run.repo, run.number);
        let value = serde_json::to_string(&run).map_err(|e| anyhow::anyhow!("run encode: {e}"))?;
        let db = std::sync::Arc::clone(&self.db);
        let key_clone = key.clone();
        let value_clone = value.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(RUNS_TABLE)?;
                table.insert(key_clone.as_str(), value_clone.as_str())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("run record join: {e}"))??;
        Ok(run)
    }

    /// Lists runs for a repo, newest first.
    pub async fn list(&self, repo: &str, limit: usize, offset: usize) -> Result<Vec<RunRecord>> {
        let prefix = format!("{repo}#");
        let db = std::sync::Arc::clone(&self.db);
        let prefix_clone = prefix;
        tokio::task::spawn_blocking(move || -> Result<Vec<RunRecord>> {
            let txn = db.begin_read()?;
            let table = txn.open_table(RUNS_TABLE)?;
            let mut rows: Vec<(String, String)> = Vec::new();
            for row in table.range(prefix_clone.as_str()..)? {
                let (k, v) = row?;
                if !k.value().starts_with(&prefix_clone) {
                    break;
                }
                rows.push((k.value().to_string(), v.value().to_string()));
            }
            let mut out = Vec::new();
            for (_, value) in rows.iter().rev().skip(offset).take(limit) {
                if let Ok(run) = serde_json::from_str::<RunRecord>(value) {
                    out.push(run);
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| anyhow::anyhow!("runs list join: {e}"))?
    }
}

/// POST /api/v1/repos/{name}/runs — record a run (write role via middleware).
pub async fn record_run(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(mut payload): Json<RunRecord>,
) -> Result<
    (axum::http::StatusCode, Json<RunRecord>),
    (axum::http::StatusCode, Json<serde_json::Value>),
> {
    payload.repo = name.clone();
    if let Err(msg) = validate_run(&payload) {
        return Err(bad_request(msg));
    }
    match state.repo_registry.get(&name).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(not_found(format!("repo '{name}' not found"))),
        Err(e) => return Err(err_json(e)),
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or_default();
    if payload.started_us == 0 {
        payload.started_us = now;
    }
    let registry = RunRegistry::new(std::sync::Arc::clone(&state.db));
    let stored = registry.record(payload).await.map_err(err_json)?;
    Ok((axum::http::StatusCode::CREATED, Json(stored)))
}

/// GET /api/v1/repos/{name}/runs — inline pagination (NO serde flatten; the
/// #144 lesson).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ListRunsParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

pub async fn list_runs(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<ListRunsParams>,
) -> ApiResult<Vec<RunRecord>> {
    let limit = params.limit.unwrap_or(50);
    if limit == 0 || limit > super::handlers::MAX_LIMIT {
        return Err(bad_request(format!(
            "limit must be between 1 and {}",
            super::handlers::MAX_LIMIT
        )));
    }
    match state.repo_registry.get(&name).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(not_found(format!("repo '{name}' not found"))),
        Err(e) => return Err(err_json(e)),
    }
    let registry = RunRegistry::new(std::sync::Arc::clone(&state.db));
    let runs = registry
        .list(&name, limit, params.offset.unwrap_or(0))
        .await
        .map_err(err_json)?;
    Ok(Json(runs))
}
