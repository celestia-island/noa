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
    git::{
        import_git_commit, store_imported_entries, ImportOptions, ImportedBody, ImportedEntry,
        StoreOptions,
    },
    object::{EntryKind, ObjectStore, RedbObjectStore, TreeId},
    refs::{RedbRefStore, RefStore},
    server::{
        handlers::{err_json, ApiError},
        mirror_github::{ApiTreeEntry, GitDataClient},
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
    /// REST base for the Git Data API fallback lane (injectable for tests).
    pub github_api_base: String,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        MirrorConfig {
            interval_secs: 900,
            scratch_dir: PathBuf::from("mirror-cache"),
            token_env: "NOA_MIRROR_GH_TOKEN".to_string(),
            github_api_base: "https://api.github.com".to_string(),
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
            github_api_base: std::env::var("NOA_MIRROR_GH_API_BASE")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "https://api.github.com".to_string()),
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
    snapshot: SnapshotId,
    changed: bool,
    /// Full git commit oid the snapshot was imported from (canary push).
    commit_oid: String,
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
        let git_lane = self.ensure_scratch(job, &scratch).await;
        // W1a-3: when the git transport is dead (L168 family — node-2 today
        // resets github.com while api.github.com stays 200), the whole wave
        // falls back to importing through the Git Data API.
        let lane = match &git_lane {
            Ok(()) => "git",
            Err(e) => {
                tracing::warn!(
                    "mirror {}: git transport failed, importing via Git Data API: {e:#}",
                    job.name
                );
                // Self-heal: a half-written clone (killed mid-clone, disk
                // error) would otherwise pin every future wave to the API
                // lane forever. Dropping it makes the next wave try a fresh
                // clone instead of a doomed `remote set-url`.
                if scratch.exists() {
                    if let Err(rm) = std::fs::remove_dir_all(&scratch) {
                        tracing::warn!("mirror {}: scratch reset failed: {rm}", job.name);
                    }
                }
                "api"
            }
        };

        // Per-branch isolation (R2 S2): one bad branch (deleted upstream,
        // missing after fetch, fidelity failure) must not stop the others;
        // the job still reports error with per-branch detail.
        let mut notes = Vec::new();
        let mut outcomes: Vec<BranchOutcome> = Vec::new();
        let mut failed = 0usize;
        if job.tags {
            let tag_result = match lane {
                "git" => self.replicate_tags_git(job, &scratch).await,
                _ => self.replicate_tags_api(job).await,
            };
            match tag_result {
                Ok(names) if names.is_empty() => {}
                Ok(names) => notes.push(format!("tags={} ({})", names.len(), names.join(","))),
                Err(e) => {
                    failed += 1;
                    notes.push(format!("tags=FAILED: {e:#}"));
                }
            }
        }
        for branch in &job.branches {
            let result = match (lane, git_lane.as_ref()) {
                ("git", Ok(())) => self.replicate_branch(job, &scratch, branch).await,
                ("api", Err(_)) => self.replicate_branch_api(job, branch).await,
                // Unreachable pair kept explicit: lane tracks git_lane 1:1.
                _ => self.replicate_branch_api(job, branch).await,
            };
            match result {
                Ok(outcome) => {
                    notes.push(format!(
                        "{}={}({},{})",
                        outcome.branch,
                        outcome.snapshot.0.chars().take(12).collect::<String>(),
                        if outcome.changed {
                            "imported"
                        } else {
                            "unchanged"
                        },
                        lane
                    ));
                    outcomes.push(outcome);
                }
                Err(e) => {
                    failed += 1;
                    notes.push(format!("{branch}=FAILED: {e:#}"));
                }
            }
        }

        // Canary push (W1a-2): opt-in per job; pushes the first replicated
        // branch's commit to `noa-mirror-canary` on the remote — first via
        // git transport, and when that lane is dead (L168 TLS family) via
        // the Git Data API. A canary failure fails the job loudly.
        if job.canary_push {
            if let Some(outcome) = outcomes.first() {
                match self.push_canary(job, &scratch, outcome).await {
                    Ok(lane) => notes.push(format!("canary={lane}")),
                    Err(e) => {
                        failed += 1;
                        notes.push(format!("canary=FAILED: {e:#}"));
                    }
                }
            }
        }

        let summary = if failed == 0 {
            "fidelity ok".to_string()
        } else {
            format!("{failed} item(s) failed")
        };
        let detail = format!("{summary}; {}", notes.join(" "));
        if failed > 0 {
            anyhow::bail!("{detail}");
        }
        Ok(detail)
    }

    /// Canary ref name pushed to the remote (one per repo, per job would
    /// multiply noise without adding channel coverage).
    const CANARY_REF: &'static str = "noa-mirror-canary";

    async fn push_canary(
        &self,
        job: &MirrorJob,
        scratch: &Path,
        outcome: &BranchOutcome,
    ) -> Result<&'static str> {
        // Leading '+' forces the ref update: after any API-lane canary
        // commit (built outside master's ancestry) a plain push would be
        // non-FF-rejected forever, locking the probe onto the API lane
        // (Round A S2).
        let push_spec = format!("+{}:refs/heads/{}", outcome.commit_oid, Self::CANARY_REF);
        match self.git(scratch, &["push", "origin", &push_spec]).await {
            Ok(()) => return Ok("git"),
            Err(e) => {
                tracing::warn!(
                    "mirror {}: git-transport canary failed, trying Git Data API: {e:#}",
                    job.name
                );
            }
        }

        // Git Data API lane — needs a token and reuses the imported snapshot.
        let token = if self.cfg.token_env.is_empty() {
            None
        } else {
            std::env::var(&self.cfg.token_env)
                .ok()
                .filter(|t| !t.is_empty())
        };
        let token = token.ok_or_else(|| {
            anyhow::anyhow!(
                "canary API lane needs a token from {} (git transport already failed)",
                self.cfg.token_env
            )
        })?;
        let client = GitDataClient::new(&self.cfg.github_api_base, token)?;

        let snap_store = RedbSnapshotStore::new(Arc::clone(&self.db))?;
        let obj_store = RedbObjectStore::new(Arc::clone(&self.db))?;
        let snap = snap_store.get(&outcome.snapshot).await?;
        let tree = obj_store.get_tree(&TreeId(snap.tree_hash)).await?;

        // Delta against the existing canary tree when one is readable:
        // steady-state ticks upload only changed blobs (a full flat upload
        // per tick would burn the API budget on large repos — R2 S2).
        // `None` remote tree (no canary yet / unreadable / truncated) falls
        // back to the full one-time upload.
        let canary_ref_path = format!("heads/{}", Self::CANARY_REF);
        let existing_canary = client.get_ref(&job.github_slug, &canary_ref_path).await?;
        let mut remote_tree_sha: Option<String> = None;
        let mut remote: Option<std::collections::HashMap<String, (String, String)>> = None;
        if let Some(commit_sha) = &existing_canary {
            match client.get_commit_tree(&job.github_slug, commit_sha).await {
                Ok(tree_sha) => match client.get_tree_flat(&job.github_slug, &tree_sha).await {
                    Ok(Some(flat)) => {
                        remote = Some(flat.into_iter().map(|(p, m, s)| (p, (m, s))).collect());
                        remote_tree_sha = Some(tree_sha);
                    }
                    Ok(None) => tracing::warn!(
                        "mirror {}: canary tree listing truncated; full upload",
                        job.name
                    ),
                    Err(e) => tracing::warn!(
                        "mirror {}: canary tree fetch failed; full upload: {e:#}",
                        job.name
                    ),
                },
                Err(e) => tracing::warn!(
                    "mirror {}: canary commit fetch failed; full upload: {e:#}",
                    job.name
                ),
            }
        }

        let mut entries: Vec<ApiTreeEntry> = Vec::new();
        let mut noa_paths = std::collections::HashSet::new();
        for e in &tree.0 {
            noa_paths.insert(e.name.clone());
            if e.kind.has_blob_body() {
                let content = obj_store
                    .get_blob(&crate::object::BlobId(e.id.clone()))
                    .await?;
                let expected = git_blob_oid(&content);
                let mode = Self::git_mode_str(e.kind);
                if let Some((rm, rs)) = remote.as_ref().and_then(|r| r.get(&e.name)) {
                    if rm.as_str() == mode && rs.as_str() == expected.as_str() {
                        continue; // unchanged — inherited via base_tree
                    }
                }
                let sha = client.put_blob(&job.github_slug, &content).await?;
                // Integrity self-check: GitHub returns the sha1 content
                // address; a mismatch means corrupted upload.
                if sha != expected {
                    anyhow::bail!(
                        "canary blob sha mismatch at {:?}: uploaded {sha} != computed {expected}",
                        e.name
                    );
                }
                entries.push(ApiTreeEntry {
                    path: e.name.clone(),
                    mode,
                    kind: "blob",
                    sha: Some(sha),
                });
            } else if e.kind == EntryKind::Gitlink {
                // The oid names a commit in another repository; GitHub
                // accepts the reference verbatim.
                let mode = Self::git_mode_str(e.kind);
                if let Some((rm, rs)) = remote.as_ref().and_then(|r| r.get(&e.name)) {
                    if rm.as_str() == mode && rs.as_str() == e.id.as_str() {
                        continue;
                    }
                }
                entries.push(ApiTreeEntry {
                    path: e.name.clone(),
                    mode,
                    kind: "commit",
                    sha: Some(e.id.clone()),
                });
            }
        }
        // Deletions (only meaningful with a base_tree to delete from).
        if let Some(r) = &remote {
            for path in r.keys() {
                if !noa_paths.contains(path) {
                    entries.push(ApiTreeEntry {
                        path: path.clone(),
                        mode: "100644",
                        kind: "blob",
                        sha: None,
                    });
                }
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        if entries.is_empty() && remote_tree_sha.is_some() {
            // Everything unchanged and nothing to delete: the canary is
            // already current. Skipping avoids minting a fresh canary
            // commit every tick (~96/day at 900s) and the empty-tree edge
            // (Round A S3). The GETs above still proved the API lane.
            return Ok("api-unchanged");
        }
        let tree_sha = client
            .put_tree(&job.github_slug, remote_tree_sha.as_deref(), &entries)
            .await?;
        let parents = existing_canary.into_iter().collect::<Vec<_>>();
        let commit_sha = client
            .put_commit(
                &job.github_slug,
                &tree_sha,
                &parents,
                &format!(
                    "noa mirror canary {} @ {}",
                    job.name,
                    outcome.commit_oid.chars().take(12).collect::<String>()
                ),
            )
            .await?;
        client
            .upsert_ref(
                &job.github_slug,
                &format!("heads/{}", Self::CANARY_REF),
                &commit_sha,
            )
            .await?;
        Ok("api")
    }

    /// Clones or updates the bare scratch clone. `Err` = git transport
    /// unusable this wave (execute falls back to the API import lane).
    async fn ensure_scratch(&self, job: &MirrorJob, scratch: &Path) -> Result<()> {
        if scratch.exists() {
            // Re-point origin every run: a job whose remote_url was updated
            // (or a name reused after delete) must never keep fetching the
            // stale URL baked into an existing clone (R2 S1).
            self.git(scratch, &["remote", "set-url", "origin", &job.remote_url])
                .await?;
            self.git(
                scratch,
                &["fetch", "--prune", "--prune-tags", "--tags", "origin"],
            )
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
                scratch,
                &[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/*:refs/remotes/origin/*",
                ],
            )
            .await?;
            self.git(
                scratch,
                &["fetch", "--prune", "--prune-tags", "--tags", "origin"],
            )
            .await?;
        }
        Ok(())
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
            snapshot: snap,
            changed,
            commit_oid: oid,
        })
    }

    /// W1a-3: imports one branch entirely through the Git Data API — no git
    /// transport involved. Per-blob ingest fidelity: the declared oid must
    /// equal the locally recomputed sha1 of the fetched content.
    async fn replicate_branch_api(&self, job: &MirrorJob, branch: &str) -> Result<BranchOutcome> {
        let client = self.api_client()?;
        let commit_sha = client
            .get_ref(&job.github_slug, &format!("heads/{branch}"))
            .await?
            .ok_or_else(|| anyhow::anyhow!("branch '{branch}' missing on remote (api) — a 404 also means the token lacks access on private repos"))?;
        let ref_name = format!("mirror/{}/{}", job.name, branch);
        let ws_name = format!("mirror.{}.{}", job.name, branch.replace('/', "__"));
        let (snap, changed) = self
            .import_api_commit(&client, job, &commit_sha, &ref_name, &ws_name, branch)
            .await?;
        Ok(BranchOutcome {
            branch: branch.to_string(),
            snapshot: snap,
            changed,
            commit_oid: commit_sha,
        })
    }

    /// Builds the Git Data API client (empty token = anonymous).
    fn api_client(&self) -> Result<GitDataClient> {
        let token = if self.cfg.token_env.is_empty() {
            String::new()
        } else {
            std::env::var(&self.cfg.token_env)
                .ok()
                .filter(|t| !t.is_empty())
                .unwrap_or_default()
        };
        GitDataClient::new(&self.cfg.github_api_base, token)
    }

    /// Imports one remote commit (by oid) through the Git Data API into
    /// `ref_name` / `ws_name`. Shared by the branch and tag lanes.
    async fn import_api_commit(
        &self,
        client: &GitDataClient,
        job: &MirrorJob,
        commit_sha: &str,
        ref_name: &str,
        ws_name: &str,
        label: &str,
    ) -> Result<(SnapshotId, bool)> {
        let (tree_sha, author, message, seconds) = client
            .get_commit_facts(&job.github_slug, commit_sha)
            .await?;
        let flat = client
            .get_tree_flat(&job.github_slug, &tree_sha)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "tree listing truncated for '{label}' (api import needs the full listing)"
                )
            })?;

        let ref_store = RedbRefStore::new(Arc::clone(&self.db))?;
        let prev = ref_store.get(ref_name).await?;

        // Read-side delta (R2 S2): compare against the previous snapshot's
        // entries and re-verify locally — unchanged blobs are NOT re-fetched
        // (a full per-wave re-download would burn the API budget on exactly
        // the degraded days this lane exists for).
        let obj_store = RedbObjectStore::new(Arc::clone(&self.db))?;
        let snap_store = RedbSnapshotStore::new(Arc::clone(&self.db))?;
        let prev_entries: Option<std::collections::HashMap<String, crate::object::TreeEntry>> =
            match &prev {
                Some(prev_id) => match snap_store.get(prev_id).await {
                    Ok(s) => obj_store
                        .get_tree(&TreeId(s.tree_hash))
                        .await
                        .ok()
                        .map(|t| t.0.into_iter().map(|e| (e.name.clone(), e)).collect()),
                    Err(_) => None,
                },
                None => None,
            };

        let mut imported = Vec::with_capacity(flat.len());
        for (path, mode, sha) in flat {
            let kind = match mode.as_str() {
                "100644" => EntryKind::Blob,
                "100755" => EntryKind::Executable,
                "120000" => EntryKind::Symlink,
                "160000" => EntryKind::Gitlink,
                other => anyhow::bail!("api import: unknown git mode {other:?} at {path:?}"),
            };
            if kind == EntryKind::Gitlink {
                // A gitlink is just the pointer oid (a commit in another
                // repository) — no content to fetch, pass it through.
                imported.push(ImportedEntry {
                    path,
                    kind: EntryKind::Gitlink,
                    body: ImportedBody::Gitlink(sha),
                });
                continue;
            }
            let mut content: Option<Vec<u8>> = None;
            if let Some(pe) = prev_entries.as_ref().and_then(|m| m.get(&path)) {
                if pe.kind == kind && kind.has_blob_body() {
                    if let Ok(local) = obj_store
                        .get_blob(&crate::object::BlobId(pe.id.clone()))
                        .await
                    {
                        if git_blob_oid(&local) == sha {
                            content = Some(local);
                        }
                    }
                }
            }
            let content = match content {
                Some(c) => c,
                None => client.get_blob(&job.github_slug, &sha).await?,
            };
            let computed = git_blob_oid(&content);
            if computed != sha {
                anyhow::bail!(
                    "api import: blob oid mismatch at {path:?}: declared {sha} != computed {computed}"
                );
            }
            imported.push(ImportedEntry {
                path,
                kind,
                body: ImportedBody::Blob(content),
            });
        }

        let author = if author.is_empty() {
            "noa-mirror-api".to_string()
        } else {
            author
        };
        let message = if message.is_empty() {
            format!(
                "mirror via git-data-api @ {}",
                commit_sha.chars().take(12).collect::<String>()
            )
        } else {
            message
        };
        let snap = store_imported_entries(
            Arc::clone(&self.db),
            imported,
            &StoreOptions {
                workspace: ws_name.to_string(),
                head_ref: ref_name.to_string(),
                parents: prev.clone().into_iter().collect(),
                skip_if_tree_unchanged: true,
                author,
                message,
                timestamp_us: seconds * 1_000_000,
            },
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("api import produced no snapshot for '{label}'"))?;
        let changed = prev.as_ref() != Some(&snap);
        Ok((snap, changed))
    }

    /// Replicates every remote tag via the git lane. Annotated tags are
    /// peeled to their commit (`%(*objectname)`); lightweight tags point at
    /// the commit directly. Each lands as `mirror/<job>/tags/<tag>`.
    async fn replicate_tags_git(&self, job: &MirrorJob, scratch: &Path) -> Result<Vec<String>> {
        let out = self
            .git_stdout(
                scratch,
                &[
                    "for-each-ref",
                    "--format=%(objecttype) %(*objectname) %(objectname) %(refname:lstrip=2)",
                    "refs/tags",
                ],
            )
            .await?;
        let mut imported = Vec::new();
        let mut tag_failures: Vec<String> = Vec::new();
        // The reconcile set is the FULL upstream listing (Round F S2):
        // a transiently failing tag is still present upstream and must NOT
        // be dropped from the mirror. Pushed AFTER the defensive `continue`s
        // so garbage names can never shield a mirror ref from deletion.
        let mut upstream_names: Vec<String> = Vec::new();
        for line in out.lines() {
            let mut parts = line.splitn(4, ' ');
            let (Some(objtype), Some(peeled), Some(objname), Some(name)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            upstream_names.push(name.to_string());
            let commit_oid = if objtype == "tag" && !peeled.is_empty() {
                peeled.to_string()
            } else {
                objname.to_string()
            };
            // Per-tag isolation: one odd tag (pointing at a blob/tree, or a
            // nested tag) must not stop the rest, and the error must name it
            // (Round A S2 — the branch lane already behaves this way).
            match self
                .import_git_commit_at(
                    scratch,
                    &commit_oid,
                    &format!("mirror/{}/tags/{}", job.name, name),
                    &format!("mirror.{}.tag.{}", job.name, name.replace('/', "__")),
                )
                .await
            {
                Ok(_) => imported.push(name.to_string()),
                Err(e) => tag_failures.push(format!("{name}: {e:#}")),
            }
        }
        imported.sort();
        self.reconcile_tag_refs(job, &upstream_names).await?;
        if !tag_failures.is_empty() {
            anyhow::bail!(
                "{} tag(s) failed: {}",
                tag_failures.len(),
                tag_failures.join("; ")
            );
        }
        Ok(imported)
    }

    /// Replicates every remote tag through the Git Data API (annotated tags
    /// peeled via `/git/tags/{sha}`).
    async fn replicate_tags_api(&self, job: &MirrorJob) -> Result<Vec<String>> {
        let client = self.api_client()?;
        let tags = client.list_tags(&job.github_slug).await?;
        let mut imported = Vec::new();
        let mut tag_failures: Vec<String> = Vec::new();
        // Reconcile set = the full upstream listing (Round F S2).
        let upstream_names: Vec<String> = tags.iter().map(|(n, _)| n.clone()).collect();
        for (name, commit_oid) in tags {
            // list_tags defers peel failures into this marker (Round E:
            // inline `?` collapsed the whole listing into one unnamed
            // failure) — surface it as a named per-tag failure.
            if let Some(peel_err) = commit_oid.strip_prefix("__peel_failed__: ") {
                tag_failures.push(format!("{name}: peel failed: {peel_err}"));
                continue;
            }
            match self
                .import_api_commit(
                    &client,
                    job,
                    &commit_oid,
                    &format!("mirror/{}/tags/{}", job.name, name),
                    &format!("mirror.{}.tag.{}", job.name, name.replace('/', "__")),
                    &format!("tag {name}"),
                )
                .await
            {
                Ok(_) => imported.push(name.clone()),
                Err(e) => tag_failures.push(format!("{name}: {e:#}")),
            }
        }
        imported.sort();
        // Reconcile BEFORE the failure bail, and against the FULL upstream
        // listing (not the imported subset): a transiently failing tag is
        // still present upstream and must NOT be dropped from the mirror
        // (Round F S2).
        self.reconcile_tag_refs(job, &upstream_names).await?;
        if !tag_failures.is_empty() {
            anyhow::bail!(
                "{} tag(s) failed: {}",
                tag_failures.len(),
                tag_failures.join("; ")
            );
        }
        Ok(imported)
    }

    /// Deletes `mirror/<job>/tags/<name>` refs whose tag no longer exists
    /// upstream. `--prune --prune-tags` only cleans the scratch clone; the
    /// mirror store must follow or upstream-deleted tags keep serving from
    /// the authority set forever (Round D F1 / Round E S1).
    async fn reconcile_tag_refs(&self, job: &MirrorJob, live: &[String]) -> Result<()> {
        let prefix = format!("mirror/{}/tags/", job.name);
        let ref_store = RedbRefStore::new(Arc::clone(&self.db))?;
        let mut removed = Vec::new();
        for (name, _) in ref_store.list().await? {
            let stale = name
                .strip_prefix(&prefix)
                .is_some_and(|tag| !live.iter().any(|l| l == tag));
            if stale && ref_store.delete(&name).await? {
                removed.push(name.strip_prefix(&prefix).unwrap_or(&name).to_string());
            }
        }
        if !removed.is_empty() {
            tracing::info!("mirror {}: pruned stale tag refs {removed:?}", job.name);
        }
        Ok(())
    }

    /// Imports one git-lane commit into an explicit ref/workspace.
    async fn import_git_commit_at(
        &self,
        scratch: &Path,
        commit_oid: &str,
        ref_name: &str,
        ws_name: &str,
    ) -> Result<SnapshotId> {
        let ref_store = RedbRefStore::new(Arc::clone(&self.db))?;
        let prev = ref_store.get(ref_name).await?;
        let opts = ImportOptions {
            workspace: ws_name.to_string(),
            head_ref: ref_name.to_string(),
            commit_hex: Some(commit_oid.to_string()),
            parents: prev.iter().cloned().collect(),
            skip_if_tree_unchanged: true,
        };
        import_git_commit(scratch, Arc::clone(&self.db), &opts)
            .await?
            .ok_or_else(|| anyhow::anyhow!("tag commit '{commit_oid}' not found after fetch"))
    }

    /// Runs git and returns trimmed stdout.
    async fn git_stdout(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        let cfg = self.cfg.clone();
        let cwd = cwd.to_path_buf();
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let args_for_error = args.join(" ");
        let out = tokio::task::spawn_blocking(move || -> std::io::Result<std::process::Output> {
            let mut cmd = Command::new("git");
            cmd.args([
                "-c",
                "http.lowSpeedLimit=1000",
                "-c",
                "http.lowSpeedTime=30",
            ]);
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
                let _ = std::fs::create_dir_all(cfg.scratch_dir.as_path());
                let _ = std::fs::write(
                    &askpass,
                    "#!/bin/sh\nprintf '%s\\n' \"$NOA_MIRROR_FETCH_TOKEN\"\n",
                );
                use std::os::unix::fs::PermissionsExt;
                if let Ok(md) = std::fs::metadata(&askpass) {
                    let mut p = md.permissions();
                    p.set_mode(0o700);
                    let _ = std::fs::set_permissions(&askpass, p);
                }
                cmd.env("GIT_ASKPASS", &askpass)
                    .env("NOA_MIRROR_FETCH_TOKEN", token);
            }
            cmd.output()
        })
        .await??;
        if !out.status.success() {
            anyhow::bail!(
                "git {args_for_error} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// GitHub tree-entry mode string for an entry kind.
    fn git_mode_str(kind: EntryKind) -> &'static str {
        match kind {
            EntryKind::Blob => "100644",
            EntryKind::Executable => "100755",
            EntryKind::Symlink => "120000",
            EntryKind::Gitlink => "160000",
            EntryKind::Tree => "040000",
        }
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
            // Global -c flags MUST precede the subcommand.
            cmd.args([
                "-c",
                "http.lowSpeedLimit=1000",
                "-c",
                "http.lowSpeedTime=30",
            ]);
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
            // Lockstep: the repo catalog entry goes with the job, otherwise
            // the next upsert re-registers it (R2 S2 delete asymmetry).
            // EXCEPT when open PRs still reference the name: job deletion
            // must never be blocked, but dropping the row would leave those
            // tickets addressable only through the flat route. Keep the row
            // (and say so) — DELETE /api/v1/repos/{name} is the guarded path.
            let has_open = match state.pr_store.list(Some(&name), None, Some("open")).await {
                Ok(prs) => !prs.is_empty(),
                // Fail CLOSED (keep the row) when open PRs cannot be
                // enumerated: dropping the row on an unknown is exactly the
                // orphaning this guard exists to prevent. Job deletion itself
                // still succeeds either way.
                Err(e) => {
                    tracing::warn!(
                        "pr store unavailable during job delete ({e}); \
                         keeping the repo catalog row"
                    );
                    true
                }
            };
            if has_open {
                tracing::warn!(
                    "mirror job {name} deleted but its repo catalog row is kept: \
                     open PRs still reference it"
                );
            } else if let Err(e) = state.repo_registry.delete(&name).await {
                tracing::warn!("repo catalog delete for {name} failed: {e}");
            }
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
        // Global -c flags MUST precede the subcommand; ambient signing
        // config must not break fixtures (Round A S3).
        let out = Command::new("git")
            .args(["-c", "tag.gpgSign=false", "-c", "commit.gpgsign=false"])
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
            github_api_base: String::new(),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: label.to_string(),
                remote_url: format!("file://{}", origin.display()),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                tags: false,
                canary_push: false,
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
            github_api_base: String::new(),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        let mk = |url: String| MirrorJob {
            name: "pilot".into(),
            remote_url: url,
            github_slug: "celestia-island/noa".into(),
            branches: vec!["master".into()],
            tags: false,
            canary_push: false,
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
            github_api_base: String::new(),
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
                tags: false,
                canary_push: false,
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

    /// Runs git capturing stdout (trimmed) — for asserting remote refs.
    fn run_out(cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn make_read_only(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        for entry in walkdir(dir) {
            if entry.is_dir() {
                std::fs::set_permissions(entry, std::fs::Permissions::from_mode(0o555)).unwrap();
            }
        }
    }

    fn walkdir(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = vec![dir.to_path_buf()];
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let e = e.unwrap().path();
                if e.is_dir() {
                    stack.push(e.clone());
                    out.push(e);
                }
            }
        }
        out
    }

    #[tokio::test]
    async fn test_canary_push_git_transport() {
        let (dir, engine, name) = engine_with_fixture("pilot").await;
        let store = MirrorJobStore::new(Arc::clone(&engine.db)).unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        store
            .upsert(MirrorJob {
                canary_push: true,
                ..job
            })
            .await
            .unwrap();
        engine.run_job(&name).await.unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        assert!(
            job.last_detail.unwrap().contains("canary=git"),
            "detail should record the git lane"
        );
        let origin = dir.path().join("origin.git");
        let canary = run_out(&origin, &["rev-parse", "refs/heads/noa-mirror-canary"]);
        let master = run_out(&origin, &["rev-parse", "refs/heads/master"]);
        assert_eq!(canary, master, "canary ref must point at master's commit");

        // Rewrite history upstream (amend + force push) so the next canary
        // push is NON-fast-forward: the leading '+' in the push spec is the
        // only thing that keeps the git lane usable (Round A S2 pin).
        let work = dir.path().join("work");
        std::fs::write(work.join("README.md"), b"# fixture amended\n").unwrap();
        run(&work, &["add", "-A"]);
        run(
            &work,
            &[
                "commit",
                "--amend",
                "-m",
                "first-amended",
                "--author=noa-test <t@noa.local>",
            ],
        );
        run(&work, &["push", "--force", "origin", "master"]);
        engine.run_job(&name).await.unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        let detail = job.last_detail.unwrap();
        assert!(
            detail.contains("canary=git"),
            "non-FF canary must still ride the git lane: {detail}"
        );
        let canary2 = run_out(&origin, &["rev-parse", "refs/heads/noa-mirror-canary"]);
        let master2 = run_out(&origin, &["rev-parse", "refs/heads/master"]);
        assert_ne!(master2, master, "upstream must have actually moved");
        assert_eq!(canary2, master2, "canary must follow the rewritten master");
    }

    #[tokio::test]
    async fn test_canary_falls_back_to_git_data_api() {
        use axum::routing::post;
        use axum::Router;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (dir, engine, name) = engine_with_fixture("pilot").await;
        engine.run_job(&name).await.unwrap();

        // Kill the git-transport push lane: origin becomes read-only (fetch
        // still works; push cannot create objects).
        let origin = dir.path().join("origin.git");
        make_read_only(&origin);

        // Git Data API mock.
        let calls = Arc::new(AtomicUsize::new(0));
        let c1 = Arc::clone(&calls);
        let c2 = Arc::clone(&calls);
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/blobs",
                post(move |axum::Json(b): axum::Json<serde_json::Value>| {
                    let c1 = Arc::clone(&c1);
                    async move {
                        c1.fetch_add(1, Ordering::SeqCst);
                        use base64::Engine;
                        let content = base64::engine::general_purpose::STANDARD
                            .decode(b["content"].as_str().unwrap())
                            .unwrap();
                        axum::Json(serde_json::json!({ "sha": git_blob_oid(&content) }))
                    }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees",
                post(move |axum::Json(b): axum::Json<serde_json::Value>| {
                    c2.fetch_add(1, Ordering::SeqCst);
                    assert!(
                        b["tree"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
                        "tree payload must carry entries"
                    );
                    async { axum::Json(serde_json::json!({ "sha": "api-tree-sha" })) }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits",
                post(|| async { axum::Json(serde_json::json!({ "sha": "api-commit-sha" })) }),
            )
            .route(
                "/repos/{owner}/{repo}/git/ref/heads/noa-mirror-canary",
                axum::routing::get(|| async { axum::http::StatusCode::NOT_FOUND }),
            )
            .route(
                "/repos/{owner}/{repo}/git/refs/heads/noa-mirror-canary",
                axum::routing::patch(|| async { axum::http::StatusCode::NOT_FOUND }),
            )
            .route(
                "/repos/{owner}/{repo}/git/refs",
                post(|| async {
                    axum::Json(serde_json::json!({ "object": { "sha": "api-commit-sha" } }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // Engine config with the mock API base + a token env var.
        const TOKEN_VAR: &str = "NOA_MIRROR_TEST_TOKEN_W1A2";
        std::env::set_var(TOKEN_VAR, "test-token");
        let store = MirrorJobStore::new(Arc::clone(&engine.db)).unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        store
            .upsert(MirrorJob {
                canary_push: true,
                ..job
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(
            Arc::clone(&engine.db),
            MirrorConfig {
                scratch_dir: engine.cfg.scratch_dir.clone(),
                token_env: TOKEN_VAR.to_string(),
                github_api_base: format!("http://{addr}"),
                ..engine.cfg.clone()
            },
        );
        engine.run_job(&name).await.unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        let detail = job.last_detail.unwrap();
        assert!(
            detail.contains("canary=api"),
            "detail should record the API lane: {detail}"
        );
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "blob+tree endpoints must have been called"
        );
        std::env::remove_var(TOKEN_VAR);
    }

    /// Builds an axum mock serving the Git Data API for one branch head
    /// `c1` whose tree t1 carries the given `(path, mode, content)` files.
    /// `corrupt` flips one declared sha to force an ingest-fidelity error.
    async fn api_import_mock(
        corrupt: bool,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use axum::routing::{get as rget, post};
        use axum::Router;
        let gets = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gets_for_route = std::sync::Arc::clone(&gets);
        let files: Vec<(&str, &str, Vec<u8>)> = vec![
            ("README.md", "100644", b"# api fixture\n".to_vec()),
            ("run.sh", "100755", b"echo hi\n".to_vec()),
            ("link", "120000", b"README.md".to_vec()),
        ];
        let mut listing: Vec<serde_json::Value> = files
            .iter()
            .map(|(p, m, c)| {
                let mut sha = git_blob_oid(c);
                if corrupt && *p == "run.sh" {
                    sha = "0".repeat(40);
                }
                serde_json::json!({ "path": p, "mode": m, "sha": sha, "type": "blob" })
            })
            .collect();
        // gitlink row: pointer oid, no blob fetch expected.
        listing.push(serde_json::json!({
            "path": "sub", "mode": "160000", "sha": "a1b2c3d4e5f6", "type": "commit"
        }));
        let blob_files = files.clone();
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/ref/heads/master",
                rget(|| async {
                    axum::Json(serde_json::json!({ "object": { "sha": "c1" } }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits/c1",
                rget(|| async {
                    axum::Json(serde_json::json!({
                        "tree": { "sha": "t1" },
                        "author": { "name": "noa-test" },
                        "committer": { "date": "2026-11-06T12:00:00-08:00" },
                        "message": "api fixture commit"
                    }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees/t1",
                rget(move || {
                    let listing = listing.clone();
                    async move {
                        axum::Json(serde_json::json!({
                            "tree": listing, "truncated": false
                        }))
                    }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/blobs/{sha}",
                rget(move |axum::extract::Path((_o, _r, sha)): axum::extract::Path<(String, String, String)>| {
                    let files = blob_files.clone();
                    let gets_for_route = std::sync::Arc::clone(&gets_for_route);
                    async move {
                        gets_for_route.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        for (_, _, c) in &files {
                            if git_blob_oid(c) == sha {
                                use base64::Engine;
                                return axum::Json(serde_json::json!({
                                    "content": base64::engine::general_purpose::STANDARD.encode(c),
                                    "encoding": "base64"
                                }));
                            }
                        }
                        // Unknown sha: serve the first file's bytes so
                        // the client's oid self-check fires (corrupt case).
                        use base64::Engine;
                        let (_, _, c) = &files[0];
                        axum::Json(serde_json::json!({
                            "content": base64::engine::general_purpose::STANDARD.encode(c),
                            "encoding": "base64"
                        }))
                    }
                }),
            )
            // blobs POST kept for potential canary wiring (unused here).
            .route(
                "/repos/{owner}/{repo}/git/blobs",
                post(|| async { axum::Json(serde_json::json!({ "sha": "x" })) }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), gets)
    }

    #[tokio::test]
    async fn test_api_import_lane_when_git_transport_dead() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let (base, blob_gets) = api_import_mock(false).await;
        let cfg = MirrorConfig {
            interval_secs: 900,
            scratch_dir: dir.path().join("scratch"),
            token_env: String::new(),
            github_api_base: base.clone(),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: "pilot".into(),
                remote_url: "file:///nonexistent-noa-mirror-fixture".into(),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                tags: false,
                canary_push: false,
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        engine.run_job("pilot").await.unwrap();

        let job = store.get("pilot").await.unwrap().unwrap();
        let detail = job.last_detail.unwrap();
        assert!(
            detail.contains("master=noa_") && detail.contains(",api)"),
            "detail should record the api lane: {detail}"
        );
        let refs = RedbRefStore::new(Arc::clone(&db)).unwrap();
        let first = refs.get("mirror/pilot/master").await.unwrap().unwrap();

        // Snapshot metadata comes from the API commit (kills date/author
        // fallback mutations).
        let snaps = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let snap = snaps.get(&first).await.unwrap();
        assert_eq!(snap.author, "noa-test");
        assert_eq!(snap.message, "api fixture commit");
        assert_eq!(
            snap.timestamp, 1_793_995_200_000_000,
            "timestamp must honour the commit's -08:00 offset (Round A S2 pin)"
        );
        let obj = RedbObjectStore::new(Arc::clone(&db)).unwrap();
        let tree = obj.get_tree(&TreeId(snap.tree_hash)).await.unwrap();
        let run = tree.0.iter().find(|e| e.name == "run.sh").unwrap();
        assert_eq!(
            run.kind,
            EntryKind::Executable,
            "100755 must map to Executable"
        );
        let link = tree.0.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        let sub = tree.0.iter().find(|e| e.name == "sub").unwrap();
        assert_eq!(sub.kind, EntryKind::Gitlink);
        assert_eq!(sub.id, "a1b2c3d4e5f6");
        let first_wave_gets = blob_gets.load(std::sync::atomic::Ordering::SeqCst);

        // Second run: unchanged via api — skip-if-unchanged holds AND the
        // delta skip means zero blob re-fetches (R2 S2 pin).
        engine.run_job("pilot").await.unwrap();
        assert_eq!(
            refs.get("mirror/pilot/master").await.unwrap().unwrap(),
            first,
            "api rerun must not advance on unchanged tree"
        );
        assert_eq!(
            blob_gets.load(std::sync::atomic::Ordering::SeqCst),
            first_wave_gets,
            "unchanged blobs must not be re-fetched"
        );
        // Round A S2 pin: a steady-state rerun must report "unchanged", not
        // keep claiming "imported" forever (the refactor hardcoded true).
        let detail = store
            .get("pilot")
            .await
            .unwrap()
            .unwrap()
            .last_detail
            .unwrap();
        assert!(
            detail.contains("(unchanged,api)"),
            "steady-state api rerun must report unchanged: {detail}"
        );
    }

    #[tokio::test]
    async fn test_api_import_chains_parents_on_changed_wave() {
        // Round D D6: unlike the git lane, the API lane had no changed-wave
        // test, so `parents: vec![]` survived. Two waves with different
        // content must chain snapshots.
        use axum::routing::get as rget;
        use axum::Router;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let wave = Arc::new(AtomicUsize::new(0));
        let wave_ref = Arc::clone(&wave);
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/ref/heads/master",
                rget(move || {
                    let n = wave_ref.fetch_add(1, Ordering::SeqCst);
                    async move {
                        let sha = if n == 0 { "c1" } else { "c2" };
                        axum::Json(serde_json::json!({ "object": { "sha": sha } }))
                    }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits/c1",
                rget(|| async {
                    axum::Json(serde_json::json!({
                        "tree": { "sha": "t1" },
                        "author": { "name": "t" },
                        "committer": { "date": "2026-11-06T00:00:00Z" },
                        "message": "one"
                    }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits/c2",
                rget(|| async {
                    axum::Json(serde_json::json!({
                        "tree": { "sha": "t2" },
                        "author": { "name": "t" },
                        "committer": { "date": "2026-11-06T01:00:00Z" },
                        "message": "two"
                    }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees/t1",
                rget(|| async {
                    let sha = git_blob_oid(b"wave one\n");
                    axum::Json(serde_json::json!({
                        "tree": [{ "path": "f.txt", "mode": "100644", "sha": sha, "type": "blob" }],
                        "truncated": false
                    }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees/t2",
                rget(|| async {
                    let sha = git_blob_oid(b"wave two\n");
                    axum::Json(serde_json::json!({
                        "tree": [{ "path": "f.txt", "mode": "100644", "sha": sha, "type": "blob" }],
                        "truncated": false
                    }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/blobs/{sha}",
                rget(
                    move |axum::extract::Path((_o, _r, sha)): axum::extract::Path<(
                        String,
                        String,
                        String,
                    )>| {
                        async move {
                            let body = if sha == git_blob_oid(b"wave one\n") {
                                b"wave one\n".to_vec()
                            } else {
                                b"wave two\n".to_vec()
                            };
                            use base64::Engine;
                            axum::Json(serde_json::json!({
                                "content": base64::engine::general_purpose::STANDARD.encode(body),
                                "encoding": "base64"
                            }))
                        }
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let dir = tempfile::tempdir().unwrap();
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
            github_api_base: format!("http://{addr}"),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: "pilot".into(),
                remote_url: "file:///nonexistent-noa-mirror-fixture".into(),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                tags: false,
                canary_push: false,
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        engine.run_job("pilot").await.unwrap();
        let refs = RedbRefStore::new(Arc::clone(&db)).unwrap();
        let first = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        engine.run_job("pilot").await.unwrap();
        let second = refs.get("mirror/pilot/master").await.unwrap().unwrap();
        assert_ne!(first, second, "changed content must advance the ref");
        let snaps = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let snap = snaps.get(&second).await.unwrap();
        assert_eq!(snap.parents, vec![first], "api wave 2 must chain to wave 1");
    }

    #[tokio::test]
    async fn test_git_lane_replicates_tags() {
        let (dir, engine, name) = engine_with_fixture("pilot").await;
        // Lightweight + annotated tag on the fixture's master commit, a
        // same-named BRANCH (refname disambiguation pin), and a tag pointing
        // at a BLOB (per-tag failure isolation pin).
        let work = dir.path().join("work");
        run(&work, &["tag", "v-light"]);
        run(&work, &["tag", "-a", "v-annot", "-m", "annotated tag"]);
        run(&work, &["push", "origin", "--tags"]);
        run(&work, &["branch", "v-light"]);
        run(&work, &["push", "origin", "refs/heads/v-light"]);
        std::fs::write(work.join("blob.txt"), b"just a blob\n").unwrap();
        let blob_sha = run_out(&work, &["hash-object", "-w", "blob.txt"]);
        run(&work, &["tag", "v-blob", &blob_sha]);
        run(&work, &["push", "origin", "v-blob"]);

        let store = MirrorJobStore::new(Arc::clone(&engine.db)).unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        store.upsert(MirrorJob { tags: true, ..job }).await.unwrap();
        // v-blob fails the job; healthy tags must still land.
        assert!(
            engine.run_job(&name).await.is_err(),
            "a tag pointing at a blob must fail the job"
        );

        let refs = RedbRefStore::new(Arc::clone(&engine.db)).unwrap();
        let light = refs
            .get("mirror/pilot/tags/v-light")
            .await
            .unwrap()
            .expect("lightweight tag imported");
        let annot = refs
            .get("mirror/pilot/tags/v-annot")
            .await
            .unwrap()
            .expect("annotated tag imported");
        // Snapshot ids embed the workspace name (they must differ); the
        // CONTENT is what must match — compare tree hashes.
        let snaps = RedbSnapshotStore::new(Arc::clone(&engine.db)).unwrap();
        assert_eq!(
            snaps.get(&light).await.unwrap().tree_hash,
            snaps.get(&annot).await.unwrap().tree_hash,
            "both tags point at the same commit => same tree hash"
        );
        let detail = store
            .get(&name)
            .await
            .unwrap()
            .unwrap()
            .last_detail
            .unwrap();
        assert!(
            detail.contains("v-blob"),
            "the failing tag must be named (isolation pins it separately)"
        );
        assert!(
            detail.contains("tags=FAILED"),
            "a failing tag must fail the job: {detail}"
        );
        assert!(
            detail.contains("v-blob"),
            "the failing tag must be named: {detail}"
        );

        // Same-named branch (Round B S1): lstrip=2 keeps the tag at
        // mirror/pilot/tags/v-light, not .../tags/tags/v-light.

        // Round D F1 / Round E S1: an upstream-DELETED tag must be
        // reconciled away from the mirror store (fetch pruning only cleans
        // the scratch clone; the ref delete is the real contract).
        run(&work, &["push", "origin", "--delete", "v-annot"]);
        assert!(
            engine.run_job(&name).await.is_err(),
            "v-blob still fails the job"
        );
        assert!(
            refs.get("mirror/pilot/tags/v-annot")
                .await
                .unwrap()
                .is_none(),
            "upstream-deleted tag must not survive in the mirror store"
        );
        assert!(
            refs.get("mirror/pilot/tags/v-light")
                .await
                .unwrap()
                .is_some(),
            "the tag ref must persist across the reconcile wave"
        );
    }

    #[tokio::test]
    async fn test_api_lane_replicates_tags_with_peel() {
        use axum::routing::get as rget;
        use axum::Router;
        let content = b"tagged\n".to_vec();
        let blob_sha = git_blob_oid(&content);
        let tree_json = serde_json::json!({
            "tree": [{ "path": "f.txt", "mode": "100644", "sha": blob_sha, "type": "blob" }],
            "truncated": false
        });
        let commit_json = serde_json::json!({
            "tree": { "sha": "tagtree" },
            "author": { "name": "t" },
            "committer": { "date": "2026-11-06T00:00:00Z" },
            "message": "tagged commit"
        });
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/refs/tags",
                rget(|| async {
                    axum::Json(serde_json::json!([
                        { "ref": "refs/tags/v-broken", "object": { "sha": "missing1", "type": "commit" } },
                        { "ref": "refs/tags/v-badpeel", "object": { "sha": "tagobj404", "type": "tag" } },
                        { "ref": "refs/tags/v-light", "object": { "sha": "commit1", "type": "commit" } },
                        { "ref": "refs/tags/v-annot", "object": { "sha": "tagobj1", "type": "tag" } }
                    ]))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/tags/tagobj1",
                rget(|| async {
                    axum::Json(serde_json::json!({ "object": { "sha": "commit1", "type": "commit" } }))
                }),
            )
            // peel failure whose BODY names the tag (real API errors carry
            // context in the message body).
            .route(
                "/repos/{owner}/{repo}/git/tags/tagobj404",
                rget(|| async {
                    axum::Json(serde_json::json!({
                        "object": { "sha": "peel-broke (tag v-badpeel)", "type": "commit" }
                    }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits/commit1",
                rget(move || {
                    let commit_json = commit_json.clone();
                    async move { axum::Json(commit_json) }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees/tagtree",
                rget(move || {
                    let tree_json = tree_json.clone();
                    async move { axum::Json(tree_json) }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/blobs/{sha}",
                rget(move || {
                    let content = content.clone();
                    async move {
                        use base64::Engine;
                        axum::Json(serde_json::json!({
                            "content": base64::engine::general_purpose::STANDARD.encode(content),
                            "encoding": "base64"
                        }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let dir = tempfile::tempdir().unwrap();
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
            github_api_base: format!("http://{addr}"),
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: "pilot".into(),
                remote_url: "file:///nonexistent-noa-mirror-fixture".into(),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                tags: true,
                canary_push: false,
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        let result = engine.run_job("pilot").await;
        // The branch lookup 404s in this mock, so the job fails — but the
        // tag lane must have run and landed both tags (annotated peeled).
        assert!(result.is_err());
        let job = store.get("pilot").await.unwrap().unwrap();
        let detail = job.last_detail.unwrap();
        // Per-tag isolation: the broken tag fails the job (correct) but must
        // NOT prevent the healthy ones, and must be named (Round A/B S2 pin).
        assert!(
            detail.contains("v-broken"),
            "the failing tag must be named: {detail}"
        );
        // Peel failure on an annotated tag must ALSO be named (Round D F2:
        // the listing must not collapse into one unnamed failure).
        assert!(
            detail.contains("v-badpeel"),
            "peel-failed tag must be named: {detail}"
        );
        assert!(
            detail.contains("imported,api") || detail.contains("master="),
            "healthy tags must still be imported: {detail}"
        );
        let refs = RedbRefStore::new(Arc::clone(&db)).unwrap();
        let light = refs.get("mirror/pilot/tags/v-light").await.unwrap();
        let annot = refs.get("mirror/pilot/tags/v-annot").await.unwrap();
        let light = light.unwrap();
        let annot = annot.unwrap();
        let snaps = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        assert_eq!(
            snaps.get(&light).await.unwrap().tree_hash,
            snaps.get(&annot).await.unwrap().tree_hash,
            "peeled annotated tag lands the same tree as the lightweight tag"
        );
    }

    #[tokio::test]
    async fn test_tree_hash_is_entry_order_independent() {
        // Round D D5: walk_tree is LIFO while GitHub listings are ordered,
        // so the storage layer MUST normalise order or a lane switch mints a
        // spurious "changed" snapshot.
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let mk = |path: &str, body: &[u8]| ImportedEntry {
            path: path.to_string(),
            kind: EntryKind::Blob,
            body: ImportedBody::Blob(body.to_vec()),
        };
        let forward = vec![
            mk(
                "a.txt", b"A
",
            ),
            mk("b.txt", b"B\n"),
            mk(
                "c/d.txt", b"D
",
            ),
        ];
        let reverse = {
            let mut v = forward.clone();
            v.reverse();
            v
        };
        let opts = |r: &str| StoreOptions {
            workspace: "ws.order".into(),
            head_ref: r.to_string(),
            parents: Vec::new(),
            skip_if_tree_unchanged: false,
            author: "t".into(),
            message: "m".into(),
            timestamp_us: 1,
        };
        let s1 = store_imported_entries(Arc::clone(&db), forward, &opts("ref-a"))
            .await
            .unwrap()
            .unwrap();
        let s2 = store_imported_entries(Arc::clone(&db), reverse, &opts("ref-b"))
            .await
            .unwrap()
            .unwrap();
        let snaps = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        assert_eq!(
            snaps.get(&s1).await.unwrap().tree_hash,
            snaps.get(&s2).await.unwrap().tree_hash,
            "tree hash must be independent of input entry order"
        );
    }

    #[tokio::test]
    async fn test_broken_scratch_is_reset_on_transport_failure() {
        // Round C pin: a half-written clone must not pin the job to the API
        // lane forever — it is dropped so the next wave can re-clone.
        let dir = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let (base, _gets) = api_import_mock(false).await;
        let scratch_root = dir.path().join("scratch");
        let broken = scratch_root.join("pilot.git");
        std::fs::create_dir_all(broken.join("objects")).unwrap();
        std::fs::write(broken.join("HEAD"), b"not a git dir\n").unwrap();

        let cfg = MirrorConfig {
            interval_secs: 900,
            scratch_dir: scratch_root.clone(),
            token_env: String::new(),
            github_api_base: base,
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: "pilot".into(),
                remote_url: "file:///nonexistent-noa-mirror-fixture".into(),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                tags: false,
                canary_push: false,
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        engine.run_job("pilot").await.unwrap();
        assert!(
            !broken.exists(),
            "broken scratch must be removed so a later wave can re-clone"
        );
    }

    #[tokio::test]
    async fn test_api_import_rejects_corrupt_blob() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(db_dir.path().join("m.redb"))
                .unwrap(),
        );
        let (base, _blob_gets) = api_import_mock(true).await;
        let cfg = MirrorConfig {
            interval_secs: 900,
            scratch_dir: dir.path().join("scratch"),
            token_env: String::new(),
            github_api_base: base,
        };
        let store = MirrorJobStore::new(Arc::clone(&db)).unwrap();
        store
            .upsert(MirrorJob {
                name: "pilot".into(),
                remote_url: "file:///nonexistent-noa-mirror-fixture".into(),
                github_slug: "celestia-island/noa".into(),
                branches: vec!["master".into()],
                tags: false,
                canary_push: false,
                enabled: true,
                last_run_us: None,
                last_status: None,
                last_detail: None,
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(Arc::clone(&db), cfg);
        let result = engine.run_job("pilot").await;
        assert!(result.is_err(), "declared-oid mismatch must fail the job");
        let job = store.get("pilot").await.unwrap().unwrap();
        let detail = job.last_detail.unwrap();
        assert!(
            detail.contains("blob oid mismatch at \"run.sh\""),
            "error must name the path: {detail}"
        );
    }

    #[tokio::test]
    async fn test_canary_api_delta_uploads_only_changes() {
        use axum::routing::post;
        use axum::Router;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (dir, engine, name) = engine_with_fixture("pilot").await;
        engine.run_job(&name).await.unwrap();
        let origin = dir.path().join("origin.git");
        make_read_only(&origin);

        let blob_calls = Arc::new(AtomicUsize::new(0));
        let bc = Arc::clone(&blob_calls);
        // Remote state: an existing canary whose tree already carries the
        // fixture's two files at their correct shas plus a stale file the
        // noa tree lost — the delta lane must upload nothing and delete
        // only the stale entry.
        let readme_sha = git_blob_oid(b"# fixture\n");
        let deep_sha = git_blob_oid(b"deep\n");
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/ref/heads/noa-mirror-canary",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({ "object": { "sha": "prev-canary" } }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits/prev-canary",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({ "tree": { "sha": "old-tree" } }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees/old-tree",
                axum::routing::get(move || {
                    let readme_sha = readme_sha.clone();
                    let deep_sha = deep_sha.clone();
                    async move {
                        axum::Json(serde_json::json!({
                            "tree": [
                                { "path": "README.md", "mode": "100644", "sha": readme_sha, "type": "blob" },
                                { "path": "nested/deep.txt", "mode": "100644", "sha": deep_sha, "type": "blob" },
                                { "path": "stale.txt", "mode": "100644", "sha": "deadbee", "type": "blob" }
                            ],
                            "truncated": false
                        }))
                    }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/blobs",
                post(move |axum::Json(b): axum::Json<serde_json::Value>| {
                    let bc = Arc::clone(&bc);
                    async move {
                        bc.fetch_add(1, Ordering::SeqCst);
                        use base64::Engine;
                        let content = base64::engine::general_purpose::STANDARD
                            .decode(b["content"].as_str().unwrap())
                            .unwrap();
                        axum::Json(serde_json::json!({ "sha": git_blob_oid(&content) }))
                    }
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees",
                post(|axum::Json(b): axum::Json<serde_json::Value>| async move {
                    assert_eq!(
                        b["base_tree"], "old-tree",
                        "delta lane must build on the existing canary tree"
                    );
                    let entries = b["tree"].as_array().unwrap();
                    assert_eq!(entries.len(), 1, "only the deletion should remain: {b}");
                    assert_eq!(entries[0]["path"], "stale.txt");
                    assert!(entries[0]["sha"].is_null(), "deletion is sha:null: {b}");
                    axum::Json(serde_json::json!({ "sha": "new-tree" }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits",
                post(|axum::Json(b): axum::Json<serde_json::Value>| async move {
                    assert_eq!(
                        b["parents"][0], "prev-canary",
                        "canary history must chain: {b}"
                    );
                    axum::Json(serde_json::json!({ "sha": "api-commit-sha" }))
                }),
            )
            .route(
                "/repos/{owner}/{repo}/git/refs/heads/noa-mirror-canary",
                axum::routing::patch(|axum::Json(_b): axum::Json<serde_json::Value>| async {
                    axum::Json(serde_json::json!({ "object": { "sha": "api-commit-sha" } }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        const TOKEN_VAR: &str = "NOA_MIRROR_TEST_TOKEN_W1A2D";
        std::env::set_var(TOKEN_VAR, "test-token");
        let store = MirrorJobStore::new(Arc::clone(&engine.db)).unwrap();
        let job = store.get(&name).await.unwrap().unwrap();
        store
            .upsert(MirrorJob {
                canary_push: true,
                ..job
            })
            .await
            .unwrap();
        let engine = MirrorEngine::new(
            Arc::clone(&engine.db),
            MirrorConfig {
                scratch_dir: engine.cfg.scratch_dir.clone(),
                token_env: TOKEN_VAR.to_string(),
                github_api_base: format!("http://{addr}"),
                ..engine.cfg.clone()
            },
        );
        engine.run_job(&name).await.unwrap();
        assert_eq!(
            blob_calls.load(Ordering::SeqCst),
            0,
            "unchanged blobs must not be re-uploaded"
        );
        std::env::remove_var(TOKEN_VAR);
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
