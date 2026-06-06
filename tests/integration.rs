use libnoa::{
    log::{AgentLog, LogEntry, OpType},
    object::ObjectStore,
    repo::Repository,
    snapshot::{content_addressed_snapshot_id, SnapshotEngine, SnapshotId, SnapshotStore},
    workspace::Workspace,
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

fn make_entry(name: &str, id: &str) -> libnoa::object::TreeEntry {
    libnoa::object::TreeEntry {
        name: name.to_string(),
        kind: libnoa::object::EntryKind::Blob,
        id: id.to_string(),
    }
}

#[tokio::test]
async fn integration_branch_create_switch_delete() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();

    let default_ws = ws_mgr.get("default").await.unwrap().unwrap();
    assert_eq!(default_ws.name, "default");

    let feature = Workspace {
        name: "feature".to_string(),
        head: default_ws.head.clone(),
        base: default_ws.head.clone(),
        agent_id: Some("agent-001".to_string()),
        last_seq: 0,
        created_at: 1000,
        updated_at: 1000,
    };
    ws_mgr.create(&feature).await.unwrap();

    let hotfix = Workspace {
        name: "hotfix".to_string(),
        head: default_ws.head.clone(),
        base: default_ws.head.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 1000,
        updated_at: 1000,
    };
    ws_mgr.create(&hotfix).await.unwrap();

    let list = ws_mgr.list().await.unwrap();
    assert_eq!(list.len(), 3);
    let names: Vec<&str> = list.iter().map(|w| w.name.as_str()).collect();
    assert!(names.contains(&"default"));
    assert!(names.contains(&"feature"));
    assert!(names.contains(&"hotfix"));

    repo.write_head("feature").unwrap();
    assert_eq!(repo.read_head().unwrap(), "feature");

    repo.write_head("default").unwrap();
    assert_eq!(repo.read_head().unwrap(), "default");

    ws_mgr.delete("hotfix").await.unwrap();
    assert!(ws_mgr.get("hotfix").await.unwrap().is_none());
    let list = ws_mgr.list().await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn integration_commit_chain_on_branch() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let agent_log = repo.agent_log("default").unwrap();
    let engine = SnapshotEngine::new(agent_log, snap_store.clone(), obj_store.clone());

    engine
        .log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    let snap1 = engine
        .compute("default", vec![], 0, "author", "first commit")
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap1.id).await.unwrap();

    engine
        .log
        .append(&make_log_entry(
            2,
            OpType::Write,
            "lib.rs",
            Some("blob2"),
            200,
        ))
        .await
        .unwrap();
    let snap2 = engine
        .compute(
            "default",
            vec![snap1.id.clone()],
            0,
            "author",
            "second commit",
        )
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap2.id).await.unwrap();

    engine
        .log
        .append(&make_log_entry(
            3,
            OpType::Write,
            "utils.rs",
            Some("blob3"),
            300,
        ))
        .await
        .unwrap();
    let snap3 = engine
        .compute(
            "default",
            vec![snap2.id.clone()],
            0,
            "author",
            "third commit",
        )
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap3.id).await.unwrap();

    let all_snaps = snap_store.list_all().await.unwrap();
    assert_eq!(all_snaps.len(), 3);

    let tree = obj_store
        .get_tree(&libnoa::object::TreeId(snap3.tree_hash.clone()))
        .await
        .unwrap();
    assert_eq!(tree.0.len(), 3);
    let names: Vec<&str> = tree.0.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"main.rs"));
    assert!(names.contains(&"lib.rs"));
    assert!(names.contains(&"utils.rs"));

    let children_of_snap1 = snap_store.children_of(&snap1.id).await.unwrap();
    assert_eq!(children_of_snap1.len(), 1);
    assert_eq!(children_of_snap1[0], snap2.id);

    let children_of_snap2 = snap_store.children_of(&snap2.id).await.unwrap();
    assert_eq!(children_of_snap2.len(), 1);
    assert_eq!(children_of_snap2[0], snap3.id);
}

#[tokio::test]
async fn integration_parallel_branches_diverge() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let default_log = repo.agent_log("default").unwrap();
    default_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "shared.rs",
            Some("blob_shared"),
            100,
        ))
        .await
        .unwrap();
    let base_engine = SnapshotEngine::new(default_log, snap_store.clone(), obj_store.clone());
    let base_snap = base_engine
        .compute("default", vec![], 0, "author", "shared base")
        .await
        .unwrap();
    ws_mgr.update_head("default", &base_snap.id).await.unwrap();

    let left = Workspace {
        name: "left".to_string(),
        head: base_snap.id.clone(),
        base: base_snap.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&left).await.unwrap();

    let right = Workspace {
        name: "right".to_string(),
        head: base_snap.id.clone(),
        base: base_snap.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&right).await.unwrap();

    let left_log = repo.agent_log("left").unwrap();
    left_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "left_only.rs",
            Some("blob_left"),
            300,
        ))
        .await
        .unwrap();
    let left_engine = SnapshotEngine::new(left_log, snap_store.clone(), obj_store.clone());
    let left_snap = left_engine
        .compute(
            "left",
            vec![base_snap.id.clone()],
            0,
            "author",
            "left change",
        )
        .await
        .unwrap();
    ws_mgr.update_head("left", &left_snap.id).await.unwrap();

    let right_log = repo.agent_log("right").unwrap();
    right_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "right_only.rs",
            Some("blob_right"),
            400,
        ))
        .await
        .unwrap();
    let right_engine = SnapshotEngine::new(right_log, snap_store.clone(), obj_store.clone());
    let right_snap = right_engine
        .compute(
            "right",
            vec![base_snap.id.clone()],
            0,
            "author",
            "right change",
        )
        .await
        .unwrap();
    ws_mgr.update_head("right", &right_snap.id).await.unwrap();

    let left_tree = obj_store
        .get_tree(&libnoa::object::TreeId(left_snap.tree_hash.clone()))
        .await
        .unwrap();
    let right_tree = obj_store
        .get_tree(&libnoa::object::TreeId(right_snap.tree_hash.clone()))
        .await
        .unwrap();

    let left_names: Vec<&str> = left_tree.0.iter().map(|e| e.name.as_str()).collect();
    let right_names: Vec<&str> = right_tree.0.iter().map(|e| e.name.as_str()).collect();
    assert!(left_names.contains(&"shared.rs"));
    assert!(left_names.contains(&"left_only.rs"));
    assert!(!left_names.contains(&"right_only.rs"));
    assert!(right_names.contains(&"shared.rs"));
    assert!(right_names.contains(&"right_only.rs"));
    assert!(!right_names.contains(&"left_only.rs"));
}

