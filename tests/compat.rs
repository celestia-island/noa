use std::{path::Path, process::Command};

fn git_lfs_available() -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn svn_available() -> bool {
    Command::new("svn")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_init_bare(path: &Path) {
    Command::new("git")
        .args(["init", "--bare"])
        .arg(path)
        .output()
        .unwrap();
}

fn git_commit(path: &Path, msg: &str) {
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(path)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
}

fn sha256_hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let hash = <sha2::Sha256 as sha2::Digest>::digest(data);
    hash.iter().fold(String::with_capacity(64), |mut s, b| {
        write!(&mut s, "{:02x}", b).unwrap();
        s
    })
}

#[test]
fn test_lfs_pointer_detection() {
    let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:abc123\nsize 12345\n";
    assert!(libnoa::git::is_lfs_pointer(pointer));

    let not_pointer = b"Hello, this is regular file content";
    assert!(!libnoa::git::is_lfs_pointer(not_pointer));

    let too_long = &[b'x'; 501];
    assert!(!libnoa::git::is_lfs_pointer(too_long));

    let binary: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a];
    assert!(!libnoa::git::is_lfs_pointer(binary));
}

#[test]
fn test_lfs_pointer_edge_cases() {
    let empty: &[u8] = b"";
    assert!(!libnoa::git::is_lfs_pointer(empty));

    let exactly_v1 = b"version https://git-lfs.github.com/spec/v1";
    assert!(libnoa::git::is_lfs_pointer(exactly_v1));

    let v2 = b"version https://git-lfs.github.com/spec/v2\noid sha256:def456\nsize 999\n";
    assert!(libnoa::git::is_lfs_pointer(v2));
}

#[test]
fn test_bitbucket_url_format_handling() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path();

    {
        libnoa::repo::Repository::init(path).unwrap();
    }
    let mut repo = libnoa::repo::Repository::open(path).unwrap();

    repo.config.add_remote(libnoa::config::RemoteConfig {
        name: "bitbucket-ssh".to_string(),
        url: "git@bitbucket.org:workspace/repo.git".to_string(),
        protocol: libnoa::config::RemoteProtocol::Git,
        pr: None,
    });
    repo.config.add_remote(libnoa::config::RemoteConfig {
        name: "bitbucket-https".to_string(),
        url: "https://user@bitbucket.org/workspace/repo.git".to_string(),
        protocol: libnoa::config::RemoteProtocol::Git,
        pr: None,
    });
    repo.config.save_to_dir(&path.join(".noa")).unwrap();

    let loaded = libnoa::config::RepoConfig::load_from_dir(&path.join(".noa")).unwrap();
    assert_eq!(loaded.remotes.len(), 2);

    let ssh = loaded.get_remote("bitbucket-ssh").unwrap();
    assert_eq!(ssh.url, "git@bitbucket.org:workspace/repo.git");

    let https = loaded.get_remote("bitbucket-https").unwrap();
    assert_eq!(https.url, "https://user@bitbucket.org/workspace/repo.git");
}

