use anyhow::Result;

use crate::{config::RemoteConfig, repo::Repository};

pub async fn run_add(repo: &mut Repository, name: &str, url: &str) -> Result<()> {
    repo.config.add_remote(RemoteConfig {
        name: name.to_string(),
        url: url.to_string(),
        protocol: "git".to_string(),
    });
    repo.save_config()?;
    println!("Added remote '{}' -> {}", name, url);
    Ok(())
}

pub async fn run_remove(repo: &mut Repository, name: &str) -> Result<()> {
    repo.config.remove_remote(name);
    repo.save_config()?;
    println!("Removed remote '{}'", name);
    Ok(())
}

pub async fn run_list(repo: &Repository) -> Result<()> {
    if repo.config.remotes.is_empty() {
        println!("No remotes configured.");
        return Ok(());
    }

    for remote in &repo.config.remotes {
        println!("{}\t{} ({})", remote.name, remote.url, remote.protocol);
    }
    Ok(())
}