#[tokio::test]
async fn integration_merge_no_conflict() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let default_log = repo.agent_log("default").unwrap();
    default_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    let base_engine = SnapshotEngine::new(default_log, snap_store.clone(), obj_store.clone());
    let base_snap = base_engine
        .compute("default", vec![], 0, "author", "base")
        .await
        .unwrap();
    ws_mgr.update_head("default", &base_snap.id).await.unwrap();

    let feature = Workspace {
        name: "feature".to_string(),
        head: base_snap.id.clone(),
        base: base_snap.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&feature).await.unwrap();

    let feat_log = repo.agent_log("feature").unwrap();
    feat_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "feature.rs",
            Some("blob_feat"),
            300,
        ))
        .await
        .unwrap();
    let feat_engine = SnapshotEngine::new(feat_log, snap_store.clone(), obj_store.clone());
    let feat_snap = feat_engine
        .compute(
            "feature",
            vec![base_snap.id.clone()],
            0,
            "author",
            "add feature",
        )
        .await
        .unwrap();
    ws_mgr.update_head("feature", &feat_snap.id).await.unwrap();

    let base_tree = obj_store
        .get_tree(&libnoa::object::TreeId(base_snap.tree_hash.clone()))
        .await
        .unwrap();
    let ours_tree = obj_store
        .get_tree(&libnoa::object::TreeId(base_snap.tree_hash.clone()))
        .await
        .unwrap();
    let theirs_tree = obj_store
        .get_tree(&libnoa::object::TreeId(feat_snap.tree_hash.clone()))
        .await
        .unwrap();

    let result = libnoa::merge::three_way_merge(&base_tree, &ours_tree, &theirs_tree).unwrap();
    assert!(!result.has_conflicts());

    let merged_entries = result.into_tree_entries(&libnoa::merge::ConflictResolution::Ours);
    let merged_tree_id = obj_store.put_tree(&merged_entries).await.unwrap();

    let merge_snap = libnoa::snapshot::Snapshot {
        id: content_addressed_snapshot_id(
            &merged_tree_id.0,
            &[base_snap.id.clone(), feat_snap.id.clone()],
            "default",
        ),
        tree_hash: merged_tree_id.0,
        parents: vec![base_snap.id.clone(), feat_snap.id.clone()],
        workspace: "default".to_string(),
        author: "author".to_string(),
        timestamp: 5000,
        message: "merge feature into default".to_string(),
    };
    snap_store.store(&merge_snap).await.unwrap();
    ws_mgr.update_head("default", &merge_snap.id).await.unwrap();

    let merged_tree = obj_store
        .get_tree(&libnoa::object::TreeId(merge_snap.tree_hash.clone()))
        .await
        .unwrap();
    let names: Vec<&str> = merged_tree.0.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"main.rs"));
    assert!(names.contains(&"feature.rs"));
}

#[tokio::test]
async fn integration_merge_with_conflict() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let default_log = repo.agent_log("default").unwrap();
    default_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    let base_engine = SnapshotEngine::new(default_log, snap_store.clone(), obj_store.clone());
    let base_snap = base_engine
        .compute("default", vec![], 0, "author", "base")
        .await
        .unwrap();
    ws_mgr.update_head("default", &base_snap.id).await.unwrap();

    let left = Workspace {
        name: "left".to_string(),
        head: base_snap.id.clone(),
        base: base_snap.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&left).await.unwrap();

    let right = Workspace {
        name: "right".to_string(),
        head: base_snap.id.clone(),
        base: base_snap.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&right).await.unwrap();

    let left_log = repo.agent_log("left").unwrap();
    left_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob_left"),
            300,
        ))
        .await
        .unwrap();
    let left_engine = SnapshotEngine::new(left_log, snap_store.clone(), obj_store.clone());
    let left_snap = left_engine
        .compute(
            "left",
            vec![base_snap.id.clone()],
            0,
            "author",
            "left edits main.rs",
        )
        .await
        .unwrap();
    ws_mgr.update_head("left", &left_snap.id).await.unwrap();

    let right_log = repo.agent_log("right").unwrap();
    right_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob_right"),
            400,
        ))
        .await
        .unwrap();
    let right_engine = SnapshotEngine::new(right_log, snap_store.clone(), obj_store.clone());
    let right_snap = right_engine
        .compute(
            "right",
            vec![base_snap.id.clone()],
            0,
            "author",
            "right edits main.rs",
        )
        .await
        .unwrap();
    ws_mgr.update_head("right", &right_snap.id).await.unwrap();

    let base_tree = obj_store
        .get_tree(&libnoa::object::TreeId(base_snap.tree_hash.clone()))
        .await
        .unwrap();
    let left_tree = obj_store
        .get_tree(&libnoa::object::TreeId(left_snap.tree_hash.clone()))
        .await
        .unwrap();
    let right_tree = obj_store
        .get_tree(&libnoa::object::TreeId(right_snap.tree_hash.clone()))
        .await
        .unwrap();

    let result = libnoa::merge::three_way_merge(&base_tree, &left_tree, &right_tree).unwrap();
    assert!(result.has_conflicts());

    let conflicts = libnoa::merge::extract_conflicts(&result.output);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, "main.rs");

    let resolved_ours = result
        .output
        .resolve_with_strategy(&libnoa::merge::ConflictResolution::Ours);
    assert_eq!(resolved_ours[0].id, "blob_left");

    let result2 = libnoa::merge::three_way_merge(&base_tree, &left_tree, &right_tree).unwrap();
    let resolved_theirs = result2
        .output
        .resolve_with_strategy(&libnoa::merge::ConflictResolution::Theirs);
    assert_eq!(resolved_theirs[0].id, "blob_right");
}

