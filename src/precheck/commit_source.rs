//! Multi-path acquisition of staged commit data for the pre-commit gate.
//!
//! The gate needs staged file paths + contents to scan for leaked secrets. In
//! ordinary local development noa reads git directly. In an agent/container
//! context (entelecheia surgery, polemos host tooling) the authoritative
//! `.git` lives on the host and is reachable only through the evernight
//! `Command.Exec` JSON-RPC daemon. entelecheia and evernight both ultimately
//! expose host command execution through that same unix socket, so the two
//! "trigger via entelecheia/evernight" paths collapse to one transport here.
//!
//! Acquisition order with [`Mode::Auto`] (the default, **local-first**):
//!   1. local `git diff --cached` — the pre-commit hook runs *inside* the
//!      repository being committed (it is spawned by `git commit`), so the
//!      local git binary and `.git` are always authoritative and available.
//!      When this succeeds — even with an empty staged list — it is used.
//!   2. evernight IPC (`Command.Exec` running host `git diff --cached`) — used
//!      only as a fallback when local git is unavailable (non-git cwd, missing
//!      `git` binary, or a failing command).
//!
//! Why local-first: evernight, when reachable, runs `git` in the **daemon's**
//! own cwd, not the committing repository's cwd. Preferring evernight
//! therefore made the default `auto` mode query the wrong tree on any machine
//! where the socket happens to be up, silently passing real leaks. Since the
//! hook itself is invoked from within the target repository, local git is the
//! correct and authoritative source; evernight only matters when noa runs in a
//! context that cannot see the target `.git` (e.g. some agent/container
//! setups), and even then `NOA_HOST_REPO` should be set to the host repo's
//! absolute path so evernight queries the right tree.
//!
//! If every source is unreachable, [`acquire_with`] returns
//! [`Acquisition::NoSource`] and the caller silently passes the commit: a
//! data-source outage must never block a commit. Real findings, once any
//! source yields data, still block as normal.
//!
//! # Protocol
//! One newline-delimited JSON-RPC 2.0 round trip per call:
//! ```text
//! >  {"jsonrpc":"2.0","method":"Command.Exec","params":{"command":"git ...","cwd":"...","timeout":N},"id":1}
//! <  {"jsonrpc":"2.0","result":{"exit_code":0,"stdout":"...","stderr":"..."},"id":1}
//! ```
//! The server keeps the connection open (one request → one newline-ended
//! response per loop iteration), so the client reads exactly one line rather
//! than reading to EOF. Every phase is bounded by [`AcquireConfig::timeout`]
//! so a wedged daemon cannot stall an interactive commit.

use std::{
    io::{self, BufRead, BufReader, Write},
    time::Duration,
};
use tracing::debug;

use super::StagedFile;

/// Default evernight IPC socket — matches entelecheia's `evernight_exec`
/// (scepter surgery hooks) and polemos's host bridge.
const DEFAULT_EVERNIGHT_SOCK: &str = "/run/entelecheia/evernight-e.sock";
/// Default per-round-trip connect/read/write budget. Kept short on purpose so
/// a dead or slow daemon cannot wedge an interactive `git commit`.
const DEFAULT_EVERNIGHT_TIMEOUT_SECS: u64 = 2;
/// Server-side timeout forwarded to `Command.Exec` for the git command itself.
/// `git diff --cached --name-only` is sub-second even on large repos; this is
/// just a generous safety ceiling for the rare slow case.
const EVERNIGHT_GIT_TIMEOUT_SECS: u64 = 10;

/// Which data source(s) to consult.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Try local git first, then evernight (the default, **local-first**).
    Auto,
    /// Only evernight; if unreachable → [`Acquisition::NoSource`].
    Evernight,
    /// Only local git.
    Local,
}

