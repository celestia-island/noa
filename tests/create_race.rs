//! A losing `run_create` must leave the log and the stores observably untouched.
//!
//! `integration_snapshot_failed_cas_leaves_log_and_stores_untouched` in
//! `integration.rs` mirrors `run_create`'s ordering inline: it builds, lets a
//! winner publish, then CASes with a stale expectation. That is a fine way to
//! pin the *engine* primitives, but it cannot see a regression in the
//! *production* ordering — if someone moved the store/compact back before the
//! CAS inside `run_create` while keeping the engine's build/store split, the
//! mirrored test stays green and the real command destroys pending log entries
//! again. That is exactly what adversarial review found on 2026-10-06.
//!
//! So this test drives `cli::snapshot_cmd::run_create` itself and races it
//! against a writer that keeps moving the publication ref, which forces the
//! stale-read + CAS to lose for real. Every loss is checked for damage; the
//! test is only conclusive once it has actually produced losses, hence the
//! final assertion on `losses`.
//!
//! Deleting this test re-opens the gap; if the flakiness ever becomes a
//! problem, make the race deterministic rather than dropping the coverage.

use libnoa::{
    log::{AgentLog, LogEntry, OpType},
    refs::RefStore,
    repo::Repository,
    snapshot::{SnapshotId, SnapshotStore},
};

fn make_log_entry(seq: u64, op: OpType, path: &str, blob_id: Option<&str>, ts: u64) -> LogEntry {
    LogEntry {
        seq,
        op,
        path: Some(path.to_string()),
        blob_id: blob_id.map(std::string::ToString::to_string),
        from_path: None,
        resolved_conflict_ours_id: None,
        resolved_conflict_theirs_id: None,
        snapshot_id: None,
        ts,
        message: None,
    }
}

/// Attempts are cheap (an empty repository plus two log entries) and the loop
/// stops as soon as it has seen enough losses, so the usual cost is a few
/// attempts.
const ATTEMPTS: u32 = 24;

/// How many losses make the run conclusive.
const ENOUGH_LOSSES: u32 = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn integration_losing_run_create_keeps_pending_log_entries() {
    let mut losses = 0u32;
    let mut wins = 0u32;

    for attempt in 0..ATTEMPTS {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let log = repo.agent_log("default").unwrap();
        log.append(&make_log_entry(
            1,
            OpType::Write,
            "a.txt",
            Some("blob_a"),
            100,
        ))
        .await
        .unwrap();
        log.append(&make_log_entry(
            2,
            OpType::Write,
            "b.txt",
            Some("blob_b"),
            200,
        ))
        .await
        .unwrap();

        // Concurrent writer: keeps moving the publication ref for as long as
        // `run_create` runs, so its read-then-CAS must lose sometimes. The
        // writer is aborted rather than counted down — a fixed flip budget can
        // run dry while the command is still working, after which the command
        // simply wins every race and the loop proves nothing.
        let ref_store = repo.ref_store().unwrap();
        let flip = tokio::spawn(async move {
            let mut i = 0u32;
            loop {
                let cur = ref_store.get("default").await.unwrap();
                let id = SnapshotId(format!("noa_flip{i:037}"));
                let _ = ref_store.cas("default", cur.as_ref(), &id).await;
                i += 1;
                tokio::task::yield_now().await;
            }
        });

        let res = libnoa::cli::snapshot_cmd::run_create(&repo, "racer", "verifier").await;
        flip.abort();

        match res {
            Ok(()) => {
                // `run_create` won its own publication race, so compacting the
                // pending entries is legitimate here.
                wins += 1;
            }
            Err(e) => {
                let msg = format!("{e:?}");
                assert!(
                    msg.contains("concurrent modification"),
                    "attempt {attempt}: unexpected error from run_create: {msg}"
                );
                losses += 1;

                let entries = repo.agent_log("default").unwrap().read_all().await.unwrap();
                assert_eq!(
                    entries.len(),
                    2,
                    "attempt {attempt}: a losing run_create destroyed pending log entries \
                     (survivors={:?})",
                    entries.iter().map(|e| e.seq).collect::<Vec<_>>()
                );
                assert_eq!(entries[0].seq, 1);
                assert_eq!(entries[1].seq, 2);

                let snaps = repo.snapshot_store().unwrap().list_all().await.unwrap();
                assert!(
                    snaps.is_empty(),
                    "attempt {attempt}: a losing run_create left {} orphan snapshot(s)",
                    snaps.len()
                );

                if losses >= ENOUGH_LOSSES {
                    break;
                }
            }
        }
    }

    assert!(
        losses >= ENOUGH_LOSSES,
        "inconclusive: only {losses} CAS losses in {ATTEMPTS} attempts (wins={wins}); the race \
         did not exercise the path this test exists to cover"
    );
}
