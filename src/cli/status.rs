use anyhow::Result;

use crate::repo::Repository;

pub fn run() -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;
    let head = repo.read_head()?;
    println!("On workspace: {}", head);
    Ok(())
}
