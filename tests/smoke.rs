use std::sync::Arc;

use libnoa::{
    log::{AgentLog, FileAgentLog, LogEntry, OpType},
    object::ObjectStore,
    refs::RefStore,
    repo::{manage_gitattributes, manage_gitignore, Repository},
    snapshot::{diff_snapshots, SnapshotEngine, SnapshotId},
};

fn make_log_entry(seq: u64, op: OpType, path: &str, blob_id: Option<&str>, ts: u64) -> LogEntry {
    LogEntry {
        seq,
        op,
        path: Some(path.to_string()),
        blob_id: blob_id.map(|s| s.to_string()),
        from_path: None,
        resolved_conflict_ours_id: None,
        resolved_conflict_theirs_id: None,
        snapshot_id: None,
        ts,
        message: None,
    }
}

#[tokio::test]
async fn smoke_test_init_open_find() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path();

    assert!(!Repository::exists(path));
    {
        Repository::init(path).unwrap();
    }
    assert!(Repository::exists(path));
    assert!(path.join(".noa").exists());
    assert!(path.join(".noa/noa.redb").exists());
    assert!(path.join(".noa/agent-logs").exists());
    assert!(path.join(".noa/HEAD").exists());
    assert!(path.join(".noa/config").exists());
    assert!(path.join(".gitignore").exists());

    let found = Repository::find(path).unwrap();
    assert_eq!(found, path);

    let subdir = path.join("src").join("deep");
    std::fs::create_dir_all(&subdir).unwrap();
    let found_deep = Repository::find(&subdir).unwrap();
    assert_eq!(found_deep, path);

    let reopened = Repository::open(path).unwrap();
    assert_eq!(reopened.read_head().unwrap(), "default");
}

#[tokio::test]
async fn smoke_test_full_snapshot_workflow() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    let agent_log = repo.agent_log("default").unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    agent_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    agent_log
        .append(&make_log_entry(
            2,
            OpType::Write,
            "lib.rs",
            Some("blob2"),
            200,
        ))
        .await
        .unwrap();

    let engine = SnapshotEngine::new(agent_log, snap_store, obj_store);
    let snapshot = engine
        .compute("default", vec![], 0, "test-author", "initial commit")
        .await
        .unwrap();

    assert!(snapshot.id.0.starts_with("noa_"));
    assert_eq!(snapshot.workspace, "default");
    assert_eq!(snapshot.author, "test-author");
    assert_eq!(snapshot.message, "initial commit");
    assert!(snapshot.parents.is_empty());

    let tree = engine
        .object_store
        .get_tree(&libnoa::object::TreeId(snapshot.tree_hash.clone()))
        .await
        .unwrap();
    assert_eq!(tree.0.len(), 2);
}

#[tokio::test]
async fn smoke_test_workspace_create_switch_merge() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let agent_log = repo.agent_log("default").unwrap();
    agent_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    let engine = SnapshotEngine::new(agent_log, snap_store.clone(), obj_store.clone());
    let base_snap = engine
        .compute("default", vec![], 0, "author", "base")
        .await
        .unwrap();
    ws_mgr.update_head("default", &base_snap.id).await.unwrap();

    let feature_ws = libnoa::workspace::Workspace {
        name: "feature".to_string(),
        head: base_snap.id.clone(),
        base: base_snap.id.clone(),
        agent_id: Some("agent-001".to_string()),
        last_seq: 0,
        created_at: 1000,
        updated_at: 1000,
    };
    ws_mgr.create(&feature_ws).await.unwrap();

    let feature_log = repo.agent_log("feature").unwrap();
    feature_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "feature.rs",
            Some("blob_feat"),
            300,
        ))
        .await
        .unwrap();
    let feature_engine = SnapshotEngine::new(feature_log, snap_store.clone(), obj_store.clone());
    let feature_snap = feature_engine
        .compute(
            "feature",
            vec![base_snap.id.clone()],
            0,
            "agent-001",
            "add feature",
        )
        .await
        .unwrap();
    ws_mgr
        .update_head("feature", &feature_snap.id)
        .await
        .unwrap();

    repo.write_orig_head(&repo.read_head().unwrap()).unwrap();
    repo.write_head("feature").unwrap();
    assert_eq!(repo.read_head().unwrap(), "feature");

    let default_tree = obj_store
        .get_tree(&libnoa::object::TreeId(base_snap.tree_hash.clone()))
        .await
        .unwrap();
    let feature_tree = obj_store
        .get_tree(&libnoa::object::TreeId(feature_snap.tree_hash.clone()))
        .await
        .unwrap();
    let result =
        libnoa::merge::three_way_merge(&default_tree, &default_tree, &feature_tree).unwrap();
    assert!(!result.has_conflicts());
    let tree = result.into_tree_entries(&libnoa::merge::ConflictResolution::Ours);
    assert!(tree.0.iter().any(|e| e.name == "feature.rs"));
}

