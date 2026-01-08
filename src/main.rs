use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use iroh::{
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery, pkarr::PkarrPublisher},
    Endpoint, PublicKey,
};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::iroh_utils::init_secret_key;

mod iroh_utils;
mod store;

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
    /// Manage watched files
    Watch {
        /// The path to watch. If omitted, lists watched paths.
        path: Option<PathBuf>,
        /// Delete the watch for the specified path
        #[arg(short, long)]
        delete: bool,
    },
    /// Allow a peer to access a path
    Allow { peer: PublicKey, path: PathBuf },
    /// Disallow a peer from accessing a path
    Disallow { peer: PublicKey, path: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_secret_key().await?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env())
        .try_init()?;

    let cli = Cli::parse();

    // Initialize store
    let store = store::Store::new().context("Failed to initialize store")?;

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
        }
        Commands::Watch { path, delete } => {
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
        }
        Commands::Allow { peer, path } => {
            let abs_path = std::fs::canonicalize(&path).context("Failed to resolve path")?;
            store.allow_peer(&abs_path, peer)?;
            println!("Allowed peer {} for path {:?}", peer, abs_path);
        }
        Commands::Disallow { peer, path } => {
            let abs_path = std::fs::canonicalize(&path).context("Failed to resolve path")?;
            store.disallow_peer(&abs_path, peer)?;
            println!("Disallowed peer {} for path {:?}", peer, abs_path);
        }
    }

    Ok(())
}
