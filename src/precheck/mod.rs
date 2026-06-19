//! Pre-commit gate logic: secret/credential scanning and an optional
//! `cargo check` compile gate.
//!
//! The installed `.git/hooks/pre-commit` shell wrapper is intentionally thin —
//! it just shells out to `noa hook pre-commit`, which dispatches here. Keeping
//! the real logic in Rust (rather than inline shell) makes it unit-testable.
//!
//! See [`scan_content`] for the pure secret-scanning primitive and
//! [`run_cargo_check`] for the build gate. [`scan_entries`] is the
//! source-agnostic scanning entry point fed by [`commit_source`], which tries
//! evernight IPC and local git to obtain staged data.

pub mod commit_source;

use std::path::Path;

use anyhow::{Context, Result, bail};
use regex::Regex;

/// Description of a single suspected secret found in a staged file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretHit {
    /// Path of the file as reported by `git diff --cached --name-only`.
    pub path: String,
    /// 1-based line number where the match starts.
    pub line: usize,
    /// Human-readable label, e.g. `"AWS access key id"`.
    pub kind: &'static str,
}

/// A compiled secret pattern: the regex and a label describing what it matches.
struct Pattern {
    re: Regex,
    kind: &'static str,
}

/// Catalogue of high-entropy credential patterns the pre-commit gate blocks.
/// Ordering only affects the order findings are reported in.
fn patterns() -> Vec<Pattern> {
    vec![
        Pattern {
            re: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            kind: "AWS access key id",
        },
        Pattern {
            re: Regex::new(r"gh[pousr]_[A-Za-z0-9]{36}").unwrap(),
            kind: "GitHub token",
        },
        Pattern {
            re: Regex::new(r"npm_[A-Za-z0-9]{36}").unwrap(),
            kind: "npm token",
        },
        Pattern {
            re: Regex::new(r"xox[baprs]-[A-Za-z0-9-]+").unwrap(),
            kind: "Slack token",
        },
        Pattern {
            re: Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap(),
            kind: "OpenAI-style secret key",
        },
        Pattern {
            re: Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").unwrap(),
            kind: "PEM private key",
        },
    ]
}

/// Scan a single file's textual content for known secret patterns.
///
/// Pure and allocation-light: takes the path (for reporting) and the already-
/// read file contents, returns every hit found. Binary/unreadable files should
/// be skipped by the caller (we never get here for them). One hit per line per
/// pattern kind — a line matching two different patterns yields two hits, which
/// is intentional (both are worth reporting).
pub fn scan_content(path: &str, content: &str) -> Vec<SecretHit> {
    let pats = patterns();
    let mut hits = Vec::new();
    for (idx, raw_line) in content.split('\n').enumerate() {
        let line_no = idx + 1;
        for pat in &pats {
            if pat.re.is_match(raw_line) {
                hits.push(SecretHit {
                    path: path.to_string(),
                    line: line_no,
                    kind: pat.kind,
                });
            }
        }
    }
    hits
}

/// Returns true when a staged path should be skipped by the scanner.
///
/// Skips: vendored/build-output directories, lock files, and the `.git`
/// internals — these either churn constantly (so they'd cause noise) or are
/// machine-generated (so a real secret there would not be actionable as a
/// "leaked into source" finding).
pub fn should_skip(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("target/")
        || lower.starts_with("node_modules/")
        || lower.starts_with(".git/")
        || lower.contains("/target/")
        || lower.contains("/node_modules/")
        || lower.contains("/.git/")
    {
        return true;
    }
    matches!(
        lower.as_str(),
        "cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "composer.lock"
            | "go.sum"
    ) || lower.ends_with(".lock")
}

/// A staged file paired with its already-read textual content.
///
/// The acquisition layer ([`commit_source`]) produces these from whichever
/// data source it could reach (evernight IPC or local git); the scanner then
/// consumes them uniformly via [`scan_entries`].
#[derive(Debug, Clone)]
pub struct StagedFile {
    /// Path of the file as reported by `git diff --cached --name-only`.
    pub path: String,
    /// Decoded textual contents of the file at that path.
    pub content: String,
}

