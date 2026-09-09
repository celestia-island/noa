//! Regression test for issue #70: repeated `noa pull` must neither oscillate
//! the workspace `default` HEAD nor misreport its status.
//!
//! Mirrors the issue repro at the CLI level: clone at v1, advance the remote
//! to v2, then `pull, pull, pull` with no remote changes between the last two.
//! Expected: heads `v1 -> v2 -> v2 -> v2`, pull 1 reports `re-imported`,
//! pulls 2 and 3 report `Already up to date.`
//!
//! Root cause (fixed): `import_git_to_noa` advanced the HEAD ref with
//! `cas(old = None)`, which succeeds exactly once, freezing HEAD at the
//! clone-time snapshot; `run_pull` then compared that frozen ref against the
//! pre-import workspace head — wrong verdict on real advances and a rollback
//! to the stale snapshot on no-op pulls.

use std::path::Path;
use std::process::{Command, Output};

fn noa_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_noa"))
}

fn run_git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {}: {e}", args.join(" ")))
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let out = run_git(dir, args);
    assert!(
        out.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn git_commit(dir: &Path, msg: &str) {
    git_ok(dir, &["add", "-A"]);
    let out = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("failed to run git commit");
    assert!(
        out.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Extract the `default` workspace head id from `noa workspace list` output
/// (lines look like `* default  head: noa_<hex> base: noa_<hex>`).
fn workspace_head(work: &Path) -> String {
    let out = noa_bin()
        .args(["workspace", "list"])
        .current_dir(work)
        .output()
        .expect("failed to run noa workspace list");
    assert!(
        out.status.success(),
        "noa workspace list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    for line in stdout.lines() {
        if line.contains("default") {
            if let Some(idx) = line.find("head:") {
                let rest = line[idx + "head:".len()..].trim_start();
                if let Some(head) = rest.split_whitespace().next() {
                    return head.to_string();
                }
            }
        }
    }
    panic!("default workspace head not found in:\n{stdout}");
}

fn noa_pull(work: &Path) -> String {
    let out = noa_bin()
        .arg("pull")
        .current_dir(work)
        .output()
        .expect("failed to run noa pull");
    assert!(
        out.status.success(),
        "noa pull failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn test_repeated_pull_keeps_head_stable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let upstream = tmp.path().join("upstream");
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&upstream).unwrap();

    git_ok(&upstream, &["init"]);
    git_ok(&upstream, &["config", "user.name", "test"]);
    git_ok(&upstream, &["config", "user.email", "test@test.com"]);
    std::fs::write(upstream.join("value.txt"), "v1\n").unwrap();
    git_commit(&upstream, "v1");

    // Forward slashes: Git-for-Windows mishandles some absolute forms, and
    // `C:/...` also satisfies noa's URL validation (scp-like syntax).
    let url = upstream.to_string_lossy().replace('\\', "/");
    let out = noa_bin()
        .args(["clone", &url, "--path", &work.to_string_lossy()])
        .output()
        .expect("failed to run noa clone");
    assert!(
        out.status.success(),
        "noa clone failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let head_v1 = workspace_head(&work);

    // Advance the remote: v1 -> v2.
    std::fs::write(upstream.join("value.txt"), "v2\n").unwrap();
    git_commit(&upstream, "v2");

    // Pull 1: real advance. Must report re-import and land on a new head.
    let pull1 = noa_pull(&work);
    assert!(
        pull1.contains("re-imported"),
        "pull 1 must report a real advance, got: {pull1}"
    );
    let head_after_pull1 = workspace_head(&work);
    assert_ne!(
        head_after_pull1, head_v1,
        "pull 1 must advance the workspace head past v1"
    );

    // Pull 2: no remote changes. Head must not move (previously rolled back
    // to the stale v1 snapshot) and the message must say so.
    let pull2 = noa_pull(&work);
    assert!(
        pull2.contains("Already up to date"),
        "pull 2 must report no changes, got: {pull2}"
    );
    let head_after_pull2 = workspace_head(&work);
    assert_eq!(
        head_after_pull2, head_after_pull1,
        "pull 2 with no remote changes must leave HEAD at the v2 snapshot"
    );

    // Pull 3: oscillation check — must stay put again.
    let pull3 = noa_pull(&work);
    assert!(
        pull3.contains("Already up to date"),
        "pull 3 must report no changes, got: {pull3}"
    );
    let head_after_pull3 = workspace_head(&work);
    assert_eq!(
        head_after_pull3, head_after_pull1,
        "pull 3 must not oscillate the workspace HEAD"
    );

    // The git checkout tracks the remote throughout (trim_end: Windows git
    // may check out CRLF depending on core.autocrlf).
    let value = std::fs::read_to_string(work.join("value.txt")).unwrap();
    assert_eq!(
        value.trim_end(),
        "v2",
        "git workdir content must stay at v2"
    );
    let log = git_ok(&work, &["log", "--oneline"]);
    assert!(log.contains("v2"), "git log must stay at v2, got: {log}");
}
