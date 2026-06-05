use anyhow::Result;
use clap::{Args, Subcommand};

use crate::repo::Repository;

#[derive(Subcommand)]
pub enum SnapshotCommands {
    Create {
        #[arg(short, long, default_value = "")]
        message: String,
        #[arg(short, long, default_value = "default")]
        author: String,
    },
    List,
    Diff {
        a: String,
        b: String,
    },
}

pub fn run(cmd: &SnapshotCommands) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    println!("snapshot command in {}", root.display());
    Ok(())
}
