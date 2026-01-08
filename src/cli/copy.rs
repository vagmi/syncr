use anyhow::{Context, Result};
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint, PublicKey,
};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    iroh_utils,
    protocol::{Message, ALPN},
};
use tracing::info;

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
    // Read Handshake from server first (server sends first in our serve.rs impl? No wait, usually client starts, let's check serve.rs)
    // serve.rs:
    let handshake = Message::Handshake { version: 1 };
    // Send Handshake
    write_message(&mut send, &handshake).await?;
    // Read Handshake
    let msg = read_message(&mut recv).await?;

    // So Server speaks first.

    // let msg = read_message(&mut recv).await?;
    match msg {
        Message::Handshake { version } => {
            info!("Handshake received from server: version {}", version);
        }
        _ => anyhow::bail!("Expected handshake, got {:?}", msg),
    }
    //
    // let handshake = Message::Handshake { version: 1 };
    // write_message(&mut send, &handshake).await?;

    // 2. Request File
    let req = Message::FileRequest {
        path: remote_path.clone(),
    };
    write_message(&mut send, &req).await?;

    // 3. Receive File Data
    let msg = read_message(&mut recv).await?;
    match msg {
        Message::FileData { path, data, .. } => {
            info!("Received file data for {}", path);
            tokio::fs::write(&local_path, data)
                .await
                .context("Failed to write local file")?;
            info!("File saved to {:?}", local_path);
        }
        Message::Error { message } => {
            anyhow::bail!("Remote error: {}", message);
        }
        _ => anyhow::bail!("Unexpected message: {:?}", msg),
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
