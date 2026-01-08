use iroh::PublicKey;
use sled::{Db, Tree};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Database error: {0}")]
    DbError(#[from] sled::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] postcard::Error),
    #[error("System error: {0}")]
    SystemError(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone)]
pub struct Store {
    #[allow(dead_code)]
    db: Db,
    watches: Tree,
    permissions: Tree,
}

impl Store {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| StoreError::SystemError("Could not find config directory".into()))?
            .join("syncr");

        std::fs::create_dir_all(&config_dir).map_err(|e| StoreError::SystemError(e.to_string()))?;

        let db_path = config_dir.join("db");
        let db = sled::open(db_path)?;

        let watches = db.open_tree("watches")?;
        let permissions = db.open_tree("permissions")?;

        Ok(Self {
            db,
            watches,
            permissions,
        })
    }

    pub fn add_watch<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        // Normalize path? For now just store absolute path string
        let path_str = path.to_string_lossy();
        self.watches.insert(path_str.as_bytes(), &[])?;
        Ok(())
    }

    pub fn remove_watch<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy();
        let old = self.watches.remove(path_str.as_bytes())?;
        Ok(old.is_some())
    }

    pub fn list_watches(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for item in self.watches.iter() {
            let (key, _) = item?;
            let path_str = String::from_utf8(key.to_vec())
                .map_err(|e| StoreError::SystemError(format!("Invalid path encoding: {}", e)))?;
            paths.push(PathBuf::from(path_str));
        }
        Ok(paths)
    }

    pub fn allow_peer<P: AsRef<Path>>(&self, path: P, peer: PublicKey) -> Result<()> {
        let path = path.as_ref();
        let path_key = path.to_string_lossy().as_bytes().to_vec();

        // Load existing permissions
        let mut allowed: Vec<PublicKey> = match self.permissions.get(&path_key)? {
            Some(bytes) => postcard::from_bytes(&bytes)?,
            None => Vec::new(),
        };

        if !allowed.contains(&peer) {
            allowed.push(peer);
            let bytes = postcard::to_stdvec(&allowed)?;
            self.permissions.insert(path_key, bytes)?;
        }

        Ok(())
    }

    pub fn disallow_peer<P: AsRef<Path>>(&self, path: P, peer: PublicKey) -> Result<()> {
        let path = path.as_ref();
        let path_key = path.to_string_lossy().as_bytes().to_vec();

        let mut allowed: Vec<PublicKey> = match self.permissions.get(&path_key)? {
            Some(bytes) => postcard::from_bytes(&bytes)?,
            None => return Ok(()),
        };

        if let Some(pos) = allowed.iter().position(|x| *x == peer) {
            allowed.remove(pos);
            let bytes = postcard::to_stdvec(&allowed)?;
            self.permissions.insert(path_key, bytes)?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_permissions<P: AsRef<Path>>(&self, path: P) -> Result<Vec<PublicKey>> {
        let path = path.as_ref();
        let path_key = path.to_string_lossy().as_bytes().to_vec();

        match self.permissions.get(&path_key)? {
            Some(bytes) => Ok(postcard::from_bytes(&bytes)?),
            None => Ok(Vec::new()),
        }
    }
}
