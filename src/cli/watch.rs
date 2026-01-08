use crate::store::Store;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn run(store: &Store, path: Option<PathBuf>, delete: bool) -> Result<()> {
    if let Some(p) = path {
        let abs_path = std::fs::canonicalize(&p).context("Failed to resolve path")?;
        if delete {
            if store.remove_watch(&abs_path)? {
                println!("Removed watch: {:?}", abs_path);
            } else {
                println!("Path was not being watched: {:?}", abs_path);
            }
        } else {
            store.add_watch(&abs_path)?;
            println!("Added watch: {:?}", abs_path);
        }
    } else {
        let watches = store.list_watches()?;
        if watches.is_empty() {
            println!("No paths are being watched.");
        } else {
            for w in watches {
                println!("{}", w.display());
            }
        }
    }
    Ok(())
}