#[tokio::test]
async fn integration_snapshot_diff_across_branches() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let default_log = repo.agent_log("default").unwrap();
    default_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    default_log
        .append(&make_log_entry(
            2,
            OpType::Write,
            "lib.rs",
            Some("blob2"),
            200,
        ))
        .await
        .unwrap();
    let base_engine = SnapshotEngine::new(default_log, snap_store.clone(), obj_store.clone());
    let snap1 = base_engine
        .compute("default", vec![], 0, "author", "initial")
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap1.id).await.unwrap();

    let feature = Workspace {
        name: "feature".to_string(),
        head: snap1.id.clone(),
        base: snap1.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&feature).await.unwrap();

    let feat_log = repo.agent_log("feature").unwrap();
    feat_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "lib.rs",
            Some("blob2_new"),
            300,
        ))
        .await
        .unwrap();
    feat_log
        .append(&make_log_entry(
            2,
            OpType::Write,
            "new.rs",
            Some("blob_new"),
            400,
        ))
        .await
        .unwrap();
    let feat_engine = SnapshotEngine::new(feat_log, snap_store.clone(), obj_store.clone());
    let snap2 = feat_engine
        .compute(
            "feature",
            vec![snap1.id.clone()],
            0,
            "author",
            "feature work",
        )
        .await
        .unwrap();

    let tree1 = obj_store
        .get_tree(&libnoa::object::TreeId(snap1.tree_hash.clone()))
        .await
        .unwrap();
    let tree2 = obj_store
        .get_tree(&libnoa::object::TreeId(snap2.tree_hash.clone()))
        .await
        .unwrap();

    let diffs = libnoa::snapshot::diff_snapshots(&tree1.0, &tree2.0);
    assert_eq!(diffs.len(), 2);
    assert!(diffs
        .iter()
        .any(|d| d.path == "lib.rs" && matches!(d.kind, libnoa::snapshot::DiffKind::Modified)));
    assert!(diffs
        .iter()
        .any(|d| d.path == "new.rs" && matches!(d.kind, libnoa::snapshot::DiffKind::Added)));
}

#[tokio::test]
async fn integration_log_compaction_after_snapshot() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let agent_log = repo.agent_log("default").unwrap();

    for i in 0..10 {
        agent_log
            .append(&make_log_entry(
                i + 1,
                OpType::Write,
                &format!("file{}.rs", i),
                Some(&format!("blob{}", i)),
                (i + 1) * 100,
            ))
            .await
            .unwrap();
    }

    assert_eq!(agent_log.read_all().await.unwrap().len(), 10);

    let engine = SnapshotEngine::new(agent_log, snap_store.clone(), obj_store.clone())
        .with_compact_on_snapshot();
    let snap = engine
        .compute("default", vec![], 0, "author", "compact me")
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap.id).await.unwrap();

    let remaining = engine.log.read_all().await.unwrap();
    assert!(
        remaining.len() < 10,
        "expected compaction to reduce entries, got {}",
        remaining.len()
    );
}

#[tokio::test]
async fn integration_incremental_snapshot() {
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
            "a.rs",
            Some("blob_a"),
            100,
        ))
        .await
        .unwrap();
    let engine = SnapshotEngine::new(agent_log, snap_store.clone(), obj_store.clone());
    let snap1 = engine
        .compute("default", vec![], 0, "author", "first")
        .await
        .unwrap();
    ws_mgr
        .update_head_and_seq("default", &snap1.id, 1)
        .await
        .unwrap();

    engine
        .log
        .append(&make_log_entry(
            2,
            OpType::Write,
            "b.rs",
            Some("blob_b"),
            200,
        ))
        .await
        .unwrap();

    let default_ws = ws_mgr.get("default").await.unwrap().unwrap();
    assert_eq!(default_ws.last_seq, 1);

    let snap2 = engine
        .compute(
            "default",
            vec![snap1.id.clone()],
            default_ws.last_seq,
            "author",
            "second",
        )
        .await
        .unwrap();
    ws_mgr
        .update_head_and_seq("default", &snap2.id, 2)
        .await
        .unwrap();

    let tree = obj_store
        .get_tree(&libnoa::object::TreeId(snap2.tree_hash.clone()))
        .await
        .unwrap();
    let names: Vec<&str> = tree.0.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"a.rs"));
    assert!(names.contains(&"b.rs"));
}

