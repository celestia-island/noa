use anyhow::Result;

use crate::{repo::Repository, snapshot::SnapshotStore};

/// Truncate a message to fit within `max_chars` visible unicode width, using
/// `ellipsis` as the suffix when truncation occurs. The returned string never
/// exceeds `max_chars` characters in total (suffix included).
///
/// Truncation is performed on unicode scalar values (Rust `char`), NOT on
/// UTF-8 bytes, so multi-byte scripts (CJK, emoji) are safe.
pub(crate) fn truncate_message(msg: &str, max_chars: usize, ellipsis: &str) -> String {
    debug_assert!(
        max_chars > ellipsis.chars().count(),
        "max_chars must exceed ellipsis length"
    );
    if msg.chars().count() <= max_chars {
        return msg.to_string();
    }
    let keep = max_chars - ellipsis.chars().count();
    let truncated: String = msg.chars().take(keep).collect();
    format!("{truncated}{ellipsis}")
}

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
        let msg = truncate_message(&snap.message, 50, "...");
        println!(
            "{:<16} {:<12} {:<16} {}",
            snap.id, snap.workspace, snap.author, msg
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::truncate_message;

    /// Long messages are truncated to exactly `max_chars` characters and end
    /// with the ellipsis. Verifies the *production* truncation helper, not a
    /// re-implemented inline copy.
    #[test]
    fn test_utf8_truncation_no_panic() {
        let msg = "你好世界".repeat(20);
        let truncated = truncate_message(&msg, 50, "...");
        // Total visible width must respect the cap.
        assert!(
            truncated.chars().count() <= 50,
            "truncated output must not exceed 50 chars, got {}: {}",
            truncated.chars().count(),
            truncated
        );
        // The truncation suffix must be present.
        assert!(
            truncated.ends_with("..."),
            "truncated output must end with the ellipsis, got: {}",
            truncated
        );
        // The kept prefix must be a prefix of the original.
        let kept: String = truncated.chars().take(47).collect();
        assert!(
            msg.starts_with(&kept),
            "truncated output must preserve the leading characters of the input"
        );

        // Emoji inputs (4-byte UTF-8) must also be safe.
        let emoji_msg = "🎉🚀💎".repeat(20);
        let truncated_emoji = truncate_message(&emoji_msg, 50, "...");
        assert!(truncated_emoji.chars().count() <= 50);
        assert!(truncated_emoji.ends_with("..."));
    }

    /// Short messages are returned unmodified.
    #[test]
    fn test_short_message_not_truncated() {
        let msg = "short msg";
        assert_eq!(truncate_message(msg, 50, "..."), msg);
        // Boundary: a message of exactly `max_chars` length is NOT truncated.
        let exact: String = "a".repeat(50);
        assert_eq!(truncate_message(&exact, 50, "..."), exact);
        // One char over the boundary IS truncated.
        let over: String = "a".repeat(51);
        let trunc = truncate_message(&over, 50, "...");
        assert_eq!(trunc.chars().count(), 50);
        assert!(trunc.ends_with("..."));
    }
}
