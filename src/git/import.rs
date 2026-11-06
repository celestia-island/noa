use anyhow::Context;
use std::{path::Path, sync::Arc};

use crate::{
    error::Result,
    object::{EntryKind, ObjectStore, TreeEntries, TreeEntry},
    refs::{RedbRefStore, RefStore},
    snapshot::{
        content_addressed_snapshot_id_with_ts, RedbSnapshotStore, Snapshot, SnapshotId,
        SnapshotStore,
    },
};

/// Options for [`import_git_commit`]. The defaults reproduce the legacy
/// `clone`/`pull` behavior (workspace `"default"`, ref `HEAD`, no parents,
/// no skip), so the wrapper stays byte-compatible.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Workspace the imported snapshot belongs to.
    pub workspace: String,
    /// noa ref advanced to the imported snapshot.
    pub head_ref: String,
    /// Explicit git commit oid (hex); `None` uses HEAD.
    pub commit_hex: Option<String>,
    /// Parent snapshot ids attached to the imported snapshot.
    pub parents: Vec<SnapshotId>,
    /// When the ref already points at a snapshot with the same tree hash,
    /// return that snapshot instead of writing a new (timestamp-addressed)
    /// one. Keeps scheduled re-imports idempotent.
    pub skip_if_tree_unchanged: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            workspace: "default".to_string(),
            head_ref: "HEAD".to_string(),
            commit_hex: None,
            parents: Vec::new(),
            skip_if_tree_unchanged: false,
        }
    }
}

pub async fn import_git_to_noa(
    git_dir: &Path,
    db: Arc<redb::Database>,
) -> Result<Option<SnapshotId>> {
    import_git_commit(git_dir, db, &ImportOptions::default()).await
}

/// Commit metadata extracted inside a blocking task so no non-Send gix
/// handles cross an await point.
struct CommitFacts {
    tree_id: gix::hash::ObjectId,
    author: String,
    message: String,
    timestamp_seconds: u64,
}

/// Imports the tree of one git commit into noa objects, writes a snapshot,
/// and advances `opts.head_ref` (retrying the CAS on races).
pub async fn import_git_commit(
    git_dir: &Path,
    db: Arc<redb::Database>,
    opts: &ImportOptions,
) -> Result<Option<SnapshotId>> {
    let git_dir = git_dir.to_path_buf();
    let commit_hex = opts.commit_hex.clone();
    let git_dir_for_walk = git_dir.clone();
    let facts = tokio::task::spawn_blocking(move || {
        let opened =
            gix::open(&git_dir).map_err(|e| anyhow::anyhow!("failed to open git repo: {e}"))?;
        let commit = match commit_hex {
            Some(hex) => {
                let oid = gix::hash::ObjectId::from_hex(hex.as_bytes())
                    .map_err(|e| anyhow::anyhow!("invalid commit oid '{hex}': {e}"))?;
                Some(
                    opened
                        .find_object(oid)?
                        .try_into_commit()
                        .with_context(|| format!("object {hex} is not a commit"))?,
                )
            }
            None => match opened.head_id() {
                Ok(head_id) => Some(
                    opened
                        .find_object(head_id.detach())?
                        .try_into_commit()
                        .with_context(|| "HEAD is not a commit")?,
                ),
                Err(_) => {
                    tracing::warn!("git repository has no HEAD (empty repo), skipping import");
                    None
                }
            },
        };
        // Extract everything we need here: gix handles are not Send and
        // must not cross the await boundary.
        let facts = commit
            .map(|c| {
                Ok::<_, anyhow::Error>(CommitFacts {
                    tree_id: c.tree_id()?.detach(),
                    author: c
                        .author()
                        .ok()
                        .map_or_else(|| "unknown".to_string(), |a| a.name.to_string()),
                    message: c
                        .message_raw()
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default(),
                    timestamp_seconds: c.time()?.seconds.max(0) as u64,
                })
            })
            .transpose()?;
        Ok::<Option<CommitFacts>, anyhow::Error>(facts)
    })
    .await??;

    let Some(facts) = facts else {
        return Ok(None);
    };

    let entries = import_tree_recursive(&git_dir_for_walk, facts.tree_id).await?;

    let imported = ImportedEntry::from_walked(entries);
    store_imported_entries(
        db,
        imported,
        &StoreOptions {
            workspace: opts.workspace.clone(),
            head_ref: opts.head_ref.clone(),
            parents: opts.parents.clone(),
            skip_if_tree_unchanged: opts.skip_if_tree_unchanged,
            author: facts.author,
            message: facts.message,
            timestamp_us: facts.timestamp_seconds * 1_000_000,
        },
    )
    .await
}

