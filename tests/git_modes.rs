//! Regression tests for issue #72: the git bridge must preserve
//! file modes/types (100755 executable, 120000 symlink, 160000 gitlink)
//! across clone/push round-trips.
//!
//! Fixtures are built with git plumbing (`hash-object`, `update-index
//! --cacheinfo`) so they do not depend on workdir symlink privileges;
//! assertions are `git ls-tree` comparisons, exactly like the issue.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use libnoa::{
    object::{EntryKind, ObjectStore, TreeId},
    refs::{RedbRefStore, RefStore},
    snapshot::{RedbSnapshotStore, SnapshotStore},
};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ls_tree(dir: &Path) -> String {
    git(dir, &["ls-tree", "-r", "HEAD"])
}

/// Commit `oid` of a throwaway submodule repository.
fn make_submodule(tmp: &Path) -> String {
    let sub = tmp.join("submod");
    std::fs::create_dir_all(&sub).unwrap();
    git(&sub, &["init"]);
    std::fs::write(sub.join("lib.txt"), "vendored\n").unwrap();
    git(&sub, &["add", "-A"]);
    git(&sub, &["commit", "-m", "submodule commit"]);
    git(&sub, &["rev-parse", "HEAD"])
}

/// Upstream with all four #72 ingredients. `with_gitlink` mirrors the
/// issue's `upstream`; without it mirrors `upstream2` plus the adverse
/// `aaa.txt` / `zzz.txt` pair from repro 3.
fn make_upstream(tmp: &Path, name: &str, with_gitlink: bool) -> (PathBuf, String) {
    let up = tmp.join(name);
    std::fs::create_dir_all(&up).unwrap();
    git(&up, &["init"]);
    // Deterministic identity for later `git commit` calls in export.
    git(&up, &["config", "user.email", "test@test.com"]);
    git(&up, &["config", "user.name", "test"]);

    std::fs::write(up.join("regular.txt"), "regular content\n").unwrap();
    std::fs::write(up.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
    // Adverse-order pair: target sorts before its link.
    std::fs::write(up.join("aaa.txt"), "precious data\n").unwrap();
    git(&up, &["add", "-A"]);
    git(&up, &["update-index", "--chmod=+x", "--", "run.sh"]);

    // Symlinks via plumbing: no workdir privilege needed to create them.
    let oid_for = |bytes: &[u8]| {
        use std::io::Write as _;
        let mut child = Command::new("git")
            .args(["hash-object", "-w", "--stdin"])
            .current_dir(&up)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let link_blob = oid_for(b"regular.txt");
    git(
        &up,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{link_blob},link.txt"),
        ],
    );
    let zzz_blob = oid_for(b"aaa.txt");
    git(
        &up,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{zzz_blob},zzz.txt"),
        ],
    );

    let sub_oid = if with_gitlink {
        let oid = make_submodule(tmp);
        git(
            &up,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{oid},vendor"),
            ],
        );
        oid
    } else {
        String::new()
    };

    git(&up, &["commit", "-m", "upstream with modes"]);
    (up, sub_oid)
}

fn ls_line(dir: &Path, path: &str) -> String {
    ls_tree(dir)
        .lines()
        .find(|l| l.ends_with(&format!("\t{path}")))
        .unwrap_or_else(|| panic!("{path} missing from ls-tree in {}", dir.display()))
        .to_string()
}

