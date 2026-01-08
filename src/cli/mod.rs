use anyhow::Result;
use clap::{Parser, Subcommand};
use iroh::PublicKey;
use std::path::PathBuf;

use crate::store::Store;

mod allow;
mod info;
mod watch;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
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

impl Cli {
    pub async fn run(self, store: Store) -> Result<()> {
        match self.command {
            Commands::Info => info::run().await?,
            Commands::Watch { path, delete } => watch::run(&store, path, delete)?,
            Commands::Allow { peer, path } => allow::run_allow(&store, peer, path)?,
            Commands::Disallow { peer, path } => allow::run_disallow(&store, peer, path)?,
        }
        Ok(())
    }
}
