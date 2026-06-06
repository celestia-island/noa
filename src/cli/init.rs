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
    pub git: bool,
}

pub fn run(args: &InitArgs) -> Result<()> {
    let path = args
        .path
        .canonicalize()
        .unwrap_or_else(|_| args.path.clone());

    if Repository::exists(&path) {
        anyhow::bail!(
            "repository already exists at {}",
            path.join(".noa").display()
        );
    }

    let repo = match args.noa_remote.as_deref() {
        Some(url) => Repository::init_with_noa_remote(&path, Some(url))?,
        None => Repository::init(&path)?,
    };

    if args.git {
        let git_dir = path.join(".git");
        if !git_dir.exists() {
            let output = std::process::Command::new("git")
                .args(["init"])
                .current_dir(&path)
                .output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git init failed: {}", stderr);
            }
        }

        let gitignore = path.join(".gitignore");
        let content = if gitignore.exists() {
            std::fs::read_to_string(&gitignore)?
        } else {
            String::new()
        };
        if !content
            .lines()
            .any(|l| l.trim() == ".noa/" || l.trim() == ".noa")
        {
            let updated = if content.is_empty() {
                ".noa/\n".to_string()
            } else {
                format!("{}\n.noa/\n", content.trim_end())
            };
            std::fs::write(&gitignore, updated)?;
        }

        println!(
            "Initialized empty noa+git repository in {}",
            repo.noa_dir.display()
        );
    } else {
        println!(
            "Initialized empty noa repository in {}",
            repo.noa_dir.display()
        );
    }

    Ok(())
}
