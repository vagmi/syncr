use anyhow::{Context, Result};
use fast_rsync::{Signature, SignatureOptions};

// Constants for rsync
const BLOCK_SIZE: u32 = 1024; // 1KB blocks
const CRYPTO_HASH_SIZE: u32 = 8; // 8 bytes for strong hash

pub fn calculate_signature(data: &[u8]) -> Result<Vec<u8>> {
    let options = SignatureOptions {
        block_size: BLOCK_SIZE,
        crypto_hash_size: CRYPTO_HASH_SIZE,
    };
    let signature = Signature::calculate(data, options);
    Ok(signature.serialized().to_vec())
}

pub fn calculate_delta(signature_data: &[u8], new_data: &[u8]) -> Result<Vec<u8>> {
    let signature = Signature::deserialize(signature_data.to_vec())
        .map_err(|e| anyhow::anyhow!("Invalid signature: {:?}", e))?;

    let mut delta = Vec::new();
    fast_rsync::diff(&signature.index(), new_data, &mut delta)
        .map_err(|e| anyhow::anyhow!("Failed to calculate delta: {:?}", e))?;
    Ok(delta)
}

pub fn apply_delta(old_data: &[u8], delta: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    fast_rsync::apply(old_data, delta, &mut out)
        .map_err(|e| anyhow::anyhow!("Failed to apply delta: {:?}", e))?;
    Ok(out)
}
