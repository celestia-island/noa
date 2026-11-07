//! Mirror job store (W1 mirror-first lane) — redb-backed job registry for
//! the server-side scheduled replication task. Jobs describe which git
//! remotes are replicated into noa under `mirror/<job>/<branch>` refs.
//!
//! Design home: workspace `_reports/noa-forge-feasibility-20261106/REPORT.md`
//! (W1: authority stays on GitHub; this lane only builds a noa-side replica
//! and proves the channel — it never pushes authoritative refs in W1).

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use redb::{ReadableDatabase, ReadableTable, TableDefinition};

use crate::error::Result;

const MIRROR_JOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("mirror_jobs");

/// A scheduled replication job. `remote_url` must not embed credentials —
/// the engine injects auth via `GIT_ASKPASS` at run time so secrets never
/// reach argv, stored config, or logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorJob {
    /// Slug, `^[a-z0-9][a-z0-9-]{0,63}$`.
    pub name: String,
    /// Clone/fetch URL (`https://…` or `file://…`). No embedded credentials.
    pub remote_url: String,
    /// `owner/name` on GitHub (informational in W1; the Git Data API lane
    /// added later uses it).
    pub github_slug: String,
    /// Tracked branches. Each becomes ref `mirror/<name>/<branch>` and
    /// workspace `mirror.<name>.<sanitized branch>`.
    pub branches: Vec<String>,
    /// Opt-in tag replication: every remote tag is imported as ref
    /// `mirror/<name>/tags/<tag>` (tags are part of the authority set the
    /// mirror must hold before a GitHub flip).
    #[serde(default)]
    pub tags: bool,
    /// Opt-in canary push (W1a-2): after replication, push the first
    /// tracked branch's commit to `noa-mirror-canary` on the remote (git
    /// transport first, Git Data API fallback). Meaningful for https
    /// remotes; a file:// remote exercises only the git lane.
    #[serde(default)]
    pub canary_push: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_us: Option<i64>,
    /// `ok` | `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_detail: Option<String>,
    /// Transport lane that served the last wave: `git` | `api`. W1e: a job
    /// can be "ok" while the git transport is dead (the API lane masked it) —
    /// alerts that only read `last_status` were blind to that. `None` =
    /// never run (or pre-W1e record).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_lane: Option<String>,
}

