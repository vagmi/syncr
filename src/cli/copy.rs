use anyhow::{Context, Result};
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint, PublicKey,
};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

use crate::{
    iroh_utils,
    protocol::{FileMetadata, Message, ALPN},
    sync_utils,
};

pub async fn run(peer: PublicKey, remote_path: String, local_path: PathBuf) -> Result<()> {
    let secret_key = iroh_utils::load_secret_key().await?;
    let endpoint = Endpoint::builder()
        .discovery(PkarrPublisher::n0_dns())
        .discovery(DnsDiscovery::n0_dns())
        .discovery(MdnsDiscovery::builder())
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;

    info!("Connecting to {}...", peer);

    // Connect to the peer
    let connection = endpoint.connect(peer, ALPN).await?;
    info!("Connected!");

    // Open a bi-directional stream
    let (mut send, mut recv) = connection.open_bi().await?;

    // 1. Handshake
    let handshake = Message::Handshake { version: 1 };
    write_message(&mut send, &handshake).await?;

    let msg = read_message(&mut recv).await?;
    match msg {
        Message::Handshake { version } => {
            info!("Handshake received from server: version {}", version);
        }
        _ => anyhow::bail!("Expected handshake, got {:?}", msg),
    }

    // Determine if we need directory list or single file
    // Strategy: Request listing for path. If it's a file, we get 1 entry. If dir, many.
    // If it fails (path not found), we error.

    info!("Requesting file listing for {}", remote_path);
    let list_req = Message::ListRequest {
        path: remote_path.clone(),
    };
    write_message(&mut send, &list_req).await?;

    let msg = read_message(&mut recv).await?;
    let files: Vec<FileMetadata> = match msg {
        Message::ListResponse { files } => files,
        Message::Error { message } => anyhow::bail!("Remote error: {}", message),
        _ => anyhow::bail!("Unexpected message: {:?}", msg),
    };

    info!("Received listing with {} files", files.len());

    // Create local root dir if needed (and if multiple files or target implies dir)
    // If local path doesn't exist, mkdir -p
    if !local_path.exists() {
        // If we have >1 file or the single file has a different name than local_path tail?
        // Usually, if we sync /remote/foo to /local/bar, we treat /local/bar as the target.
        // If /remote/foo is a dir, /local/bar becomes that dir.
        // If /remote/foo is a file, /local/bar becomes that file.

        // Check first item to see if it's a dir?
        // Metadata has is_dir.
        // But listing might be flat list of files.
        // Let's iterate.
        if files.is_empty() {
            info!("Remote path is empty or invalid.");
            return Ok(());
        }
    }

    // We need to determine the base relative path to strip.
    // remote_path: /remote/dir
    // file path: /remote/dir/file.txt
    // relative: file.txt
    // local path: /local/dir
    // target: /local/dir/file.txt

    let remote_base = std::path::Path::new(&remote_path);

    for file in files {
        if file.is_dir {
            // Ensure dir exists locally
            let relative = std::path::Path::new(&file.path)
                .strip_prefix(remote_base)
                .unwrap_or(std::path::Path::new(""));
            if relative.as_os_str().is_empty() {
                // It's the root dir itself
                std::fs::create_dir_all(&local_path)?;
            } else {
                let target = local_path.join(relative);
                std::fs::create_dir_all(&target)?;
            }
            continue;
        }

        // It's a file
        let relative = std::path::Path::new(&file.path)
            .strip_prefix(remote_base)
            .unwrap_or(std::path::Path::new(""));

        // If relative is empty, it means remote_path pointed directly to this file.
        // So target is local_path.
        let target_path = if relative.as_os_str().is_empty() {
            local_path.clone()
        } else {
            // Ensure parent dir exists
            let target = local_path.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            target
        };

        // Sync the file
        sync_file(&mut send, &mut recv, &file.path, &target_path).await?;
    }

    Ok(())
}

async fn sync_file(
    send: &mut iroh::endpoint::SendStream,
    recv: &mut iroh::endpoint::RecvStream,
    remote_file_path: &str,
    local_target_path: &PathBuf,
) -> Result<()> {
    info!("Syncing {} -> {:?}", remote_file_path, local_target_path);

    if local_target_path.exists() && local_target_path.is_file() {
        info!("Local file exists, attempting rsync delta transfer...");
        let local_data = tokio::fs::read(local_target_path).await?;
        let signature = sync_utils::calculate_signature(&local_data)?;

        let req = Message::FileSignature {
            path: remote_file_path.to_string(),
            signature,
        };
        write_message(send, &req).await?;

        let msg = read_message(recv).await?;
        match msg {
            Message::FileDelta { path: _, delta } => {
                info!("Received delta ({} bytes)", delta.len());
                let new_data = sync_utils::apply_delta(&local_data, &delta)?;
                tokio::fs::write(local_target_path, new_data).await?;
                info!("File patched and saved.");
            }
            Message::Error { message } => {
                anyhow::bail!("Remote error: {}", message);
            }
            _ => anyhow::bail!("Unexpected message during sync_file: {:?}", msg),
        }
    } else {
        info!("Local file not found, requesting full download...");
        // 3. Request Full File
        let req = Message::FileRequest {
            path: remote_file_path.to_string(),
        };
        write_message(send, &req).await?;

        // 4. Receive File Data
        let msg = read_message(recv).await?;
        match msg {
            Message::FileData { path: _, data, .. } => {
                info!("Received file data ({} bytes)", data.len());
                tokio::fs::write(local_target_path, data)
                    .await
                    .context("Failed to write local file")?;
                info!("File saved.");
            }
            Message::Error { message } => {
                anyhow::bail!("Remote error: {}", message);
            }
            _ => anyhow::bail!("Unexpected message during sync_file: {:?}", msg),
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