/// Knobs for acquisition, normally populated from the environment via
/// [`AcquireConfig::from_env`]. Fields are `pub` so tests can inject values
/// without touching the (process-global, non-thread-safe) environment.
#[derive(Debug, Clone)]
pub struct AcquireConfig {
    /// evernight IPC socket path.
    pub socket: String,
    /// Per-round-trip connect/read/write budget.
    pub timeout: Duration,
    /// `cwd` passed to evernight `Command.Exec` (host repo root).
    pub host_repo: String,
    /// `cwd` for the local-git fallback and for local working-tree reads in
    /// the evernight path. Defaults to `"."` (the noa process's cwd).
    pub local_repo: String,
    pub mode: Mode,
}

impl AcquireConfig {
    /// Build from environment variables, falling back to ecosystem defaults.
    ///
    /// - `EVERNIGHT_SOCK` — evernight socket path (shared convention with
    ///   entelecheia/evernight; default `/run/entelecheia/evernight-e.sock`).
    /// - `NOA_EVERNIGHT_TIMEOUT_SECS` — round-trip budget (default `2`).
    /// - `NOA_HOST_REPO` — host cwd for evernight (default `.`).
    /// - `NOA_LOCAL_REPO` — local cwd for git/reads (default `.`).
    /// - `NOA_HOOK_DATA_SOURCE` — `auto` (default) | `evernight` | `local`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_env() -> Self {
        let socket =
            std::env::var("EVERNIGHT_SOCK").unwrap_or_else(|_| DEFAULT_EVERNIGHT_SOCK.to_string());
        let timeout = std::env::var("NOA_EVERNIGHT_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&secs: &u64| secs > 0)
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_EVERNIGHT_TIMEOUT_SECS));
        let host_repo = std::env::var("NOA_HOST_REPO").unwrap_or_else(|_| ".".to_string());
        let local_repo = std::env::var("NOA_LOCAL_REPO").unwrap_or_else(|_| ".".to_string());
        let mode = match std::env::var("NOA_HOOK_DATA_SOURCE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "evernight" => Mode::Evernight,
            "local" => Mode::Local,
            _ => Mode::Auto,
        };
        Self {
            socket,
            timeout,
            host_repo,
            local_repo,
            mode,
        }
    }
}

impl Default for AcquireConfig {
    fn default() -> Self {
        Self {
            socket: DEFAULT_EVERNIGHT_SOCK.to_string(),
            timeout: Duration::from_secs(DEFAULT_EVERNIGHT_TIMEOUT_SECS),
            host_repo: ".".to_string(),
            local_repo: ".".to_string(),
            mode: Mode::Auto,
        }
    }
}

/// Outcome of an acquisition attempt.
pub enum Acquisition {
    /// At least one source returned data. The list may be empty when nothing
    /// is staged — that is still a valid, scannable state (cargo check still
    /// runs in that case).
    Ok(Vec<StagedFile>),
    /// No source was reachable. The caller should silently pass the commit
    /// (exit 0) rather than block on a data-source outage.
    NoSource,
}

/// Acquire staged file data using environment-derived configuration.
pub fn acquire_staged() -> Acquisition {
    acquire_with(&AcquireConfig::from_env())
}

/// Acquire staged file data using the given configuration. Any error from an
/// individual source is logged at `debug` level and the next source is tried;
/// only total failure yields [`Acquisition::NoSource`].
///
/// Order in `Mode::Auto` is **local-first**: local git is authoritative for a
/// pre-commit hook (which runs inside the committing repo), so it is tried
/// before evernight. Explicit `Mode::Local` / `Mode::Evernight` force a single
/// path.
pub fn acquire_with(cfg: &AcquireConfig) -> Acquisition {
    let try_local = matches!(cfg.mode, Mode::Auto | Mode::Local);
    let try_evernight = matches!(cfg.mode, Mode::Auto | Mode::Evernight);

    if try_local {
        match via_local_git(cfg) {
            Ok(files) => return Acquisition::Ok(files),
            Err(e) => debug!(error = %e, "local git commit-data source unavailable"),
        }
    }
    if try_evernight {
        match via_evernight(cfg) {
            Ok(files) => return Acquisition::Ok(files),
            Err(e) => debug!(error = %e, "evernight commit-data source unavailable"),
        }
    }
    debug!("no commit-data source available; pre-commit gate will silently pass");
    Acquisition::NoSource
}

