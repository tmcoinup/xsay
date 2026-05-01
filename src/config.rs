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
    /// Whether xsay starts its own passive rdev/evdev keyboard listener.
    ///
    /// This keeps the historical "press the configured hotkey directly"
    /// behavior working. Users on restrictive Wayland setups can disable it
    /// and bind a desktop shortcut to `xsay toggle` instead.
    pub internal_listener: bool,
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
    /// Path to a local model file/directory; empty = managed cache
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
    /// ASR backend exposed by the Chinese model catalogue:
    ///   "sensevoice"      — SenseVoice-Small int8 ONNX
    ///   "sensevoice-fp32" — SenseVoice-Small float32 ONNX
    ///   "paraformer"      — Paraformer-zh ONNX
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
            internal_listener: true,
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
            hf_repo: "k2-fsa/sherpa-onnx".to_string(),
            hf_filename: crate::model_catalog::DEFAULT_MODEL_FILENAME.to_string(),
        }
    }
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            translate: false,
            n_threads: 4,
            backend: crate::model_catalog::default_model().backend.to_string(),
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            // Chinese-service default: keep the resident xsay badge in the
            // top-right so it is visible even when the native tray is hidden
            // by GNOME/AppIndicator availability.
            position: "top-right".to_string(),
            opacity: 0.9,
        }
    }
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            method: "clipboard".to_string(),
            clipboard_delay_ms: 120,
            // Chinese-service default targets terminal AI coding tools first:
            // Claude Code / Codex CLI / GNOME Terminal all expect Ctrl+Shift+V.
            // Most GUI apps also accept it as paste-plain-text.
            paste_shortcut: "ctrl-shift-v".to_string(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, XsayError> {
        let path = Self::config_path()?;
        if !path.exists() {
            let mut default = Config::default();
            default.normalize_for_chinese_service();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let text = toml::to_string_pretty(&default)?;
            std::fs::write(&path, text)?;
            log::info!("Created default config at {}", path.display());
            return Ok(default);
        }
        let text = std::fs::read_to_string(&path)?;
        let legacy_missing_internal_listener = hotkey_internal_listener_missing(&text);
        let mut cfg: Config = toml::from_str(&text)?;
        if legacy_missing_internal_listener {
            cfg.hotkey.internal_listener = true;
        }
        if cfg.normalize_for_chinese_service() {
            if let Ok(text) = toml::to_string_pretty(&cfg) {
                let _ = std::fs::write(&path, text);
                log::info!(
                    "Migrated config to Chinese model catalogue at {}",
                    path.display()
                );
            }
        }
        if legacy_missing_internal_listener {
            if let Ok(text) = toml::to_string_pretty(&cfg) {
                let _ = std::fs::write(&path, text);
                log::info!(
                    "Migrated legacy hotkey config to keep internal listener enabled at {}",
                    path.display()
                );
            }
        }
        Ok(cfg)
    }

    pub fn config_path() -> Result<PathBuf, XsayError> {
        let base = dirs::config_dir().ok_or(XsayError::NoConfigDir)?;
        Ok(base.join("xsay").join("config.toml"))
    }

    /// Normalize older configs that pointed at Whisper models. The current
    /// product surface is Chinese-first and exposes only the three ONNX
    /// models in `model_catalog`, so stale `ggml-*.bin` selections would
    /// otherwise leave users on an invisible Whisper backend with no model.
    fn normalize_for_chinese_service(&mut self) -> bool {
        let mut changed = false;

        match crate::model_catalog::backend_for_filename(&self.model.hf_filename) {
            Some(backend) => {
                if self.transcription.backend != backend {
                    self.transcription.backend = backend.to_string();
                    changed = true;
                }
            }
            None => {
                let default = crate::model_catalog::default_model();
                self.model.hf_filename = default.filename.to_string();
                self.transcription.backend = default.backend.to_string();
                self.model.path.clear();
                changed = true;
            }
        }

        if self.model.hf_repo != "k2-fsa/sherpa-onnx" {
            self.model.hf_repo = "k2-fsa/sherpa-onnx".to_string();
            changed = true;
        }
        if !matches!(self.transcription.language.as_str(), "zh" | "auto" | "yue") {
            self.transcription.language = "zh".to_string();
            changed = true;
        }
        if self.hotkey.key.is_empty() {
            self.hotkey.key = "z".to_string();
            changed = true;
        }
        if self.hotkey.mode != "hold" && self.hotkey.key.eq_ignore_ascii_case("z") {
            self.hotkey.mode = "hold".to_string();
            changed = true;
        }
        if self.injection.paste_shortcut == "both" {
            self.injection.paste_shortcut = "ctrl-shift-v".to_string();
            changed = true;
        }
        if self.injection.clipboard_delay_ms < 120 {
            self.injection.clipboard_delay_ms = 120;
            changed = true;
        }

        changed
    }
}

fn hotkey_internal_listener_missing(text: &str) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return false;
    };
    value
        .get("hotkey")
        .and_then(|hotkey| hotkey.get("internal_listener"))
        .is_none()
}
