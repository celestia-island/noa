use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub kind: DiffKind,
}

pub fn diff_snapshots(
    old_entries: &[crate::object::TreeEntry],
    new_entries: &[crate::object::TreeEntry],
) -> Vec<FileDiff> {
    let mut diffs = Vec::new();

    let old_map: std::collections::HashMap<&str, &crate::object::TreeEntry> =
        old_entries.iter().map(|e| (e.name.as_str(), e)).collect();

    let new_map: std::collections::HashMap<&str, &crate::object::TreeEntry> =
        new_entries.iter().map(|e| (e.name.as_str(), e)).collect();

    for entry in new_entries {
        match old_map.get(entry.name.as_str()) {
            None => diffs.push(FileDiff {
                path: entry.name.clone(),
                kind: DiffKind::Added,
            }),
            Some(old) if old.id != entry.id => diffs.push(FileDiff {
                path: entry.name.clone(),
                kind: DiffKind::Modified,
            }),
            _ => {}
        }
    }

    for entry in old_entries {
        if !new_map.contains_key(entry.name.as_str()) {
            diffs.push(FileDiff {
                path: entry.name.clone(),
                kind: DiffKind::Deleted,
            });
        }
    }

    diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{EntryKind, TreeEntry};

    fn entry(name: &str, id: &str) -> TreeEntry {
        TreeEntry {
            name: name.to_string(),
            kind: EntryKind::Blob,
            id: id.to_string(),
        }
    }

    #[test]
    fn test_added() {
        let old = vec![];
        let new = vec![entry("a.rs", "hash1")];
        let diffs = diff_snapshots(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].kind, DiffKind::Added);
    }

    #[test]
    fn test_modified() {
        let old = vec![entry("a.rs", "hash1")];
        let new = vec![entry("a.rs", "hash2")];
        let diffs = diff_snapshots(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].kind, DiffKind::Modified);
    }

    #[test]
    fn test_deleted() {
        let old = vec![entry("a.rs", "hash1")];
        let new = vec![];
        let diffs = diff_snapshots(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].kind, DiffKind::Deleted);
    }

    #[test]
    fn test_unchanged() {
        let old = vec![entry("a.rs", "hash1")];
        let new = vec![entry("a.rs", "hash1")];
        let diffs = diff_snapshots(&old, &new);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_mixed() {
        let old = vec![
            entry("a.rs", "h1"),
            entry("b.rs", "h2"),
            entry("c.rs", "h3"),
        ];
        let new = vec![
            entry("a.rs", "h1"),
            entry("b.rs", "h2_changed"),
            entry("d.rs", "h4"),
        ];
        let diffs = diff_snapshots(&old, &new);
        assert_eq!(diffs.len(), 3);
        assert!(diffs
            .iter()
            .any(|d| d.path == "b.rs" && matches!(d.kind, DiffKind::Modified)));
        assert!(diffs
            .iter()
            .any(|d| d.path == "c.rs" && matches!(d.kind, DiffKind::Deleted)));
        assert!(diffs
            .iter()
            .any(|d| d.path == "d.rs" && matches!(d.kind, DiffKind::Added)));
    }
}
