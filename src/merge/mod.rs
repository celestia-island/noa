use crate::error::{NoaError, Result};
use crate::object::{EntryKind, TreeEntries, TreeEntry};
use crate::snapshot::SnapshotId;

mod conflict;
mod consolidate;

pub use conflict::{ConflictDetector, ConflictResolution, FileConflict};
pub use consolidate::Consolidator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub tree: TreeEntries,
    pub conflicts: Vec<FileConflict>,
}

pub fn three_way_merge(
    base: &TreeEntries,
    ours: &TreeEntries,
    theirs: &TreeEntries,
) -> Result<MergeResult> {
    let base_map: std::collections::HashMap<&str, &TreeEntry> = base
        .0
        .iter()
        .map(|e| (e.name.as_str(), e))
        .collect();
    let ours_map: std::collections::HashMap<&str, &TreeEntry> = ours
        .0
        .iter()
        .map(|e| (e.name.as_str(), e))
        .collect();
    let theirs_map: std::collections::HashMap<&str, &TreeEntry> = theirs
        .0
        .iter()
        .map(|e| (e.name.as_str(), e))
        .collect();

    let mut result_map: std::collections::BTreeMap<String, TreeEntry> = std::collections::BTreeMap::new();
    let mut conflicts = Vec::new();

    let all_paths: std::collections::BTreeSet<&str> = base_map
        .keys()
        .chain(ours_map.keys())
        .chain(theirs_map.keys())
        .copied()
        .collect();

    for path in all_paths {
        let base_entry = base_map.get(path);
        let ours_entry = ours_map.get(path);
        let theirs_entry = theirs_map.get(path);

        enum Change<'a> {
            Unchanged,
            Modified(&'a TreeEntry),
            Deleted,
        }

        let ours_change = match (base_entry, ours_entry) {
            (None, None) => Change::Unchanged,
            (None, Some(o)) => Change::Modified(o),
            (Some(_), Some(o)) if base_entry.unwrap().id != o.id => Change::Modified(o),
            (Some(_), Some(_)) => Change::Unchanged,
            (Some(_), None) => Change::Deleted,
        };

        let theirs_change = match (base_entry, theirs_entry) {
            (None, None) => Change::Unchanged,
            (None, Some(t)) => Change::Modified(t),
            (Some(_), Some(t)) if base_entry.unwrap().id != t.id => Change::Modified(t),
            (Some(_), Some(_)) => Change::Unchanged,
            (Some(_), None) => Change::Deleted,
        };

        match (ours_change, theirs_change) {
            (Change::Unchanged, Change::Unchanged) => {
                if let Some(entry) = base_entry {
                    result_map.insert(entry.name.clone(), (*entry).clone());
                }
            }
            (Change::Modified(o), Change::Unchanged) => {
                result_map.insert(o.name.clone(), (*o).clone());
            }
            (Change::Unchanged, Change::Modified(t)) => {
                result_map.insert(t.name.clone(), (*t).clone());
            }
            (Change::Deleted, Change::Unchanged) => {}
            (Change::Unchanged, Change::Deleted) => {}
            (Change::Modified(o), Change::Deleted) => {
                conflicts.push(FileConflict {
                    path: path.to_string(),
                    ours_id: o.id.clone(),
                    theirs_id: String::new(),
                    base_id: base_entry.map(|b| b.id.clone()),
                });
            }
            (Change::Deleted, Change::Modified(t)) => {
                conflicts.push(FileConflict {
                    path: path.to_string(),
                    ours_id: String::new(),
                    theirs_id: t.id.clone(),
                    base_id: base_entry.map(|b| b.id.clone()),
                });
            }
            (Change::Modified(o), Change::Modified(t)) => {
                if o.id == t.id {
                    result_map.insert(o.name.clone(), (*o).clone());
                } else {
                    conflicts.push(FileConflict {
                        path: path.to_string(),
                        ours_id: o.id.clone(),
                        theirs_id: t.id.clone(),
                        base_id: base_entry.map(|b| b.id.clone()),
                    });
                    result_map.insert(o.name.clone(), (*o).clone());
                }
            }
            (Change::Deleted, Change::Deleted) => {}
        }
    }

    Ok(MergeResult {
        tree: TreeEntries(result_map.into_values().collect()),
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, id: &str) -> TreeEntry {
        TreeEntry {
            name: name.to_string(),
            kind: EntryKind::Blob,
            id: id.to_string(),
        }
    }

    #[test]
    fn test_no_conflict_both_same() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.conflicts.is_empty());
        assert_eq!(result.tree.0[0].id, "h2");
    }

    #[test]
    fn test_no_conflict_different_paths() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h1"), entry("b.rs", "h3")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_conflict_same_path_different_content() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h3")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].path, "a.rs");
    }

    #[test]
    fn test_delete_vs_modify_conflict() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![]);
        let theirs = TreeEntries(vec![entry("a.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert_eq!(result.conflicts.len(), 1);
    }

    #[test]
    fn test_add_new_no_conflict() {
        let base = TreeEntries(vec![]);
        let ours = TreeEntries(vec![entry("a.rs", "h1")]);
        let theirs = TreeEntries(vec![entry("b.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.conflicts.is_empty());
        assert_eq!(result.tree.0.len(), 2);
    }

    #[test]
    fn test_empty_base_both_modify() {
        let base = TreeEntries(vec![]);
        let ours = TreeEntries(vec![entry("a.rs", "h1")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert_eq!(result.conflicts.len(), 1);
    }
}