/// Local fallback: run `git diff --cached --name-only` in [`local_repo`] and
/// read each file's working-tree contents from disk.
fn via_local_git(cfg: &AcquireConfig) -> io::Result<Vec<StagedFile>> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only", "--diff-filter=ACMRTUXB"])
        .current_dir(&cfg.local_repo)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "git diff --cached failed: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let base = std::path::Path::new(&cfg.local_repo);
    let mut files = Vec::new();
    for name in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Ok(content) = std::fs::read_to_string(base.join(name)) {
            files.push(StagedFile {
                path: name.to_string(),
                content,
            });
        }
    }
    Ok(files)
}

/// evernight fallback: ask the host daemon to run `git diff --cached
/// --name-only` (the authoritative host view), then read each file's contents
/// — locally first, via evernight as a fallback.
///
/// Only reached in `Mode::Auto` when local git was unavailable (e.g. noa
/// running where it cannot see the target `.git`). Note that evernight runs
/// `git` in its own cwd unless `NOA_HOST_REPO` is set, so callers in
/// agent/container contexts must point `NOA_HOST_REPO` at the committing
/// repo's absolute path to avoid scanning the wrong tree.
#[cfg(unix)]
fn via_evernight(cfg: &AcquireConfig) -> io::Result<Vec<StagedFile>> {
    let names_raw = exec_via_evernight(cfg, "diff --cached --name-only --diff-filter=ACMRTUXB")?;
    let base = std::path::Path::new(&cfg.local_repo);
    let mut files = Vec::new();
    for name in names_raw.lines().map(str::trim).filter(|l| !l.is_empty()) {
        // Prefer a local working-tree read (fast; works when the tree is
        // bind-mounted even if `.git` is host-only). Fall back to fetching the
        // staged blob via evernight when the file is not reachable locally.
        let content = match std::fs::read_to_string(base.join(name)) {
            Ok(c) => c,
            Err(_) => match exec_via_evernight(cfg, &format!("show :{name}")) {
                Ok(c) => c,
                Err(_) => continue,
            },
        };
        files.push(StagedFile {
            path: name.to_string(),
            content,
        });
    }
    Ok(files)
}

#[cfg(not(unix))]
fn via_evernight(_cfg: &AcquireConfig) -> io::Result<Vec<StagedFile>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "evernight IPC requires a Unix domain socket",
    ))
}

