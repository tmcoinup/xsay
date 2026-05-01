use crate::{config::ModelConfig, error::XsayError};
use std::path::{Path, PathBuf};

/// Return the local path of the configured model, without downloading.
pub fn find_local(config: &ModelConfig) -> Option<PathBuf> {
    if !config.path.is_empty() {
        let p = PathBuf::from(&config.path);
        if p.exists() {
            return Some(p);
        }
    }
    let cache_dir = dirs::cache_dir()?.join("xsay").join("models");
    let cached = cache_dir.join(&config.hf_filename);
    if let Some(info) = crate::model_catalog::by_filename(&config.hf_filename) {
        if onnx_model_installed(&cached, info.onnx_model_file) {
            return Some(cached);
        }
        return None;
    }
    if cached.exists() { Some(cached) } else { None }
}

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
    if let Some(info) = crate::model_catalog::by_filename(&config.hf_filename) {
        if onnx_model_installed(&cached, info.onnx_model_file) {
            log::info!("Using cached ONNX model at {}", cached.display());
            return Ok(cached);
        }
        return download_onnx_archive(info, &cache_dir);
    } else {
        if cached.exists() {
            log::info!("Using cached model at {}", cached.display());
            return Ok(cached);
        }
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

    let api = hf_hub::api::sync::Api::new().map_err(|e| XsayError::HfHub(e.to_string()))?;
    let repo = api.model(config.hf_repo.clone());
    let downloaded = repo
        .get(&config.hf_filename)
        .map_err(|e| XsayError::HfHub(e.to_string()))?;

    // Copy to our cache dir so we control the path
    std::fs::copy(&downloaded, &cached)?;

    eprintln!("Model saved to {}", cached.display());
    Ok(cached)
}

fn onnx_model_installed(dir: &Path, model_file: &str) -> bool {
    dir.join(model_file).is_file() && dir.join("tokens.txt").is_file()
}

fn download_onnx_archive(
    info: &'static crate::model_catalog::ModelInfo,
    cache_dir: &Path,
) -> Result<PathBuf, XsayError> {
    let dest_dir = cache_dir.join(info.filename);
    if onnx_model_installed(&dest_dir, info.onnx_model_file) {
        return Ok(dest_dir);
    }

    std::fs::create_dir_all(cache_dir)?;
    let archive = cache_dir.join(format!("{}.tar.bz2", info.filename));

    log::info!("Downloading {} from {}", info.name, info.archive_url);
    eprintln!("Downloading '{}' from sherpa-onnx releases...", info.name);
    eprintln!("This may take a few minutes on first run.");

    let mut response = ureq::get(info.archive_url)
        .call()
        .map_err(|e| XsayError::HfHub(e.to_string()))?
        .into_reader();
    let mut file = std::fs::File::create(&archive)?;
    std::io::copy(&mut response, &mut file)?;

    extract_onnx_archive(&archive, &dest_dir, info.onnx_model_file).map_err(XsayError::HfHub)?;
    let _ = std::fs::remove_file(&archive);

    eprintln!("Model saved to {}", dest_dir.display());
    Ok(dest_dir)
}

fn extract_onnx_archive(archive: &Path, dest_dir: &Path, model_file: &str) -> Result<(), String> {
    let extract_tmp = std::env::temp_dir().join(format!("xsay-cli-extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&extract_tmp);
    std::fs::create_dir_all(&extract_tmp).map_err(|e| format!("mkdir: {}", e))?;
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("mkdir: {}", e))?;

    let status = std::process::Command::new("tar")
        .arg("-xjf")
        .arg(archive)
        .arg("-C")
        .arg(&extract_tmp)
        .status()
        .map_err(|e| format!("tar spawn: {}", e))?;
    if !status.success() {
        return Err(format!("tar exited {}", status));
    }

    let inner_dir = std::fs::read_dir(&extract_tmp)
        .map_err(|e| format!("readdir: {}", e))?
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .ok_or_else(|| "archive had no inner directory".to_string())?;

    for fname in [model_file, "tokens.txt"] {
        let src = inner_dir.join(fname);
        let dst = dest_dir.join(fname);
        if !src.exists() {
            return Err(format!("archive missing {}", fname));
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("copy {}: {}", fname, e))?;
    }

    let _ = std::fs::remove_dir_all(&extract_tmp);
    Ok(())
}
