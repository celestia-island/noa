use anyhow::Result;

use crate::{repo::Repository, snapshot::SnapshotStore};

pub async fn run(workspace: Option<&str>, limit: usize) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let snap_store = repo.snapshot_store()?;
    let all = snap_store.list_all().await?;

    let ws_name = workspace
        .map(std::string::ToString::to_string)
        .or_else(|| repo.read_head().ok());
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

    println!("{:<16} {:<12} {:<16} MESSAGE", "ID", "WORKSPACE", "AUTHOR");
    for snap in &display {
        let msg = if snap.message.chars().count() > 50 {
            let truncated: String = snap.message.chars().take(47).collect();
            format!("{truncated}...")
        } else {
            snap.message.clone()
        };
        println!(
            "{:<16} {:<12} {:<16} {}",
            snap.id, snap.workspace, snap.author, msg
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_utf8_truncation_no_panic() {
        let msg = "你好世界".repeat(20);
        // This would panic with byte slicing (&msg[..47])
        let truncated: String = msg.chars().take(47).collect();
        assert_eq!(truncated.chars().count(), 47);
        assert!(format!("{}...", truncated).chars().count() <= 50);

        let emoji_msg = "🎉🚀💎".repeat(20);
        let truncated_emoji: String = emoji_msg.chars().take(47).collect();
        assert_eq!(truncated_emoji.chars().count(), 47);
    }

    #[test]
    fn test_short_message_not_truncated() {
        let msg = "short msg";
        assert!(msg.chars().count() <= 50);
    }
}