/// One source-agnostic imported entry (git walk OR Git Data API).
#[derive(Debug, Clone)]
pub struct ImportedEntry {
    pub path: String,
    pub kind: EntryKind,
    pub body: ImportedBody,
}

#[derive(Debug, Clone)]
pub enum ImportedBody {
    Blob(Vec<u8>),
    /// Gitlink: the referenced commit oid in hex.
    Gitlink(String),
}

impl ImportedEntry {
    fn from_walked(walked: Vec<WalkedEntry>) -> Vec<ImportedEntry> {
        walked
            .into_iter()
            .map(|w| ImportedEntry {
                path: w.path,
                kind: w.kind,
                body: match w.payload {
                    WalkedPayload::Blob(b) => ImportedBody::Blob(b),
                    WalkedPayload::Gitlink(oid) => ImportedBody::Gitlink(oid),
                },
            })
            .collect()
    }
}

/// Storage-side options for [`store_imported_entries`].
pub struct StoreOptions {
    pub workspace: String,
    pub head_ref: String,
    pub parents: Vec<SnapshotId>,
    pub skip_if_tree_unchanged: bool,
    pub author: String,
    pub message: String,
    pub timestamp_us: u64,
}

/// Stores entries as noa blobs + one tree, writes the content-addressed
/// snapshot, advances `head_ref` (CAS with retry), and creates/updates the
/// workspace head. Source-agnostic: fed by the git walk or the Git Data API
/// lane. Returns the (existing or new) snapshot id; `None` never happens
/// here (empty-repo handling belongs to callers).
pub async fn store_imported_entries(
    db: Arc<redb::Database>,
    entries: Vec<ImportedEntry>,
    opts: &StoreOptions,
) -> Result<Option<SnapshotId>> {
    let obj_store = crate::object::RedbObjectStore::new(Arc::clone(&db))?;
    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(Arc::clone(&db))?;
    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;

    let mut sorted = Vec::with_capacity(entries.len());
    for e in entries {
        let id = match e.body {
            ImportedBody::Blob(content) => obj_store.put_blob(&content).await?.0,
            ImportedBody::Gitlink(oid_hex) => {
                // No noa object exists for a gitlink: the oid names a commit
                // in another repository. Keep the oid as the entry id so the
                // reference survives the round trip.
                oid_hex
            }
        };
        sorted.push(TreeEntry {
            name: e.path,
            kind: e.kind,
            id,
        });
    }
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let noa_tree_id = obj_store.put_tree(&TreeEntries(sorted)).await?;

    if opts.skip_if_tree_unchanged {
        if let Some(prev_id) = ref_store.get(&opts.head_ref).await? {
            if let Ok(prev_snap) = snap_store.get(&prev_id).await {
                if prev_snap.tree_hash == noa_tree_id.0 {
                    return Ok(Some(prev_id));
                }
            }
        }
    }

    let snapshot = Snapshot {
        id: content_addressed_snapshot_id_with_ts(
            &noa_tree_id.0,
            &opts.parents,
            &opts.workspace,
            &opts.author,
            &opts.message,
            opts.timestamp_us,
        ),
        tree_hash: noa_tree_id.0,
        parents: opts.parents.clone(),
        workspace: opts.workspace.clone(),
        author: opts.author.clone(),
        timestamp: opts.timestamp_us,
        message: opts.message.clone(),
    };
    snap_store.store(&snapshot).await?;
    // Write-through: advance the head ref from its actual current value so
    // repeated imports track the latest snapshot. Retry on races; only this
    // import writes the ref (issue #70 precedent).
    let mut current = ref_store.get(&opts.head_ref).await?;
    loop {
        if ref_store
            .cas(&opts.head_ref, current.as_ref(), &snapshot.id)
            .await?
        {
            break;
        }
        current = ref_store.get(&opts.head_ref).await?;
    }

    let now = crate::now_micros();
    let ws = crate::workspace::Workspace {
        name: opts.workspace.clone(),
        head: snapshot.id.clone(),
        base: crate::snapshot::empty_snapshot_id(),
        agent_id: None,
        last_seq: 0,
        created_at: now,
        updated_at: now,
    };
    if let Err(e) = ws_mgr.create(&ws).await {
        // Workspace may already exist from init_with_remotes; update in place
        if crate::error::is_workspace_already_exists(&e) {
            ws_mgr.update_head(&opts.workspace, &snapshot.id).await?;
        } else {
            return Err(e);
        }
    }

    Ok(Some(snapshot.id))
}

