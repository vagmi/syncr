use anyhow::{Context, Result};
use iroh::{Endpoint, PublicKey};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::{
    protocol::{Message, ALPN},
    store::Store,
    watcher::FileWatcher,
};

/// Manages active syncs, watches, and peer communication
pub struct SyncManager {
    store: Store,
    endpoint: Endpoint,
    watcher: Arc<Mutex<FileWatcher>>,
}

impl SyncManager {
    pub fn new(store: Store, endpoint: Endpoint, watcher: FileWatcher) -> Self {
        Self {
            store,
            endpoint,
            watcher: Arc::new(Mutex::new(watcher)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        let mut watcher = self.watcher.lock().await;

        // Load existing watches
        let watched_paths = self.store.list_watches()?;
        for path in &watched_paths {
            if path.exists() {
                info!("Watching path: {:?}", path);
                watcher.watch(path)?;
            } else {
                warn!("Watched path does not exist: {:?}", path);
            }
        }
        drop(watcher); // Unlock

        let watcher_clone = self.watcher.clone();
        let store_clone = self.store.clone();
        let endpoint_clone = self.endpoint.clone();

        // Spawn the watcher event loop
        tokio::spawn(async move {
            loop {
                let mut w = watcher_clone.lock().await;
                let event = w.next_event().await;
                drop(w); // Unlock during processing

                if let Some(res) = event {
                    match res {
                        Ok(path) => {
                            info!("File changed locally: {:?}", path);
                            if let Err(e) =
                                Self::handle_local_change(&store_clone, &endpoint_clone, path).await
                            {
                                error!("Failed to handle local change: {:?}", e);
                            }
                        }
                        Err(e) => error!("Watcher error: {}", e),
                    }
                } else {
                    break;
                }
            }
        });

        Ok(())
    }

    async fn handle_local_change(store: &Store, endpoint: &Endpoint, path: PathBuf) -> Result<()> {
        let syncs = store.list_syncs()?;
        for (local_root, configs) in syncs {
            // Check if 'path' is inside 'local_root'
            if path.starts_with(&local_root) {
                // Calculate relative path
                let relative_path = path.strip_prefix(&local_root)?.to_string_lossy();

                for config in configs {
                    // Construct remote path
                    // If local_root was "/tmp/a.txt" and path is "/tmp/a.txt", relative is "".
                    // remote_path should be config.remote_path.

                    // If local_root was "/tmp/dir" and path is "/tmp/dir/file.txt", relative is "file.txt".
                    // remote_path should be config.remote_path + "/" + relative.

                    let target_remote_path = if relative_path.is_empty() {
                        config.remote_path.clone()
                    } else {
                        // naive path join, assuming unix style forward slashes for wire protocol
                        if config.remote_path.ends_with('/') {
                            format!("{}{}", config.remote_path, relative_path)
                        } else {
                            format!("{}/{}", config.remote_path, relative_path)
                        }
                    };

                    info!(
                        "Notifying peer {} about update to {}",
                        config.peer, target_remote_path
                    );
                    if let Err(e) =
                        Self::notify_peer(endpoint, config.peer, target_remote_path).await
                    {
                        error!("Failed to notify peer {}: {}", config.peer, e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn notify_peer(endpoint: &Endpoint, peer: PublicKey, remote_path: String) -> Result<()> {
        // Connect to the peer
        // TODO: Reuse connections if possible
        let connection = endpoint
            .connect(peer, ALPN)
            .await
            .context("Failed to connect to peer")?;

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .context("Failed to open stream")?;

        // 1. Handshake
        // Server speaks first (see serve.rs)
        let msg = read_message(&mut recv).await?;
        match msg {
            Message::Handshake { .. } => {}
            _ => anyhow::bail!("Expected handshake from server"),
        }

        let handshake = Message::Handshake { version: 1 };
        write_message(&mut send, &handshake).await?;

        // 2. Send Notification
        let msg = Message::FileUpdateNotification { path: remote_path };
        write_message(&mut send, &msg).await?;

        // We don't expect a response to notification immediately?
        // Or maybe we should wait for ack?
        // For now, fire and close.
        send.finish()?;

        Ok(())
    }
}

async fn write_message<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &Message) -> Result<()> {
    let data = postcard::to_stdvec(msg)?;
    let len = data.len() as u32;
    writer.write_u32(len).await?;
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Message> {
    let len = reader.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    let msg = postcard::from_bytes(&buf)?;
    Ok(msg)
}