/// Scan pre-fetched staged file contents for known secret patterns. This is
/// the source-agnostic core of the gate: [`scan_staged`] is a thin local-read
/// wrapper around it, while the evernight path in [`commit_source`] feeds it
/// directly with host-acquired contents.
pub fn scan_entries(entries: &[StagedFile]) -> Vec<SecretHit> {
    let mut hits = Vec::new();
    for e in entries {
        if should_skip(&e.path) {
            continue;
        }
        hits.extend(scan_content(&e.path, &e.content));
    }
    hits
}

/// Scan every staged file, skipping lock files / build outputs / binary files.
/// Returns the combined list of hits across all files. Reads are best-effort:
/// a file that cannot be decoded as UTF-8 is silently skipped (binary blobs
/// rarely contain the ASCII secret patterns we look for anyway).
pub fn scan_staged(files: &[String]) -> Vec<SecretHit> {
    let entries: Vec<StagedFile> = files
        .iter()
        .filter_map(|p| {
            std::fs::read_to_string(p).ok().map(|content| StagedFile {
                path: p.clone(),
                content,
            })
        })
        .collect();
    scan_entries(&entries)
}

/// Whether the caller has requested that the pre-commit gate be skipped.
///
/// Honoured by both the shell wrapper and the Rust entry point so a single
/// `NOA_SKIP_HOOKS=1 git commit ...` works regardless of which layer runs.
pub fn skip_requested() -> bool {
    matches!(std::env::var("NOA_SKIP_HOOKS"), Ok(v) if !v.is_empty())
}

/// Whether the caller has requested that only the `cargo check` gate be
/// skipped while the secret scan still runs.
///
/// Lets a deployment opt out of the (potentially slow) compile gate — e.g.
/// a self-healing loop that intentionally stages non-compiling intermediate
/// states — without weakening secret protection. Coarser [`skip_requested`]
/// (NOA_SKIP_HOOKS) always wins: when both are set, everything is skipped.
pub fn cargo_check_skip_requested() -> bool {
    matches!(std::env::var("NOA_SKIP_CARGO_CHECK"), Ok(v) if !v.is_empty())
}

/// Run `cargo check --workspace` against the current working directory if and
/// only if a `Cargo.toml` is present at the repo root. Non-Rust repos are
/// silently skipped. Capped at [`CARGO_CHECK_TIMEOUT_SECS`] seconds so a stuck
/// build toolchain can't wedge an interactive commit indefinitely.
pub fn run_cargo_check() -> Result<()> {
    if !Path::new("Cargo.toml").exists() {
        return Ok(());
    }
    eprintln!(
        "[noa pre-commit] Cargo.toml detected; running `cargo check --workspace` (timeout {}s)...",
        CARGO_CHECK_TIMEOUT_SECS
    );
    let status = run_cargo_check_inner()?;
    if !status.success() {
        bail!(
            "cargo check failed; aborting commit. \
             Fix the compile errors above or set NOA_SKIP_HOOKS=1 to bypass."
        );
    }
    Ok(())
}

/// Maximum wall-clock time `run_cargo_check` will allow `cargo check` to run
/// before killing it. Tuned to be generous enough for warm incremental builds
/// of moderately sized workspaces while still bounding worst-case latency.
const CARGO_CHECK_TIMEOUT_SECS: u64 = 300;

