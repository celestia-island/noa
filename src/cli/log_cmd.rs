use anyhow::Result;

use crate::repo::Repository;
use crate::snapshot::{SnapshotId, SnapshotStore};

pub async fn run(workspace: Option<&str>, limit: usize) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let snap_store = repo.snapshot_store()?;
    let all = snap_store.list_all().await?;

    let ws_name = workspace.map(|s| s.to_string()).or_else(|| repo.read_head().ok());
    let filtered: Vec<_> = if let Some(ref ws) = ws_name {
        all.into_iter().filter(|s| &s.workspace == ws).collect()
    } else {
        all
    };

    let display: Vec<_> = filtered.into_iter().rev().take(limit).collect();

    if display.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    println!("{:<16} {:<12} {:<16} {}", "ID", "WORKSPACE", "AUTHOR", "MESSAGE");
    for snap in &display {
        let msg = if snap.message.len() > 50 {
            format!("{}...", &snap.message[..47])
        } else {
            snap.message.clone()
        };
        println!("{:<16} {:<12} {:<16} {}", snap.id, snap.workspace, snap.author, msg);
    }

    Ok(())
}
