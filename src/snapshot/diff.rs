use serde::{Deserialize, Serialize};

use crate::object::{EntryKind, ObjectStore, TreeEntry, TreeId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    Added,
    Modified,
    Deleted,
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

    let mut old_map: std::collections::HashMap<&str, &crate::object::TreeEntry> =
        std::collections::HashMap::new();
    for e in old_entries {
        if old_map.insert(e.name.as_str(), e).is_some() {
            tracing::warn!("duplicate entry in old tree: {}", e.name);
        }
    }

    let mut new_map: std::collections::HashMap<&str, &crate::object::TreeEntry> =
        std::collections::HashMap::new();
    for e in new_entries {
        if new_map.insert(e.name.as_str(), e).is_some() {
            tracing::warn!("duplicate entry in new tree: {}", e.name);
        }
    }

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

/// Recursively diff two tree entry lists, descending into Tree-typed entries.
/// Requires an ObjectStore to resolve sub-trees. Paths in the result include
/// the full path from the root (e.g., "src/main.rs").
#[allow(dead_code)]
pub async fn diff_trees<O: ObjectStore + Clone + 'static>(
    old_entries: &[TreeEntry],
    new_entries: &[TreeEntry],
    object_store: O,
) -> Vec<FileDiff> {
    let mut diffs = Vec::new();

    let mut old_map: std::collections::HashMap<&str, &TreeEntry> = std::collections::HashMap::new();
    for e in old_entries {
        old_map.insert(&e.name, e);
    }

    let mut new_map: std::collections::HashMap<&str, &TreeEntry> = std::collections::HashMap::new();
    for e in new_entries {
        new_map.insert(&e.name, e);
    }

    for entry in new_entries {
        match old_map.get(entry.name.as_str()) {
            None => diffs.push(FileDiff {
                path: entry.name.clone(),
                kind: DiffKind::Added,
            }),
            Some(old) if old.id != entry.id => {
                if old.kind == EntryKind::Tree && entry.kind == EntryKind::Tree {
                    let old_id = TreeId(old.id.clone());
                    let new_id = TreeId(entry.id.clone());
                    let (old_sub_res, new_sub_res) = tokio::join!(
                        object_store.get_tree(&old_id),
                        object_store.get_tree(&new_id),
                    );
                    if let (Ok(old_sub), Ok(new_sub)) = (old_sub_res, new_sub_res) {
                        let sub_diffs =
                            Box::pin(diff_trees(&old_sub.0, &new_sub.0, object_store.clone()))
                                .await;
                        for mut d in sub_diffs {
                            d.path = format!("{}/{}", entry.name, d.path);
                            diffs.push(d);
                        }
                    }
                } else {
                    diffs.push(FileDiff {
                        path: entry.name.clone(),
                        kind: DiffKind::Modified,
                    });
                }
            }
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
