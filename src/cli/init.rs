use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::repo::Repository;

#[derive(Args)]
pub struct InitArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[arg(long)]
    pub noa_remote: Option<String>,

    #[arg(long)]
    pub no_git: bool,
}

pub fn run(args: &InitArgs) -> Result<()> {
    let path = args
        .path
        .canonicalize()
        .unwrap_or_else(|_| args.path.clone());

    if Repository::exists(&path) {
        anyhow::bail!(
            "repository already exists at {}",
            Repository::resolve_noa_dir(&path).display()
        );
    }

    let repo = match args.noa_remote.as_deref() {
        Some(url) => Repository::init_with_noa_remote(&path, Some(url))?,
        None => Repository::init(&path)?,
    };

    if !args.no_git {
        let git_dir = path.join(".git");
        if !git_dir.exists() {
            let output = std::process::Command::new("git")
                .args(["init"])
                .current_dir(&path)
                .output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git init failed: {stderr}");
            }
        }
    }

    println!(
        "Initialized empty noa{} repository in {}",
        if args.no_git { "" } else { "+git" },
        repo.noa_dir.display()
    );

    Ok(())
}
