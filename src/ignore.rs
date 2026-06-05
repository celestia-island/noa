use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub struct IgnoreMatcher {
    gi: Gitignore,
}

impl IgnoreMatcher {
    pub fn from_repo_root(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);

        if let Some(ignore_file) = Self::find_gitignore(root) {
            let _ = builder.add(&ignore_file);
        }

        Self::add_nested_gitignores(root, &mut builder);

        let exclude_path = root.join(".git").join("info").join("exclude");
        if exclude_path.exists() {
            let _ = builder.add(&exclude_path);
        }

        let gi = builder.build().unwrap_or_else(|_| {
            let b = GitignoreBuilder::new(root);
            b.build().unwrap()
        });

        IgnoreMatcher { gi }
    }

    pub fn should_skip(&self, path: &str, is_dir: bool) -> bool {
        if Self::is_noa_internal(path) {
            return true;
        }

        if self.is_ignored(path, is_dir) {
            return true;
        }

        let mut current = Path::new(path);
        while let Some(parent) = current.parent() {
            if parent == Path::new("") {
                break;
            }
            if self.is_ignored(&parent.to_string_lossy(), true) {
                return true;
            }
            current = parent;
        }

        false
    }

    pub fn is_ignored(&self, path: &str, is_dir: bool) -> bool {
        let p = Path::new(path);
        match self.gi.matched(p, is_dir) {
            ignore::Match::Ignore(_) => true,
            ignore::Match::Whitelist(_) => false,
            ignore::Match::None => false,
        }
    }

    fn is_noa_internal(path: &str) -> bool {
        path.starts_with(".noa/") || path == ".noa" || path.starts_with(".noa\\")
    }

    fn find_gitignore(root: &Path) -> Option<std::path::PathBuf> {
        let g = root.join(".gitignore");
        if g.exists() {
            Some(g)
        } else {
            None
        }
    }

    fn add_nested_gitignores(dir: &Path, builder: &mut GitignoreBuilder) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name == ".git" || name == ".noa" {
                    continue;
                }
                let gi = path.join(".gitignore");
                if gi.exists() {
                    let _ = builder.add(&gi);
                }
                Self::add_nested_gitignores(&path, builder);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_repo_root(tmp: &TempDir) -> &Path {
        tmp.path()
    }

    #[test]
    fn test_noa_paths_always_skipped() {
        let tmp = TempDir::new().unwrap();
        let matcher = IgnoreMatcher::from_repo_root(make_repo_root(&tmp));
        assert!(matcher.should_skip(".noa", true));
        assert!(matcher.should_skip(".noa/config", false));
        assert!(matcher.should_skip(".noa/noa.redb", false));
        assert!(matcher.should_skip(".noa/agent-logs/default.log", false));
    }

    #[test]
    fn test_gitignore_patterns() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        fs::write(root.join(".gitignore"), "*.log\ntarget/\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("debug.log", false));
        assert!(matcher.should_skip("target", true));
        assert!(matcher.should_skip("target/dep.rs", false));
        assert!(!matcher.should_skip("src/main.rs", false));
    }

    #[test]
    fn test_negation_patterns() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        fs::write(root.join(".gitignore"), "*.log\n!important.log\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("debug.log", false));
        assert!(!matcher.should_skip("important.log", false));
    }

    #[test]
    fn test_nested_gitignore() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        fs::write(root.join(".gitignore"), "").unwrap();

        let subdir = root.join("src");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join(".gitignore"), "*.gen.rs\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("src/foo.gen.rs", false));
        assert!(!matcher.should_skip("src/main.rs", false));
    }

    #[test]
    fn test_git_info_exclude() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        let git_info = root.join(".git").join("info");
        fs::create_dir_all(&git_info).unwrap();
        fs::write(git_info.join("exclude"), "*.secret\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("keys.secret", false));
    }

    #[test]
    fn test_no_gitignore_nothing_ignored_except_noa() {
        let tmp = TempDir::new().unwrap();
        let matcher = IgnoreMatcher::from_repo_root(make_repo_root(&tmp));
        assert!(!matcher.should_skip("main.rs", false));
        assert!(!matcher.should_skip("src/lib.rs", false));
        assert!(matcher.should_skip(".noa", true));
    }

    #[test]
    fn test_deep_nested_gitignore() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        std::fs::write(root.join(".gitignore"), "").unwrap();

        let deep = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join(".gitignore"), "*.gen\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("a/b/c/foo.gen", false));
        assert!(!matcher.should_skip("a/b/c/foo.rs", false));
    }

    #[test]
    fn test_directory_pattern_with_children() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("build", true));
        assert!(matcher.should_skip("build/output.js", false));
        assert!(matcher.should_skip("build/debug/bin", true));
        assert!(!matcher.should_skip("src/main.rs", false));
    }

    #[test]
    fn test_wildcard_patterns() {
        let tmp = TempDir::new().unwrap();
        let root = make_repo_root(&tmp);
        std::fs::write(root.join(".gitignore"), "*.rs.bk\ndocs/*.pdf\n").unwrap();

        let matcher = IgnoreMatcher::from_repo_root(root);
        assert!(matcher.should_skip("main.rs.bk", false));
        assert!(!matcher.should_skip("main.rs", false));
        assert!(matcher.should_skip("docs/manual.pdf", false));
        assert!(!matcher.should_skip("docs/readme.md", false));
    }

    #[test]
    fn test_noa_internal_various_paths() {
        let tmp = TempDir::new().unwrap();
        let matcher = IgnoreMatcher::from_repo_root(make_repo_root(&tmp));
        assert!(matcher.should_skip(".noa", true));
        assert!(matcher.should_skip(".noa/noa.redb", false));
        assert!(matcher.should_skip(".noa/agent-logs/default.log", false));
        assert!(matcher.should_skip(".noa/config", false));
        assert!(matcher.should_skip(".noa/HEAD", false));
    }
}