#[tokio::test]
async fn smoke_test_ignore_filtering_in_snapshot() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join(".gitignore"), "*.log\nbuild/\n").unwrap();

    let repo = Repository::init(root).unwrap();

    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(gitignore.contains(".noa/"));

    let agent_log = repo.agent_log("default").unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    agent_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("b1"),
            100,
        ))
        .await
        .unwrap();
    agent_log
        .append(&make_log_entry(
            2,
            OpType::Write,
            "debug.log",
            Some("b2"),
            200,
        ))
        .await
        .unwrap();
    agent_log
        .append(&make_log_entry(
            3,
            OpType::Write,
            "build/output.js",
            Some("b3"),
            300,
        ))
        .await
        .unwrap();
    agent_log
        .append(&make_log_entry(
            4,
            OpType::Write,
            ".noa/config",
            Some("b4"),
            400,
        ))
        .await
        .unwrap();

    let matcher = libnoa::ignore::IgnoreMatcher::from_repo_root(root);
    let engine = SnapshotEngine::new(agent_log, snap_store, obj_store).with_ignore(matcher);
    let snap = engine
        .compute("default", vec![], 0, "test", "filtered")
        .await
        .unwrap();

    let tree = engine
        .object_store
        .get_tree(&libnoa::object::TreeId(snap.tree_hash))
        .await
        .unwrap();
    assert_eq!(tree.0.len(), 1);
    assert_eq!(tree.0[0].name, "main.rs");
}

#[tokio::test]
async fn smoke_test_ref_cas_and_list() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    let ref_store = repo.ref_store().unwrap();

    let id1 = SnapshotId("noa_ref1".to_string());
    let ok = ref_store.cas("main", None, &id1).await.unwrap();
    assert!(ok);

    let id2 = SnapshotId("noa_ref2".to_string());
    let ok = ref_store.cas("main", Some(&id1), &id2).await.unwrap();
    assert!(ok);

    let id3 = SnapshotId("noa_ref3".to_string());
    let ok = ref_store.cas("main", None, &id3).await.unwrap();
    assert!(!ok);

    let got = ref_store.get("main").await.unwrap();
    assert_eq!(got, Some(id2));

    let refs = ref_store.list().await.unwrap();
    assert!(!refs.is_empty());
}

#[tokio::test]
async fn smoke_test_snapshot_diff() {
    let tree_a = libnoa::object::TreeEntries(vec![
        libnoa::object::TreeEntry {
            name: "a.rs".to_string(),
            kind: libnoa::object::EntryKind::Blob,
            id: "h1".to_string(),
        },
        libnoa::object::TreeEntry {
            name: "b.rs".to_string(),
            kind: libnoa::object::EntryKind::Blob,
            id: "h2".to_string(),
        },
    ]);
    let tree_b = libnoa::object::TreeEntries(vec![
        libnoa::object::TreeEntry {
            name: "a.rs".to_string(),
            kind: libnoa::object::EntryKind::Blob,
            id: "h1_changed".to_string(),
        },
        libnoa::object::TreeEntry {
            name: "c.rs".to_string(),
            kind: libnoa::object::EntryKind::Blob,
            id: "h3".to_string(),
        },
    ]);

    let diffs = diff_snapshots(&tree_a.0, &tree_b.0);
    assert_eq!(diffs.len(), 3);
    assert!(diffs
        .iter()
        .any(|d| d.path == "a.rs" && matches!(d.kind, libnoa::snapshot::DiffKind::Modified)));
    assert!(diffs
        .iter()
        .any(|d| d.path == "b.rs" && matches!(d.kind, libnoa::snapshot::DiffKind::Deleted)));
    assert!(diffs
        .iter()
        .any(|d| d.path == "c.rs" && matches!(d.kind, libnoa::snapshot::DiffKind::Added)));
}

