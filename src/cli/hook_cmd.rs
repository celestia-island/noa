use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::precheck;
use crate::precheck::commit_source::Acquisition;

const COMMIT_MSG_HOOK: &str = include_str!("../../assets/commit-msg.sh");
const PRE_COMMIT_HOOK: &str = include_str!("../../assets/pre-commit.sh");

/// Sentinel string present in every noa-managed hook script.
/// Used to detect whether an existing hook file is safe to overwrite.
const NOA_MANAGED_SENTINEL: &str = "noa-managed";

pub struct InstallArgs {
    pub repo: PathBuf,
    pub force: bool,
    pub noa_bin: Option<String>,
}

/// Install all noa-managed git hooks (currently: `commit-msg` and `pre-commit`)
/// into the target repository's `.git/hooks/` directory.
pub fn run(args: InstallArgs) -> Result<()> {
    let repo = if args.repo.is_absolute() {
        args.repo.clone()
    } else {
        std::env::current_dir()?.join(&args.repo)
    };
    let dot_git = repo.join(".git");
    let hooks_dir = if dot_git.is_dir() {
        dot_git.join("hooks")
    } else {
        bail!(
            "not a git repository (no .git directory at {})",
            dot_git.display()
        );
    };
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating hooks dir {}", hooks_dir.display()))?;

    let noa_bin = args
        .noa_bin
        .clone()
        .or_else(detect_noa_bin)
        .unwrap_or_else(|| "noa".to_string());
    let resolved = if noa_bin.contains(' ') || std::path::Path::new(&noa_bin).exists() {
        noa_bin
    } else {
        which_noa(&noa_bin).unwrap_or(noa_bin)
    };

    install_hook(&hooks_dir, "commit-msg", COMMIT_MSG_HOOK, &resolved, args.force)?;
    install_hook(
        &hooks_dir,
        "pre-commit",
        PRE_COMMIT_HOOK,
        &resolved,
        args.force,
    )?;

    println!("Installed noa hooks into {}", hooks_dir.display());
    println!("Resolver: {resolved} co-author resolve");
    Ok(())
}

/// Write a single hook script into `hooks_dir/<name>`, substituting `@NOA_BIN@`
/// with the resolved noa binary path. Refuses to overwrite a non-noa-managed
/// hook unless `force` is set. Marks the file executable on Unix.
fn install_hook(
    hooks_dir: &Path,
    name: &str,
    template: &str,
    noa_bin: &str,
    force: bool,
) -> Result<()> {
    let hook_path = hooks_dir.join(name);
    if hook_path.exists() && !force {
        let existing = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if !existing.contains(NOA_MANAGED_SENTINEL) {
            bail!(
                "{name} hook already exists at {} and is not managed by noa. \
                 Re-run with --force to overwrite.",
                hook_path.display()
            );
        }
    }

    let content = template.replace("@NOA_BIN@", noa_bin);
    std::fs::write(&hook_path, content)
        .with_context(|| format!("writing hook {}", hook_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)?;
    }

    println!("Installed {name} hook at {}", hook_path.display());
    Ok(())
}

/// Entry point for `noa hook pre-commit`. Runs the pre-commit gate:
/// secret scan + (optionally) `cargo check`. Invoked by the installed
/// `.git/hooks/pre-commit` shell wrapper. Exits non-zero (via `Result`) on
/// any finding, which aborts the commit.
///
/// Staged commit data is acquired through [`precheck::commit_source`], which
/// tries the evernight IPC daemon (host-side git, for agent/container
/// contexts) and falls back to local git. If **no** source is reachable the
/// gate silently passes (exit 0): a data-source outage must never block a
/// commit. Real findings, once data is obtained, still abort.
pub fn run_pre_commit() -> Result<()> {
    if precheck::skip_requested() {
        eprintln!("[noa pre-commit] NOA_SKIP_HOOKS set; skipping checks.");
        return Ok(());
    }

    let entries = match precheck::commit_source::acquire_staged() {
        Acquisition::Ok(files) => files,
        Acquisition::NoSource => {
            tracing::debug!(
                "no commit-data source reachable (evernight down, no local git); \
                 silently passing pre-commit gate"
            );
            return Ok(());
        }
    };

    let hits = precheck::scan_entries(&entries);
    if !hits.is_empty() {
        for hit in &hits {
            eprintln!(
                "[noa pre-commit] potential {kind} in {path}:{line}",
                kind = hit.kind,
                path = hit.path,
                line = hit.line
            );
        }
        bail!(
            "secret scan found {} potential leak(s) in staged files; \
             aborting commit. Set NOA_SKIP_HOOKS=1 to bypass.",
            hits.len()
        );
    }

    if precheck::cargo_check_skip_requested() {
        tracing::debug!(
            "NOA_SKIP_CARGO_CHECK set; skipping cargo check gate (secret scan already ran)"
        );
    } else {
        precheck::run_cargo_check()?;
    }

    Ok(())
}

fn detect_noa_bin() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        return Some(exe.to_string_lossy().to_string());
    }
    None
}

fn which_noa(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}
