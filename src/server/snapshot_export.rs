//! Snapshot → git object serialization (slice 6 of the seven-slice
//! series): the SERVER-side bridge that gives noa-native snapshots (pure
//! workspace content that never passed through the git transport) a git
//! object form inside the mirror scratch — so the back-flow lane can carry
//! them to the origin like any other content.
//!
//! The client CLI has always had this bridge (`noa push` writes a
//! `[noa export]` commit); this is the server twin. Blobs hash
//! byte-identically to git (the object algebra invariant), trees are
//! constructed as git tree objects (mode + name + 20-byte sha) and written
//! with `git hash-object -t tree`, the commit with `git commit-tree`, and
//! the result lands on `refs/heads/noa/<workspace>` — inside the heads
//! namespace the back-flow lane already pushes (`+refs/heads/*`).

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::Result;
use crate::object::{EntryKind, ObjectStore, TreeEntries, TreeId};
use crate::snapshot::Snapshot;

/// Serializes one snapshot into the scratch as a git commit on
/// `refs/heads/noa/<workspace>`; returns the commit sha.
pub async fn export_snapshot_to_scratch<O: ObjectStore + Clone + 'static>(
    scratch: &Path,
    store: O,
    snap_store: crate::snapshot::RedbSnapshotStore,
    snap: &Snapshot,
) -> Result<String> {
    let scratch = scratch.to_path_buf();
    let snap = snap.clone();

    // Phase 1: flatten the snapshot tree to path → (mode, blob content or
    // gitlink target). Symlinks serialize their target path as the blob.
    let flat = {
        let store = store.clone();
        let root = TreeId(snap.tree_hash.clone());
        tokio::task::spawn_blocking(move || flatten_tree(store, root))
            .await
            .map_err(|e| anyhow::anyhow!("flatten join: {e}"))??
    };

    // Phase 2: write every blob (git hash-object -w --stdin).
    let mut blob_shas: BTreeMap<String, String> = BTreeMap::new();
    for (path, item) in &flat {
        if let FlatItem::Blob(content) = item {
            let sha = hash_object(&scratch, "blob", Some(content)).await?;
            blob_shas.insert(path.clone(), sha);
        }
    }

    // Phase 3: build git tree objects bottom-up (deepest paths first) —
    // a directory's tree object needs its children's shas, so construct
    // leaves before parents.
    let mut tree_shas: BTreeMap<String, String> = BTreeMap::new(); // "" = root
    let mut dirs: Vec<String> = flat
        .keys()
        .flat_map(|p| parent_dirs(p))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len())); // deepest first
    dirs.push(String::new()); // root last
    for dir in dirs {
        let entries = tree_entries_for(&dir, &flat, &blob_shas, &tree_shas);
        let body = build_tree_object(&entries);
        let sha = hash_object(&scratch, "tree", Some(&body)).await?;
        tree_shas.insert(dir.clone(), sha);
    }
    let root_tree = tree_shas.get("").cloned().unwrap_or_default();

    // Phase 4: commit (parent = the ref's current value; the noa parent
    // chain is not git history — the export ref stays linear).
    let ref_name = format!("refs/heads/noa/{}", snap.workspace);
    let parent = git_ref(&scratch, &ref_name).await.ok();
    let msg = format!(
        "[noa export] snapshot {} from workspace {}",
        snap.id.0, snap.workspace
    );
    let commit_sha = commit_tree(&scratch, &root_tree, parent.as_deref(), &msg, &snap).await?;

    // Phase 5: advance the ref.
    git_update_ref(&scratch, &ref_name, &commit_sha).await?;
    let _ = snap_store; // reserved: parent-chain mapping later
    Ok(commit_sha)
}

enum FlatItem {
    Blob(Vec<u8>),
    Gitlink(String),
}