#[tokio::test]
async fn integration_workspace_base_tracks_fork_point() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let default_log = repo.agent_log("default").unwrap();
    default_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    let engine = SnapshotEngine::new(default_log, snap_store.clone(), obj_store.clone());
    let snap1 = engine
        .compute("default", vec![], 0, "author", "initial")
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap1.id).await.unwrap();

    let feature = Workspace {
        name: "feature".to_string(),
        head: snap1.id.clone(),
        base: snap1.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&feature).await.unwrap();

    let feat_log = repo.agent_log("feature").unwrap();
    feat_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "feat.rs",
            Some("blob_feat"),
            300,
        ))
        .await
        .unwrap();
    let feat_engine = SnapshotEngine::new(feat_log, snap_store.clone(), obj_store.clone());
    let snap2 = feat_engine
        .compute(
            "feature",
            vec![snap1.id.clone()],
            0,
            "author",
            "feature work",
        )
        .await
        .unwrap();
    ws_mgr.update_head("feature", &snap2.id).await.unwrap();

    let feat_ws = ws_mgr.get("feature").await.unwrap().unwrap();
    assert_eq!(feat_ws.base, snap1.id);
    assert_eq!(feat_ws.head, snap2.id);
    assert_ne!(feat_ws.base, feat_ws.head);
}

#[tokio::test]
async fn integration_delete_vs_modify_conflict_in_merge() {
    let base =
        libnoa::object::TreeEntries(vec![make_entry("a.rs", "h1"), make_entry("b.rs", "h2")]);
    let ours = libnoa::object::TreeEntries(vec![make_entry("a.rs", "h1")]);
    let theirs =
        libnoa::object::TreeEntries(vec![make_entry("a.rs", "h1"), make_entry("b.rs", "h2_new")]);

    let result = libnoa::merge::three_way_merge(&base, &ours, &theirs).unwrap();
    assert!(result.has_conflicts());

    let conflicts = libnoa::merge::extract_conflicts(&result.output);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, "b.rs");
    assert_eq!(conflicts[0].ours_id, None);
    assert_eq!(conflicts[0].theirs_id, Some("h2_new".to_string()));
    assert_eq!(conflicts[0].base_id, Some("h2".to_string()));
}

#[tokio::test]
async fn integration_multiple_merge_rounds() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();

    let default_log = repo.agent_log("default").unwrap();
    default_log
        .append(&make_log_entry(
            1,
            OpType::Write,
            "main.rs",
            Some("blob1"),
            100,
        ))
        .await
        .unwrap();
    let base_engine = SnapshotEngine::new(default_log, snap_store.clone(), obj_store.clone());
    let snap1 = base_engine
        .compute("default", vec![], 0, "author", "base")
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap1.id).await.unwrap();

    let feat_a = Workspace {
        name: "feat-a".to_string(),
        head: snap1.id.clone(),
        base: snap1.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&feat_a).await.unwrap();

    let feat_b = Workspace {
        name: "feat-b".to_string(),
        head: snap1.id.clone(),
        base: snap1.id.clone(),
        agent_id: None,
        last_seq: 0,
        created_at: 2000,
        updated_at: 2000,
    };
    ws_mgr.create(&feat_b).await.unwrap();

    let log_a = repo.agent_log("feat-a").unwrap();
    log_a
        .append(&make_log_entry(
            1,
            OpType::Write,
            "a.rs",
            Some("blob_a"),
            300,
        ))
        .await
        .unwrap();
    let engine_a = SnapshotEngine::new(log_a, snap_store.clone(), obj_store.clone());
    let snap_a = engine_a
        .compute("feat-a", vec![snap1.id.clone()], 0, "author", "add a.rs")
        .await
        .unwrap();
    ws_mgr.update_head("feat-a", &snap_a.id).await.unwrap();

    let log_b = repo.agent_log("feat-b").unwrap();
    log_b
        .append(&make_log_entry(
            1,
            OpType::Write,
            "b.rs",
            Some("blob_b"),
            400,
        ))
        .await
        .unwrap();
    let engine_b = SnapshotEngine::new(log_b, snap_store.clone(), obj_store.clone());
    let snap_b = engine_b
        .compute("feat-b", vec![snap1.id.clone()], 0, "author", "add b.rs")
        .await
        .unwrap();
    ws_mgr.update_head("feat-b", &snap_b.id).await.unwrap();

    let base_tree = obj_store
        .get_tree(&libnoa::object::TreeId(snap1.tree_hash.clone()))
        .await
        .unwrap();
    let default_tree = obj_store
        .get_tree(&libnoa::object::TreeId(snap1.tree_hash.clone()))
        .await
        .unwrap();
    let a_tree = obj_store
        .get_tree(&libnoa::object::TreeId(snap_a.tree_hash.clone()))
        .await
        .unwrap();

    let merge1 = libnoa::merge::three_way_merge(&base_tree, &default_tree, &a_tree).unwrap();
    assert!(!merge1.has_conflicts());
    let merged1 = merge1.into_tree_entries(&libnoa::merge::ConflictResolution::Ours);
    let tree1_id = obj_store.put_tree(&merged1).await.unwrap();
    let merge_snap1 = libnoa::snapshot::Snapshot {
        id: content_addressed_snapshot_id(
            &tree1_id.0,
            &[snap1.id.clone(), snap_a.id.clone()],
            "default",
        ),
        tree_hash: tree1_id.0,
        parents: vec![snap1.id.clone(), snap_a.id.clone()],
        workspace: "default".to_string(),
        author: "author".to_string(),
        timestamp: 5000,
        message: "merge feat-a".to_string(),
    };
    snap_store.store(&merge_snap1).await.unwrap();
    ws_mgr
        .update_head("default", &merge_snap1.id)
        .await
        .unwrap();

    let current_tree = obj_store
        .get_tree(&libnoa::object::TreeId(merge_snap1.tree_hash.clone()))
        .await
        .unwrap();
    let b_tree = obj_store
        .get_tree(&libnoa::object::TreeId(snap_b.tree_hash.clone()))
        .await
        .unwrap();

    let merge2 = libnoa::merge::three_way_merge(&base_tree, &current_tree, &b_tree).unwrap();
    assert!(!merge2.has_conflicts());
    let merged2 = merge2.into_tree_entries(&libnoa::merge::ConflictResolution::Ours);
    let tree2_id = obj_store.put_tree(&merged2).await.unwrap();
    let merge_snap2 = libnoa::snapshot::Snapshot {
        id: content_addressed_snapshot_id(
            &tree2_id.0,
            &[merge_snap1.id.clone(), snap_b.id.clone()],
            "default",
        ),
        tree_hash: tree2_id.0,
        parents: vec![merge_snap1.id.clone(), snap_b.id.clone()],
        workspace: "default".to_string(),
        author: "author".to_string(),
        timestamp: 6000,
        message: "merge feat-b".to_string(),
    };
    snap_store.store(&merge_snap2).await.unwrap();
    ws_mgr
        .update_head("default", &merge_snap2.id)
        .await
        .unwrap();

    let final_tree = obj_store
        .get_tree(&libnoa::object::TreeId(merge_snap2.tree_hash.clone()))
        .await
        .unwrap();
    let names: Vec<&str> = final_tree.0.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"main.rs"));
    assert!(names.contains(&"a.rs"));
    assert!(names.contains(&"b.rs"));
}

