use anyhow::Result;
use clap::{Parser, Subcommand};
use iroh::PublicKey;
use std::path::PathBuf;

use crate::store::Store;

mod allow;
pub mod copy; // Make public for sync to use
mod info;
pub mod serve;
mod sync;
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
    /// Run the syncr daemon/server to accept connections
    Serve,
    /// Copy a file from a remote peer
    Copy {
        /// The peer to copy from
        peer: PublicKey,
        /// The remote path to copy
        remote_path: String,
        /// The local destination path
        local_path: PathBuf,
    },
    /// Sync a file/folder with a remote peer
    Sync {
        /// The peer to sync with
        peer: PublicKey,
        /// The remote path to sync
        remote_path: String,
        /// The local destination path
        local_path: PathBuf,
    },
}

impl Cli {
    pub async fn run(self, store: Store) -> Result<()> {
        match self.command {
            Commands::Info => info::run().await?,
            Commands::Watch { path, delete } => watch::run(&store, path, delete)?,
            Commands::Allow { peer, path } => allow::run_allow(&store, peer, path)?,
            Commands::Disallow { peer, path } => allow::run_disallow(&store, peer, path)?,
            Commands::Serve => serve::run(store).await?,
            Commands::Copy {
                peer,
                remote_path,
                local_path,
            } => copy::run(peer, remote_path, local_path).await?,
            Commands::Sync {
                peer,
                remote_path,
                local_path,
            } => sync::run(store, peer, remote_path, local_path).await?,
        }
        Ok(())
    }
}
