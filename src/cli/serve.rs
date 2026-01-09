use anyhow::{Context, Result};
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint, PublicKey,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info, warn};

use crate::{
    iroh_utils,
    protocol::{Message, ALPN},
    store::Store,
    sync_manager::SyncManager,
    sync_utils,
    watcher::FileWatcher,
};

pub async fn run(store: Store) -> Result<()> {
    let secret_key = iroh_utils::load_secret_key().await?;
    let endpoint = Endpoint::builder()
        .discovery(PkarrPublisher::n0_dns())
        .discovery(DnsDiscovery::n0_dns())
        .discovery(MdnsDiscovery::builder())
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;

    info!("Listening on Peer ID: {}", endpoint.id());

    // Initialize watcher
    let watcher = FileWatcher::new()?;

    // Initialize SyncManager
    let sync_manager = SyncManager::new(store.clone(), endpoint.clone(), watcher);
    sync_manager.run().await?; // Starts watcher loop

    // Loop to accept incoming connections
    while let Some(incoming) = endpoint.accept().await {
        let store = store.clone();
        let endpoint_clone = endpoint.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, store, endpoint_clone).await {
                error!("Connection error: {:?}", e);
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    incoming: iroh::endpoint::Incoming,
    store: Store,
    endpoint: Endpoint,
) -> Result<()> {
    let connection = incoming.accept()?;
    let connection = connection.await?;
    let remote_id = connection.remote_id();
    info!("Accepted connection from {}", remote_id);

    // Accept a bi-directional stream for control messages
    let (mut send, mut recv) = connection.accept_bi().await?;
    info!("Bi-directional stream established with {}", remote_id);

    // Send Handshake
    let handshake = Message::Handshake { version: 1 };
    write_message(&mut send, &handshake).await?;

    // Read Handshake
    let msg = read_message(&mut recv).await?;
    match msg {
        Message::Handshake { version } => {
            info!("Handshake received from {}: version {}", remote_id, version);
        }
        _ => {
            anyhow::bail!("Expected handshake, got {:?}", msg);
        }
    }

    // Loop to handle requests
    loop {
        // Read next message (might be EOF if client closes)
        let msg = match read_message(&mut recv).await {
            Ok(m) => m,
            Err(_) => break, // Assume disconnection or error means stop
        };

        match msg {
            Message::FileRequest { path } => {
                info!("Client {} requested file: {}", remote_id, path);

                let path_buf = std::path::PathBuf::from(&path);
                if path_buf.exists() && path_buf.is_file() {
                    let data = tokio::fs::read(&path_buf).await?;
                    let resp = Message::FileData {
                        path: path.clone(),
                        data,
                        offset: 0,
                        is_last: true,
                    };
                    write_message(&mut send, &resp).await?;
                } else {
                    let err = Message::Error {
                        message: format!("File not found: {}", path),
                    };
                    write_message(&mut send, &err).await?;
                }
            }
            Message::FileSignature { path, signature } => {
                info!("Client {} sent signature for: {}", remote_id, path);

                let path_buf = std::path::PathBuf::from(&path);
                if path_buf.exists() && path_buf.is_file() {
                    let data = tokio::fs::read(&path_buf).await?;

                    // Calculate delta
                    match sync_utils::calculate_delta(&signature, &data) {
                        Ok(delta) => {
                            info!("Calculated delta size: {} bytes", delta.len());
                            let resp = Message::FileDelta {
                                path: path.clone(),
                                delta,
                            };
                            write_message(&mut send, &resp).await?;
                        }
                        Err(e) => {
                            let err = Message::Error {
                                message: format!("Delta calculation failed: {}", e),
                            };
                            write_message(&mut send, &err).await?;
                        }
                    }
                } else {
                    let err = Message::Error {
                        message: format!("File not found: {}", path),
                    };
                    write_message(&mut send, &err).await?;
                }
            }
            Message::FileUpdateNotification { path } => {
                info!("Peer {} notified update for: {}", remote_id, path);

                // Trigger Pull (Sync)
                // We need to find where this file maps to locally.
                // This requires a reverse lookup: RemotePath + Peer -> LocalPath.
                // Currently `store` doesn't support efficient reverse lookup, we have to scan.

                // TODO: Optimize this
                // For now, iterate all syncs
                if let Ok(syncs) = store.list_syncs() {
                    for (local_root, configs) in syncs {
                        for config in configs {
                            if config.peer == remote_id {
                                // Check if this notification matches the configured remote path
                                // Case 1: Notification path == Config remote path (Exact file match)
                                // Case 2: Notification path is inside Config remote path (Directory sync)

                                // Simplified logic for exact match first
                                if path == config.remote_path {
                                    info!(
                                        "Found matching sync config. Syncing to {:?}",
                                        local_root
                                    );

                                    // Spawn a task to perform the pull to avoid blocking the server loop
                                    let endpoint_clone = endpoint.clone();
                                    let remote_id_clone = remote_id;
                                    let path_clone = path.clone();
                                    let local_root_clone = local_root.clone();

                                    tokio::spawn(async move {
                                        if let Err(e) = crate::cli::copy::run(
                                            remote_id_clone,
                                            path_clone,
                                            local_root_clone,
                                        )
                                        .await
                                        {
                                            error!("Failed to sync update: {:?}", e);
                                        }
                                    });
                                } else if path.starts_with(&config.remote_path) {
                                    // Directory match
                                    // We need to map the subpath
                                    // e.g. Config Remote: /remote/dir -> Local: /local/dir
                                    // Update: /remote/dir/subdir/file.txt
                                    // Relative: subdir/file.txt
                                    // Target: /local/dir/subdir/file.txt

                                    if let Ok(relative) = std::path::Path::new(&path)
                                        .strip_prefix(&config.remote_path)
                                    {
                                        let target_local = local_root.join(relative);
                                        info!(
                                            "Found matching dir sync. Syncing to {:?}",
                                            target_local
                                        );

                                        let endpoint_clone = endpoint.clone();
                                        let remote_id_clone = remote_id;
                                        let path_clone = path.clone();

                                        tokio::spawn(async move {
                                            if let Err(e) = crate::cli::copy::run(
                                                remote_id_clone,
                                                path_clone,
                                                target_local,
                                            )
                                            .await
                                            {
                                                error!("Failed to sync update: {:?}", e);
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Message::StartSync { path } => {
                info!("Peer {} requesting to sync path: {}", remote_id, path);

                // 1. Check if allowed
                // We check if this path is in the `permissions` list for this peer.
                // Store permissions are keyed by path.
                // NOTE: 'path' here is the path on THIS machine (Server).
                // Protocol assumes Client requests known path.

                let path_buf = std::path::PathBuf::from(&path);
                // We need to handle relative/absolute path resolution correctly relative to what was allowed.
                // If I allowed `/tmp/foo`, and client asks for `/tmp/foo`, it matches.
                // If client asks for `foo` (relative), we might fail if we stored absolute.
                // Let's try canonicalizing if possible, or just exact string match against allowed.

                // Best effort resolution
                let abs_path = if path_buf.is_absolute() {
                    path_buf.clone()
                } else {
                    std::fs::canonicalize(&path_buf).unwrap_or(path_buf.clone())
                };

                let allowed_peers = store.get_permissions(&abs_path)?;
                if allowed_peers.contains(&remote_id) {
                    info!("Access granted. Registering reverse sync config.");

                    // 2. Add Sync Config
                    // peer: remote_id
                    // remote_path: path (This is tricky. We are registering that WE want to notify Remote about 'path'.
                    // So 'remote_path' in SyncConfig effectively becomes the identifier we send in FileUpdateNotification.
                    // If we use 'path', we notify Remote about 'path'. Remote must have mapped 'path' to its local.
                    // This matches the current logic.)

                    store.add_sync(remote_id, path.clone(), abs_path.clone())?;

                    // 3. Add Watch
                    store.add_watch(&abs_path)?;

                    // TODO: Send success response?
                } else {
                    warn!("Access denied for peer {} on path {}", remote_id, path);
                    let err = Message::Error {
                        message: "Access denied or path not allowed".to_string(),
                    };
                    write_message(&mut send, &err).await?;
                }
            }
            _ => {
                info!("Received unexpected message: {:?}", msg);
            }
        }
    }

    Ok(())
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