async fn head_tree_entries(
    db: &Arc<redb::Database>,
) -> Vec<libnoa::object::TreeEntry> {
    let ref_store = RedbRefStore::new(Arc::clone(db)).unwrap();
    let snap_store = RedbSnapshotStore::new(Arc::clone(db)).unwrap();
    let obj_store = libnoa::object::RedbObjectStore::new(Arc::clone(db)).unwrap();
    let head = ref_store.get("HEAD").await.unwrap().expect("HEAD ref");
    let snap = snap_store.get(&head).await.unwrap();
    obj_store
        .get_tree(&TreeId(snap.tree_hash.clone()))
        .await
        .unwrap()
        .0
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submodule_clone_succeeds_and_roundtrips() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (upstream, sub_oid) = make_upstream(tmp.path(), "upstream", true);
    let work = tmp.path().join("work");

    // Repro 1: this hard-failed before the fix (object lookup on the
    // gitlink oid, which a plain clone never fetches).
    libnoa::git::clone_git_to_noa(&upstream.to_string_lossy(), &work)
        .await
        .unwrap();

    // The noa tree carries the gitlink oid (no object fetch attempted).
    let repo = libnoa::repo::Repository::open(&work).unwrap();
    let entries = head_tree_entries(&repo.db).await;
    let vendor = entries.iter().find(|e| e.name == "vendor").unwrap();
    assert_eq!(vendor.kind, EntryKind::Gitlink);
    assert_eq!(vendor.id, sub_oid);

    // Repro 2/3/4 verdicts: every pre-existing ls-tree line survives the
    // export byte-for-byte (export may add lines, e.g. .gitignore, but must
    // never change one).
    let before = ls_tree(&work);
    assert!(before.contains("160000 commit"));
    let db = Arc::clone(&repo.db);
    drop(repo);
    libnoa::git::export_noa_to_git(&work, db).await.unwrap();
    let after = ls_tree(&work);
    for line in before.lines() {
        assert!(
            after.lines().any(|l| l == line),
            "export changed ls-tree line: {line}\nbefore:\n{before}\nafter:\n{after}"
        );
    }
    assert!(ls_line(&work, "vendor").starts_with(&format!("160000 commit {sub_oid}")));
    assert!(ls_line(&work, "link.txt").starts_with("120000 blob "));
    assert!(ls_line(&work, "run.sh").starts_with("100755 blob "));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_symlink_target_survives_push_and_modes_stick() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (upstream, _) = make_upstream(tmp.path(), "upstream2", false);
    let work = tmp.path().join("work2");

    libnoa::git::clone_git_to_noa(&upstream.to_string_lossy(), &work)
        .await
        .unwrap();

    // Mirror the issue: delete the link from disk, then push. Export must
    // restore it as a link (120000), keep the exec bit, and — critically —
    // never write link bytes through the link into its target.
    let link_path = work.join("link.txt");
    if link_path.is_symlink() || link_path.exists() {
        std::fs::remove_file(&link_path).unwrap();
    }
    let repo = libnoa::repo::Repository::open(&work).unwrap();
    let db = Arc::clone(&repo.db);
    drop(repo);
    libnoa::git::export_noa_to_git(&work, db).await.unwrap();

    // Repro 3 (data loss): adverse-order pair intact.
    assert_eq!(
        std::fs::read(work.join("aaa.txt")).unwrap(),
        b"precious data\n",
        "symlink target was clobbered through the link"
    );
    assert_eq!(
        std::fs::read(work.join("regular.txt")).unwrap(),
        b"regular content\n"
    );

    // Repros 2 + 4: modes round-trip with identical blobs.
    let link_line = ls_line(&work, "link.txt");
    assert!(link_line.starts_with("120000 blob "), "got: {link_line}");
    let run_line = ls_line(&work, "run.sh");
    assert!(run_line.starts_with("100755 blob "), "got: {run_line}");
    let zzz_line = ls_line(&work, "zzz.txt");
    assert!(zzz_line.starts_with("120000 blob "), "got: {zzz_line}");
    // Same content blobs as upstream (mode changed, bytes didn't).
    for (name, before_line) in [
        ("link.txt", ls_line(&upstream, "link.txt")),
        ("run.sh", ls_line(&upstream, "run.sh")),
        ("zzz.txt", ls_line(&upstream, "zzz.txt")),
        ("aaa.txt", ls_line(&upstream, "aaa.txt")),
    ] {
        let after_line = ls_line(&work, name);
        let before_blob = before_line.split_whitespace().nth(2).unwrap();
        let after_blob = after_line.split_whitespace().nth(2).unwrap();
        assert_eq!(before_blob, after_blob, "blob changed for {name}");
    }
}

#[test]
fn test_entry_kind_git_mode_mapping() {
    use libnoa::object::EntryKind;
    for (kind, mode) in [
        (EntryKind::Blob, 0o100644),
        (EntryKind::Tree, 0o040000),
        (EntryKind::Executable, 0o100755),
        (EntryKind::Symlink, 0o120000),
        (EntryKind::Gitlink, 0o160000),
    ] {
        assert_eq!(kind.git_mode(), mode);
        assert_eq!(EntryKind::from_git_mode(mode), kind);
    }
    // Unknown blob-ish modes degrade to plain files, never fail import.
    assert_eq!(EntryKind::from_git_mode(0o100664), EntryKind::Blob);
}