#[must_use]
pub fn is_lfs_pointer(content: &[u8]) -> bool {
    if content.len() > 500 {
        return false;
    }
    let Ok(s) = std::str::from_utf8(content) else {
        return false;
    };
    s.starts_with("version https://git-lfs.github.com/spec/")
}

/// One non-tree entry collected from the git object store.
struct WalkedEntry {
    path: String,
    kind: EntryKind,
    payload: WalkedPayload,
}

/// The body of a [`WalkedEntry`]: blob bytes exactly as git stores them for
/// [`EntryKind::Blob`], [`EntryKind::Executable`] and [`EntryKind::Symlink`]
/// (symlink bytes are the link-target path), or the referenced commit oid in
/// hex for [`EntryKind::Gitlink`].
enum WalkedPayload {
    Blob(Vec<u8>),
    Gitlink(String),
}

fn walk_tree(repo: &gix::Repository, tree_id: gix::hash::ObjectId) -> Result<Vec<WalkedEntry>> {
    let mut stack = vec![(tree_id, String::new())];
    let mut results = Vec::new();

    while let Some((current_id, prefix)) = stack.pop() {
        let obj = repo.find_object(current_id)?;
        let tree = obj.try_into_tree().with_context(|| "not a tree")?;

        for entry_result in tree.iter() {
            let entry = entry_result?;
            let mode = entry.mode();
            let entry_id = entry.oid();
            let name = entry.filename().to_string();
            let full_name = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };

            if mode.is_tree() {
                stack.push((entry_id.to_owned(), full_name));
            } else if mode.is_commit() {
                // Gitlink (160000): a plain clone never fetches submodule
                // objects, so there is nothing to look up. Record the
                // referenced commit oid; export writes it back verbatim.
                results.push(WalkedEntry {
                    path: full_name,
                    kind: EntryKind::Gitlink,
                    payload: WalkedPayload::Gitlink(entry_id.to_string()),
                });
            } else {
                let kind = if mode.is_link() {
                    EntryKind::Symlink
                } else if mode.is_executable() {
                    EntryKind::Executable
                } else {
                    EntryKind::from_git_mode(mode.value().into())
                };
                let blob_obj = repo.find_object(entry_id)?;
                let blob = blob_obj.try_into_blob().with_context(|| "not a blob")?;
                results.push(WalkedEntry {
                    path: full_name,
                    kind,
                    payload: WalkedPayload::Blob(blob.data.clone()),
                });
            }
        }
    }

    Ok(results)
}

async fn import_tree_recursive(
    git_dir: &Path,
    tree_id: gix::hash::ObjectId,
) -> Result<Vec<WalkedEntry>> {
    let git_dir = git_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let opened =
            gix::open(&git_dir).map_err(|e| anyhow::anyhow!("failed to open git repo: {e}"))?;
        walk_tree(&opened, tree_id)
    })
    .await?
}
