//! Static catalogue of Whisper GGML models offered in the UI.

pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    pub size_mb: u32,
    pub desc: &'static str,
}

pub static MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Tiny",
        filename: "ggml-tiny.bin",
        size_mb: 75,
        desc: "最快，精度一般，适合低配设备",
    },
    ModelInfo {
        name: "Base",
        filename: "ggml-base.bin",
        size_mb: 147,
        desc: "快速，精度良好",
    },
    ModelInfo {
        name: "Small",
        filename: "ggml-small.bin",
        size_mb: 488,
        desc: "平衡速度与精度",
    },
    ModelInfo {
        name: "Medium",
        filename: "ggml-medium.bin",
        size_mb: 1500,
        desc: "高精度，推荐使用",
    },
    ModelInfo {
        name: "Large v3",
        filename: "ggml-large-v3.bin",
        size_mb: 3100,
        desc: "最高精度，速度较慢，需要大量内存",
    },
];