#[test]
fn test_multiple_remote_protocols() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path();

    {
        libnoa::repo::Repository::init(path).unwrap();
    }
    let mut repo = libnoa::repo::Repository::open(path).unwrap();

    repo.config.add_remote(libnoa::config::RemoteConfig {
        name: "github".to_string(),
        url: "https://github.com/user/repo.git".to_string(),
        protocol: libnoa::config::RemoteProtocol::Git,
        pr: None,
    });
    repo.config.add_remote(libnoa::config::RemoteConfig {
        name: "gitlab".to_string(),
        url: "git@gitlab.com:user/repo.git".to_string(),
        protocol: libnoa::config::RemoteProtocol::Git,
        pr: None,
    });
    repo.config.add_remote(libnoa::config::RemoteConfig {
        name: "svn-origin".to_string(),
        url: "https://svn.example.com/repo/trunk".to_string(),
        protocol: libnoa::config::RemoteProtocol::Svn,
        pr: None,
    });
    repo.config.save_to_dir(&path.join(".noa")).unwrap();

    let loaded = libnoa::config::RepoConfig::load_from_dir(&path.join(".noa")).unwrap();
    assert_eq!(loaded.remotes.len(), 3);
    assert_eq!(
        loaded.get_remote("github").unwrap().protocol,
        libnoa::config::RemoteProtocol::Git
    );
    assert_eq!(
        loaded.get_remote("svn-origin").unwrap().protocol,
        libnoa::config::RemoteProtocol::Svn
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_git_clone_and_push_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let remote_path = tmp.path().join("remote.git");
    let source_path = tmp.path().join("source");
    let cloned_path = tmp.path().join("cloned");

    git_init_bare(&remote_path);

    std::fs::create_dir_all(&source_path).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&source_path)
        .output()
        .unwrap();
    std::fs::write(source_path.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(source_path.join("lib.rs"), "pub fn lib() {}").unwrap();
    git_commit(&source_path, "initial");
    Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(&remote_path)
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "-u", "origin", "master"])
        .current_dir(&source_path)
        .output()
        .unwrap();

    libnoa::git::clone_git_to_noa(&remote_path.to_string_lossy(), &cloned_path)
        .await
        .unwrap();

    assert!(cloned_path.join(".git").exists());
    let noa_dir = libnoa::repo::Repository::resolve_noa_dir(&cloned_path);
    assert!(noa_dir.exists());
    assert!(cloned_path.join("main.rs").exists());
    assert!(cloned_path.join("lib.rs").exists());
}

#[test]
fn test_export_noa_to_git_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let remote_path = tmp.path().join("remote.git");
    let source_path = tmp.path().join("source");
    let work_path = tmp.path().join("work");

    git_init_bare(&remote_path);

    std::fs::create_dir_all(&source_path).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&source_path)
        .output()
        .unwrap();
    std::fs::write(source_path.join("main.rs"), "fn main() {}").unwrap();
    git_commit(&source_path, "initial");
    Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(&remote_path)
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "-u", "origin", "master"])
        .current_dir(&source_path)
        .output()
        .unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        libnoa::git::clone_git_to_noa(&remote_path.to_string_lossy(), &work_path)
            .await
            .unwrap();
    });

    let new_content = "fn main() { println!(\"updated\"); }";
    std::fs::write(work_path.join("main.rs"), new_content).unwrap();

    let blob_id = sha256_hex(new_content.as_bytes());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;

    let log_entry = format!(
        r#"{{"seq":1,"op":"write","path":"main.rs","blob_id":"{}","ts":{}}}"#,
        blob_id, ts
    );
    let noa_dir = libnoa::repo::Repository::resolve_noa_dir(&work_path);
    std::fs::create_dir_all(noa_dir.join("agent-logs")).unwrap();
    std::fs::write(noa_dir.join("agent-logs").join("default.log"), &log_entry).unwrap();

    let rt2 = tokio::runtime::Runtime::new().unwrap();
    rt2.block_on(async {
        let repo = libnoa::repo::Repository::open(&work_path).unwrap();
        libnoa::cli::snapshot_cmd::run_create(&repo, "update main", "test")
            .await
            .unwrap();
        let db = std::sync::Arc::clone(&repo.db);
        drop(repo);
        libnoa::git::export_noa_to_git(&work_path, db)
            .await
            .unwrap();
    });

    let push_output = Command::new("git")
        .args(["push", &remote_path.to_string_lossy()])
        .current_dir(&work_path)
        .output()
        .unwrap();
    assert!(push_output.status.success());

    let log_output = Command::new("git")
        .args(["log", "--oneline", "-3"])
        .current_dir(&remote_path)
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(log_str.contains("initial") || log_str.contains("noa export"));
}