// ============================================================
// Batch 1: Multi-branch lifecycle (Git t3200 patterns)
// ============================================================

#[tokio::test]
async fn branch_create_fails_on_duplicate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let ws = Workspace {
        name: "feature".to_string(),
        head: SnapshotId("noa_empty".to_string()),
        base: SnapshotId("noa_empty".to_string()),
        agent_id: None,
        last_seq: 0,
        created_at: 1000,
        updated_at: 1000,
    };
    ws_mgr.create(&ws).await.unwrap();
    assert!(ws_mgr.create(&ws).await.is_err());
}

#[tokio::test]
async fn branch_delete_default_succeeds_at_data_layer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    ws_mgr.delete("default").await.unwrap();
    assert!(ws_mgr.get("default").await.unwrap().is_none());
}

#[tokio::test]
async fn branch_delete_nonexistent_is_ok() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let result = ws_mgr.delete("ghost").await;
    assert!(result.is_ok());
    assert!(ws_mgr.get("ghost").await.unwrap().is_none());
}

#[tokio::test]
async fn branch_create_many() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let base = ws_mgr.get("default").await.unwrap().unwrap();
    for i in 0..20 {
        ws_mgr
            .create(&Workspace {
                name: format!("b-{}", i),
                head: base.head.clone(),
                base: base.head.clone(),
                agent_id: None,
                last_seq: 0,
                created_at: 1000 + i,
                updated_at: 1000 + i,
            })
            .await
            .unwrap();
    }
    assert_eq!(ws_mgr.list().await.unwrap().len(), 21);
}

#[tokio::test]
async fn branch_create_delete_all() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let base = ws_mgr.get("default").await.unwrap().unwrap();
    for i in 0..5 {
        ws_mgr
            .create(&Workspace {
                name: format!("t-{}", i),
                head: base.head.clone(),
                base: base.head.clone(),
                agent_id: None,
                last_seq: 0,
                created_at: 1000,
                updated_at: 1000,
            })
            .await
            .unwrap();
    }
    for i in 0..5 {
        ws_mgr.delete(&format!("t-{}", i)).await.unwrap();
    }
    let rem = ws_mgr.list().await.unwrap();
    assert_eq!(rem.len(), 1);
    assert_eq!(rem[0].name, "default");
}

#[tokio::test]
async fn branch_switch_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let base = ws_mgr.get("default").await.unwrap().unwrap();
    ws_mgr
        .create(&Workspace {
            name: "feature".to_string(),
            head: base.head.clone(),
            base: base.head.clone(),
            agent_id: None,
            last_seq: 0,
            created_at: 1000,
            updated_at: 1000,
        })
        .await
        .unwrap();
    repo.write_head("feature").unwrap();
    assert_eq!(repo.read_head().unwrap(), "feature");
    repo.write_head("default").unwrap();
    assert_eq!(repo.read_head().unwrap(), "default");
}

#[tokio::test]
async fn branch_fork_inherits_base() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let obj_store = repo.object_store().unwrap();
    let snap_store = repo.snapshot_store().unwrap();
    let log = repo.agent_log("default").unwrap();
    log.append(&make_log_entry(1, OpType::Write, "a.rs", Some("b"), 100))
        .await
        .unwrap();
    let engine = SnapshotEngine::new(log, snap_store.clone(), obj_store);
    let snap = engine
        .compute("default", vec![], 0, "a", "base")
        .await
        .unwrap();
    ws_mgr.update_head("default", &snap.id).await.unwrap();
    ws_mgr
        .create(&Workspace {
            name: "f1".to_string(),
            head: snap.id.clone(),
            base: snap.id.clone(),
            agent_id: None,
            last_seq: 0,
            created_at: 2000,
            updated_at: 2000,
        })
        .await
        .unwrap();
    ws_mgr
        .create(&Workspace {
            name: "f2".to_string(),
            head: snap.id.clone(),
            base: snap.id.clone(),
            agent_id: None,
            last_seq: 0,
            created_at: 2000,
            updated_at: 2000,
        })
        .await
        .unwrap();
    assert_eq!(ws_mgr.get("f1").await.unwrap().unwrap().base, snap.id);
    assert_eq!(ws_mgr.get("f2").await.unwrap().unwrap().base, snap.id);
}

