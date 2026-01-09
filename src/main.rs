use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::iroh_utils::init_secret_key;

mod cli;
mod iroh_utils;
mod protocol;
pub mod store;
mod sync_manager;
pub mod sync_utils;
mod watcher;

#[tokio::main]
async fn main() -> Result<()> {
    init_secret_key().await?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env())
        .try_init()?;

    let cli = cli::Cli::parse();

    // Initialize store
    let store = store::Store::new().context("Failed to initialize store")?;

    cli.run(store).await
}
