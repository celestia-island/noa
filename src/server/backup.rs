//! Server-state backup & restore (slice 2 of the seven-slice series).
//!
//! The whole server state is three things: the redb file (every table —
//! refs, snapshots, objects, users, repos, PRs, runs, mirror jobs), the
//! scratch bare clones (one per mirror job), and the env file (credentials —
//! deliberately EXCLUDED: restoring credentials is re-provisioning, not
//! copying; A-class bootstrap config never travels inside a backup).
//!
//! Export (running server): hold a write transaction for the duration of
//! the redb file copy — the write lock blocks every committing writer, so
//! no pages are appended mid-copy — then `git bundle` each scratch into a
//! single-file portable artifact (a bundle is itself cloneable), and write
//! a manifest with per-artifact sha256. Import (offline subcommand): place
//! the redb file, un-bundle each bundle back into a bare scratch clone.
//! The restore drill is the acceptance gate: a restored instance must come
//! up and answer refs/jobs/users identically.

use std::path::{Path, PathBuf};

use axum::{extract::State, Json};
use serde_json::json;

use super::handlers::AppState;

/// Everything an export produces inside the target directory:
///
/// ```text
/// <dir>/noa.redb          — the database file (consistent copy)
/// <dir>/<job>.bundle      — one git bundle per mirror scratch (--all refs)
/// <dir>/manifest.json     — version, mirror-job configs, sha256 per artifact
/// ```
type ApiResult<T> = Result<Json<T>, (axum::http::StatusCode, Json<serde_json::Value>)>;

fn err_json(msg: impl std::fmt::Display) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    // Admin-only surface: the failure detail IS the diagnostic (an operator
    // fixing a backup needs the reason, not a generic 500).
    tracing::error!("backup export failed: {}", msg);
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

pub const MANIFEST_NAME: &str = "manifest.json";
pub const DB_ARTIFACT: &str = "noa.redb";

/// Runs the export against a LIVE server state. Holding a write
/// transaction on the database freezes commits for the copy's duration
/// (single-digit seconds at family scale — the production database is
/// tens of MiB); readers keep working (MVCC).
pub async fn export_state(state: &AppState, out_dir: &Path) -> Result<serde_json::Value, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create out dir: {e}"))?;

    let db_path = PathBuf::from(&state.db_path);
    let db_copy = out_dir.join(DB_ARTIFACT);

    // Phase 1: consistent database copy under the write lock. The
    // transaction writes NOTHING (open table handles force the commit
    // machinery to stay idle — dropping it rolls back an empty txn).
    {
        let db = std::sync::Arc::clone(&state.db);
        let db_copy_clone = db_copy.clone();
        let db_path_clone = db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let txn = db.begin_write().map_err(|e| format!("write lock: {e}"))?;
            // Touch a table so the transaction is real (an empty txn with
            // no table handles may be optimized away by redb).
            {
                let _table = txn
                    .open_table(super::mirror_store::MIRROR_JOBS)
                    .map_err(|e| format!("lock table: {e}"))?;
            }
            std::fs::copy(&db_path_clone, &db_copy_clone).map_err(|e| format!("db copy: {e}"))?;
            drop(txn); // empty txn rolls back; the file copy is already done
            Ok(())
        })
        .await
        .map_err(|e| format!("join: {e}"))??;
    }

    // Phase 2: bundle every scratch clone. Bundles are created from the
    // live scratch dirs — git bundle is a read-only operation on the
    // source (objects are immutable; refs may advance concurrently, the
    // bundle just captures this instant).
    let scratch_dir = state.mirror_config.scratch_dir.clone();
    let mut bundles = Vec::new();
    if scratch_dir.exists() {
        let mut entries = std::fs::read_dir(&scratch_dir)
            .map_err(|e| format!("read scratch dir: {e}"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|x| x == "git") || p.to_string_lossy().ends_with(".git")
            })
            .collect::<Vec<_>>();
        entries.sort();
        for scratch in entries {
            let job_name = scratch
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if job_name.is_empty() {
                continue;
            }
            let bundle_path = out_dir.join(format!("{job_name}.bundle"));
            let out = tokio::process::Command::new("git")
                .args([
                    "-C",
                    scratch.to_str().unwrap_or_default(),
                    "bundle",
                    "create",
                    bundle_path.to_str().unwrap_or_default(),
                    "--all",
                ])
                .output()
                .await
                .map_err(|e| format!("bundle {job_name}: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "bundle {job_name} failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            bundles.push(bundle_path);
        }
    }

    // Phase 3: manifest (version + artifact checksums).
    let mut artifacts: Vec<(String, PathBuf)> = vec![(DB_ARTIFACT.to_string(), db_copy.clone())];
    artifacts.extend(bundles.iter().map(|b| {
        (
            b.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            b.clone(),
        )
    }));
    let mut files = Vec::new();
    for (name, path) in &artifacts {
        let sha = sha256_file(path).map_err(|e| format!("hash {name}: {e}"))?;
        let len = path.metadata().map(|m| m.len()).unwrap_or(0);
        files.push(json!({ "name": name, "sha256": sha, "bytes": len }));
    }
    let manifest = json!({
        "schema": "noa.backup/1",
        "version": env!("CARGO_PKG_VERSION"),
        "files": files,
    });
    let manifest_path = out_dir.join(MANIFEST_NAME);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|e| format!("write manifest: {e}"))?;
    Ok(manifest)
}

