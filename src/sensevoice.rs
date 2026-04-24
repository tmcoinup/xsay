//! SenseVoice Small offline ASR backend via sherpa-rs / sherpa-onnx.
//!
//! Why: Whisper's Chinese accuracy on Large is middling and the model is
//! heavy (3GB, slow on CPU). SenseVoice-Small is a 230MB ONNX int8 model
//! that matches or beats Whisper-large on Chinese (trained on extra
//! Mandarin, Cantonese, Japanese, Korean, English data at 40k hours) and
//! runs ~7x faster on the same hardware because it's a non-autoregressive
//! CTC decoder.
//!
//! Layout on disk (downloaded once, unpacked into cache_dir):
//!   ~/.cache/xsay/models/sensevoice/
//!     model.int8.onnx        — quantized model (~230 MB)
//!     tokens.txt             — 27 k BPE vocab
//!
//! Feature-gated behind `sensevoice` so the default build stays whisper-
//! only and doesn't pull the ~50MB sherpa-onnx shared library.

#![cfg(feature = "sensevoice")]

use parking_lot::Mutex;
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Thread-safe recognizer holder. SenseVoiceRecognizer is !Send on its
/// own (C++ context pointer), so we gate it behind a Mutex. Inference is
/// also serialized — running two decodes in parallel doesn't help anyway
/// because the ONNX session itself manages its thread pool.
static RECOGNIZER: LazyLock<Mutex<Option<SenseVoiceRecognizer>>> =
    LazyLock::new(|| Mutex::new(None));

/// Absolute path to the directory xsay unpacks SenseVoice into.
pub fn model_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_default()
        .join("xsay")
        .join("models")
        .join("sensevoice")
}

/// True iff both `model.int8.onnx` and `tokens.txt` are present. Keeping
/// the check cheap (just stat()s) so callers can poll it from UI without
/// touching the heavy ONNX session.
pub fn is_installed() -> bool {
    let d = model_dir();
    d.join("model.int8.onnx").is_file() && d.join("tokens.txt").is_file()
}

/// Recognizer configuration snapshot — everything we need to decide if
/// the in-memory session can be reused or must be rebuilt.
#[derive(Debug, Clone, PartialEq)]
pub struct SenseVoiceOptions {
    /// "auto" | "zh" | "en" | "ja" | "ko" | "yue"
    pub language: String,
    /// Add punctuation + numerals (inverse text normalization). Usually on.
    pub use_itn: bool,
    /// Execution provider. "cpu" by default; "cuda" with sensevoice-cuda
    /// feature enabled + NVIDIA GPU available.
    pub provider: String,
    pub num_threads: i32,
}

impl Default for SenseVoiceOptions {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            use_itn: true,
            provider: "cpu".into(),
            num_threads: 4,
        }
    }
}

/// Transcribe 16 kHz mono f32 samples to UTF-8 text. Returns None on
/// model-unavailable / load failure so the caller can fall back to a
/// different backend or surface an error to the user.
///
/// The recognizer is created lazily on first use and kept alive for the
/// process lifetime. Subsequent calls skip the ~500ms session init.
pub fn transcribe(samples: &[f32], opts: &SenseVoiceOptions) -> Option<String> {
    if samples.is_empty() {
        return Some(String::new());
    }
    if !is_installed() {
        log::warn!(
            "SenseVoice model not found at {}",
            model_dir().display()
        );
        return None;
    }

    let mut guard = RECOGNIZER.lock();
    if guard.is_none() {
        match build_recognizer(opts) {
            Ok(r) => {
                log::info!(
                    "SenseVoice recognizer ready (provider={}, threads={})",
                    opts.provider,
                    opts.num_threads
                );
                *guard = Some(r);
            }
            Err(e) => {
                log::error!("SenseVoice init failed: {}", e);
                return None;
            }
        }
    }
    let recognizer = guard.as_mut().expect("just inserted");
    let result = recognizer.transcribe(16000, samples);
    Some(result.text)
}

fn build_recognizer(opts: &SenseVoiceOptions) -> Result<SenseVoiceRecognizer, String> {
    let d = model_dir();
    let config = SenseVoiceConfig {
        model: path_str(&d.join("model.int8.onnx")),
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

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
