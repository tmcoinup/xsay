//! Chinese-first ASR model catalogue.
//!
//! xsay is currently packaged as a Chinese voice-input service, so the UI
//! exposes only the three sherpa-onnx models that perform well for Chinese
//! dictation. Whisper code remains in the crate for legacy/developer builds,
//! but normal config loading migrates users onto this catalogue.

pub const DEFAULT_MODEL_FILENAME: &str = "sensevoice";

pub struct ModelInfo {
    pub name: &'static str,
    /// Leaf directory under ~/.cache/xsay/models/.
    pub filename: &'static str,
    pub size_mb: u32,
    pub desc: &'static str,
    /// ASR backend id persisted in TranscriptionConfig.backend.
    pub backend: &'static str,
    /// Full tar.bz2 URL from k2-fsa/sherpa-onnx releases.
    pub archive_url: &'static str,
    /// ONNX file copied from the archive into the model directory.
    pub onnx_model_file: &'static str,
}

pub static MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "SenseVoice Small",
        filename: "sensevoice",
        size_mb: 234,
        desc: "Sherpa ONNX int8。中文/粤语/英/日/韩，速度最快，推荐日常输入",
        backend: "sensevoice",
        archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2",
        onnx_model_file: "model.int8.onnx",
    },
    ModelInfo {
        name: "SenseVoice Small FP32",
        filename: "sensevoice-fp32",
        size_mb: 894,
        desc: "Sherpa ONNX float32。精度更稳但更占内存，CPU 速度明显慢于 int8",
        backend: "sensevoice-fp32",
        archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17.tar.bz2",
        onnx_model_file: "model.onnx",
    },
    ModelInfo {
        name: "Paraformer-zh",
        filename: "paraformer",
        size_mb: 950,
        desc: "达摩院中文专用，非自回归 CTC 解码，低延迟。仅中文，内置 ITN（标点 + 数字）",
        backend: "paraformer",
        archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-zh-2024-03-09.tar.bz2",
        onnx_model_file: "model.int8.onnx",
    },
];

pub fn by_filename(filename: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.filename == filename)
}

pub fn backend_for_filename(filename: &str) -> Option<&'static str> {
    by_filename(filename).map(|m| m.backend)
}

pub fn default_model() -> &'static ModelInfo {
    by_filename(DEFAULT_MODEL_FILENAME).expect("default model must exist in catalogue")
}
