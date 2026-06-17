use serde::{Deserialize, Serialize};

use crate::object::{EntryKind, ObjectStore, TreeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

pub async fn diff_snapshots_recursive<O: ObjectStore>(
    old_entries: &[crate::object::TreeEntry],
    new_entries: &[crate::object::TreeEntry],
    prefix: &str,
    object_store: &O,
) -> Vec<FileDiff> {
    let mut diffs = Vec::new();

    let mut old_map: std::collections::HashMap<&str, &crate::object::TreeEntry> =
        std::collections::HashMap::new();
    for e in old_entries {
        old_map.insert(e.name.as_str(), e);
    }

    let mut new_map: std::collections::HashMap<&str, &crate::object::TreeEntry> =
        std::collections::HashMap::new();
    for e in new_entries {
        new_map.insert(e.name.as_str(), e);
    }

    for entry in new_entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", prefix, entry.name)
        };

        match old_map.get(entry.name.as_str()) {
            None => {
                if entry.kind == EntryKind::Tree {
                    let mut added = diff_tree_entries_inner(&entry.id, &path, DiffKind::Added, object_store).await;
                    diffs.append(&mut added);
                } else {
                    diffs.push(FileDiff {
                        path,
                        kind: DiffKind::Added,
                    });
                }
            }
            Some(old) if old.id != entry.id => {
                if entry.kind == EntryKind::Tree && old.kind == EntryKind::Tree {
                    if let (Ok(old_sub), Ok(new_sub)) = (
                        object_store.get_tree(&TreeId(old.id.clone())).await,
                        object_store.get_tree(&TreeId(entry.id.clone())).await,
                    ) {
                        let mut sub = Box::pin(diff_snapshots_recursive(
                            &old_sub.0,
                            &new_sub.0,
                            &path,
                            object_store,
                        ))
                        .await;
                        diffs.append(&mut sub);
                    } else {
                        diffs.push(FileDiff {
                            path,
                            kind: DiffKind::Modified,
                        });
                    }
                } else {
                    diffs.push(FileDiff {
                        path,
                        kind: DiffKind::Modified,
                    });
                }
            }
            _ => {}
        }
    }

    for entry in old_entries {
        if !new_map.contains_key(entry.name.as_str()) {
            let path = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", prefix, entry.name)
            };
            if entry.kind == EntryKind::Tree {
                let mut deleted = diff_tree_entries_inner(&entry.id, &path, DiffKind::Deleted, object_store).await;
                diffs.append(&mut deleted);
            } else {
                diffs.push(FileDiff {
                    path,
                    kind: DiffKind::Deleted,
                });
            }
        }
    }

    diffs
}

async fn diff_tree_entries_inner<O: ObjectStore>(
    entry_id: &str,
    prefix: &str,
    kind: DiffKind,
    store: &O,
) -> Vec<FileDiff> {
    let mut diffs = Vec::new();
    let mut stack: Vec<(String, String)> = Vec::new();
    stack.push((entry_id.to_string(), prefix.to_string()));

    while let Some((id, path)) = stack.pop() {
        if let Ok(sub) = store.get_tree(&TreeId(id)).await {
            for child in &sub.0 {
                let child_path = format!("{}/{}", path, child.name);
                if child.kind == EntryKind::Tree {
                    stack.push((child.id.clone(), child_path));
                } else {
                    diffs.push(FileDiff {
                        path: child_path,
                        kind,
                    });
                }
            }
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
