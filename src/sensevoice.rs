//! ONNX offline ASR backends via sherpa-rs / sherpa-onnx.
//!
//! Historically this file only wrapped SenseVoice; it now dispatches
//! between multiple ONNX models that share sherpa-rs's C++ runtime. The
//! file name is kept for git-history continuity even though the scope is
//! broader — all backends here are invoked through the `transcribe()`
//! entry point based on the backend id from config.
//!
//! | backend id       | model                      | strengths                    |
//! |------------------|----------------------------|------------------------------|
//! | sensevoice       | SenseVoice Small int8      | 多语言高精度，低内存，最快   |
//! | sensevoice-fp32  | SenseVoice Small float32   | 多语言高精度，量化损失更少   |
//! | paraformer       | Paraformer-zh (达摩院)     | 中文强项，非自回归，低延迟   |
//!
//! All models live under ~/.cache/xsay/models/<backend>/ with an ONNX
//! model file + tokens.txt.
//!
//! Feature-gated behind `sensevoice` (historical name) so the default
//! build stays whisper-only and doesn't pull the ~50MB sherpa-onnx lib.

#![cfg(feature = "sensevoice")]

use parking_lot::Mutex;
use sherpa_rs::paraformer::{ParaformerConfig, ParaformerRecognizer};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// Separate recognizer holders so the ONNX sessions stay warm between
// utterances. Both are lazy-initialized on first use — that's ~500ms of
// model load that we don't want to pay every time. Mutex because both
// SenseVoiceRecognizer and ParaformerRecognizer hold raw C pointers and
// aren't Send on their own.
static SENSEVOICE: LazyLock<Mutex<Option<(String, SenseVoiceRecognizer)>>> =
    LazyLock::new(|| Mutex::new(None));
static PARAFORMER: LazyLock<Mutex<Option<ParaformerRecognizer>>> =
    LazyLock::new(|| Mutex::new(None));

fn models_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_default()
        .join("xsay")
        .join("models")
}

pub fn sensevoice_dir() -> PathBuf {
    models_root().join("sensevoice")
}

fn sensevoice_variant(backend: &str) -> Option<(&'static str, &'static str)> {
    match backend {
        "sensevoice" => Some(("sensevoice", "model.int8.onnx")),
        "sensevoice-fp32" => Some(("sensevoice-fp32", "model.onnx")),
        _ => None,
    }
}

fn sensevoice_dir_for(backend: &str) -> Option<PathBuf> {
    let (dir, _) = sensevoice_variant(backend)?;
    Some(models_root().join(dir))
}

pub fn paraformer_dir() -> PathBuf {
    models_root().join("paraformer")
}

/// Installed ONNX models share a model file + tokens.txt layout in a
/// per-backend subdirectory. This lets `is_installed`
/// stay a cheap two-stat check without touching the heavy ONNX session.
pub fn is_installed(backend: &str) -> bool {
    let (d, model_file) = match backend {
        b if sensevoice_variant(b).is_some() => {
            let (_, model_file) = sensevoice_variant(b).unwrap();
            (sensevoice_dir_for(b).unwrap(), model_file)
        }
        "paraformer" => (paraformer_dir(), "model.int8.onnx"),
        _ => return false,
    };
    d.join(model_file).is_file() && d.join("tokens.txt").is_file()
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnnxOptions {
    /// "auto" | "zh" | "en" | "ja" | "ko" | "yue". Paraformer is
    /// Chinese-only and ignores this field; SenseVoice uses it.
    pub language: String,
    /// Add punctuation + numerals (inverse text normalization).
    /// SenseVoice only; Paraformer has ITN baked in.
    pub use_itn: bool,
    /// "cpu" | "cuda".
    pub provider: String,
    pub num_threads: i32,
}

impl Default for OnnxOptions {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            use_itn: true,
            provider: "cpu".into(),
            num_threads: 4,
        }
    }
}

/// Transcribe 16 kHz mono f32 samples using the given ONNX backend.
/// Returns `None` if the backend isn't known, the model isn't installed,
/// or session init failed — callers fall back to Whisper.
pub fn transcribe(backend: &str, samples: &[f32], opts: &OnnxOptions) -> Option<String> {
    if samples.is_empty() {
        return Some(String::new());
    }
    match backend {
        b if sensevoice_variant(b).is_some() => transcribe_sensevoice(b, samples, opts),
        "paraformer" => transcribe_paraformer(samples, opts),
        other => {
            log::warn!("unknown ONNX backend {:?}", other);
            None
        }
    }
}