fn flatten_tree<O: ObjectStore + Clone>(
    store: O,
    root: TreeId,
) -> Result<BTreeMap<String, FlatItem>> {
    // Blocking context (called inside spawn_blocking): use a small
    // recursive walk with a tokio-free handle — ObjectStore is async, so
    // run under a minimal block_on for each level. To keep this simple
    // and correct, walk with a fresh runtime per whole flatten.
    tokio::runtime::Handle::current().block_on(async move {
        let mut out: BTreeMap<String, FlatItem> = BTreeMap::new();
        let mut pending = vec![(String::new(), root)];
        while let Some((prefix, id)) = pending.pop() {
            let tree: TreeEntries = store.get_tree(&id).await?;
            for e in tree.0 {
                let full = if prefix.is_empty() {
                    e.name.clone()
                } else {
                    format!("{prefix}/{}", e.name)
                };
                match e.kind {
                    EntryKind::Tree => {
                        pending.push((full, TreeId(e.id.clone())));
                    }
                    EntryKind::Gitlink => {
                        out.insert(full, FlatItem::Gitlink(e.id.clone()));
                    }
                    EntryKind::Symlink => {
                        let content = store.get_blob(&crate::object::BlobId(e.id.clone())).await?;
                        out.insert(full, FlatItem::Blob(content));
                    }
                    EntryKind::Blob | EntryKind::Executable => {
                        let content = store.get_blob(&crate::object::BlobId(e.id.clone())).await?;
                        out.insert(full, FlatItem::Blob(content));
                    }
                }
            }
        }
        Ok(out)
    })
}

fn parent_dirs(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = path;
    while let Some(idx) = cur.rfind('/') {
        cur = &cur[..idx];
        out.push(cur.to_string());
    }
    out
}

struct TreeEntryLine {
    mode: &'static str,
    name: String,
    sha_hex: String,
}

fn tree_entries_for(
    dir: &str,
    flat: &BTreeMap<String, FlatItem>,
    blob_shas: &BTreeMap<String, String>,
    tree_shas: &BTreeMap<String, String>,
) -> Vec<TreeEntryLine> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut out = Vec::new();
    for (path, item) in flat {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') {
            // nested — the immediate child dir gets one entry
            let child = &name[..name.find('/').unwrap_or(0)];
            let child_dir = if dir.is_empty() {
                child.to_string()
            } else {
                format!("{dir}/{child}")
            };
            if !out.iter().any(|e: &TreeEntryLine| e.name == child) {
                if let Some(sha) = tree_shas.get(&child_dir) {
                    out.push(TreeEntryLine {
                        mode: "40000",
                        name: child.to_string(),
                        sha_hex: sha.clone(),
                    });
                }
            }
            continue;
        }
        match item {
            FlatItem::Blob(_) => {
                if let Some(sha) = blob_shas.get(path) {
                    out.push(TreeEntryLine {
                        mode: "100644",
                        name: name.to_string(),
                        sha_hex: sha.clone(),
                    });
                }
            }
            FlatItem::Gitlink(target) => {
                out.push(TreeEntryLine {
                    mode: "160000",
                    name: name.to_string(),
                    sha_hex: target.clone(),
                });
            }
        }
    }
    out
}