#[tokio::test]
async fn branch_with_agent_id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    ws_mgr
        .create(&Workspace {
            name: "a".to_string(),
            head: SnapshotId("noa_empty".to_string()),
            base: SnapshotId("noa_empty".to_string()),
            agent_id: Some("agent-007".to_string()),
            last_seq: 0,
            created_at: 1000,
            updated_at: 1000,
        })
        .await
        .unwrap();
    assert_eq!(
        ws_mgr.get("a").await.unwrap().unwrap().agent_id,
        Some("agent-007".to_string())
    );
}

// ============================================================
// Batch 2: Merge conflict matrix (Git t7600/t7605 patterns)
// ============================================================

fn te(name: &str, id: &str) -> libnoa::object::TreeEntry {
    make_entry(name, id)
}
fn tr(entries: Vec<libnoa::object::TreeEntry>) -> libnoa::object::TreeEntries {
    libnoa::object::TreeEntries(entries)
}

#[test]
fn merge_modify_modify_conflict() {
    let base = tr(vec![te("a.rs", "h0")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h1")]),
        &tr(vec![te("a.rs", "h2")]),
    )
    .unwrap();
    assert!(r.has_conflicts());
    let c = libnoa::merge::extract_conflicts(&r.output);
    assert_eq!(c.len(), 1);
    assert_eq!(
        c[0],
        libnoa::merge::FileConflict {
            path: "a.rs".into(),
            ours_id: Some("h1".into()),
            theirs_id: Some("h2".into()),
            base_id: Some("h0".into())
        }
    );
}

#[test]
fn merge_modify_same_content_no_conflict() {
    let base = tr(vec![te("a.rs", "h0")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h1")]),
        &tr(vec![te("a.rs", "h1")]),
    )
    .unwrap();
    assert!(!r.has_conflicts());
}

#[test]
fn merge_add_add_conflict() {
    let base = tr(vec![]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("n.rs", "h1")]),
        &tr(vec![te("n.rs", "h2")]),
    )
    .unwrap();
    assert!(r.has_conflicts());
    let c = libnoa::merge::extract_conflicts(&r.output);
    assert_eq!(c[0].base_id, None);
}

#[test]
fn merge_add_add_same_no_conflict() {
    let base = tr(vec![]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("n.rs", "h1")]),
        &tr(vec![te("n.rs", "h1")]),
    )
    .unwrap();
    assert!(!r.has_conflicts());
}

