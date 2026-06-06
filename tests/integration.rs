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
                (i + 1) as u64 * 100,
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
