use std::path::Path;

use crate::error::Result;

pub async fn export_noa_to_git(
    _db_path: &Path,
    _git_dir: &Path,
) -> Result<()> {
    todo!("noa → git export via gix::remote push")
}

pub async fn clone_git_to_noa(
    _url: &str,
    _target: &Path,
) -> Result<()> {
    todo!("git clone via gix::prepare_clone, then import")
}
