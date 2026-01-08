use crate::store::Store;
use anyhow::{Context, Result};
use iroh::PublicKey;
use std::path::PathBuf;

pub fn run_allow(store: &Store, peer: PublicKey, path: PathBuf) -> Result<()> {
    let abs_path = std::fs::canonicalize(&path).context("Failed to resolve path")?;
    store.allow_peer(&abs_path, peer)?;
    println!("Allowed peer {} for path {:?}", peer, abs_path);
    Ok(())
}

pub fn run_disallow(store: &Store, peer: PublicKey, path: PathBuf) -> Result<()> {
    let abs_path = std::fs::canonicalize(&path).context("Failed to resolve path")?;
    store.disallow_peer(&abs_path, peer)?;
    println!("Disallowed peer {} for path {:?}", peer, abs_path);
    Ok(())
}
