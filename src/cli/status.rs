use anyhow::Result;

use crate::object::ObjectStore;
use crate::refs::RefStore;
use crate::repo::Repository;
use crate::snapshot::SnapshotStore;
use crate::workspace::WorkspaceManager;

pub async fn run() -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;
    let head = repo.read_head()?;

    let ws_mgr = repo.workspace_manager()?;
    let snap_store = repo.snapshot_store()?;

    let ws = ws_mgr.get(&head).await;
    let head_info = match ws {
        Ok(Some(w)) => {
            let snap = snap_store.get(&w.head).await.ok();
            let msg = snap.map(|s| s.message).unwrap_or_default();
            format!("{} (head: {}, msg: {})", w.name, w.head, msg)
        }
        _ => head.clone(),
    };

    println!("On workspace: {}", head_info);

    let ref_store = repo.ref_store()?;
    let refs = ref_store.list().await?;
    if !refs.is_empty() {
        println!("\nRefs:");
        for (name, id) in refs {
            println!("  {} -> {}", name, id);
        }
    }

    Ok(())
}
