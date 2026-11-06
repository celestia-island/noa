//! Server-side mirror engine (W1 mirror-first lane).
//!
//! For every configured [`MirrorJob`] the engine keeps a bare scratch clone
//! under the scratch dir, fetches it on each run, and imports each tracked
//! branch into noa under ref `mirror/<job>/<branch>` with a workspace
//! `mirror.<job>.<sanitized branch>`. After each import it re-verifies the
//! noa-side tree against the git tree byte-for-byte (git blob oids are
//! recomputed from noa blobs) so a green job proves the replication channel,
//! not just that nothing crashed.
//!
//! W1 scope (see the workspace feasibility report): this lane only READS
//! from the remote and writes noa-side replicas. It never pushes
//! authoritative refs; push lanes (git transport canary + Git Data API
//! fallback) land in the next slice.
//!
//! Credentials: `remote_url` never carries a token. When
//! `NOA_MIRROR_GH_TOKEN` (or the configured env) is set, git is handed the
//! token through a generated `GIT_ASKPASS` helper — the token never appears
//! in argv, in stored config, or in error output.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use tokio::sync::Mutex;

use crate::{
    error::Result,
    git::{import_git_commit, ImportOptions},
    object::{EntryKind, ObjectStore, RedbObjectStore, TreeId},
    refs::{RedbRefStore, RefStore},
    server::{
        handlers::{err_json, ApiError},
        mirror_store::{MirrorJob, MirrorJobStore},
    },
    snapshot::{RedbSnapshotStore, SnapshotId, SnapshotStore},
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MirrorConfig {
    /// Seconds between scheduler ticks (minimum 30 enforced at spawn).
    pub interval_secs: u64,
    /// Root dir for bare scratch clones (`<scratch>/<job>.git`).
    pub scratch_dir: PathBuf,
    /// Env var the engine reads the git token from at run time (rotation
    /// without restart). Empty string disables authenticated fetch.
    pub token_env: String,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        MirrorConfig {
            interval_secs: 900,
            scratch_dir: PathBuf::from("mirror-cache"),
            token_env: "NOA_MIRROR_GH_TOKEN".to_string(),
        }
    }
}

