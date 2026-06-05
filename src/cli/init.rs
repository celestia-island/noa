use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::repo::Repository;

#[derive(Args)]
pub struct InitArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

pub fn run(args: &InitArgs) -> Result<()> {
    let path = args.path.canonicalize().unwrap_or_else(|_| args.path.clone());

    if Repository::exists(&path) {
        anyhow::bail!(
            "repository already exists at {}",
            path.join(".noa").display()
        );
    }

    let repo = Repository::init(&path)?;

    println!(
        "Initialized empty noa repository in {}",
        repo.noa_dir.display()
    );

    Ok(())
}