/// Eagerly initialize the recognizer for `backend` so the first real
/// transcription doesn't pay the ~500ms–3s ONNX session-construction
/// cost (Paraformer especially tends to be slow on first call). Fed a
/// 500ms silent buffer to warm up the compute graph; the result is
/// discarded. Safe to call on non-ONNX backends (no-op).
pub fn warmup(backend: &str, opts: &OnnxOptions) {
    if !is_installed(backend) {
        return;
    }
    let silence = vec![0.0_f32; 8000]; // 0.5s at 16 kHz
    let start = std::time::Instant::now();
    let _ = transcribe(backend, &silence, opts);
    log::info!(
        "{} warmup complete in {:?} — first real utterance will be snappy",
        backend,
        start.elapsed()
    );
}

fn transcribe_sensevoice(backend: &str, samples: &[f32], opts: &OnnxOptions) -> Option<String> {
    if !is_installed(backend) {
        let path = sensevoice_dir_for(backend).unwrap_or_else(sensevoice_dir);
        log::warn!("SenseVoice model not found at {}", path.display());
        return None;
    }
    let mut guard = SENSEVOICE.lock();
    let needs_reload = guard
        .as_ref()
        .map(|(loaded_backend, _)| loaded_backend != backend)
        .unwrap_or(true);
    if needs_reload {
        match build_sensevoice(backend, opts) {
            Ok(r) => {
                log::info!(
                    "SenseVoice recognizer ready (backend={}, provider={}, threads={})",
                    backend,
                    opts.provider,
                    opts.num_threads
                );
                *guard = Some((backend.to_string(), r));
            }
            Err(e) => {
                log::error!("SenseVoice init failed: {}", e);
                return None;
            }
        }
    }
    let (_, recognizer) = guard.as_mut().expect("just inserted");
    let result = recognizer.transcribe(16000, samples);
    Some(result.text)
}

fn transcribe_paraformer(samples: &[f32], opts: &OnnxOptions) -> Option<String> {
    if !is_installed("paraformer") {
        log::warn!(
            "Paraformer model not found at {}",
            paraformer_dir().display()
        );
        return None;
    }
    let mut guard = PARAFORMER.lock();
    if guard.is_none() {
        match build_paraformer(opts) {
            Ok(r) => {
                log::info!(
                    "Paraformer recognizer ready (provider={}, threads={})",
                    opts.provider,
                    opts.num_threads
                );
                *guard = Some(r);
            }
            Err(e) => {
                log::error!("Paraformer init failed: {}", e);
                return None;
            }
        }
    }
    let recognizer = guard.as_mut().expect("just inserted");
    let result = recognizer.transcribe(16000, samples);
    Some(result.text)
}

fn build_sensevoice(backend: &str, opts: &OnnxOptions) -> Result<SenseVoiceRecognizer, String> {
    let d = sensevoice_dir_for(backend).ok_or_else(|| format!("unknown backend {}", backend))?;
    let (_, model_file) =
        sensevoice_variant(backend).ok_or_else(|| format!("unknown backend {}", backend))?;
    let config = SenseVoiceConfig {
        model: path_str(&d.join(model_file)),
        tokens: path_str(&d.join("tokens.txt")),
        language: opts.language.clone(),
        use_itn: opts.use_itn,
        provider: Some(opts.provider.clone()),
        num_threads: Some(opts.num_threads),
        debug: false,
    };
    // sherpa-rs returns eyre::Result, which isn't a direct dep — stringify
    // at this boundary so the rest of xsay doesn't need to pull eyre.
    SenseVoiceRecognizer::new(config).map_err(|e| e.to_string())
}

fn build_paraformer(opts: &OnnxOptions) -> Result<ParaformerRecognizer, String> {
    let d = paraformer_dir();
    let config = ParaformerConfig {
        model: path_str(&d.join("model.int8.onnx")),
        tokens: path_str(&d.join("tokens.txt")),
        provider: Some(opts.provider.clone()),
        num_threads: Some(opts.num_threads),
        debug: false,
    };
    ParaformerRecognizer::new(config).map_err(|e| e.to_string())
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