impl MirrorConfig {
    fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
        std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
    }

    /// `NOA_MIRROR_INTERVAL_SECS` (default 900) · `NOA_MIRROR_SCRATCH_DIR`
    /// (default `<db dir>/mirror-cache`) · `NOA_MIRROR_GH_TOKEN_ENV`
    /// (default `NOA_MIRROR_GH_TOKEN`).
    pub fn from_env(default_scratch: PathBuf) -> Self {
        MirrorConfig {
            interval_secs: Self::env_parse("NOA_MIRROR_INTERVAL_SECS").unwrap_or(900),
            scratch_dir: std::env::var("NOA_MIRROR_SCRATCH_DIR")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or(default_scratch),
            token_env: std::env::var("NOA_MIRROR_GH_TOKEN_ENV")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "NOA_MIRROR_GH_TOKEN".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MirrorEngine {
    db: Arc<redb::Database>,
    cfg: MirrorConfig,
}

/// Per-branch outcome collected for the job status line.
struct BranchOutcome {
    branch: String,
    snapshot: String,
    changed: bool,
}

impl MirrorEngine {
    #[must_use]
    pub fn new(db: Arc<redb::Database>, cfg: MirrorConfig) -> Self {
        MirrorEngine { db, cfg }
    }

    /// Runs one job by name and records the outcome in the job store.
    pub async fn run_job(&self, name: &str) -> Result<()> {
        let store = MirrorJobStore::new(Arc::clone(&self.db))?;
        let Some(job) = store.get(name).await? else {
            tracing::warn!("mirror run: unknown job '{name}'");
            anyhow::bail!("unknown mirror job '{name}'");
        };
        match self.execute(&job).await {
            Ok(detail) => {
                store
                    .record_run(&job.name, "ok", &detail)
                    .await
                    .unwrap_or_else(|e| tracing::warn!("mirror: record_run failed: {e}"));
                tracing::info!("mirror job {} ok: {}", job.name, detail);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                store
                    .record_run(&job.name, "error", &msg)
                    .await
                    .unwrap_or_else(|e2| tracing::warn!("mirror: record_run failed: {e2}"));
                tracing::error!("mirror job {} failed: {msg}", job.name);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Runs every enabled job; one job's failure never stops the others.
    pub async fn run_enabled(&self) -> Result<()> {
        let store = MirrorJobStore::new(Arc::clone(&self.db))?;
        for job in store.list().await? {
            if !job.enabled {
                continue;
            }
            // Errors already recorded + logged per job.
            let _ = self.run_job(&job.name).await;
        }
        Ok(())
    }

    async fn execute(&self, job: &MirrorJob) -> Result<String> {
        let scratch = self.cfg.scratch_dir.join(format!("{}.git", job.name));
        if let Some(parent) = scratch.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if scratch.exists() {
            // Re-point origin every run: a job whose remote_url was updated
            // (or a name reused after delete) must never keep fetching the
            // stale URL baked into an existing clone (R2 S1).
            self.git(&scratch, &["remote", "set-url", "origin", &job.remote_url])
                .await?;
            self.git(&scratch, &["fetch", "--prune", "--tags", "origin"])
                .await?;
        } else {
            let scratch_str = scratch.to_string_lossy().to_string();
            self.git(
                Path::new("."),
                &["clone", "--bare", &job.remote_url, &scratch_str],
            )
            .await?;
            // Bare clones have no fetch refspec, so branch tracking would
            // never populate refs/remotes/origin/*; configure it explicitly
            // and prime it with an initial fetch.
            self.git(
                &scratch,
                &[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/*:refs/remotes/origin/*",
                ],
            )
            .await?;
            self.git(&scratch, &["fetch", "--prune", "--tags", "origin"])
                .await?;
        }

        // Per-branch isolation (R2 S2): one bad branch (deleted upstream,
        // missing after fetch, fidelity failure) must not stop the others;
        // the job still reports error with per-branch detail.
        let mut notes = Vec::new();
        let mut failed = 0usize;
        for branch in &job.branches {
            match self.replicate_branch(job, &scratch, branch).await {
                Ok(outcome) => notes.push(format!(
                    "{}={}({})",
                    outcome.branch,
                    outcome.snapshot,
                    if outcome.changed {
                        "imported"
                    } else {
                        "unchanged"
                    }
                )),
                Err(e) => {
                    failed += 1;
                    notes.push(format!("{branch}=FAILED: {e:#}"));
                }
            }
        }
        let summary = if failed == 0 {
            "fidelity ok".to_string()
        } else {
            format!("{failed} branch(es) failed")
        };
        let detail = format!("{summary}; {}", notes.join(" "));
        if failed > 0 {
            anyhow::bail!("{detail}");
        }
        Ok(detail)
    }

    async fn replicate_branch(
        &self,
        job: &MirrorJob,
        scratch: &Path,
        branch: &str,
    ) -> Result<BranchOutcome> {
        let ref_name = format!("mirror/{}/{}", job.name, branch);
        let ws_name = format!("mirror.{}.{}", job.name, branch.replace('/', "__"));
        let oid = resolve_remote_branch(scratch, branch).await?;
        let ref_store = RedbRefStore::new(Arc::clone(&self.db))?;
        let prev = ref_store.get(&ref_name).await?;
        let opts = ImportOptions {
            workspace: ws_name,
            head_ref: ref_name,
            commit_hex: Some(oid.clone()),
            parents: prev.iter().cloned().collect(),
            skip_if_tree_unchanged: true,
        };
        let snap = import_git_commit(scratch, Arc::clone(&self.db), &opts)
            .await?
            .ok_or_else(|| anyhow::anyhow!("branch '{branch}' missing after fetch"))?;
        let changed = prev.as_ref() != Some(&snap);
        self.verify_fidelity(scratch, &oid, &snap).await?;
        Ok(BranchOutcome {
            branch: branch.to_string(),
            snapshot: snap.0.chars().take(12).collect(),
            changed,
        })
    }

    /// Recomputes git blob oids from the noa-side snapshot tree and compares
    /// them against the git tree of `commit_hex`. Any path/kind/oid mismatch
    /// is an error naming the first divergent entry.
    async fn verify_fidelity(
        &self,
        scratch: &Path,
        commit_hex: &str,
        snapshot: &SnapshotId,
    ) -> Result<()> {
        let snap_store = RedbSnapshotStore::new(Arc::clone(&self.db))?;
        let snap = snap_store.get(snapshot).await?;
        let obj_store = RedbObjectStore::new(Arc::clone(&self.db))?;
        let tree = obj_store.get_tree(&TreeId(snap.tree_hash.clone())).await?;

        let git_entries = walk_git_tree_oids(scratch, commit_hex).await?;
        let mut noa_by_path: std::collections::HashMap<String, _> =
            tree.0.into_iter().map(|e| (e.name.clone(), e)).collect();

        if git_entries.len() != noa_by_path.len() {
            anyhow::bail!(
                "fidelity: entry count differs git={} noa={}",
                git_entries.len(),
                noa_by_path.len()
            );
        }
        for (path, kind, git_oid) in git_entries {
            let Some(entry) = noa_by_path.remove(&path) else {
                anyhow::bail!("fidelity: path {path:?} missing on noa side");
            };
            if entry.kind != kind {
                anyhow::bail!(
                    "fidelity: kind differs at {path:?}: git={kind:?} noa={:?}",
                    entry.kind
                );
            }
            if kind.has_blob_body() {
                let content = obj_store
                    .get_blob(&crate::object::BlobId(entry.id.clone()))
                    .await?;
                let expected = git_blob_oid(&content);
                if expected != git_oid {
                    anyhow::bail!(
                        "fidelity: blob oid differs at {path:?}: git={git_oid} noa={expected}"
                    );
                }
            } else if kind == EntryKind::Gitlink && entry.id != git_oid {
                anyhow::bail!(
                    "fidelity: gitlink oid differs at {path:?}: git={git_oid} noa={}",
                    entry.id
                );
            }
        }
        Ok(())
    }

    /// Runs git with the askpass helper when a token is configured. The
    /// token only ever exists in the child's environment.
    async fn git(&self, cwd: &Path, args: &[&str]) -> Result<()> {
        let cfg = self.cfg.clone();
        let cwd = cwd.to_path_buf();
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let args_for_error = args.join(" ");
        let out = tokio::task::spawn_blocking(move || -> std::io::Result<std::process::Output> {
            let mut cmd = Command::new("git");
            cmd.args(&args)
                .current_dir(&cwd)
                .env("GIT_TERMINAL_PROMPT", "0");
            let token = if cfg.token_env.is_empty() {
                None
            } else {
                std::env::var(&cfg.token_env).ok().filter(|t| !t.is_empty())
            };
            if let Some(token) = token {
                let askpass = cfg.scratch_dir.join(".askpass.sh");
                if let Some(parent) = askpass.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(
                    &askpass,
                    "#!/bin/sh\nprintf '%s\\n' \"$NOA_MIRROR_FETCH_TOKEN\"\n",
                )?;
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&askpass)?.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(&askpass, perms)?;
                cmd.env("GIT_ASKPASS", &askpass)
                    .env("NOA_MIRROR_FETCH_TOKEN", token);
            }
            cmd.output()
        })
        .await??;
        if out.status.success() {
            Ok(())
        } else {
            // stderr of git never contains the token (it is not in argv nor
            // the URL), so it is safe to surface.
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("git {args_for_error} failed: {}", stderr.trim());
        }
    }
}

async fn resolve_remote_branch(scratch: &Path, branch: &str) -> Result<String> {
    let scratch = scratch.to_path_buf();
    let branch = branch.to_string();
    let oid = tokio::task::spawn_blocking(move || -> Result<String> {
        let repo = gix::open(&scratch).map_err(|e| anyhow::anyhow!("open scratch clone: {e}"))?;
        let reference = repo
            .find_reference(&format!("refs/remotes/origin/{branch}"))
            .map_err(|e| anyhow::anyhow!("branch '{branch}' not on origin: {e}"))?;
        let id = reference
            .try_id()
            .ok_or_else(|| anyhow::anyhow!("branch '{branch}' has symbolic target"))?
            .detach();
        Ok(id.to_string())
    })
    .await??;
    Ok(oid)
}

/// One flat git-tree entry: path, kind, oid (hex).
async fn walk_git_tree_oids(
    scratch: &Path,
    commit_hex: &str,
) -> Result<Vec<(String, EntryKind, String)>> {
    let scratch = scratch.to_path_buf();
    let commit_hex = commit_hex.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<(String, EntryKind, String)>> {
        let repo = gix::open(&scratch).map_err(|e| anyhow::anyhow!("open scratch clone: {e}"))?;
        let oid = gix::hash::ObjectId::from_hex(commit_hex.as_bytes())
            .map_err(|e| anyhow::anyhow!("invalid oid '{commit_hex}': {e}"))?;
        let commit = repo
            .find_object(oid)?
            .try_into_commit()
            .map_err(|_| anyhow::anyhow!("object {commit_hex} is not a commit"))?;
        let tree_id = commit.tree_id()?.detach();

        let mut stack = vec![(tree_id, String::new())];
        let mut out = Vec::new();
        while let Some((cur, prefix)) = stack.pop() {
            let tree = repo
                .find_object(cur)?
                .try_into_tree()
                .map_err(|_| anyhow::anyhow!("object is not a tree"))?;
            for entry_res in tree.iter() {
                let entry = entry_res?;
                let mode = entry.mode();
                let name = entry.filename().to_string();
                let full = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if mode.is_tree() {
                    stack.push((entry.oid().to_owned(), full));
                } else {
                    let kind = if mode.is_commit() {
                        EntryKind::Gitlink
                    } else {
                        EntryKind::from_git_mode(mode.value().into())
                    };
                    out.push((full, kind, entry.oid().to_string()));
                }
            }
        }
        Ok(out)
    })
    .await?
}

/// SHA-1 git blob oid (`git hash-object`). NB: a SHA-256-format origin repo
/// fails fidelity on every entry — detectable, not silently wrong; acceptable
/// until a SHA-256 origin actually shows up (workspace repos are SHA-1).
fn git_blob_oid(content: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", content.len()).as_bytes());
    hasher.update(content);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(40);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// One mirror run at a time, ever (the scheduler tick, an API-triggered run
/// and a manual run must not fetch the same scratch clone concurrently).
static RUN_LOCK: Mutex<()> = Mutex::const_new(());

pub fn spawn_scheduler(db: Arc<redb::Database>, cfg: MirrorConfig) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = Duration::from_secs(cfg.interval_secs.max(30));
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let _guard = RUN_LOCK.lock().await;
            let engine = MirrorEngine::new(Arc::clone(&db), cfg.clone());
            if let Err(e) = engine.run_enabled().await {
                tracing::error!("mirror scheduler wave failed: {e:#}");
            }
        }
    })
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RunRequest {
    /// Job name; `None` runs every enabled job.
    #[serde(default)]
    pub job: Option<String>,
}

pub async fn list_jobs(
    State(state): State<crate::server::AppState>,
) -> Result<Json<Vec<MirrorJob>>, (StatusCode, Json<ApiError>)> {
    let store = MirrorJobStore::new(Arc::clone(&state.db)).map_err(err_json)?;
    let jobs = store.list().await.map_err(err_json)?;
    Ok(Json(jobs))
}

pub async fn upsert_job(
    State(state): State<crate::server::AppState>,
    Json(job): Json<MirrorJob>,
) -> Result<(StatusCode, Json<MirrorJob>), (StatusCode, Json<ApiError>)> {
    let store = MirrorJobStore::new(Arc::clone(&state.db)).map_err(err_json)?;
    let saved = store.upsert(job).await.map_err(bad_request)?;
    Ok((StatusCode::CREATED, Json(saved)))
}

pub async fn delete_job(
    State(state): State<crate::server::AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let store = MirrorJobStore::new(Arc::clone(&state.db)).map_err(err_json)?;
    match store.delete(&name).await.map_err(err_json)? {
        true => {
            // Remove the scratch clone so a job name reused with a different
            // remote cannot inherit the dead clone's origin (R2 S1 trap);
            // noa-side refs/snapshots stay as replica data (R2 S2 scope).
            let scratch = state.mirror_config.scratch_dir.join(format!("{name}.git"));
            if let Err(e) = std::fs::remove_dir_all(&scratch) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("mirror: scratch cleanup for {name} failed: {e}");
                }
            }
            Ok(StatusCode::NO_CONTENT)
        }
        false => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "mirror job not found".to_string(),
            }),
        )),
    }
}

pub async fn run_jobs(
    State(state): State<crate::server::AppState>,
    Json(req): Json<RunRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let db = Arc::clone(&state.db);
    let cfg = state.mirror_config.clone();
    let job = req.job;
    tokio::spawn(async move {
        let _guard = RUN_LOCK.lock().await;
        let engine = MirrorEngine::new(db, cfg);
        match job {
            Some(name) => {
                if let Err(e) = engine.run_job(&name).await {
                    tracing::error!("mirror run '{name}' failed: {e:#}");
                }
            }
            None => {
                if let Err(e) = engine.run_enabled().await {
                    tracing::error!("mirror run-all failed: {e:#}");
                }
            }
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "started": true })),
    )
}

fn bad_request(msg: impl std::fmt::Display) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: msg.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_fixture(dir: &Path) -> PathBuf {
        let origin = dir.join("origin.git");
        run(
            dir,
            &[
                "init",
                "--bare",
                "--initial-branch=master",
                origin.to_str().unwrap(),
            ],
        );
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        run(&work, &["clone", origin.to_str().unwrap(), "."]);
        std::fs::write(work.join("README.md"), b"# fixture\n").unwrap();
        std::fs::create_dir_all(work.join("nested")).unwrap();
        std::fs::write(work.join("nested/deep.txt"), b"deep\n").unwrap();
        run(&work, &["add", "-A"]);
        run(
            &work,
            &["commit", "-m", "first", "--author=noa-test <t@noa.local>"],
        );
        run(&work, &["push", "origin", "master"]);
        origin
    }

    /// A one-commit bare origin containing a single marker file.
    fn make_origin(dir: &Path, name: &str, file: &str, content: &[u8]) -> PathBuf {
        let origin = dir.join(name);
        run(
            dir,
            &[
                "init",
                "--bare",
                "--initial-branch=master",
                origin.to_str().unwrap(),
            ],
        );
        let work = dir.join(format!("work-{name}"));
        std::fs::create_dir_all(&work).unwrap();
        run(&work, &["clone", origin.to_str().unwrap(), "."]);
        std::fs::write(work.join(file), content).unwrap();
        run(&work, &["add", "-A"]);
        run(
            &work,
            &["commit", "-m", "marker", "--author=noa-test <t@noa.local>"],
        );
        run(&work, &["push", "origin", "master"]);
        origin
    }

    fn run(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "noa-test")
            .env("GIT_AUTHOR_EMAIL", "t@noa.local")
            .env("GIT_COMMITTER_NAME", "noa-test")
            .env("GIT_COMMITTER_EMAIL", "t@noa.local")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    async fn engine_with_fixture(label: &str) -> (tempfile::TempDir, MirrorEngine, String) {
        let dir = tempfile::tempdir().unwrap();
        let origin = git_fixture(dir.path());
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let cfg = MirrorConfig {
            interval_secs: 900,
            scratch_dir: dir.path().join("scratch"),
            token_env: String::new(),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: label.to_string(),
                remote_url: format!("file://{}", origin.display()),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(db, cfg);
        (dir, engine, label.to_string())
    }

    #[tokio::test]
    async fn test_run_job_imports_and_verifies() {
        let (_d, engine, name) = engine_with_fixture("pilot").await;
        engine.run_job(&name).await.unwrap();
        let store = MirrorJobStore::new(Arc::clone(&engine.db)).unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        assert_eq!(job.last_status.as_deref(), Some("ok"));
        assert!(
            job.last_detail.unwrap().contains("master="),
            "detail should name the branch snapshot"
        );
        let refs = RedbRefStore::new(Arc::clone(&engine.db)).unwrap();
        assert!(refs.get("mirror/pilot/master").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_rerun_is_unchanged_and_chains_parents() {
        let (dir, engine, name) = engine_with_fixture("pilot").await;
        engine.run_job(&name).await.unwrap();
        let refs = RedbRefStore::new(Arc::clone(&engine.db)).unwrap();
        let first = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        engine.run_job(&name).await.unwrap();
        let second = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        assert_eq!(first, second, "skip_if_tree_unchanged must not advance");

        // Push a second commit; the new snapshot must chain to the first.
        let work = dir.path().join("work");
        std::fs::write(work.join("nested/new.txt"), b"second\n").unwrap();
        run(&work, &["add", "-A"]);
        run(
            &work,
            &["commit", "-m", "second", "--author=noa-test <t@noa.local>"],
        );
        run(&work, &["push", "origin", "master"]);
        engine.run_job(&name).await.unwrap();
        let third = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        assert_ne!(second, third);
        let snaps = RedbSnapshotStore::new(Arc::clone(&engine.db)).unwrap();
        let snap = snaps.get(&third).await.unwrap();
        assert_eq!(snap.parents, vec![second], "mirror snapshots must chain");
    }

    #[tokio::test]
    async fn test_url_update_repoints_existing_clone() {
        // R2 S1 regression: an upsert changing remote_url must repoint the
        // existing scratch clone instead of silently fetching the old one.
        let dir = tempfile::tempdir().unwrap();
        let origin_a = git_fixture(dir.path());
        let origin_b = make_origin(dir.path(), "b.git", "marker-b.txt", b"from B\n");
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let cfg = MirrorConfig {
            interval_secs: 900,
            scratch_dir: dir.path().join("scratch"),
            token_env: String::new(),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        let mk = |url: String| MirrorJob {
            name: "pilot".into(),
            remote_url: url,
            github_slug: "celestia-island/noa".into(),
            branches: vec!["master".into()],
            enabled: true,
            last_run_us: None,
            last_status: None,
            last_detail: None,
        };
        store
            .upsert(mk(format!("file://{}", origin_a.display())))
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        engine.run_job("pilot").await.unwrap();

        // Flip the job to origin B and rerun on the SAME scratch clone.
        store
            .upsert(mk(format!("file://{}", origin_b.display())))
            .await
            .unwrap();
        engine.run_job("pilot").await.unwrap();

        let refs = RedbRefStore::new(Arc::clone(&db)).unwrap();
        let id = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        let snaps = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let snap = snaps.get(&id).await.unwrap();
        let obj = RedbObjectStore::new(Arc::clone(&db)).unwrap();
        let tree = obj.get_tree(&TreeId(snap.tree_hash)).await.unwrap();
        assert!(
            tree.0.iter().any(|e| e.name == "marker-b.txt"),
            "replica must come from origin B after the URL update"
        );
    }

    #[tokio::test]
    async fn test_branch_failure_is_isolated() {
        // R2 S2 regression: one bad branch must not stop the others; the
        // job reports error with per-branch detail.
        let dir = tempfile::tempdir().unwrap();
        let origin = git_fixture(dir.path());
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let cfg = MirrorConfig {
            interval_secs: 900,
            scratch_dir: dir.path().join("scratch"),
            token_env: String::new(),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: "pilot".into(),
                remote_url: format!("file://{}", origin.display()),
                github_slug: "celestia-island/noa".into(),
                // Failing branch FIRST: an "abort remaining branches"
                // regression must fail this test (master would never import).
                branches: vec!["ghost".into(), "master".into()],
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        let result = engine.run_job("pilot").await;
        assert!(result.is_err(), "a failing branch must fail the job");

        let job = store.get("pilot").await.unwrap().unwrap();
        assert_eq!(job.last_status.as_deref(), Some("error"));
        let detail = job.last_detail.unwrap();
        assert!(detail.contains("ghost=FAILED"), "detail: {detail}");
        assert!(
            detail.contains("master=") && !detail.contains("master=FAILED"),
            "healthy branch must still import: {detail}"
        );
        let refs = RedbRefStore::new(Arc::clone(&db)).unwrap();
        assert!(
            refs.get("mirror/pilot/master").await.unwrap().is_some(),
            "master must have replicated despite ghost failing"
        );
    }

    #[tokio::test]
    async fn test_fidelity_detects_divergence() {
        let (dir, engine, name) = engine_with_fixture("pilot").await;
        engine.run_job(&name).await.unwrap();
        // Tamper: rebuild the snapshot tree with README.md pointing at
        // different bytes, store it, and verify — fidelity must fail and
        // name the path.
        let refs = RedbRefStore::new(Arc::clone(&engine.db)).unwrap();
        let snap_id = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        let snaps = RedbSnapshotStore::new(Arc::clone(&engine.db)).unwrap();
        let snap = snaps.get(&snap_id).await.unwrap();
        let obj = RedbObjectStore::new(Arc::clone(&engine.db)).unwrap();
        let tree = obj.get_tree(&TreeId(snap.tree_hash.clone())).await.unwrap();
        let tampered_id = obj.put_blob(b"# tampered\n").await.unwrap();
        let mut entries = tree.0.clone();
        for e in entries.iter_mut() {
            if e.name == "README.md" {
                e.id = tampered_id.0.clone();
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let tampered_tree = obj
            .put_tree(&crate::object::TreeEntries(entries))
            .await
            .unwrap();
        let mut tampered = snap.clone();
        tampered.tree_hash = tampered_tree.0.clone();
        tampered.id = crate::snapshot::content_addressed_snapshot_id_with_ts(
            &tampered.tree_hash,
            &tampered.parents,
            &tampered.workspace,
            &tampered.author,
            &tampered.message,
            tampered.timestamp,
        );
        snaps.store(&tampered).await.unwrap();

        let scratch = engine.cfg.scratch_dir.join("pilot.git");
        let oid = resolve_remote_branch(&scratch, "master").await.unwrap();
        let result = engine.verify_fidelity(&scratch, &oid, &tampered.id).await;
        assert!(result.is_err(), "tampered blob must fail fidelity");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("README.md"),
            "error should name the path: {msg}"
        );
        drop(dir);
    }

    #[test]
    fn test_git_blob_oid_matches_known_value() {
        // git hash-object - <<<"" => e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
        assert_eq!(
            git_blob_oid(b""),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        // git hash-object - <<<"hello\n" => ce013625030ba8dba906f756967f9e9ca394464a
        assert_eq!(
            git_blob_oid(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }
}