#[cfg(unix)]
fn run_cargo_check_inner() -> Result<std::process::ExitStatus> {
    use std::process::{Command, Stdio};
    // Prefer the system `timeout` wrapper when available — it's ubiquitous on
    // Linux/macOS and lets us avoid pulling in a Rust timeout-on-child crate.
    let status = Command::new("timeout")
        .args([
            &CARGO_CHECK_TIMEOUT_SECS.to_string(),
            "cargo",
            "check",
            "--workspace",
            "--quiet",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("spawning `timeout cargo check`")?;
    // `timeout` returns 124 when it had to kill the child for exceeding the
    // limit. Surface that as a distinct, actionable error.
    if status.code() == Some(124) {
        bail!(
            "cargo check exceeded the {}s budget and was killed; \
             aborting commit. Set NOA_SKIP_HOOKS=1 to bypass.",
            CARGO_CHECK_TIMEOUT_SECS
        );
    }
    Ok(status)
}

#[cfg(not(unix))]
fn run_cargo_check_inner() -> Result<std::process::ExitStatus> {
    use std::process::{Command, Stdio};
    // No portable `timeout` on Windows; run without an external cap. The
    // process still inherits stdio so the user sees live progress.
    let status = Command::new("cargo")
        .args(["check", "--workspace", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("spawning `cargo check`")?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_access_key() {
        // AKIA followed by exactly 16 uppercase alphanumerics.
        let content = format!("aws_key = \"AKIA{}\"\n", "ABCDEF0123456789");
        let hits = scan_content("config.toml", &content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "AWS access key id");
    }

    #[test]
    fn detects_github_token() {
        let token = format!("ghp_{}", "a".repeat(36));
        let hits = scan_content("ci.yml", &format!("token: {token}"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "GitHub token");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn detects_npm_token() {
        let token = format!("npm_{}", "Z".repeat(36));
        let hits = scan_content(".npmrc", &format!("//registry.npmjs.org/:_authToken={token}"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "npm token");
    }

    #[test]
    fn detects_slack_token() {
        let hits = scan_content("bot.conf", "SLACK_TOKEN=xoxb-123456789012-abcdefgh");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "Slack token");
    }

    #[test]
    fn detects_openai_key() {
        let key = format!("sk-{}", "9".repeat(24));
        let hits = scan_content("app.py", &format!("openai.api_key = '{key}'"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "OpenAI-style secret key");
    }

    #[test]
    fn detects_pem_private_key() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----\n";
        let hits = scan_content("id_rsa", content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "PEM private key");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn detects_ec_pem_private_key() {
        let content = "-----BEGIN EC PRIVATE KEY-----\nMHQCAQEE...\n";
        let hits = scan_content("ec.pem", content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "PEM private key");
    }

    #[test]
    fn reports_correct_line_numbers() {
        let token = format!("gho_{}", "x".repeat(36));
        let content = format!("line one\nline two\nGITHUB_TOKEN={token}\nline four\n");
        let hits = scan_content("env", &content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 3);
    }

    #[test]
    fn clean_content_has_no_hits() {
        let content = "fn main() { println!(\"hello\"); }\nconst VERSION = \"1.0.0\";\n";
        assert!(scan_content("main.rs", content).is_empty());
    }

    #[test]
    fn short_secret_prefix_does_not_match() {
        // `sk-` requires >= 20 chars after the dash; a short one must not match.
        let hits = scan_content("a.txt", "key = sk-short");
        assert!(hits.is_empty());
    }

    #[test]
    fn truncated_github_token_does_not_match() {
        // gh[pousr]_ requires exactly 36 alphanumerics.
        let too_short = format!("ghp_{}", "a".repeat(35));
        assert!(scan_content("a.txt", &too_short).is_empty());
    }

    #[test]
    fn multiple_hits_on_one_line_are_all_reported() {
        let aws = format!("AKIA{}", "0123456789ABCDEF");
        let ghp = format!("ghp_{}", "a".repeat(36));
        let line = format!("keys: {aws} and {ghp}");
        let hits = scan_content("keys.txt", &line);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|h| h.kind == "AWS access key id"));
        assert!(hits.iter().any(|h| h.kind == "GitHub token"));
    }

    #[test]
    fn should_skip_lock_files_and_build_dirs() {
        assert!(should_skip("Cargo.lock"));
        assert!(should_skip("package-lock.json"));
        assert!(should_skip("yarn.lock"));
        assert!(should_skip("pnpm-lock.yaml"));
        assert!(should_skip("composer.lock"));
        assert!(should_skip("go.sum"));
        assert!(should_skip("some.lock"));
        assert!(should_skip("target/debug/foo"));
        assert!(should_skip("crates/x/target/release/foo"));
        assert!(should_skip("node_modules/react/index.js"));
        assert!(should_skip(".git/HEAD"));
    }

    #[test]
    fn should_not_skip_source_files() {
        assert!(!should_skip("src/main.rs"));
        assert!(!should_skip("crates/foo/src/lib.rs"));
        assert!(!should_skip("README.md"));
        assert!(!should_skip("config/settings.toml"));
        assert!(!should_skip("scripts/deploy.sh"));
        // `mylock.rs` ends in `.rs`, not `.lock` — must not be skipped.
        assert!(!should_skip("src/mylock.rs"));
    }

    #[test]
    fn scan_staged_reads_files_and_aggregates_hits() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dirty = tmp.path().join("dirty.rs");
        std::fs::write(
            &dirty,
            "let k = \"AKIA0123456789ABCDEF\";\n",
        )
        .unwrap();
        let clean = tmp.path().join("clean.rs");
        std::fs::write(&clean, "fn main() {}\n").unwrap();

        let dirty_str = dirty.to_string_lossy().to_string();
        let clean_str = clean.to_string_lossy().to_string();
        let files = vec![dirty_str.clone(), clean_str];
        let hits = scan_staged(&files);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, dirty_str);
        assert_eq!(hits[0].kind, "AWS access key id");
    }

    #[test]
    fn cargo_check_skip_requested_returns_true_when_set() {
        // Snapshot-and-restore so parallel tests are unaffected.
        let prev = std::env::var("NOA_SKIP_CARGO_CHECK").ok();
        std::env::set_var("NOA_SKIP_CARGO_CHECK", "1");
        assert!(cargo_check_skip_requested());
        if let Some(v) = prev {
            std::env::set_var("NOA_SKIP_CARGO_CHECK", v);
        } else {
            std::env::remove_var("NOA_SKIP_CARGO_CHECK");
        }
    }

    #[test]
    fn cargo_check_skip_requested_returns_false_when_unset() {
        let prev = std::env::var("NOA_SKIP_CARGO_CHECK").ok();
        std::env::remove_var("NOA_SKIP_CARGO_CHECK");
        assert!(!cargo_check_skip_requested());
        if let Some(v) = prev {
            std::env::set_var("NOA_SKIP_CARGO_CHECK", v);
        }
    }

    #[test]
    fn cargo_check_skip_requested_returns_false_when_empty() {
        // An empty value is treated as "not set", consistent with skip_requested.
        let prev = std::env::var("NOA_SKIP_CARGO_CHECK").ok();
        std::env::set_var("NOA_SKIP_CARGO_CHECK", "");
        assert!(!cargo_check_skip_requested());
        if let Some(v) = prev {
            std::env::set_var("NOA_SKIP_CARGO_CHECK", v);
        } else {
            std::env::remove_var("NOA_SKIP_CARGO_CHECK");
        }
    }

    #[test]
    fn run_cargo_check_skips_non_rust_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        // No Cargo.toml present -> immediate Ok, no cargo invocation.
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let res = run_cargo_check();
        std::env::set_current_dir(prev).unwrap();
        assert!(res.is_ok());
    }

    #[test]
    fn run_cargo_check_blocks_on_failing_project() {
        // A minimal Rust project that fails to compile: `cargo check` should
        // return non-zero, and `run_cargo_check` should turn that into an Err.
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"failcheck\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[[bin]]\nname = \"failcheck\"\npath = \"main.rs\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("main.rs"),
            "fn main() { this_is_not_valid_rust ::: ; }\n",
        )
        .unwrap();

        // cargo must be available for this test to be meaningful; skip otherwise.
        if std::process::Command::new("cargo")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping run_cargo_check_blocks_on_failing_project: cargo not on PATH");
            return;
        }

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let res = run_cargo_check();
        std::env::set_current_dir(prev).unwrap();
        assert!(res.is_err(), "expected cargo check failure to be an Err");
        let msg = res.unwrap_err().to_string();
        assert!(
            msg.contains("cargo check failed") || msg.contains("exceeded"),
            "unexpected error message: {msg}"
        );
    }
}
