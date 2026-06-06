mod conflict;
mod consolidate;

pub use conflict::{ConflictDetector, ConflictResolution, FileConflict, MergeOutput, MergedEntry};
pub use consolidate::Consolidator;

use crate::{
    error::Result,
    object::{TreeEntries, TreeEntry},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub output: MergeOutput,
}

impl MergeResult {
    pub fn has_conflicts(&self) -> bool {
        self.output.has_conflicts
    }

    pub fn into_tree_entries(&self, resolution: &ConflictResolution) -> TreeEntries {
        TreeEntries(self.output.resolve_with_strategy(resolution))
    }
}

pub fn three_way_merge(
    base: &TreeEntries,
    ours: &TreeEntries,
    theirs: &TreeEntries,
) -> Result<MergeResult> {
    let base_map: std::collections::HashMap<&str, &TreeEntry> =
        base.0.iter().map(|e| (e.name.as_str(), e)).collect();
    let ours_map: std::collections::HashMap<&str, &TreeEntry> =
        ours.0.iter().map(|e| (e.name.as_str(), e)).collect();
    let theirs_map: std::collections::HashMap<&str, &TreeEntry> =
        theirs.0.iter().map(|e| (e.name.as_str(), e)).collect();

    let mut entries: std::collections::BTreeMap<String, MergedEntry> =
        std::collections::BTreeMap::new();
    let mut has_conflicts = false;

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

        let merged = match (ours_change, theirs_change) {
            (Change::Unchanged, Change::Unchanged) => {
                base_entry.map(|entry| MergedEntry::Clean((*entry).clone()))
            }
            (Change::Modified(o), Change::Unchanged) => Some(MergedEntry::Clean((*o).clone())),
            (Change::Unchanged, Change::Modified(t)) => Some(MergedEntry::Clean((*t).clone())),
            (Change::Deleted, Change::Unchanged) => None,
            (Change::Unchanged, Change::Deleted) => None,
            (Change::Modified(o), Change::Deleted) => {
                has_conflicts = true;
                Some(MergedEntry::Conflict {
                    ours: Some((*o).clone()),
                    theirs: None,
                    base: base_entry.map(|b| (*b).clone()),
                })
            }
            (Change::Deleted, Change::Modified(t)) => {
                has_conflicts = true;
                Some(MergedEntry::Conflict {
                    ours: None,
                    theirs: Some((*t).clone()),
                    base: base_entry.map(|b| (*b).clone()),
                })
            }
            (Change::Modified(o), Change::Modified(t)) => {
                if o.id == t.id {
                    Some(MergedEntry::Clean((*o).clone()))
                } else {
                    has_conflicts = true;
                    Some(MergedEntry::Conflict {
                        ours: Some((*o).clone()),
                        theirs: Some((*t).clone()),
                        base: base_entry.map(|b| (*b).clone()),
                    })
                }
            }
            (Change::Deleted, Change::Deleted) => None,
        };

        if let Some(m) = merged {
            entries.insert(path.to_string(), m);
        }
    }

    Ok(MergeResult {
        output: MergeOutput {
            entries: entries.into_values().collect(),
            has_conflicts,
        },
    })
}