#[test]
fn merge_delete_modify_conflict() {
    let base = tr(vec![te("a.rs", "h0"), te("b.rs", "h1")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h0")]),
        &tr(vec![te("a.rs", "h0"), te("b.rs", "h2")]),
    )
    .unwrap();
    assert!(r.has_conflicts());
    let c = libnoa::merge::extract_conflicts(&r.output);
    assert_eq!(c[0].ours_id, None);
    assert_eq!(c[0].theirs_id, Some("h2".into()));
}

#[test]
fn merge_modify_delete_conflict() {
    let base = tr(vec![te("a.rs", "h0")]);
    let r =
        libnoa::merge::three_way_merge(&base, &tr(vec![te("a.rs", "h1")]), &tr(vec![])).unwrap();
    assert!(r.has_conflicts());
    let c = libnoa::merge::extract_conflicts(&r.output);
    assert_eq!(c[0].ours_id, Some("h1".into()));
    assert_eq!(c[0].theirs_id, None);
}

#[test]
fn merge_both_delete_no_conflict() {
    let base = tr(vec![te("a.rs", "h0"), te("b.rs", "h1")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h0")]),
        &tr(vec![te("a.rs", "h0")]),
    )
    .unwrap();
    assert!(!r.has_conflicts());
}

#[test]
fn merge_different_files_no_conflict() {
    let base = tr(vec![te("a.rs", "h0")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h0"), te("b.rs", "h1")]),
        &tr(vec![te("a.rs", "h0"), te("c.rs", "h2")]),
    )
    .unwrap();
    assert!(!r.has_conflicts());
    let t = r.into_tree_entries(&libnoa::merge::ConflictResolution::Ours);
    let n: Vec<&str> = t.0.iter().map(|e| e.name.as_str()).collect();
    assert!(n.contains(&"b.rs"));
    assert!(n.contains(&"c.rs"));
}

#[test]
fn merge_empty_all_no_conflict() {
    let e = tr(vec![]);
    let r = libnoa::merge::three_way_merge(&e, &e, &e).unwrap();
    assert!(!r.has_conflicts());
    assert!(r
        .into_tree_entries(&libnoa::merge::ConflictResolution::Ours)
        .0
        .is_empty());
}

#[test]
fn merge_ours_strategy() {
    let base = tr(vec![te("a.rs", "h0")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h1")]),
        &tr(vec![te("a.rs", "h2")]),
    )
    .unwrap();
    assert_eq!(
        r.into_tree_entries(&libnoa::merge::ConflictResolution::Ours)
            .0[0]
            .id,
        "h1"
    );
}

#[test]
fn merge_theirs_strategy() {
    let base = tr(vec![te("a.rs", "h0")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "h1")]),
        &tr(vec![te("a.rs", "h2")]),
    )
    .unwrap();
    assert_eq!(
        r.into_tree_entries(&libnoa::merge::ConflictResolution::Theirs)
            .0[0]
            .id,
        "h2"
    );
}

#[test]
fn merge_multi_conflict() {
    let base = tr(vec![te("a.rs", "h0"), te("b.rs", "h1"), te("c.rs", "h2")]);
    let r = libnoa::merge::three_way_merge(
        &base,
        &tr(vec![te("a.rs", "x"), te("b.rs", "y"), te("c.rs", "h2")]),
        &tr(vec![te("a.rs", "z"), te("b.rs", "w"), te("c.rs", "h2")]),
    )
    .unwrap();
    assert_eq!(libnoa::merge::extract_conflicts(&r.output).len(), 2);
}

#[test]
fn merge_stress_100_files() {
    let mut b = Vec::new();
    let mut o = Vec::new();
    let mut t = Vec::new();
    for i in 0..100 {
        b.push(te(&format!("f{}.rs", i), &format!("b{}", i)));
        if i % 4 == 0 {
            o.push(te(&format!("f{}.rs", i), &format!("o{}", i)));
        } else {
            o.push(te(&format!("f{}.rs", i), &format!("b{}", i)));
        }
        if i % 4 == 0 {
            t.push(te(&format!("f{}.rs", i), &format!("t{}", i)));
        } else {
            t.push(te(&format!("f{}.rs", i), &format!("b{}", i)));
        }
    }
    let r = libnoa::merge::three_way_merge(&tr(b), &tr(o), &tr(t)).unwrap();
    assert_eq!(libnoa::merge::extract_conflicts(&r.output).len(), 25);
}

#[test]
fn merge_idempotent() {
    let base = tr(vec![te("a.rs", "h0")]);
    let ours = tr(vec![te("a.rs", "h1")]);
    let theirs = tr(vec![te("a.rs", "h2")]);
    let r1 = libnoa::merge::three_way_merge(&base, &ours, &theirs).unwrap();
    let r2 = libnoa::merge::three_way_merge(&base, &ours, &theirs).unwrap();
    assert_eq!(
        libnoa::merge::extract_conflicts(&r1.output),
        libnoa::merge::extract_conflicts(&r2.output)
    );
}

// ============================================================
// Batch 3: Snapshot, compaction, diff edge cases
// ============================================================

#[tokio::test]
async fn snapshot_empty_tree() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    let engine = SnapshotEngine::new(
        log,
        repo.snapshot_store().unwrap(),
        repo.object_store().unwrap(),
    );
    let s = engine
        .compute("default", vec![], 0, "a", "empty")
        .await
        .unwrap();
    let t = engine
        .object_store
        .get_tree(&libnoa::object::TreeId(s.tree_hash.clone()))
        .await
        .unwrap();
    assert!(t.0.is_empty());
}

#[tokio::test]
async fn snapshot_delete_removes_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    let obj = repo.object_store().unwrap();
    let engine = SnapshotEngine::new(log, repo.snapshot_store().unwrap(), obj);
    engine
        .log
        .append(&make_log_entry(1, OpType::Write, "a.rs", Some("ba"), 100))
        .await
        .unwrap();
    engine
        .log
        .append(&make_log_entry(2, OpType::Write, "b.rs", Some("bb"), 200))
        .await
        .unwrap();
    let s1 = engine
        .compute("default", vec![], 0, "a", "w/b")
        .await
        .unwrap();
    engine
        .log
        .append(&make_log_entry(3, OpType::Delete, "b.rs", None, 300))
        .await
        .unwrap();
    let s2 = engine
        .compute("default", vec![s1.id], 0, "a", "del b")
        .await
        .unwrap();
    let t = engine
        .object_store
        .get_tree(&libnoa::object::TreeId(s2.tree_hash.clone()))
        .await
        .unwrap();
    assert!(t.0.iter().any(|e| e.name == "a.rs"));
    assert!(!t.0.iter().any(|e| e.name == "b.rs"));
}

#[tokio::test]
async fn snapshot_rename() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    let obj = repo.object_store().unwrap();
    let engine = SnapshotEngine::new(log, repo.snapshot_store().unwrap(), obj);
    engine
        .log
        .append(&make_log_entry(1, OpType::Write, "old.rs", Some("b1"), 100))
        .await
        .unwrap();
    let s1 = engine
        .compute("default", vec![], 0, "a", "init")
        .await
        .unwrap();
    engine
        .log
        .append(&LogEntry {
            seq: 2,
            op: OpType::Rename,
            path: Some("new.rs".into()),
            blob_id: None,
            from_path: Some("old.rs".into()),
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 200,
            message: None,
        })
        .await
        .unwrap();
    let s2 = engine
        .compute("default", vec![s1.id], 0, "a", "mv")
        .await
        .unwrap();
    let t = engine
        .object_store
        .get_tree(&libnoa::object::TreeId(s2.tree_hash.clone()))
        .await
        .unwrap();
    assert!(!t.0.iter().any(|e| e.name == "old.rs"));
    assert!(t.0.iter().any(|e| e.name == "new.rs"));
}

#[tokio::test]
async fn compaction_removes_old() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    for i in 0..20 {
        log.append(&make_log_entry(
            i + 1,
            OpType::Write,
            &format!("f{}.rs", i),
            Some(&format!("b{}", i)),
            (i + 1) * 100,
        ))
        .await
        .unwrap();
    }
    log.compact_to(10).await.unwrap();
    let rem = log.read_all().await.unwrap();
    assert!(rem.len() < 20);
    assert!(rem.iter().all(|e| e.seq > 10));
}

