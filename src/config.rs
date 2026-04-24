use crate::error::XsayError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: HotkeyConfig,
    pub audio: AudioConfig,
    pub model: ModelConfig,
    pub transcription: TranscriptionConfig,
    pub overlay: OverlayConfig,
    pub injection: InjectionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// rdev::Key variant name, e.g. "F9", "ScrollLock", "AltGr"
    pub key: String,
    /// Optional modifier names: "ctrl", "alt", "shift", "super"
    pub modifiers: Vec<String>,
    /// "hold" (push-to-talk) or "toggle" (tap to start, tap to stop)
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Normalized RMS below which audio is considered silence
    pub silence_threshold: f32,
    /// Consecutive silent chunks (of ~1024 samples at 16kHz) before a pause fires
    pub silence_frames: u32,
    /// Maximum recording duration in seconds before forced transcription
    pub max_record_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    /// Path to a local GGML model file; empty = auto-download
    pub path: String,
    pub hf_repo: String,
    pub hf_filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    /// "auto", "zh", "en", etc.
    pub language: String,
    pub translate: bool,
    pub n_threads: i32,
    /// ASR backend:
    ///   "whisper"    — whisper.cpp via whisper-rs (default, CPU/GPU via features)
    ///   "sensevoice" — SenseVoice-Small ONNX via sherpa-rs; requires
    ///                  xsay built with `--features sensevoice`, model
    ///                  downloaded to ~/.cache/xsay/models/sensevoice/.
    ///                  Better Chinese accuracy, ~5-7x faster than Whisper-L.
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// "top-right", "top-left", "bottom-right", "bottom-left"
    pub position: String,
    pub opacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InjectionConfig {
    /// "clipboard" (Ctrl+V) or "type" (key events)
    pub method: String,
    pub clipboard_delay_ms: u64,
    /// Which key combo the Wayland uinput paste emits:
    ///   "ctrl-v"        — GUI text fields (default, works in most editors/browsers)
    ///   "ctrl-shift-v"  — terminals (GNOME Terminal, kitty, VS Code terminal, Claude Code CLI)
    ///   "both"          — send Ctrl+V then Ctrl+Shift+V back-to-back; maximum coverage
    ///                     but may open paste-special dialogs in some apps
    pub paste_shortcut: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: HotkeyConfig::default(),
            audio: AudioConfig::default(),
            model: ModelConfig::default(),
            transcription: TranscriptionConfig::default(),
            overlay: OverlayConfig::default(),
            injection: InjectionConfig::default(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key: "z".to_string(),
            modifiers: vec!["super".to_string()],
            mode: "hold".to_string(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            silence_threshold: 0.01,
            silence_frames: 24,
            max_record_seconds: 30,
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            hf_repo: "ggerganov/whisper.cpp".to_string(),
            hf_filename: "ggml-base.bin".to_string(),
        }
    }
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            language: "auto".to_string(),
            translate: false,
            n_threads: 4,
            backend: "whisper".to_string(),
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            // Bottom-center: least-obtrusive default for a voice input
            // overlay — user attention is typically mid-screen text
            // fields, and top-right badges collide with notification
            // toasts on GNOME/KDE.
            position: "bottom-center".to_string(),
            opacity: 0.9,
        }
    }
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            method: "clipboard".to_string(),
            clipboard_delay_ms: 80,
            // Default "both" so out-of-box usage works in both GUI apps
            // and terminals without the user needing to know to flip a
            // toggle. Power users in LibreOffice / VS Code can switch to
            // "ctrl-v" to avoid paste-special side effects.
            paste_shortcut: "both".to_string(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, XsayError> {
        let path = Self::config_path()?;
        if !path.exists() {
            let default = Config::default();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let text = toml::to_string_pretty(&default)?;
            std::fs::write(&path, text)?;
            log::info!("Created default config at {}", path.display());
            return Ok(default);
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn config_path() -> Result<PathBuf, XsayError> {
        let base = dirs::config_dir().ok_or(XsayError::NoConfigDir)?;
        Ok(base.join("xsay").join("config.toml"))
    }
}