#[test]
#[ignore = "requires git-lfs on PATH; run with `cargo test -- --ignored` on a host that has it"]
fn test_git_lfs_clone_roundtrip() {
    if !git_lfs_available() {
        panic!(
            "git-lfs is required for the LFS clone regression test; \
             install it (`git lfs install`) or run this test on a host that has it. \
             Silent skip was removed because this is the only coverage for the \
             LFS-aware clone path."
        );
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let source_path = tmp.path().join("source");
    let remote_path = tmp.path().join("remote.git");
    let cloned_path = tmp.path().join("cloned");

    std::fs::create_dir_all(&source_path).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["lfs", "install"])
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["lfs", "track", "*.bin"])
        .current_dir(&source_path)
        .output()
        .unwrap();

    std::fs::write(
        source_path.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    std::fs::write(source_path.join("data.bin"), &large_data).unwrap();
    std::fs::write(source_path.join("readme.txt"), "hello").unwrap();

    git_commit(&source_path, "add LFS files");

    Command::new("git")
        .args(["clone", "--bare", "."])
        .arg(&remote_path)
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(&remote_path)
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "-u", "origin", "master"])
        .current_dir(&source_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["lfs", "push", "--all", "origin"])
        .current_dir(&source_path)
        .output()
        .unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        libnoa::git::clone_git_to_noa(&remote_path.to_string_lossy(), &cloned_path)
            .await
            .unwrap();
    });

    assert!(cloned_path.join("data.bin").exists());
    assert!(cloned_path.join("readme.txt").exists());
    assert!(cloned_path.join(".noa").exists());

    let bin_content = std::fs::read(cloned_path.join("data.bin")).unwrap();
    assert_eq!(bin_content.len(), 10000);
    assert!(!libnoa::git::is_lfs_pointer(&bin_content));
}

