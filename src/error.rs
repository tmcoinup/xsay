use thiserror::Error;

// Some variants aren't raised today but cover the error surface we want
// to expose to future callers — keeping them unused-but-defined is the
// point of a typed error enum.
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum XsayError {
    #[error("No config directory found")]
    NoConfigDir,

    #[error("No cache directory found")]
    NoCacheDir,

    #[error("Model file not found: {0}")]
    ModelNotFound(String),

    #[error("No audio input device found")]
    NoInputDevice,

    #[error("Config parse error: {0}")]
    Config(#[from] toml::de::Error),

    #[error("Config serialize error: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Whisper error: {0}")]
    Whisper(String),

    #[error("HF Hub error: {0}")]
    HfHub(String),

    #[error("Wayland is not supported; run with XDG_SESSION_TYPE=x11 or DISPLAY set")]
    WaylandUnsupported,
}
