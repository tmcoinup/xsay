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
        desc: "最快（CPU 实时因子 ~0.1x），精度一般",
    },
    ModelInfo {
        name: "Base",
        filename: "ggml-base.bin",
        size_mb: 147,
        desc: "推荐：CPU 实时因子 ~0.3x，精度良好，交互最流畅",
    },
    ModelInfo {
        name: "Small",
        filename: "ggml-small.bin",
        size_mb: 488,
        desc: "精度更好，CPU 实时因子 ~1x（和录音时长相当）",
    },
    ModelInfo {
        name: "Medium",
        filename: "ggml-medium.bin",
        size_mb: 1500,
        desc: "高精度。CPU 下 ~3x 实时（1 秒录音算 3 秒），有 GPU 可缩到 ~0.3x",
    },
    ModelInfo {
        name: "Large v3",
        filename: "ggml-large-v3.bin",
        size_mb: 3100,
        desc: "最高精度。CPU 下 ~10x 实时（几乎不可用），需 GPU 加速版 xsay + 4GB+ 显存",
    },
];