pub fn extract_conflicts(output: &MergeOutput) -> Vec<FileConflict> {
    output
        .entries
        .iter()
        .filter_map(|entry| match entry {
            MergedEntry::Conflict { ours, theirs, base } => Some(FileConflict {
                path: ours
                    .as_ref()
                    .or(theirs.as_ref())
                    .or(base.as_ref())
                    .map(|e| e.name.clone())?,
                ours_id: ours.as_ref().map(|e| e.id.clone()),
                theirs_id: theirs.as_ref().map(|e| e.id.clone()),
                base_id: base.as_ref().map(|e| e.id.clone()),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::EntryKind;

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
        assert!(!result.has_conflicts());
        let tree = result.into_tree_entries(&ConflictResolution::Ours);
        assert_eq!(tree.0[0].id, "h2");
    }

    #[test]
    fn test_no_conflict_different_paths() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h1"), entry("b.rs", "h3")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(!result.has_conflicts());
    }

    #[test]
    fn test_conflict_same_path_different_content() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h3")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.has_conflicts());
        let conflicts = extract_conflicts(&result.output);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "a.rs");
    }

    #[test]
    fn test_delete_vs_modify_conflict() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![]);
        let theirs = TreeEntries(vec![entry("a.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.has_conflicts());
    }

    #[test]
    fn test_add_new_no_conflict() {
        let base = TreeEntries(vec![]);
        let ours = TreeEntries(vec![entry("a.rs", "h1")]);
        let theirs = TreeEntries(vec![entry("b.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(!result.has_conflicts());
        let tree = result.into_tree_entries(&ConflictResolution::Ours);
        assert_eq!(tree.0.len(), 2);
    }

    #[test]
    fn test_empty_base_both_modify() {
        let base = TreeEntries(vec![]);
        let ours = TreeEntries(vec![entry("a.rs", "h1")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h2")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.has_conflicts());
    }

    #[test]
    fn test_both_delete_no_conflict() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![]);
        let theirs = TreeEntries(vec![]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(!result.has_conflicts());
        let tree = result.into_tree_entries(&ConflictResolution::Ours);
        assert!(tree.0.is_empty());
    }

    #[test]
    fn test_unchanged_preserves_base() {
        let base = TreeEntries(vec![entry("a.rs", "h1"), entry("b.rs", "h2")]);
        let result = three_way_merge(&base, &base, &base).unwrap();
        assert!(!result.has_conflicts());
        let tree = result.into_tree_entries(&ConflictResolution::Ours);
        assert_eq!(tree.0.len(), 2);
    }

    #[test]
    fn test_large_tree_merge() {
        let base: TreeEntries = TreeEntries(
            (0..50)
                .map(|i| entry(&format!("f{}.rs", i), &format!("h{}", i)))
                .collect(),
        );
        let mut ours_entries: Vec<TreeEntry> = base.0.clone();
        ours_entries.push(entry("new_ours.rs", "h_ours"));
        let ours = TreeEntries(ours_entries);
        let mut theirs_entries: Vec<TreeEntry> = base.0.clone();
        theirs_entries.push(entry("new_theirs.rs", "h_theirs"));
        let theirs = TreeEntries(theirs_entries);

        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(!result.has_conflicts());
        let tree = result.into_tree_entries(&ConflictResolution::Ours);
        assert_eq!(tree.0.len(), 52);
    }

    #[test]
    fn test_only_one_side_modified() {
        let base = TreeEntries(vec![entry("a.rs", "h1"), entry("b.rs", "h2")]);
        let ours = TreeEntries(vec![entry("a.rs", "h1"), entry("b.rs", "h2_changed")]);
        let theirs = base.clone();
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(!result.has_conflicts());
        let tree = result.into_tree_entries(&ConflictResolution::Ours);
        assert_eq!(tree.0.len(), 2);
        assert!(tree
            .0
            .iter()
            .any(|e| e.name == "b.rs" && e.id == "h2_changed"));
    }

    #[test]
    fn test_conflict_does_not_force_ours() {
        let base = TreeEntries(vec![entry("a.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h3")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        assert!(result.has_conflicts());

        let tree_ours = result.into_tree_entries(&ConflictResolution::Ours);
        assert_eq!(tree_ours.0[0].id, "h2");

        let tree_theirs = result.into_tree_entries(&ConflictResolution::Theirs);
        assert_eq!(tree_theirs.0[0].id, "h3");
    }

    #[test]
    fn test_extract_conflicts() {
        let base = TreeEntries(vec![entry("a.rs", "h0"), entry("b.rs", "h1")]);
        let ours = TreeEntries(vec![entry("a.rs", "h2"), entry("b.rs", "h1")]);
        let theirs = TreeEntries(vec![entry("a.rs", "h3"), entry("b.rs", "h1")]);
        let result = three_way_merge(&base, &ours, &theirs).unwrap();
        let conflicts = extract_conflicts(&result.output);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "a.rs");
        assert_eq!(conflicts[0].ours_id, Some("h2".to_string()));
        assert_eq!(conflicts[0].theirs_id, Some("h3".to_string()));
        assert_eq!(conflicts[0].base_id, Some("h0".to_string()));
    }
}
