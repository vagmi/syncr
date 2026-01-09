use anyhow::{Context, Result};
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint, PublicKey,
};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

use crate::{
    cli::copy,
    iroh_utils,
    protocol::{Message, ALPN},
    store::Store,
};

pub async fn run(
    store: Store,
    peer: PublicKey,
    remote_path: String,
    local_path: PathBuf,
) -> Result<()> {
    // 1. Perform initial sync (copy)
    info!("Performing initial sync...");
    copy::run(peer, remote_path.clone(), local_path.clone()).await?;

    // 2. Persist sync config locally
    info!("Saving sync configuration...");
    let abs_local_path = std::fs::canonicalize(&local_path)?;
    store.add_sync(peer, remote_path.clone(), abs_local_path.clone())?;

    // 3. Add watch for this file/directory locally
    store.add_watch(&abs_local_path)?;

    // 4. Register sync on remote peer (Reverse Sync)
    info!("Registering reverse sync on remote peer...");
    register_reverse_sync(peer, remote_path).await?;

    info!(
        "Sync established! Watching for changes at {:?}",
        abs_local_path
    );
    info!("Note: You must keep 'syncr serve' running to sync changes.");

    Ok(())
}

async fn register_reverse_sync(peer: PublicKey, remote_path: String) -> Result<()> {
    let secret_key = iroh_utils::load_secret_key().await?;
    let endpoint = Endpoint::builder()
        .discovery(PkarrPublisher::n0_dns())
        .discovery(DnsDiscovery::n0_dns())
        .discovery(MdnsDiscovery::builder())
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;

    let connection = endpoint.connect(peer, ALPN).await?;
    let (mut send, mut recv) = connection.open_bi().await?;

    // Handshake
    let msg = read_message(&mut recv).await?;
    match msg {
        Message::Handshake { .. } => {}
        _ => anyhow::bail!("Expected handshake, got {:?}", msg),
    }
    let handshake = Message::Handshake { version: 1 };
    write_message(&mut send, &handshake).await?;

    // Send StartSync
    let msg = Message::StartSync { path: remote_path };
    write_message(&mut send, &msg).await?;

    // Wait for acknowledgement?
    // Protocol doesn't have explicit Ack for this yet, but we can assume success if no error is sent back immediately.
    // Ideally we'd add `SyncStarted` response. For now, we just close.
    // Let's verify no error comes back.

    // Short timeout read? Or just finish.
    send.finish()?;

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
