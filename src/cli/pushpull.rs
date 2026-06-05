use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PushArgs {
    #[arg(short, long, default_value = "origin")]
    pub remote: String,
}

#[derive(Args)]
pub struct PullArgs {
    #[arg(short, long, default_value = "origin")]
    pub remote: String,
}

#[derive(Args)]
pub struct FetchArgs {
    #[arg(short, long, default_value = "origin")]
    pub remote: String,
}

#[derive(Args)]
pub struct CloneArgs {
    pub url: String,
    #[arg(short, long, default_value = ".")]
    pub path: String,
}

pub fn push(args: &PushArgs) -> Result<()> {
    println!("pushing to {}", args.remote);
    Ok(())
}

pub fn pull(args: &PullArgs) -> Result<()> {
    println!("pulling from {}", args.remote);
    Ok(())
}

pub fn fetch(args: &FetchArgs) -> Result<()> {
    println!("fetching from {}", args.remote);
    Ok(())
}

pub fn clone(args: &CloneArgs) -> Result<()> {
    println!("cloning {} into {}", args.url, args.path);
    Ok(())
}