/// One newline-delimited JSON-RPC round trip to evernight, running
/// `git <subcommand>` via `Command.Exec`. Returns the command's stdout.
///
/// Connect/read/write are each bounded by [`AcquireConfig::timeout`]; a missing
/// socket, a refused/stale socket, or a timed-out response all surface as an
/// `Err` so the caller can fall through to the next source or silently pass.
#[cfg(unix)]
fn exec_via_evernight(cfg: &AcquireConfig, git_subcommand: &str) -> io::Result<String> {
    use std::os::unix::net::UnixStream;

    let command = format!("git {git_subcommand}");
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "Command.Exec",
        "params": {
            "command": command,
            "cwd": cfg.host_repo,
            "timeout": EVERNIGHT_GIT_TIMEOUT_SECS,
        },
        "id": 1,
    });
    let mut line = serde_json::to_string(&request)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    line.push('\n');

    let mut stream = UnixStream::connect(&cfg.socket)?;
    stream.set_read_timeout(Some(cfg.timeout))?;
    stream.set_write_timeout(Some(cfg.timeout))?;
    stream.write_all(line.as_bytes())?;
    stream.flush()?;

    // The server keeps the connection open (one request → one newline-ended
    // response per loop iteration), so read exactly one line, not to EOF.
    let mut reader = BufReader::new(&stream);
    let mut resp = String::with_capacity(8192);
    reader.read_line(&mut resp)?;
    if resp.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "evernight: connection closed before response",
        ));
    }

    let v: serde_json::Value = serde_json::from_str(resp.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("evernight parse: {e}")))?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(io::Error::other(format!("evernight: {msg}")));
    }
    let result = v
        .get("result")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "evernight: no result field"))?;
    let exit_code = result
        .get("exit_code")
        .and_then(|c| c.as_i64())
        .unwrap_or(-1);
    if exit_code != 0 {
        return Err(io::Error::other(format!(
            "evernight: git exited with code {exit_code}"
        )));
    }
    let stdout = result
        .get("stdout")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_defaults_when_unset() {
        // Snapshot taken with the vars unset; restore afterwards so parallel
        // tests are unaffected. We only assert the resolved defaults here.
        let prev_sock = std::env::var("EVERNIGHT_SOCK").ok();
        let prev_mode = std::env::var("NOA_HOOK_DATA_SOURCE").ok();
        std::env::remove_var("EVERNIGHT_SOCK");
        std::env::remove_var("NOA_HOOK_DATA_SOURCE");

        let cfg = AcquireConfig::from_env();
        assert_eq!(cfg.socket, DEFAULT_EVERNIGHT_SOCK);
        assert_eq!(
            cfg.timeout,
            Duration::from_secs(DEFAULT_EVERNIGHT_TIMEOUT_SECS)
        );
        assert_eq!(cfg.mode, Mode::Auto);

        if let Some(v) = prev_sock {
            std::env::set_var("EVERNIGHT_SOCK", v);
        }
        if let Some(v) = prev_mode {
            std::env::set_var("NOA_HOOK_DATA_SOURCE", v);
        }
    }

    #[test]
    fn evernight_missing_socket_yields_no_source_when_forced() {
        // Point at a path that provably does not exist and force evernight-
        // only mode: the connect must fail fast and acquisition must degrade
        // to NoSource rather than panic or block.
        let cfg = AcquireConfig {
            socket: "/tmp/noa-precheck-definitely-missing.sock".to_string(),
            mode: Mode::Evernight,
            ..AcquireConfig::default()
        };
        match acquire_with(&cfg) {
            Acquisition::NoSource => {}
            Acquisition::Ok(files) => {
                panic!("expected NoSource, acquired {} files", files.len());
            }
        }
    }

    #[test]
    fn no_source_when_all_paths_unavailable() {
        // evernight socket missing AND local_repo has no `.git`: every path
        // fails, so the gate must signal a silent pass.
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = AcquireConfig {
            socket: tmp
                .path()
                .join("missing.sock")
                .to_string_lossy()
                .to_string(),
            local_repo: tmp.path().to_string_lossy().to_string(),
            mode: Mode::Auto,
            ..AcquireConfig::default()
        };
        match acquire_with(&cfg) {
            Acquisition::NoSource => {}
            Acquisition::Ok(files) => panic!("expected NoSource, got {} files", files.len()),
        }
    }

    #[cfg(unix)]
    #[test]
    fn evernight_mock_returns_staged_file_and_reads_content() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("evernight.sock");
        // The "staged" file exists in local_repo; content is read locally.
        std::fs::write(tmp.path().join("leak.txt"), "key = AKIA0123456789ABCDEF\n").unwrap();

        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut conn, _) = listener.accept().unwrap();
            // Drain the request line, then write exactly one response line.
            let mut buf = [0u8; 8192];
            let _ = conn.read(&mut buf);
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "result": { "exit_code": 0, "stdout": "leak.txt\n", "stderr": "" },
                "id": 1,
            });
            let mut line = serde_json::to_string(&resp).unwrap();
            line.push('\n');
            let _ = conn.write_all(line.as_bytes());
            let _ = conn.flush();
            // Hold the connection open briefly to prove the client reads only
            // one line and does not block waiting for EOF.
            thread::sleep(Duration::from_millis(300));
        });

        let cfg = AcquireConfig {
            socket: sock.to_string_lossy().to_string(),
            local_repo: tmp.path().to_string_lossy().to_string(),
            host_repo: tmp.path().to_string_lossy().to_string(),
            mode: Mode::Evernight,
            ..AcquireConfig::default()
        };
        let files = match acquire_with(&cfg) {
            Acquisition::Ok(f) => f,
            Acquisition::NoSource => panic!("expected data from mock evernight"),
        };
        handle.join().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "leak.txt");
        assert!(files[0].content.contains("AKIA0123456789ABCDEF"));
    }

    #[cfg(unix)]
    #[test]
    fn evernight_error_response_is_treated_as_unavailable() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("evernight-err.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = conn.read(&mut buf);
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "error": { "code": -32603, "message": "boom" },
                "id": 1,
            });
            let mut line = serde_json::to_string(&resp).unwrap();
            line.push('\n');
            let _ = conn.write_all(line.as_bytes());
            let _ = conn.flush();
            thread::sleep(Duration::from_millis(200));
        });

        let cfg = AcquireConfig {
            socket: sock.to_string_lossy().to_string(),
            mode: Mode::Evernight,
            ..AcquireConfig::default()
        };
        match acquire_with(&cfg) {
            Acquisition::NoSource => {}
            Acquisition::Ok(files) => panic!("expected NoSource on error response, got {files:?}"),
        }
        handle.join().unwrap();
    }

    #[test]
    fn local_git_acquires_staged_files() {
        // End-to-end local path: init a throwaway repo, stage a file, confirm
        // acquisition surfaces it. Fail loudly when git is missing — this is
        // the only coverage for the secret-leak prevention in commit-source
        // acquisition, and silent skip would mask a real regression on
        // minimal CI images.
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            panic!(
                "git must be on PATH for the local-git acquisition regression test; \
                 this is a CI environment bug, not a benign skip"
            );
        }
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        git_in(repo, &["init", "-q"]);
        git_in(repo, &["config", "user.name", "noa-test"]);
        git_in(repo, &["config", "user.email", "noa-test@example.com"]);
        std::fs::write(repo.join("leak.txt"), "AKIA0123456789ABCDEF\n").unwrap();
        git_in(repo, &["add", "leak.txt"]);

        let cfg = AcquireConfig {
            local_repo: repo.to_string_lossy().to_string(),
            mode: Mode::Local,
            ..AcquireConfig::default()
        };
        let files = match acquire_with(&cfg) {
            Acquisition::Ok(f) => f,
            Acquisition::NoSource => panic!("expected local git to provide data"),
        };
        assert_eq!(files.len(), 1, "got {files:?}");
        assert_eq!(files[0].path, "leak.txt");
        assert!(files[0].content.contains("AKIA0123456789ABCDEF"));
    }

    fn git_in(repo: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git spawns");
        assert!(
            status.success(),
            "git {} failed in {}",
            args.join(" "),
            repo.display()
        );
    }

    /// The core regression guard: in Mode::Auto, local git must win even
    /// though the (mocked) evernight socket is reachable. Unix-only because
    /// the test mocks the evernight socket via a UnixListener — on Windows
    /// the test would silently `eprintln!` and report PASS without running
    /// a single assertion, which masked the contract on non-Unix platforms.
    /// Gating with `#[cfg(unix)]` makes the test show up as filtered (not
    /// passing) on Windows.
    #[test]
    #[cfg(unix)]
    fn auto_prefers_local_git_when_available() {
        // git is a hard prerequisite for this test. Fail loudly when it is
        // missing — silent skip would mask the regression on minimal CI.
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            panic!(
                "git must be on PATH for the auto-mode local-git preference test; \
                 this is a CI environment bug, not a benign skip"
            );
        }
        {
            use std::os::unix::net::UnixListener;
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc;
            use std::thread;

            let tmp = tempfile::TempDir::new().unwrap();
            let repo = tmp.path();
            let sock = tmp.path().join("would-be-consulted.sock");
            // Real staged file locally.
            git_in(repo, &["init", "-q"]);
            git_in(repo, &["config", "user.name", "noa-test"]);
            git_in(repo, &["config", "user.email", "noa-test@example.com"]);
            std::fs::write(repo.join("local_leak.txt"), "AKIA0123456789ABCDEF\n").unwrap();
            git_in(repo, &["add", "local_leak.txt"]);

            // Mock evernight: non-blocking accept loop that polls briefly, so
            // the thread exits cleanly whether or not a client connects. The
            // shared flag records any connection — if evernight is consulted
            // at all, the test fails.
            let contacted = Arc::new(AtomicBool::new(false));
            let listener = UnixListener::bind(&sock).unwrap();
            listener.set_nonblocking(true).unwrap();
            let contacted_srv = contacted.clone();
            let handle = thread::spawn(move || {
                use std::io::{Read, Write};
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                loop {
                    match listener.accept() {
                        Ok((mut conn, _)) => {
                            contacted_srv.store(true, Ordering::SeqCst);
                            let mut buf = [0u8; 8192];
                            let _ = conn.read(&mut buf);
                            let resp = serde_json::json!({
                                "jsonrpc": "2.0",
                                "result": {
                                    "exit_code": 0,
                                    "stdout": "evernight_only_file.txt\n",
                                    "stderr": ""
                                },
                                "id": 1,
                            });
                            let mut line = serde_json::to_string(&resp).unwrap();
                            line.push('\n');
                            // Propagate write errors instead of swallowing
                            // them with `let _ =` — a broken mock must
                            // surface as a test failure, not a silent skip.
                            conn.write_all(line.as_bytes())
                                .expect("mock server write_all");
                            conn.flush().expect("mock server flush");
                            return;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return;
                            }
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => return,
                    }
                }
            });

            let cfg = AcquireConfig {
                socket: sock.to_string_lossy().to_string(),
                local_repo: repo.to_string_lossy().to_string(),
                host_repo: repo.to_string_lossy().to_string(),
                mode: Mode::Auto,
                ..AcquireConfig::default()
            };
            let files = match acquire_with(&cfg) {
                Acquisition::Ok(f) => f,
                Acquisition::NoSource => panic!("expected local git to provide data in auto mode"),
            };
            handle.join().expect("server thread panicked");

            assert!(
                !contacted.load(Ordering::SeqCst),
                "auto mode must NOT consult evernight when local git is available"
            );
            assert_eq!(files.len(), 1, "got {files:?}");
            assert_eq!(
                files[0].path, "local_leak.txt",
                "auto mode must source from local git, not evernight"
            );
            assert!(files[0].content.contains("AKIA0123456789ABCDEF"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn auto_falls_back_to_evernight_when_local_git_unavailable() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        // local_repo points at a directory with no `.git`, so via_local_git
        // fails and auto must fall through to evernight.
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("fallback.sock");
        // Separate dir that has the working-tree file but NO `.git`, so local
        // git fails while evernight's local read still resolves the file.
        let no_git_dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            no_git_dir.path().join("leak.txt"),
            "key = AKIA0123456789ABCDEF\n",
        )
        .unwrap();

        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = conn.read(&mut buf);
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "result": { "exit_code": 0, "stdout": "leak.txt\n", "stderr": "" },
                "id": 1,
            });
            let mut line = serde_json::to_string(&resp).unwrap();
            line.push('\n');
            let _ = conn.write_all(line.as_bytes());
            let _ = conn.flush();
            thread::sleep(Duration::from_millis(200));
        });

        let cfg = AcquireConfig {
            socket: sock.to_string_lossy().to_string(),
            local_repo: no_git_dir.path().to_string_lossy().to_string(),
            host_repo: tmp.path().to_string_lossy().to_string(),
            mode: Mode::Auto,
            ..AcquireConfig::default()
        };
        let files = match acquire_with(&cfg) {
            Acquisition::Ok(f) => f,
            Acquisition::NoSource => panic!("auto must fall back to evernight when local fails"),
        };
        handle.join().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "leak.txt");
        assert!(files[0].content.contains("AKIA0123456789ABCDEF"));
    }
}