impl MirrorJob {
    fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.name.len() > 64 {
            anyhow::bail!("mirror job name must be 1..=64 chars");
        }
        let mut chars = self.name.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !first_ok
            || !self
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            anyhow::bail!("mirror job name must match ^[a-z0-9][a-z0-9-]{{0,63}}$");
        }
        let scheme_ok =
            self.remote_url.starts_with("https://") || self.remote_url.starts_with("file://");
        if !scheme_ok {
            anyhow::bail!("mirror remote_url must be https:// or file://");
        }
        if self.remote_url.contains('@') || self.remote_url.contains('?') {
            anyhow::bail!("mirror remote_url must not embed credentials");
        }
        let mut slug = self.github_slug.split('/');
        let owner = slug.next().unwrap_or_default();
        let repo = slug.next().unwrap_or_default();
        if owner.is_empty()
            || repo.is_empty()
            || slug.next().is_some()
            || owner.starts_with('.')
            || repo.starts_with('.')
            || owner.contains("..")
            || repo.contains("..")
            || !owner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            || !repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            anyhow::bail!("github_slug must be owner/repo");
        }
        if self.branches.is_empty() {
            anyhow::bail!("mirror job must track at least one branch");
        }
        // Branch names feed workspace names via '/'→"__" replacement, so a
        // literal "__" inside a branch would collide with a slashed sibling
        // (R2 S2) — rejected here to keep the mapping injective. Duplicates
        // are rejected for the same reason. The derived workspace name is
        // validated with the workspace module's own authority so an
        // over-long branch cannot pass upsert and only blow up (or worse,
        // settle into a green-but-workspace-less state) at run time (R-A S2).
        let mut seen = std::collections::HashSet::new();
        for b in &self.branches {
            if b.is_empty()
                || b.len() > 200
                || b.starts_with('-')
                || b.starts_with('/')
                || b.ends_with('/')
                || b.contains(' ')
                || b.contains("..")
                || b.contains('~')
                || b.contains('^')
                || b.contains(':')
                || b.contains('\\')
                || b.contains('?')
                || b.contains('*')
                || b.contains('[')
                || b.contains(']')
                || b.contains("__")
                || b.ends_with(".lock")
                // `tags/…` collides with the tag ref namespace
                // mirror/<job>/tags/<tag> (Round A S3).
                || b == "tags"
                || b.starts_with("tags/")
                || b.chars().any(|c| c.is_ascii_control())
                || !seen.insert(b.as_str())
            {
                anyhow::bail!("invalid branch name {b:?}");
            }
            let ws_name = format!("mirror.{}.{}", self.name, b.replace('/', "__"));
            crate::workspace::validate_workspace_name(&ws_name)
                .map_err(|e| anyhow::anyhow!("branch {b:?} derives an invalid workspace: {e}"))?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct MirrorJobStore {
    db: Arc<redb::Database>,
    /// Shared with AppState (one instance per server) so upsert auto-register
    /// doesn't construct a fresh registry per call (Round C S2).
    registry: crate::server::repo_registry::RepoRegistry,
}

impl MirrorJobStore {
    pub fn new(db: Arc<redb::Database>) -> Result<Self> {
        let registry = crate::server::repo_registry::RepoRegistry::new(Arc::clone(&db))?;
        let store = MirrorJobStore { db, registry };
        store.ensure_table()?;
        Ok(store)
    }

    fn ensure_table(&self) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(MIRROR_JOBS)?;
        txn.commit()?;
        Ok(())
    }

    /// Inserts or updates a job by name. Validation runs before the write;
    /// bookkeeping fields (`last_*`) of an existing job are preserved on
    /// update unless the payload carries new values.
    pub async fn upsert(&self, mut job: MirrorJob) -> Result<MirrorJob> {
        job.validate()?;
        let db = self.db.clone();
        let job_for_registry = job.clone();
        let returned_job = tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(MIRROR_JOBS)?;
                // Copy out first so the read guard is dropped before insert.
                let existing: Option<Vec<u8>> =
                    table.get(job.name.as_str())?.map(|g| g.value().to_vec());
                if let Some(bytes) = existing {
                    let old: MirrorJob = rmp_serde::from_slice(&bytes)?;
                    job.last_run_us = job.last_run_us.or(old.last_run_us);
                    job.last_status = job.last_status.or(old.last_status);
                    job.last_detail = job.last_detail.or(old.last_detail);
                }
                let data = rmp_serde::to_vec_named(&job)?;
                table.insert(job.name.as_str(), data.as_slice())?;
            }
            txn.commit()?;
            Ok::<_, anyhow::Error>(job)
        })
        .await??;
        // W2b: the repo catalog follows the mirror lockstep (async context —
        // the registry registers in its own spawn_blocking).
        let reg = &self.registry;
        reg.register(&job_for_registry.name, None, None, "mirror")
            .await?;
        Ok(returned_job)
    }

    pub async fn get(&self, name: &str) -> Result<Option<MirrorJob>> {
        let db = self.db.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(MIRROR_JOBS)?;
            match table.get(name.as_str())? {
                Some(guard) => Ok(Some(rmp_serde::from_slice(guard.value())?)),
                None => Ok(None),
            }
        })
        .await?
    }

    pub async fn list(&self) -> Result<Vec<MirrorJob>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(MIRROR_JOBS)?;
            let mut jobs: Vec<MirrorJob> = Vec::new();
            for guard in table.iter()? {
                let (_, v) = guard?;
                jobs.push(rmp_serde::from_slice(v.value())?);
            }
            jobs.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(jobs)
        })
        .await?
    }

    pub async fn delete(&self, name: &str) -> Result<bool> {
        let db = self.db.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            let removed = {
                let mut table = txn.open_table(MIRROR_JOBS)?;
                let x = table.remove(name.as_str())?.is_some();
                x
            };
            txn.commit()?;
            Ok(removed)
        })
        .await?
    }

    /// Records a run outcome (status/detail/timestamp) without touching the
    /// replication fields.
    pub async fn record_run(
        &self,
        name: &str,
        status: &str,
        detail: &str,
        lane: Option<&str>,
    ) -> Result<()> {
        let db = self.db.clone();
        let name = name.to_string();
        let status = status.to_string();
        let detail = detail.to_string();
        let lane = lane.map(ToString::to_string);
        let now = crate::now_micros() as i64;
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(MIRROR_JOBS)?;
                // Copy out first so the read guard is dropped before insert.
                let existing: Option<Vec<u8>> =
                    table.get(name.as_str())?.map(|g| g.value().to_vec());
                if let Some(bytes) = existing {
                    let mut job: MirrorJob = rmp_serde::from_slice(&bytes)?;
                    job.last_run_us = Some(now);
                    job.last_status = Some(status);
                    job.last_detail = Some(detail);
                    // W1e: lane visibility — "ok" on the API lane masked a
                    // dead git transport from status-only alerts.
                    job.last_lane = lane;
                    let data = rmp_serde::to_vec_named(&job)?;
                    table.insert(name.as_str(), data.as_slice())?;
                }
            }
            txn.commit()?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn tmp_store() -> (tempfile::TempDir, MirrorJobStore) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(dir.path().join("t.redb"))
                .unwrap(),
        );
        let store = MirrorJobStore::new(db).unwrap();
        (dir, store)
    }

    fn job(name: &str) -> MirrorJob {
        MirrorJob {
            name: name.to_string(),
            remote_url: "https://github.com/celestia-island/noa.git".into(),
            github_slug: "celestia-island/noa".into(),
            branches: vec!["master".into()],
            tags: false,
            canary_push: false,
            enabled: true,
            last_run_us: None,
            last_status: None,
            last_detail: None,
            last_lane: None,
        }
    }

    #[tokio::test]
    async fn test_upsert_get_list_delete() {
        let (_d, s) = tmp_store().await;
        s.upsert(job("pilot")).await.unwrap();
        s.upsert(MirrorJob {
            name: "second".into(),
            ..job("second")
        })
        .await
        .unwrap();
        let all = s.list().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "pilot");
        assert!(s.get("pilot").await.unwrap().is_some());
        assert!(s.delete("pilot").await.unwrap());
        assert!(s.get("pilot").await.unwrap().is_none());
        assert!(!s.delete("pilot").await.unwrap());
    }

    #[tokio::test]
    async fn test_upsert_preserves_run_history() {
        let (_d, s) = tmp_store().await;
        s.upsert(job("pilot")).await.unwrap();
        s.record_run("pilot", "ok", "imported master", Some("git"))
            .await
            .unwrap();
        let updated = MirrorJob {
            branches: vec!["master".into(), "dev-x".into()],
            ..job("pilot")
        };
        s.upsert(updated).await.unwrap();
        let got = s.get("pilot").await.unwrap().unwrap();
        assert_eq!(got.branches, vec!["master", "dev-x"]);
        assert_eq!(got.last_status.as_deref(), Some("ok"));
        assert!(got.last_run_us.is_some());
    }

    #[tokio::test]
    async fn test_validation_rejects() {
        let (_d, s) = tmp_store().await;
        for bad in [
            MirrorJob {
                name: "Bad_Name".into(),
                ..job("x")
            },
            MirrorJob {
                name: "-start".into(),
                ..job("x")
            },
            MirrorJob {
                remote_url: "http://insecure".into(),
                ..job("x")
            },
            MirrorJob {
                remote_url: "https://user:tok@host/r.git".into(),
                ..job("x")
            },
            MirrorJob {
                remote_url: "https://host/r.git?token=SECRET".into(),
                ..job("x")
            },
            MirrorJob {
                github_slug: "not-a-slug".into(),
                ..job("x")
            },
            MirrorJob {
                github_slug: "../..".into(),
                ..job("x")
            },
            MirrorJob {
                branches: vec![],
                ..job("x")
            },
            MirrorJob {
                branches: vec!["bad..name".into()],
                ..job("x")
            },
            // '/'→"__" mapping must stay injective (R2 S2): literal "__"
            // in a branch would collide with a slashed sibling.
            MirrorJob {
                branches: vec!["a__b".into()],
                ..job("x")
            },
            MirrorJob {
                branches: vec!["master".into(), "master".into()],
                ..job("x")
            },
            // '/'→"__" doubles length: a 199-char branch with 99 slashes
            // derives a >255-char workspace name — must be rejected at
            // upsert, not blow up (or settle green-without-workspace) at
            // run time (Round A S2).
            MirrorJob {
                branches: vec![format!("{}a", "a/".repeat(99))],
                ..job("x")
            },
        ] {
            assert!(s.upsert(bad).await.is_err(), "expected rejection");
        }
        // Slashed branches stay legal (nested ref names), just not "__".
        let nested = MirrorJob {
            branches: vec!["feature/nested".into()],
            ..job("x")
        };
        s.upsert(nested).await.unwrap();
        // '#'/'%' are legal git ref chars; the URL hazard is handled by the
        // encoder, so validation must KEEP accepting them (Round C S2 pin —
        // re-adding the ban must now fail).
        for ok_branch in ["a#b", "a%b"] {
            s.upsert(MirrorJob {
                branches: vec![ok_branch.to_string()],
                ..job("x")
            })
            .await
            .unwrap_or_else(|e| panic!("'{ok_branch}' must stay accepted: {e}"));
        }
    }
}
