use anyhow::Result;
use clap::{Parser, Subcommand};
use iroh::{Endpoint, discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher}};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::iroh_utils::init_secret_key;

mod iroh_utils;
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Get peer id and version info
    Info,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_secret_key().await?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env())
        .try_init()?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Info => {
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

            // Wait a bit or shutdown? Endpoint doesn't have explicit shutdown that blocks usually,
            // but for a CLI command we just exit.
        }
    }

    Ok(())
}
