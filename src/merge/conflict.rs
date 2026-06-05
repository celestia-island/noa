use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    Ours,
    Theirs,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConflict {
    pub path: String,
    pub ours_id: String,
    pub theirs_id: String,
    pub base_id: Option<String>,
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

    #[test]
    fn test_upstream_wins() {
        let conflicts = vec![FileConflict {
            path: "a.rs".to_string(),
            ours_id: "h1".to_string(),
            theirs_id: "h2".to_string(),
            base_id: None,
        }];
        let resolved = ConflictDetector::resolve_upstream_wins(&conflicts);
        assert_eq!(resolved[0].1, ConflictResolution::Theirs);
    }

    #[test]
    fn test_ours_wins() {
        let conflicts = vec![FileConflict {
            path: "b.rs".to_string(),
            ours_id: "h3".to_string(),
            theirs_id: "h4".to_string(),
            base_id: None,
        }];
        let resolved = ConflictDetector::resolve_ours_wins(&conflicts);
        assert_eq!(resolved[0].1, ConflictResolution::Ours);
    }
}
