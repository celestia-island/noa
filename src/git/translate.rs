use crate::error::{NoaError, Result};
use crate::object::{BlobId, EntryKind, ObjectStore, TreeEntries, TreeEntry, TreeId};

pub struct GitTranslator;

impl GitTranslator {
    pub fn noa_blob_to_git(content: &[u8]) -> Vec<u8> {
        let header = format!("blob {}\0", content.len());
        let mut out = Vec::with_capacity(header.len() + content.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(content);
        out
    }

    pub fn git_blob_to_noa(git_obj: &[u8]) -> Result<Vec<u8>> {
        let null_pos = git_obj.iter().position(|&b| b == 0)
            .ok_or_else(|| NoaError::Serialization("invalid git blob: no null separator".into()))?;
        Ok(git_obj[null_pos + 1..].to_vec())
    }

    pub fn noa_tree_to_git(entries: &TreeEntries) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in &entries.0 {
            let mode = match entry.kind {
                EntryKind::Blob => "100644",
                EntryKind::Tree => "40000",
            };
            out.extend_from_slice(mode.as_bytes());
            out.push(b' ');
            out.extend_from_slice(entry.name.as_bytes());
            out.push(b'\0');
            let hash_bytes = hex::decode(&entry.id).unwrap_or_default();
            out.extend_from_slice(&hash_bytes);
        }
        out
    }

    pub fn git_tree_to_noa(git_data: &[u8]) -> Result<TreeEntries> {
        let mut entries = Vec::new();
        let mut pos = 0;

        if let Some(null_pos) = git_data.iter().position(|&b| b == 0) {
            let header = std::str::from_utf8(&git_data[..null_pos]).unwrap_or("");
            if header.starts_with("tree ") {
                pos = null_pos + 1;
            }
        }

        while pos < git_data.len() {
            let space_pos = git_data[pos..].iter().position(|&b| b == b' ')
                .map(|p| pos + p)
                .ok_or_else(|| NoaError::Serialization("invalid git tree entry".into()))?;

            let mode = std::str::from_utf8(&git_data[pos..space_pos])
                .map_err(|e| NoaError::Serialization(e.to_string()))?;

            let name_start = space_pos + 1;
            let name_end = git_data[name_start..].iter().position(|&b| b == 0)
                .map(|p| name_start + p)
                .ok_or_else(|| NoaError::Serialization("invalid git tree entry name".into()))?;

            let name = std::str::from_utf8(&git_data[name_start..name_end])
                .map_err(|e| NoaError::Serialization(e.to_string()))?
                .to_string();

            let hash_start = name_end + 1;
            let hash_end = hash_start + 20;
            if hash_end > git_data.len() {
                break;
            }
            let hash_hex = hex::encode(&git_data[hash_start..hash_end]);

            let kind = if mode.starts_with("40") {
                EntryKind::Tree
            } else {
                EntryKind::Blob
            };

            entries.push(TreeEntry { name, kind, id: hash_hex });
            pos = hash_end;
        }

        Ok(TreeEntries(entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_roundtrip() {
        let content = b"hello git";
        let git_obj = GitTranslator::noa_blob_to_git(content);
        let back = GitTranslator::git_blob_to_noa(&git_obj).unwrap();
        assert_eq!(back, content);
    }

    #[test]
    fn test_tree_to_git_and_back() {
        let entries = TreeEntries(vec![
            TreeEntry { name: "main.rs".into(), kind: EntryKind::Blob, id: "ab".repeat(20) },
            TreeEntry { name: "lib".into(), kind: EntryKind::Tree, id: "cd".repeat(20) },
        ]);
        let git_data = GitTranslator::noa_tree_to_git(&entries);
        let parsed = GitTranslator::git_tree_to_noa(&git_data).unwrap();
        assert_eq!(parsed.0.len(), 2);
        assert_eq!(parsed.0[0].name, "main.rs");
        assert_eq!(parsed.0[0].kind, EntryKind::Blob);
        assert_eq!(parsed.0[1].name, "lib");
        assert_eq!(parsed.0[1].kind, EntryKind::Tree);
    }

    #[test]
    fn test_blob_empty_content() {
        let content = b"";
        let git_obj = GitTranslator::noa_blob_to_git(content);
        let back = GitTranslator::git_blob_to_noa(&git_obj).unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn test_blob_large_content() {
        let content = vec![0u8; 1024 * 1024];
        let git_obj = GitTranslator::noa_blob_to_git(&content);
        assert!(git_obj.len() > content.len());
        let back = GitTranslator::git_blob_to_noa(&git_obj).unwrap();
        assert_eq!(back.len(), content.len());
    }

    #[test]
    fn test_blob_binary_content() {
        let content: Vec<u8> = (0..=255).collect();
        let git_obj = GitTranslator::noa_blob_to_git(&content);
        let back = GitTranslator::git_blob_to_noa(&git_obj).unwrap();
        assert_eq!(back, content);
    }

    #[test]
    fn test_git_blob_to_noa_invalid_input() {
        let result = GitTranslator::git_blob_to_noa(b"no null byte here");
        assert!(result.is_err());
    }

    #[test]
    fn test_tree_empty_entries() {
        let entries = TreeEntries(vec![]);
        let git_data = GitTranslator::noa_tree_to_git(&entries);
        assert!(git_data.is_empty());
        let parsed = GitTranslator::git_tree_to_noa(&git_data).unwrap();
        assert!(parsed.0.is_empty());
    }

    #[test]
    fn test_tree_single_blob_entry() {
        let entries = TreeEntries(vec![
            TreeEntry { name: "README.md".into(), kind: EntryKind::Blob, id: "ff".repeat(20) },
        ]);
        let git_data = GitTranslator::noa_tree_to_git(&entries);
        let parsed = GitTranslator::git_tree_to_noa(&git_data).unwrap();
        assert_eq!(parsed.0.len(), 1);
        assert_eq!(parsed.0[0].id, "ff".repeat(20));
    }

    #[test]
    fn test_tree_with_header_prefix() {
        let entries = TreeEntries(vec![
            TreeEntry { name: "test.rs".into(), kind: EntryKind::Blob, id: "ab".repeat(20) },
        ]);
        let git_data = GitTranslator::noa_tree_to_git(&entries);
        let mut with_header = format!("tree {}\0", git_data.len()).into_bytes();
        with_header.extend_from_slice(&git_data);
        let parsed = GitTranslator::git_tree_to_noa(&with_header).unwrap();
        assert_eq!(parsed.0.len(), 1);
        assert_eq!(parsed.0[0].name, "test.rs");
    }
}
