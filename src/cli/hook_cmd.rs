use std::path::PathBuf;

use anyhow::{Context, Result, bail};

const COMMIT_MSG_HOOK: &str = include_str!("../../assets/commit-msg.sh");

pub struct InstallArgs {
    pub repo: PathBuf,
    pub force: bool,
    pub noa_bin: Option<String>,
}

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

    let hook_path = hooks_dir.join("commit-msg");
    if hook_path.exists() && !args.force {
        let existing = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if !existing.contains("noa co-author resolve") {
            bail!(
                "commit-msg hook already exists at {} and is not managed by noa. \
                 Re-run with --force to overwrite.",
                hook_path.display()
            );
        }
    }

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

    let content = COMMIT_MSG_HOOK.replace("@NOA_BIN@", &resolved);
    std::fs::write(&hook_path, content)
        .with_context(|| format!("writing hook {}", hook_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)?;
    }

    println!("Installed commit-msg hook at {}", hook_path.display());
    println!("Resolver: {resolved} co-author resolve");
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