/// Offline restore: place the database, un-bundle every bundle into a bare
/// scratch clone. The server must NOT be running against `db_path` (the
/// subcommand form guarantees this — it restores before ever opening the
/// database).
pub fn import_state(backup_dir: &Path, db_path: &Path, scratch_dir: &Path) -> Result<(), String> {
    let manifest_path = backup_dir.join(MANIFEST_NAME);
    let manifest_raw = std::fs::read(&manifest_path).map_err(|e| format!("read manifest: {e}"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_raw).map_err(|e| format!("parse manifest: {e}"))?;
    if manifest["schema"] != "noa.backup/1" {
        return Err(format!("unknown backup schema: {}", manifest["schema"]));
    }

    // Verify every artifact against the manifest's checksums BEFORE
    // touching anything — a half-corrupt backup must restore nothing.
    let files = manifest["files"]
        .as_array()
        .ok_or("manifest has no files array")?;
    for f in files {
        let name = f["name"].as_str().unwrap_or_default();
        let expected = f["sha256"].as_str().unwrap_or_default();
        let path = backup_dir.join(name);
        let actual = sha256_file(&path).map_err(|e| format!("hash {name}: {e}"))?;
        if actual != expected {
            return Err(format!(
                "checksum mismatch for {name}: {actual} != {expected}"
            ));
        }
    }

    // Database.
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create db parent: {e}"))?;
    }
    std::fs::copy(backup_dir.join(DB_ARTIFACT), db_path).map_err(|e| format!("restore db: {e}"))?;

    // Bundles → bare scratch clones.
    std::fs::create_dir_all(scratch_dir).map_err(|e| format!("create scratch: {e}"))?;
    for f in files {
        let name = f["name"].as_str().unwrap_or_default();
        if !name.ends_with(".bundle") {
            continue;
        }
        let job = name.trim_end_matches(".bundle");
        let target = scratch_dir.join(format!("{job}.git"));
        if target.exists() {
            // Idempotent restore: an existing scratch keeps its state (it
            // may be AHEAD — the restored db + a fresher scratch is the
            // good direction; the next wave reconciles).
            continue;
        }
        let out = std::process::Command::new("git")
            .args([
                "clone",
                "--bare",
                backup_dir.join(name).to_str().unwrap_or_default(),
                target.to_str().unwrap_or_default(),
            ])
            .output()
            .map_err(|e| format!("unbundle {job}: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "unbundle {job} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        // The bundle's remote is the backup file; re-point origin at the
        // job's real remote happens on the next wave (ensure_scratch does
        // `remote set-url` from the job config — the config lives in the
        // restored db).
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    h.update(&data);
    let digest = h.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// POST /api/v1/admin/export — admin-only live export.
pub async fn export_handler(State(state): State<AppState>) -> ApiResult<serde_json::Value> {
    let out_dir = std::env::var("NOA_EXPORT_DIR").unwrap_or_else(|_| "noa-backup".to_string());
    match export_state(&state, Path::new(&out_dir)).await {
        Ok(manifest) => Ok(Json(manifest)),
        Err(e) => Err(err_json(e)),
    }
}