#[test]
#[ignore = "requires svn on PATH; run with `cargo test -- --ignored` on a host that has it"]
fn test_svn_export_and_import() {
    if !svn_available() {
        panic!(
            "svn is required for the SVN-export → git-import regression test; \
             install it or run this test on a host that has it. Silent skip was \
             removed because this is the only end-to-end coverage of the svn bridge."
        );
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let svn_repo = tmp.path().join("svn-repo");
    let svn_wc = tmp.path().join("svn-wc");
    let noa_path = tmp.path().join("noa-import");

    Command::new("svnadmin")
        .args(["create"])
        .arg(&svn_repo)
        .output()
        .unwrap();

    let svn_url = format!("file://{}", svn_repo.display());
    Command::new("svn")
        .args(["mkdir", "-m", "init", &format!("{}/trunk", svn_url)])
        .output()
        .unwrap();

    std::fs::create_dir_all(&svn_wc).unwrap();
    Command::new("svn")
        .args(["checkout", &svn_url, &svn_wc.to_string_lossy()])
        .output()
        .unwrap();

    std::fs::write(svn_wc.join("trunk").join("README.md"), "SVN project").unwrap();
    std::fs::write(svn_wc.join("trunk").join("main.rs"), "fn main() {}").unwrap();
    Command::new("svn")
        .args(["add", "trunk/README.md", "trunk/main.rs"])
        .current_dir(&svn_wc)
        .output()
        .unwrap();
    Command::new("svn")
        .args(["commit", "-m", "initial"])
        .current_dir(&svn_wc)
        .output()
        .unwrap();

    std::fs::create_dir_all(&noa_path).unwrap();
    Command::new("svn")
        .args(["export", "--force", &format!("{}/trunk", svn_url)])
        .arg(&noa_path)
        .output()
        .unwrap();

    assert!(noa_path.join("README.md").exists());
    assert!(noa_path.join("main.rs").exists());

    Command::new("git")
        .args(["init"])
        .current_dir(&noa_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&noa_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "imported from SVN"])
        .current_dir(&noa_path)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        libnoa::git::import::import_git_to_noa(&noa_path, {
            let db_path = noa_path.join(".noa").join("noa.redb");
            std::fs::create_dir_all(noa_path.join(".noa").join("agent-logs")).unwrap();
            let db = std::sync::Arc::new(redb::Database::builder().create(&db_path).unwrap());
            {
                let txn = db.begin_write().unwrap();
                {
                    // Propagate errors from open_table — a redb schema or
                    // version mismatch here would otherwise surface as a
                    // confusing downstream "table not found" error during
                    // import_git_to_noa, instead of pointing at the actual
                    // setup failure.
                    let _ = txn
                        .open_table(redb::TableDefinition::<&[u8], &[u8]>::new("blobs"))
                        .expect("create blobs table");
                    let _ = txn
                        .open_table(redb::TableDefinition::<&[u8], &[u8]>::new("trees"))
                        .expect("create trees table");
                    let _ = txn
                        .open_table(redb::TableDefinition::<&str, &[u8]>::new("snapshots"))
                        .expect("create snapshots table");
                    let _ = txn
                        .open_table(redb::TableDefinition::<&str, &[u8]>::new("workspaces"))
                        .expect("create workspaces table");
                    let _ = txn
                        .open_table(redb::TableDefinition::<&str, &[u8]>::new("refs"))
                        .expect("create refs table");
                }
                txn.commit().unwrap();
            }
            db
        })
        .await
        .unwrap();
    });

    assert!(noa_path.join(".noa/noa.redb").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn test_git_import_no_deadlock_on_single_thread_runtime() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    Command::new("git")
        .args(["init"])
        .current_dir(&root)
        .output()
        .unwrap();

    std::fs::write(root.join("hello.txt"), b"hello world").unwrap();
    git_commit(&root, "initial");

    let db_path = root.join(".noa").join("noa.redb");
    std::fs::create_dir_all(root.join(".noa").join("agent-logs")).unwrap();
    let db = std::sync::Arc::new(redb::Database::builder().create(&db_path).unwrap());
    {
        let txn = db.begin_write().unwrap();
        {
            // Propagate errors — see the sibling setup block above.
            let _ = txn
                .open_table::<&[u8], &[u8]>(redb::TableDefinition::new("blobs"))
                .expect("create blobs table");
            let _ = txn
                .open_table::<&[u8], &[u8]>(redb::TableDefinition::new("trees"))
                .expect("create trees table");
            let _ = txn
                .open_table::<&str, &[u8]>(redb::TableDefinition::new("snapshots"))
                .expect("create snapshots table");
            let _ = txn
                .open_table::<&str, &[u8]>(redb::TableDefinition::new("workspaces"))
                .expect("create workspaces table");
            let _ = txn
                .open_table::<&str, &[u8]>(redb::TableDefinition::new("refs"))
                .expect("create refs table");
        }
        txn.commit().unwrap();
    }

    // This would deadlock before the fix on single-thread runtime
    libnoa::git::import::import_git_to_noa(&root, db)
        .await
        .unwrap();

    assert!(root.join(".noa/noa.redb").exists());
}

// ---- verify.run/1 ledger entries (OpType::Verify) ----------------------
//
// Compatibility contract of the first verify-ledger cut:
//   * old log lines (no `verify` field) keep parsing, payload == None;
//   * new lines carrying a full verify payload roundtrip losslessly;
//   * old binaries reading new lines only ever drop data gracefully
//     (unknown field ignored; verify-op line skipped by the reader's
//     corrupted-line path) — pinned at unit level in src/log/format.rs.

