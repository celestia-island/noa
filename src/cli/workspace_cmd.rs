use anyhow::Result;
use clap::{Args, Subcommand};

use crate::repo::Repository;

#[derive(Subcommand)]
pub enum WorkspaceCommands {
    Create {
        name: String,
        #[arg(short, long)]
        agent: Option<String>,
    },
    Switch {
        name: String,
    },
    List,
    Delete {
        name: String,
    },
    Merge {
        from: String,
    },
}

pub fn run(cmd: &WorkspaceCommands) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    println!("workspace command in {}", root.display());
    Ok(())
}