#[tokio::test]
async fn incremental_reads_new_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    let log = repo.agent_log("default").unwrap();
    let obj = repo.object_store().unwrap();
    let engine = SnapshotEngine::new(log, repo.snapshot_store().unwrap(), obj);
    engine
        .log
        .append(&make_log_entry(1, OpType::Write, "a.rs", Some("ba"), 100))
        .await
        .unwrap();
    let s1 = engine
        .compute("default", vec![], 0, "a", "1st")
        .await
        .unwrap();
    ws_mgr
        .update_head_and_seq("default", &s1.id, 1)
        .await
        .unwrap();
    engine
        .log
        .append(&make_log_entry(2, OpType::Write, "b.rs", Some("bb"), 200))
        .await
        .unwrap();
    engine
        .log
        .append(&make_log_entry(3, OpType::Write, "c.rs", Some("bc"), 300))
        .await
        .unwrap();
    let ws = ws_mgr.get("default").await.unwrap().unwrap();
    let s2 = engine
        .compute("default", vec![s1.id], ws.last_seq, "a", "inc")
        .await
        .unwrap();
    let t = engine
        .object_store
        .get_tree(&libnoa::object::TreeId(s2.tree_hash.clone()))
        .await
        .unwrap();
    let n: Vec<&str> = t.0.iter().map(|e| e.name.as_str()).collect();
    assert!(n.contains(&"a.rs") && n.contains(&"b.rs") && n.contains(&"c.rs"));
}

#[tokio::test]
async fn parent_chain_children_of() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    let ss = repo.snapshot_store().unwrap();
    let engine = SnapshotEngine::new(log, ss.clone(), repo.object_store().unwrap());
    engine
        .log
        .append(&make_log_entry(1, OpType::Write, "a.rs", Some("b"), 100))
        .await
        .unwrap();
    let s1 = engine
        .compute("default", vec![], 0, "a", "1")
        .await
        .unwrap();
    engine
        .log
        .append(&make_log_entry(2, OpType::Write, "b.rs", Some("b"), 200))
        .await
        .unwrap();
    let s2 = engine
        .compute("default", vec![s1.id.clone()], 0, "a", "2")
        .await
        .unwrap();
    engine
        .log
        .append(&make_log_entry(3, OpType::Write, "c.rs", Some("b"), 300))
        .await
        .unwrap();
    let s3 = engine
        .compute("default", vec![s2.id.clone()], 0, "a", "3")
        .await
        .unwrap();
    assert_eq!(ss.children_of(&s1.id).await.unwrap().len(), 1);
    assert_eq!(ss.children_of(&s2.id).await.unwrap().len(), 1);
    assert!(ss.children_of(&s3.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn merge_parent_in_children_of() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    let ss = repo.snapshot_store().unwrap();
    let engine = SnapshotEngine::new(log, ss.clone(), repo.object_store().unwrap());
    engine
        .log
        .append(&make_log_entry(1, OpType::Write, "a.rs", Some("b"), 100))
        .await
        .unwrap();
    let s1 = engine
        .compute("default", vec![], 0, "a", "base")
        .await
        .unwrap();
    let ms = engine
        .compute(
            "default",
            vec![s1.id.clone(), SnapshotId("noa_other".into())],
            0,
            "a",
            "merge",
        )
        .await
        .unwrap();
    assert_eq!(ms.parents.len(), 2);
    assert_eq!(ss.children_of(&s1.id).await.unwrap()[0], ms.id);
}

#[test]
fn diff_added_modified_deleted() {
    let a = tr(vec![te("a.rs", "h1"), te("b.rs", "h2")]);
    let b = tr(vec![te("a.rs", "h1c"), te("c.rs", "h3")]);
    let d = libnoa::snapshot::diff_snapshots(&a.0, &b.0);
    assert_eq!(d.len(), 3);
    assert!(d
        .iter()
        .any(|x| x.path == "a.rs" && matches!(x.kind, libnoa::snapshot::DiffKind::Modified)));
    assert!(d
        .iter()
        .any(|x| x.path == "b.rs" && matches!(x.kind, libnoa::snapshot::DiffKind::Deleted)));
    assert!(d
        .iter()
        .any(|x| x.path == "c.rs" && matches!(x.kind, libnoa::snapshot::DiffKind::Added)));
}

#[tokio::test]
async fn log_read_since_filters() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let log = repo.agent_log("default").unwrap();
    for i in 0..10 {
        log.append(&make_log_entry(
            i + 1,
            OpType::Write,
            &format!("f{}.rs", i),
            Some(&format!("b{}", i)),
            (i + 1) * 100,
        ))
        .await
        .unwrap();
    }
    let s5 = log.read_since(5).await.unwrap();
    assert!(s5.iter().all(|e| e.seq >= 5));
}

#[tokio::test]
async fn workspace_update_head_and_seq() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    let ws_mgr = repo.workspace_manager().unwrap();
    assert_eq!(ws_mgr.get("default").await.unwrap().unwrap().last_seq, 0);
    ws_mgr
        .update_head_and_seq("default", &SnapshotId("noa_x".into()), 42)
        .await
        .unwrap();
    let u = ws_mgr.get("default").await.unwrap().unwrap();
    assert_eq!(u.head, SnapshotId("noa_x".into()));
    assert_eq!(u.last_seq, 42);
}
