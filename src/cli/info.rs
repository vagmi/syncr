use anyhow::Result;
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint,
};

use crate::iroh_utils;

pub async fn run() -> Result<()> {
    let secret_key = iroh_utils::load_secret_key().await?;
    // Create an endpoint with a random secret key and default configuration
    let endpoint = Endpoint::builder()
        .discovery(PkarrPublisher::n0_dns())
        .discovery(DnsDiscovery::n0_dns())
        .discovery(MdnsDiscovery::builder())
        .secret_key(secret_key)
        .bind()
        .await?;

    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!("Peer ID: {}", endpoint.id());

    Ok(())
}
