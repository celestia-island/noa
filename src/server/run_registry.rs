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
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
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
    // Head identity accepts BOTH worlds: a git commit sha (40 hex) or a
    // noa tree hash (64 hex) — merge-time verify claims are made against
    // whichever identity the claimer holds.
    let hex_ok = run.head_sha.len() == 40 || run.head_sha.len() == 64;
    if !hex_ok || !run.head_sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("head_sha must be 40 or 64 hex chars".to_string());
    }
    if run.kind.is_empty() || run.kind.len() > 64 {
        return Err("kind must be 1..=64 chars".to_string());
    }
    if !RUN_STATUSES.contains(&run.status.as_str()) {
        return Err(format!("status must be one of {RUN_STATUSES:?}"));
    }
    if run.actor.is_empty() || run.actor.len() > 200 {
        return Err("actor must be 1..=200 chars".to_string());
    }
    for (name, field) in [("log_ref", &run.log_ref), ("summary", &run.summary)] {
        if let Some(v) = field {
            if v.len() > 2000 {
                return Err(format!("{name} must be at most 2000 chars"));
            }
        }
    }
    Ok(())
}

fn run_key(repo: &str, number: u64) -> String {
    format!("{repo}#{number:020}")
}

/// CI run store. Per-request construction follows the mirror-job routes'
/// pattern (ensure_table is a one-time write txn; caching in AppState is a
/// follow-up shared with the other per-request stores there).
#[derive(Clone)]
pub struct RunRegistry {
    db: std::sync::Arc<redb::Database>,
}

impl RunRegistry {
    #[must_use]
    pub fn new(db: std::sync::Arc<redb::Database>) -> Self {
        // Table creation happens on the FIRST write transaction (record);
        // construction is once-per-startup from AppState — no per-request
        // writer-lock take (R2: the earlier per-request ensure_table
        // spawned a thread + joined on the tokio worker per request).
        Self { db }
    }

    /// Records a run, assigning the next per-repo number. The scan and the
    /// insert share ONE write transaction — redb's writer lock serializes
    /// concurrent records, so two simultaneous recordings cannot assign the
    /// same number and overwrite each other (R2 next_number race).
    pub async fn record(&self, mut run: RunRecord) -> Result<RunRecord> {
        let prefix = format!("{}#", run.repo);
        let db = std::sync::Arc::clone(&self.db);
        let prefix_clone = prefix.clone();
        let stored_json = tokio::task::spawn_blocking(move || -> Result<String> {
            let txn = db.begin_write()?;
            let (number, stored_json) = {
                let mut table = txn.open_table(RUNS_TABLE)?;
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
                let number = max + 1;
                run.number = number;
                let value =
                    serde_json::to_string(&run).map_err(|e| anyhow::anyhow!("run encode: {e}"))?;
                let key = run_key(prefix_clone.strip_suffix('#').unwrap_or(""), number);
                table.insert(key.as_str(), value.as_str())?;
                (number, value)
            };
            txn.commit()?;
            let _ = number;
            Ok(stored_json)
        })
        .await
        .map_err(|e| anyhow::anyhow!("run record join: {e}"))??;
        let stored: RunRecord =
            serde_json::from_str(&stored_json).map_err(|e| anyhow::anyhow!("run decode: {e}"))?;
        Ok(stored)
    }

    /// Lists runs for a repo, newest first.
    pub async fn list(&self, repo: &str, limit: usize, offset: usize) -> Result<Vec<RunRecord>> {
        let prefix = format!("{repo}#");
        let db = std::sync::Arc::clone(&self.db);
        let prefix_clone = prefix;
        tokio::task::spawn_blocking(move || -> Result<Vec<RunRecord>> {
            let txn = db.begin_read()?;
            // No table yet = no runs recorded — empty list, not an error.
            let table = match txn.open_table(RUNS_TABLE) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(e) => return Err(e.into()),
            };
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
    let registry = &state.run_registry;
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
    let registry = &state.run_registry;
    let runs = registry
        .list(&name, limit, params.offset.unwrap_or(0))
        .await
        .map_err(err_json)?;
    Ok(Json(runs))
}
