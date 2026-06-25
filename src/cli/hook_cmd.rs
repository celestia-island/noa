use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const COMMIT_MSG_HOOK: &str = include_str!("../../assets/commit-msg.sh");

/// Sentinel string present in every noa-managed hook script.
/// Used to detect whether an existing hook file is safe to overwrite.
const NOA_MANAGED_SENTINEL: &str = "noa-managed";

pub struct InstallArgs {
    pub repo: PathBuf,
    pub force: bool,
    pub noa_bin: Option<String>,
}

/// Install noa-managed git hooks into the target repository's `.git/hooks/`
/// directory. Currently this is just `commit-msg` (AI co-author trailers).
///
/// Pre-commit build/secret validation used to live here but has moved to
/// entelecheia's review agent — it is not noa's responsibility. The
/// `noa hook pre-commit` subcommand is retained as a no-op so hooks installed
/// by older noa versions don't break (they just do nothing).
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

    install_hook(
        &hooks_dir,
        "commit-msg",
        COMMIT_MSG_HOOK,
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

/// Entry point for `noa hook pre-commit`.
///
/// Pre-commit build/secret validation has moved to entelecheia's review agent
/// and is no longer noa's responsibility. This subcommand is retained as a
/// **no-op** so `.git/hooks/pre-commit` scripts installed by older noa versions
/// keep working (they call into here, do nothing, and exit 0) instead of
/// breaking commits with an unknown-subcommand error. It is no longer
/// installed by `noa hook install`.
pub fn run_pre_commit() -> Result<()> {
    eprintln!(
        "[noa pre-commit] pre-commit checks have moved to the entelecheia review \
         agent; this hook is now a no-op and can be removed from .git/hooks."
    );
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
