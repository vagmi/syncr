use iroh::SecretKey;
use tokio::fs;


#[derive(Debug, thiserror::Error)]
pub enum IrohUtilsError {
    #[error("Failed to generate secret key: {0}")]
    SecretKeyGenerationError(String),
    #[error("Failed to load secret key {0}")]
    SecretKeyLoadError(String),
}

pub type Result<T> = std::result::Result<T, IrohUtilsError>;

pub async fn init_secret_key() -> Result<()> {
    // Only init if the file is not already present
    if let Ok(_) = load_secret_key().await {
        return Ok(());
    }
    let secret_key = iroh::SecretKey::generate(&mut rand::rng());
    let sk_bytes = secret_key.to_bytes();
    // make ~/.config/syncr if it doesn't exist
    let iroh_config_dir = dirs::config_dir().unwrap().join("syncr");
    fs::create_dir_all(&iroh_config_dir).await.map_err(|e| IrohUtilsError::SecretKeyGenerationError(e.to_string()))?;
    fs::write(iroh_config_dir.join("secret_key"), &sk_bytes).await.map_err(|e| IrohUtilsError::SecretKeyGenerationError(e.to_string()))?;
    Ok(())
}

pub async fn load_secret_key() -> Result<iroh::SecretKey> {
    let iroh_config_dir = dirs::config_dir().unwrap().join("syncr");
    let sk_path = iroh_config_dir.join("secret_key");
    let sk_vec = fs::read(sk_path).await.map_err(|e| IrohUtilsError::SecretKeyLoadError(e.to_string()))?;

    if sk_vec.len() != 32 {
        return Err(IrohUtilsError::SecretKeyLoadError("Invalid secret key length".to_string()));
    }

    let mut sk_bytes = [0u8; 32];
    for (i, byte) in sk_vec.iter().enumerate().take(32) {
        sk_bytes[i] = *byte;
    }
    
    Ok(SecretKey::from_bytes(&sk_bytes))
}
