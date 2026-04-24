//! Static catalogue of ASR models offered in the UI.
//!
//! Two families live side by side:
//!   - **Whisper (ggml)**: plain .bin files from ggerganov/whisper.cpp
//!     on HuggingFace. Downloaded directly, one file per model. Runs on
//!     the whisper-rs backend (CPU by default, GPU with feature flags).
//!   - **Sherpa ONNX**: tar.bz2 archives from k2-fsa/sherpa-onnx releases
//!     containing an ONNX model + tokens file. Needs `sensevoice` feature
//!     built in. Runs on the sherpa-rs backend. Typically faster and
//!     higher accuracy on Chinese than Whisper.

pub struct ModelInfo {
    pub name: &'static str,
    /// For Whisper: filename in the HF repo (ggerganov/whisper.cpp).
    /// For Sherpa ONNX: leaf directory name under ~/.cache/xsay/models/
    /// that the archive unpacks into (e.g. "sensevoice").
    pub filename: &'static str,
    pub size_mb: u32,
    pub desc: &'static str,
    /// Which ASR backend this model uses. Selecting a model also flips
    /// TranscriptionConfig.backend to match, so users only think in terms
    /// of "model" — the backend is inferred.
    pub backend: &'static str,
    /// For sherpa ONNX models, the full tar.bz2 URL to fetch. Whisper
    /// models leave this empty; they use hf_repo + filename instead.
    pub archive_url: &'static str,
}

pub static MODELS: &[ModelInfo] = &[
    // ---- Whisper (ggml) ----
    ModelInfo {
        name: "Tiny",
        filename: "ggml-tiny.bin",
        size_mb: 75,
        desc: "Whisper 最小款。CPU 实时因子 ~0.1x，精度一般，适合低配",
        backend: "whisper",
        archive_url: "",
    },
    ModelInfo {
        name: "Base",
        filename: "ggml-base.bin",
        size_mb: 147,
        desc: "Whisper 入门首选。CPU 实时因子 ~0.3x，精度良好，交互流畅",
        backend: "whisper",
        archive_url: "",
    },
    ModelInfo {
        name: "Small",
        filename: "ggml-small.bin",
        size_mb: 488,
        desc: "Whisper 中档。精度更好，CPU 实时因子 ~1x",
        backend: "whisper",
        archive_url: "",
    },
    ModelInfo {
        name: "Medium",
        filename: "ggml-medium.bin",
        size_mb: 1500,
        desc: "Whisper 高精度。CPU ~3x 实时，有 GPU 缩到 ~0.3x",
        backend: "whisper",
        archive_url: "",
    },
    ModelInfo {
        name: "Large v3",
        filename: "ggml-large-v3.bin",
        size_mb: 3100,
        desc: "Whisper 最高精度。CPU ~10x 实时（不可用），需 GPU + 4GB 显存",
        backend: "whisper",
        archive_url: "",
    },
    ModelInfo {
        name: "Large v3 Turbo",
        filename: "ggml-large-v3-turbo.bin",
        size_mb: 810,
        desc: "Whisper 官方蒸馏版。精度接近 Large，速度 4x 快，参数少一半",
        backend: "whisper",
        archive_url: "",
    },
    // ---- Sherpa ONNX (requires xsay built with --features sensevoice) ----
    ModelInfo {
        name: "SenseVoice Small",
        filename: "sensevoice",
        size_mb: 234,
        desc: "阿里开源多语言模型。中文精度超 Whisper Large，5x 快。支持中/英/日/韩/粤",
        backend: "sensevoice",
        archive_url:
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/\
             sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17.tar.bz2",
    },
    ModelInfo {
        name: "Paraformer-zh",
        filename: "paraformer",
        size_mb: 950,
        desc: "达摩院中文专用，非自回归 CTC 解码，低延迟。仅中文，内置 ITN（标点 + 数字）",
        backend: "paraformer",
        archive_url:
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/\
             sherpa-onnx-paraformer-zh-2024-03-09.tar.bz2",
    },
];
