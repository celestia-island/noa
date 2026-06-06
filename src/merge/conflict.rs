use serde::{Deserialize, Serialize};

use crate::object::TreeEntry;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    Ours,
    Theirs,
    Manual { resolved_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConflict {
    pub path: String,
    pub ours_id: Option<String>,
    pub theirs_id: Option<String>,
    pub base_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergedEntry {
    Clean(TreeEntry),
    Conflict {
        ours: Option<TreeEntry>,
        theirs: Option<TreeEntry>,
        base: Option<TreeEntry>,
    },
}

impl MergedEntry {
    pub fn path(&self) -> Option<&str> {
        match self {
            MergedEntry::Clean(entry) => Some(&entry.name),
            MergedEntry::Conflict { ours, theirs, .. } => {
                ours.as_ref().or(theirs.as_ref()).map(|e| e.name.as_str())
            }
        }
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, MergedEntry::Conflict { .. })
    }

    pub fn into_clean_entry(self) -> Option<TreeEntry> {
        match self {
            MergedEntry::Clean(entry) => Some(entry),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeOutput {
    pub entries: Vec<MergedEntry>,
    pub has_conflicts: bool,
}

impl MergeOutput {
    pub fn clean_entries(&self) -> Vec<&TreeEntry> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                MergedEntry::Clean(entry) => Some(entry),
                _ => None,
            })
            .collect()
    }

    pub fn conflict_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_conflict()).count()
    }

    pub fn resolve_with_strategy(&self, resolution: &ConflictResolution) -> Vec<TreeEntry> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                MergedEntry::Clean(e) => Some(e.clone()),
                MergedEntry::Conflict { ours, theirs, base } => match resolution {
                    ConflictResolution::Ours => ours.clone(),
                    ConflictResolution::Theirs => theirs.clone(),
                    ConflictResolution::Manual { resolved_id } => {
                        base.as_ref().map(|b| TreeEntry {
                            name: b.name.clone(),
                            kind: b.kind,
                            id: resolved_id.clone(),
                        })
                    }
                },
            })
            .collect()
    }
}

pub struct ConflictDetector;

impl ConflictDetector {
    pub fn resolve_upstream_wins(conflicts: &[FileConflict]) -> Vec<(String, ConflictResolution)> {
        conflicts
            .iter()
            .map(|c| (c.path.clone(), ConflictResolution::Theirs))
            .collect()
    }

    pub fn resolve_ours_wins(conflicts: &[FileConflict]) -> Vec<(String, ConflictResolution)> {
        conflicts
            .iter()
            .map(|c| (c.path.clone(), ConflictResolution::Ours))
            .collect()
    }
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
    fn test_upstream_wins() {
        let conflicts = vec![FileConflict {
            path: "a.rs".to_string(),
            ours_id: Some("h1".to_string()),
            theirs_id: Some("h2".to_string()),
            base_id: None,
        }];
        let resolved = ConflictDetector::resolve_upstream_wins(&conflicts);
        assert_eq!(resolved[0].1, ConflictResolution::Theirs);
    }

    #[test]
    fn test_ours_wins() {
        let conflicts = vec![FileConflict {
            path: "b.rs".to_string(),
            ours_id: Some("h3".to_string()),
            theirs_id: Some("h4".to_string()),
            base_id: None,
        }];
        let resolved = ConflictDetector::resolve_ours_wins(&conflicts);
        assert_eq!(resolved[0].1, ConflictResolution::Ours);
    }

    #[test]
    fn test_merged_entry_clean() {
        let e = entry("a.rs", "h1");
        let merged = MergedEntry::Clean(e.clone());
        assert_eq!(merged.path(), Some("a.rs"));
        assert!(!merged.is_conflict());
        assert_eq!(merged.into_clean_entry(), Some(e));
    }

    #[test]
    fn test_merged_entry_conflict() {
        let e = entry("a.rs", "h1");
        let merged = MergedEntry::Conflict {
            ours: Some(e.clone()),
            theirs: Some(entry("a.rs", "h2")),
            base: Some(entry("a.rs", "h0")),
        };
        assert_eq!(merged.path(), Some("a.rs"));
        assert!(merged.is_conflict());
        assert!(merged.into_clean_entry().is_none());
    }

    #[test]
    fn test_merge_output_resolve_ours() {
        let output = MergeOutput {
            entries: vec![
                MergedEntry::Clean(entry("b.rs", "h3")),
                MergedEntry::Conflict {
                    ours: Some(entry("a.rs", "h1")),
                    theirs: Some(entry("a.rs", "h2")),
                    base: Some(entry("a.rs", "h0")),
                },
            ],
            has_conflicts: true,
        };
        let resolved = output.resolve_with_strategy(&ConflictResolution::Ours);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id, "h3");
        assert_eq!(resolved[1].id, "h1");
    }

    #[test]
    fn test_merge_output_resolve_theirs() {
        let output = MergeOutput {
            entries: vec![
                MergedEntry::Clean(entry("b.rs", "h3")),
                MergedEntry::Conflict {
                    ours: Some(entry("a.rs", "h1")),
                    theirs: Some(entry("a.rs", "h2")),
                    base: Some(entry("a.rs", "h0")),
                },
            ],
            has_conflicts: true,
        };
        let resolved = output.resolve_with_strategy(&ConflictResolution::Theirs);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[1].id, "h2");
    }

    #[test]
    fn test_merge_output_clean_entries() {
        let output = MergeOutput {
            entries: vec![
                MergedEntry::Clean(entry("a.rs", "h1")),
                MergedEntry::Conflict {
                    ours: Some(entry("b.rs", "h2")),
                    theirs: Some(entry("b.rs", "h3")),
                    base: None,
                },
                MergedEntry::Clean(entry("c.rs", "h4")),
            ],
            has_conflicts: true,
        };
        let clean = output.clean_entries();
        assert_eq!(clean.len(), 2);
        assert_eq!(clean[0].name, "a.rs");
        assert_eq!(clean[1].name, "c.rs");
    }

    #[test]
    fn test_merge_output_conflict_count() {
        let output = MergeOutput {
            entries: vec![
                MergedEntry::Clean(entry("a.rs", "h1")),
                MergedEntry::Conflict {
                    ours: Some(entry("b.rs", "h2")),
                    theirs: Some(entry("b.rs", "h3")),
                    base: None,
                },
            ],
            has_conflicts: true,
        };
        assert_eq!(output.conflict_count(), 1);
    }
}
