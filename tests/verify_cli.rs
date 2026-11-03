//! CLI-level integration tests for `noa verify append` (the authority-switch
//! precondition ⑥). Each test drives the REAL binary via
//! `CARGO_BIN_EXE_noa` in a throwaway repository — no library shortcuts, the
//! exact code path a hook or the Python ledger writer would take.

use std::io::Write;
use std::process::{Command, Stdio};

fn noa_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_noa"))
}

fn init_repo(dir: &std::path::Path) {
    let out = noa_bin()
        .arg("init")
        .arg(dir)
        .arg("--no-git")
        .output()
        .expect("noa init runs");
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const GOOD_RECORD: &str = r#"{"schema":"verify.run/1","kind":"partial","repo":"o/r","head":"abc","tree_clean":true,"env":{"toolchain":"t","platform":"p"},"commands":[],"not_run":[{"what":"clippy"}],"outputs":[],"agent":"itest","ts":1}"#;

fn append_stdin(dir: &std::path::Path, payload: &str, ws: Option<&str>) -> (bool, String) {
    let mut cmd = noa_bin();
    cmd.current_dir(dir)
        .arg("verify")
        .arg("append")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(ws) = ws {
        cmd.arg("--workspace").arg(ws);
    }
    let mut child = cmd.spawn().expect("spawn noa verify append");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn agent_log_line_count(dir: &std::path::Path) -> usize {
    let path = dir.join(".noa").join("agent-logs").join("default.log");
    std::fs::read_to_string(path)
        .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

#[test]
fn cli_append_lands_and_counts_seq() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    let (ok1, out1) = append_stdin(tmp.path(), GOOD_RECORD, None);
    assert!(ok1, "first append failed: {out1}");
    assert!(out1.contains("seq=1"), "seq starts at 1: {out1}");
    let (ok2, out2) = append_stdin(tmp.path(), GOOD_RECORD, None);
    assert!(ok2, "second append failed: {out2}");
    assert!(out2.contains("seq=2"), "seq increments: {out2}");
    assert_eq!(agent_log_line_count(tmp.path()), 2);
}

#[test]
fn cli_append_rejects_malformed_and_empty() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    let (ok, out) = append_stdin(tmp.path(), "not json", None);
    assert!(!ok, "malformed payload must fail: {out}");
    assert!(
        out.contains("verify.run/1"),
        "error names the contract: {out}"
    );
    let (ok_empty, out_empty) = append_stdin(tmp.path(), "", None);
    assert!(!ok_empty, "empty stdin must fail: {out_empty}");
    assert_eq!(agent_log_line_count(tmp.path()), 0, "nothing may half-land");
}

#[test]
fn cli_append_rejects_unknown_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    let (ok, out) = append_stdin(tmp.path(), GOOD_RECORD, Some("typo-ws"));
    assert!(!ok, "typo workspace must be rejected: {out}");
    assert!(
        out.contains("does not exist"),
        "error says the ledger would be an orphan: {out}"
    );
    let (ok_trav, out_trav) = append_stdin(tmp.path(), GOOD_RECORD, Some("../escaped"));
    assert!(!ok_trav, "traversal must be rejected: {out_trav}");
    // and nothing may exist outside agent-logs/
    assert!(
        !tmp.path().join(".noa").join("escaped.log").exists(),
        "no file may escape agent-logs/"
    );
}

#[test]
fn cli_append_file_flag_matches_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    let rec_path = tmp.path().join("rec.json");
    std::fs::write(&rec_path, GOOD_RECORD).unwrap();
    let out = noa_bin()
        .current_dir(tmp.path())
        .arg("verify")
        .arg("append")
        .arg("--file")
        .arg(&rec_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(agent_log_line_count(tmp.path()), 1);
}