#[tokio::test]
async fn test_verify_entry_appended_to_old_format_log() {
    use libnoa::log::{
        AgentLog, FileAgentLog, LogEntry, OpType, VerifyCommand, VerifyEnv, VerifyKind,
        VerifyNotRun, VerifyRecord,
    };

    let tmp = tempfile::TempDir::new().unwrap();
    let log_path = tmp.path().join("default.log");

    // A pre-change log file on disk: two hand-written old-format lines.
    std::fs::write(
        &log_path,
        concat!(
            r#"{"seq":1,"op":"write","path":"a.rs","blob_id":"hash-a","ts":100}"#,
            "\n",
            r#"{"seq":2,"op":"snapshot","snapshot_id":"snap-1","ts":200}"#,
            "\n",
        ),
    )
    .unwrap();

    let log = FileAgentLog::create(&log_path).unwrap();

    let record = VerifyRecord {
        schema: "verify.run/1".to_string(),
        kind: VerifyKind::Partial,
        repo: "celestia-island/noa".to_string(),
        head: "0123456789abcdef0123456789abcdef01234567".to_string(),
        tree_clean: false,
        env: VerifyEnv {
            toolchain: "stable 1.82.0".to_string(),
            platform: "x86_64-unknown-linux-gnu".to_string(),
            lock_hash: Some("deadbeef".to_string()),
            offline: true,
        },
        commands: vec![VerifyCommand {
            argv: vec![
                "cargo".to_string(),
                "check".to_string(),
                "--offline".to_string(),
                "--locked".to_string(),
                "--quiet".to_string(),
            ],
            exit_code: Some(0),
            duration_ms: Some(42_000),
            summary: Some("cargo check clean".to_string()),
        }],
        not_run: vec![
            VerifyNotRun {
                what: "--all-features".to_string(),
                reason: Some("feature graph not wired on this runner".to_string()),
            },
            VerifyNotRun {
                what: "tests/webui".to_string(),
                reason: None,
            },
        ],
        outputs: Vec::new(),
        agent: "verify-impl".to_string(),
        ts: 1_736_000_000_000_000,
    };

    let entry = LogEntry {
        // append() reassigns seq from durable state; the old file tops at 2.
        seq: 0,
        op: OpType::Verify,
        path: None,
        blob_id: None,
        from_path: None,
        resolved_conflict_ours_id: None,
        resolved_conflict_theirs_id: None,
        snapshot_id: None,
        remote_seq: None,
        remote_sender: None,
        verify: Some(record),
        ts: 300,
        message: Some("verify.run ledger entry".to_string()),
    };

    let assigned = log.append(&entry).await.unwrap();
    assert_eq!(assigned, 3);

    let all = log.read_all().await.unwrap();
    assert_eq!(all.len(), 3);
    // Old lines parse with no payload.
    assert_eq!(all[0].op, OpType::Write);
    assert_eq!(all[0].verify, None);
    assert_eq!(all[1].op, OpType::Snapshot);
    assert_eq!(all[1].verify, None);
    // New line roundtrips through the file with its payload intact.
    assert_eq!(all[2].op, OpType::Verify);
    assert_eq!(all[2].seq, 3);
    let v = all[2].verify.as_ref().unwrap();
    assert_eq!(v.schema, "verify.run/1");
    assert_eq!(v.kind, VerifyKind::Partial);
    assert!(!v.tree_clean);
    assert!(v.env.offline);
    assert_eq!(v.env.lock_hash.as_deref(), Some("deadbeef"));
    assert_eq!(v.commands.len(), 1);
    assert_eq!(v.commands[0].exit_code, Some(0));
    assert_eq!(
        v.commands[0].argv,
        ["cargo", "check", "--offline", "--locked", "--quiet"]
    );
    assert_eq!(v.not_run.len(), 2);
    assert_eq!(v.not_run[0].what, "--all-features");
    assert_eq!(
        v.not_run[0].reason.as_deref(),
        Some("feature graph not wired on this runner")
    );
    assert_eq!(v.not_run[1].what, "tests/webui");
    assert_eq!(v.not_run[1].reason, None);
    assert!(v.outputs.is_empty());

    // Seq allocation mechanics are untouched by the new op type.
    assert_eq!(log.next_seq().await.unwrap(), 4);

    // The serialized line pins the wire shape end to end: op "verify" plus
    // the payload flattened into the JSONL line.
    let raw = std::fs::read_to_string(&log_path).unwrap();
    let verify_line = raw.lines().last().unwrap();
    assert!(verify_line.contains(r#""op":"verify""#));
    assert!(verify_line.contains(r#""schema":"verify.run/1""#));
    assert!(verify_line.contains(r#""not_run":[{"what":"--all-features""#));
}