fn build_tree_object(entries: &[TreeEntryLine]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut sorted: Vec<&TreeEntryLine> = entries.iter().collect();
    // git tree order: names sorted with directories treated as having '/'
    sorted.sort_by(|a, b| {
        let ka = format!("{}/", a.name);
        let kb = format!("{}/", b.name);
        ka.cmp(&kb)
    });
    for e in sorted {
        body.extend_from_slice(e.mode.as_bytes());
        body.push(b' ');
        body.extend_from_slice(e.name.as_bytes());
        body.push(0);
        let mut sha = [0u8; 20];
        for (i, hex_pair) in e.sha_hex.as_bytes().chunks(2).enumerate() {
            let hi = (hex_pair[0] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (hex_pair[1] as char).to_digit(16).unwrap_or(0) as u8;
            sha[i] = (hi << 4) | lo;
        }
        body.extend_from_slice(&sha);
    }
    body
}

async fn hash_object(scratch: &Path, kind: &str, content: Option<&[u8]>) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("git")
        .args([
            "-C",
            scratch.to_str().unwrap_or_default(),
            "hash-object",
            "-w",
            "-t",
            kind,
            "--stdin",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    if let Some(c) = content {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(c).await?;
        }
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "hash-object {kind} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn git_ref(scratch: &Path, refname: &str) -> Result<String> {
    let out = tokio::process::Command::new("git")
        .args([
            "-C",
            scratch.to_str().unwrap_or_default(),
            "rev-parse",
            refname,
        ])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("ref {refname} not found");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn commit_tree(
    scratch: &Path,
    tree: &str,
    parent: Option<&str>,
    msg: &str,
    snap: &Snapshot,
) -> Result<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args([
        "-C",
        scratch.to_str().unwrap_or_default(),
        "commit-tree",
        tree,
    ]);
    if let Some(p) = parent {
        cmd.arg("-p").arg(p);
    }
    cmd.env("GIT_AUTHOR_NAME", &snap.author)
        .env("GIT_AUTHOR_EMAIL", format!("{}@noa.local", snap.author))
        .env(
            "GIT_AUTHOR_DATE",
            format!("{} +0000", snap.timestamp / 1_000_000),
        )
        .env("GIT_COMMITTER_NAME", "noa-server")
        .env("GIT_COMMITTER_EMAIL", "noa@celestia.local")
        .env(
            "GIT_COMMITTER_DATE",
            format!("{} +0000", snap.timestamp / 1_000_000),
        )
        .arg("-m")
        .arg(msg)
        .stdin(std::process::Stdio::null());
    let out = cmd.output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "commit-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn git_update_ref(scratch: &Path, refname: &str, sha: &str) -> Result<()> {
    let out = tokio::process::Command::new("git")
        .args([
            "-C",
            scratch.to_str().unwrap_or_default(),
            "update-ref",
            refname,
            sha,
        ])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "update-ref failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// POST /api/v1/repos/{name}/snapshots/{id}/export — serialize a noa
/// snapshot into the repo's mirror scratch (refs/heads/noa/<workspace>);
/// the next wave's back-flow carries it to the origin.
pub async fn export_handler(
    axum::extract::State(state): axum::extract::State<crate::server::AppState>,
    axum::extract::Path((repo, id)): axum::extract::Path<(String, String)>,
) -> Result<axum::Json<serde_json::Value>, (axum::http::StatusCode, axum::Json<serde_json::Value>)>
{
    let scratch = state.mirror_config.scratch_dir.join(format!("{repo}.git"));
    if !scratch.exists() {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": format!("no mirrored repo '{repo}'") })),
        ));
    }
    let db = std::sync::Arc::clone(&state.db);
    let obj_store = crate::object::RedbObjectStore::new(db)
        .map_err(|e| internal(format!("object store: {e}")))?;
    let snap_store = crate::snapshot::RedbSnapshotStore::new(std::sync::Arc::clone(&state.db))
        .map_err(|e| internal(format!("snapshot store: {e}")))?;
    use crate::snapshot::SnapshotStore as _;
    let snap = snap_store
        .get(&crate::snapshot::SnapshotId(id.clone()))
        .await
        .map_err(|_| not_found(format!("snapshot {id} not found")))?;
    let commit = export_snapshot_to_scratch(&scratch, obj_store, snap_store, &snap)
        .await
        .map_err(|e| internal(format!("{e:#}")))?;
    Ok(axum::Json(serde_json::json!({
        "commit": commit,
        "ref": format!("refs/heads/noa/{}", snap.workspace),
    })))
}

fn internal(msg: String) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({ "error": msg })),
    )
}

fn not_found(msg: String) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        axum::http::StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({ "error": msg })),
    )
}
