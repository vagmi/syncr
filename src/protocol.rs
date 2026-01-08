use serde::{Deserialize, Serialize};

pub const ALPN: &[u8] = b"syncr/1";

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Handshake {
        version: u32,
    },
    /// Request to open a path for syncing
    OpenPath {
        path: String,
    },
    /// Response to OpenPath
    PathOpened,
    /// Request file list
    ListRequest,
    /// File list response
    ListResponse {
        files: Vec<FileMetadata>,
    },
    /// Send file signature (from Receiver to Sender) to request delta
    FileSignature {
        path: String,
        signature: Vec<u8>,
    },
    /// Send file delta (from Sender to Receiver)
    FileDelta {
        path: String,
        delta: Vec<u8>,
    },
    /// Request full file (if no local copy)
    FileRequest {
        path: String,
    },
    /// Send full file data (simple chunking or full blob for now)
    FileData {
        path: String,
        data: Vec<u8>,
        offset: u64,
        is_last: bool,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub len: u64,
    pub modified: u64, // Unix timestamp
    pub is_dir: bool,
}
