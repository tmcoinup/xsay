use crate::{config::ModelConfig, error::XsayError};
use std::path::PathBuf;

pub fn ensure_model(config: &ModelConfig) -> Result<PathBuf, XsayError> {
    // Use explicit path if provided
    if !config.path.is_empty() {
        let p = PathBuf::from(&config.path);
        if p.exists() {
            return Ok(p);
        }
        return Err(XsayError::ModelNotFound(config.path.clone()));
    }

    // Check default cache location
    let cache_dir = dirs::cache_dir()
        .ok_or(XsayError::NoCacheDir)?
        .join("xsay")
        .join("models");
    std::fs::create_dir_all(&cache_dir)?;

    let cached = cache_dir.join(&config.hf_filename);
    if cached.exists() {
        log::info!("Using cached model at {}", cached.display());
        return Ok(cached);
    }

    // Download from Hugging Face
    log::info!(
        "Downloading model {} from {}...",
        config.hf_filename,
        config.hf_repo
    );
    eprintln!(
        "Downloading model '{}' from Hugging Face ({})...",
        config.hf_filename, config.hf_repo
    );
    eprintln!("This may take a few minutes on first run.");

    let api = hf_hub::api::sync::Api::new()
        .map_err(|e| XsayError::HfHub(e.to_string()))?;
    let repo = api.model(config.hf_repo.clone());
    let downloaded = repo
        .get(&config.hf_filename)
        .map_err(|e| XsayError::HfHub(e.to_string()))?;

    // Copy to our cache dir so we control the path
    std::fs::copy(&downloaded, &cached)?;

    eprintln!("Model saved to {}", cached.display());
    Ok(cached)
}