#[tokio::test]
async fn smoke_test_concurrent_agent_logs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    let ws_mgr = repo.workspace_manager().unwrap();

    let mut handles = Vec::new();
    for i in 0..5 {
        let ws_name = format!("agent-{}", i);
        let ws = libnoa::workspace::Workspace {
            name: ws_name.clone(),
            head: SnapshotId("noa_empty".to_string()),
            base: SnapshotId("noa_empty".to_string()),
            agent_id: Some(format!("agent-{}", i)),
            last_seq: 0,
            created_at: 0,
            updated_at: 0,
        };
        ws_mgr.create(&ws).await.unwrap();

        let log_path = repo.agent_log_path(&ws_name);
        let log = Arc::new(FileAgentLog::create(&log_path).unwrap());
        handles.push(tokio::spawn(async move {
            for j in 0..20 {
                let entry = make_log_entry(
                    j + 1,
                    OpType::Write,
                    &format!("file_{}.rs", j),
                    Some(&format!("blob_{}", j)),
                    (j + 1) * 100,
                );
                log.append(&entry).await.unwrap();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    for i in 0..5 {
        let ws_name = format!("agent-{}", i);
        let log = repo.agent_log(&ws_name).unwrap();
        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 20);
    }
}

#[tokio::test]
async fn smoke_test_init_with_noa_remote() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init_with_noa_remote(tmp.path(), Some("https://noa.example.com/myrepo"))
        .unwrap();

    let gitattributes = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(gitattributes.contains("noa-remote=https://noa.example.com/myrepo"));

    let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".noa/"));

    assert_eq!(
        repo.config.noa_remote,
        Some("https://noa.example.com/myrepo".to_string())
    );
}

#[tokio::test]
async fn smoke_test_log_entry_all_ops() {
    let tmp = tempfile::TempDir::new().unwrap();
    let log = FileAgentLog::create(&tmp.path().join("test.log")).unwrap();

    let entries = vec![
        LogEntry {
            seq: 1,
            op: OpType::Write,
            path: Some("a.rs".to_string()),
            blob_id: Some("h1".to_string()),
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 100,
            message: None,
        },
        LogEntry {
            seq: 2,
            op: OpType::Delete,
            path: Some("b.rs".to_string()),
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 200,
            message: None,
        },
        LogEntry {
            seq: 3,
            op: OpType::Rename,
            path: Some("c.rs".to_string()),
            blob_id: None,
            from_path: Some("old_c.rs".to_string()),
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 300,
            message: None,
        },
        LogEntry {
            seq: 4,
            op: OpType::Snapshot,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: Some("noa_snap1".to_string()),
            ts: 400,
            message: Some("my snapshot".to_string()),
        },
        LogEntry {
            seq: 5,
            op: OpType::Merge,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: Some("noa_snap2".to_string()),
            ts: 500,
            message: Some("merge from ws-2".to_string()),
        },
    ];

    for e in &entries {
        log.append(e).await.unwrap();
    }

    let read_back = log.read_all().await.unwrap();
    assert_eq!(read_back.len(), 5);
    for (i, e) in read_back.iter().enumerate() {
        assert_eq!(e.seq, entries[i].seq);
        assert_eq!(e.op, entries[i].op);
    }
}

#[tokio::test]
async fn smoke_test_three_way_merge_no_conflict() {
    let make_entry = |name: &str, id: &str| libnoa::object::TreeEntry {
        name: name.to_string(),
        kind: libnoa::object::EntryKind::Blob,
        id: id.to_string(),
    };

    let base =
        libnoa::object::TreeEntries(vec![make_entry("a.rs", "h1"), make_entry("b.rs", "h2")]);
    let ours = libnoa::object::TreeEntries(vec![
        make_entry("a.rs", "h1"),
        make_entry("b.rs", "h2_changed"),
    ]);
    let theirs = libnoa::object::TreeEntries(vec![
        make_entry("a.rs", "h1"),
        make_entry("b.rs", "h2"),
        make_entry("c.rs", "h3"),
    ]);

    let result = libnoa::merge::three_way_merge(&base, &ours, &theirs).unwrap();
    assert!(!result.has_conflicts());
    let tree = result.into_tree_entries(&libnoa::merge::ConflictResolution::Ours);
    assert_eq!(tree.0.len(), 3);
}

#[tokio::test]
async fn smoke_test_object_store_large_blob() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = Arc::new(
        redb::Database::builder()
            .create(tmp.path().join("test.redb"))
            .unwrap(),
    );
    let store = libnoa::object::RedbObjectStore::new(db).unwrap();

    let large = vec![0xABu8; 1024 * 100];
    let id = store.put_blob(&large).await.unwrap();
    let retrieved = store.get_blob(&id).await.unwrap();
    assert_eq!(retrieved.len(), large.len());
    assert_eq!(retrieved, large);
}

#[tokio::test]
async fn smoke_test_manage_gitignore_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    manage_gitignore(root).unwrap();
    let first = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(first.contains(".noa/"));

    manage_gitignore(root).unwrap();
    let second = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert_eq!(first, second);
}

#[tokio::test]
async fn smoke_test_manage_gitattributes_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    manage_gitattributes(root, "https://noa.example.com/repo").unwrap();
    let first = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
    assert!(first.contains("noa-remote="));

    manage_gitattributes(root, "https://noa.example.com/repo").unwrap();
    let second = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
    let count = second.matches("noa-remote=").count();
    assert_eq!(count, 1);
}
