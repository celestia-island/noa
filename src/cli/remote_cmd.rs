use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum RemoteCommands {
    Add {
        name: String,
        url: String,
    },
    Remove {
        name: String,
    },
    List,
}

pub fn run(cmd: &RemoteCommands) -> Result<()> {
    match cmd {
        RemoteCommands::Add { name, url } => {
            println!("Adding remote '{}' -> {}", name, url);
        }
        RemoteCommands::Remove { name } => {
            println!("Removing remote '{}'", name);
        }
        RemoteCommands::List => {
            println!("Listing remotes");
        }
    }
    Ok(())
}
