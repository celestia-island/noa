use anyhow::Result;
use clap::Args;

use crate::repo::Repository;

#[derive(Args)]
pub struct LogArgs {
    #[arg(short, long)]
    pub workspace: Option<String>,
    #[arg(short, long, default_value = "20")]
    pub limit: usize,
}

pub fn run(args: &LogArgs) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    println!("log command in {}", root.display());
    Ok(())
}
