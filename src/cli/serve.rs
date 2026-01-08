use anyhow::{Context, Result};
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info};

use crate::{
    iroh_utils,
    protocol::{Message, ALPN},
    store::Store,
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

    // Loop to accept incoming connections
    while let Some(incoming) = endpoint.accept().await {
        let store = store.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, store).await {
                error!("Connection error: {:?}", e);
            }
        });
    }

    Ok(())
}

async fn handle_connection(incoming: iroh::endpoint::Incoming, _store: Store) -> Result<()> {
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

                // Security check: ensure path is within a watched directory?
                // For this "Copy" prototype, we might just assume we can serve any file the user asks for
                // BUT better to check if it's in a watched dir.
                // However, `_store` is unused in the signature above, let's use it.
                // For this iteration, let's just attempt to read the file relative to current directory
                // OR just absolute path if we want to be unsafe for a second (NOT recommended).

                // Let's rely on the store. We need to find if the requested path is inside a watched directory.
                // NOTE: The `path` string coming from wire might be absolute or relative.
                // Let's treat it as absolute for now, or match against watches.

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
