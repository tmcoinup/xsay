# Rust AI 语音输入系统 · 系统设计稿

> 版本：v1.0 · 2026-04-23
> 作者：系统架构师视角
> 项目代号：**xsay**（本仓库是该设计的 MVP 实现起点）

本文件系统性地回答"从 0 到 1 构建一个 Rust 实现的 AI 语音输入系统"。不是教科书，是工程蓝图：每章给出决策而非选项罗列。

---

## 目录

1. [产品定位](#1-产品定位)
2. [总体架构](#2-总体架构)
3. [模块拆解](#3-模块拆解)
4. [技术选型对比](#4-技术选型对比)
5. [项目目录结构](#5-项目目录结构)
6. [核心 trait 与数据结构](#6-核心-trait-与数据结构)
7. [异步并发模型](#7-异步并发模型)
8. [四种模式设计](#8-四种模式设计)
9. [Claude Code / Codex 集成](#9-claude-code--codex-集成)
10. [安全设计](#10-安全设计)
11. [性能优化](#11-性能优化)
12. [跨平台适配](#12-跨平台适配)
13. [测试方案](#13-测试方案)
14. [发布方案](#14-发布方案)
15. [开发路线图](#15-开发路线图)

---

## 1. 产品定位

### 1.1 一句话定义

**xsay 是面向开发者和重度键盘用户的本地优先 AI 语音输入层**——按快捷键说话，文字落到当前光标，并可选由 AI 做语义增强、命令整形或直接注入 Claude Code / Codex 会话。

### 1.2 核心问题

- **打字慢**：中文输入平均 40–60 WPM，说话 150+ WPM。
- **上下文切换贵**：在终端和 AI 对话中反复切窗口、改措辞、转码特殊字符。
- **系统输入法盲区**：macOS Dictation 隐私堪忧、离线差；Windows 讯飞/搜狗面向 GUI，不面向终端；Linux 几乎没有。
- **AI CLI 被输入速度拖累**：Claude Code / Codex / Aider 的生产力上限被用户打字速度限制。

### 1.3 和已有产品的边界

| 对比 | xsay 定位 |
|---|---|
| 系统听写（macOS/Windows） | 本地、离线、跨平台、可编程化输出、对 CLI/代码友好 |
| 讯飞/搜狗语音 | 不绑定云服务、不做输入法；面向终端和 AI 工具 |
| Wispr Flow / Superwhisper | 开源、可插件化、Rust 性能、可整合 Claude Code 链路 |
| ChatGPT 桌面"语音聊天" | 不锁定某个 AI 供应商；输出位置用户可控 |

### 1.4 为什么 Rust

- **实时音频**：cpal 回调线程要求毫秒级、零分配，Rust 不会突然 GC。
- **内存开销**：whisper.cpp 大模型 3 GB，Rust 避免托管语言额外 RAM。
- **跨平台发行**：静态链接单文件，无 GC 依赖。
- **绑定方便**：whisper-rs、tokenizers、ort 都是 Rust FFI 成熟方案。
- **长期进程稳定性**：常驻后台服务不会因为 GC 抖动或 OOM 掉线。

### 1.5 典型场景

- **终端口述**：`在当前目录创建 src components 子文件夹` → `mkdir -p src/components`
- **AI 对话**：对 Claude Code 说"重构一下 AuthService，改用 tokio::sync::RwLock"——自动整理成高质量 prompt 并填入终端。
- **代码口述**：`function getUserById接受一个u32返回Option<User>` → `fn get_user_by_id(id: u32) -> Option<User>`
- **普通文本**：邮件、工单、草稿。
- **多语混输**：`会议 minutes 记录：alice 同步 sprint 进度`。
- **会议速记**：一直开着，写到历史日志。

### 1.6 三档产品规划

| 档 | 目标用户 | 关键能力 | 不做 |
|---|---|---|---|
| **MVP** | 个人开发者 | 录 → 识 → 注入，本地 Whisper，一种快捷键模式，一个主窗口 | AI 后处理、插件、远程 ASR |
| **进阶** | 效率工具爱好者 | 四种模式、Claude Code 集成、AI 纠错、历史搜索、流式识别 | 账号、云同步、多用户 |
| **生产商用** | 团队 / 企业 | 插件系统、企业 API Key 管理、审计日志、自更新、崩溃上报、多 profile | 自建云 ASR 产品化 |

---

## 2. 总体架构

### 2.1 分层

```
┌──────────────────────────────────────────────────┐
│ 8. 扩展层（Plugin / 自定义后处理器 / CLI 适配器） │
├──────────────────────────────────────────────────┤
│ 7. 配置与存储层（TOML + SQLite + 诊断日志）       │
├──────────────────────────────────────────────────┤
│ 6. 交互控制层（Hotkey / Tray / UI / State）       │
├──────────────────────────────────────────────────┤
│ 5. 输出执行层（Clipboard / Keyboard / CLI Pipe） │
├──────────────────────────────────────────────────┤
│ 4. AI 后处理层（Punct / Correct / Mode Router）   │
├──────────────────────────────────────────────────┤
│ 3. 识别引擎层（Local Whisper / Remote ASR 抽象）  │
├──────────────────────────────────────────────────┤
│ 2. 音频预处理层（VAD / Resample / Denoise / Slice）│
├──────────────────────────────────────────────────┤
│ 1. 输入采集层（cpal / 设备管理 / 环形缓冲）       │
└──────────────────────────────────────────────────┘
           ▲                                  ▲
           └── Platform abstraction ──────────┘
```

### 2.2 模块依赖关系

- **单向依赖**：高层调用低层，低层不感知高层。通过 trait + channel 解耦。
- **Platform abstraction** 横切：音频设备、热键、剪贴板、托盘、聚焦窗口查询四个点。
- **Pipeline 流转**：采集 → VAD → ASR → 后处理 → 注入，每一段都可以被替换或短路。

### 2.3 数据流（标准路径）

```
麦克风 PCM → RingBuffer → Resampler → VAD segmenter
                                           ↓
                                    AudioSegment (f32, 16kHz)
                                           ↓
                                  AsrEngine::transcribe
                                           ↓
                                   RawTranscript { text, lang, confidence }
                                           ↓
                              TextPostProcessor::process(ctx)
                                           ↓
                                   PolishedText { mode, payload }
                                           ↓
                                TextInjector::inject
                                           ↓
                              光标 / 剪贴板 / CLI stdin
```

### 2.4 控制流

```
Hotkey 事件 ─► SessionController ─► AudioSource.start
                         │
                         ├─► VadSegmenter.on_chunk (流式)
                         │       │
                         │       └─► AsrEngine.push (partial)
                         │
                         └─► (release / pause / esc)
                              └─► AsrEngine.finish
                                   └─► TextPostProcessor
                                        └─► TextInjector
                                             └─► State = Idle
```

### 2.5 同步 / 异步边界

| 边界 | 规则 |
|---|---|
| cpal 回调 | **绝对同步、零等待、零分配**，只 push 到 ring buffer |
| 音频处理线程 | 同步 CPU 循环（resample/VAD），和 cpal 解耦 |
| ASR 本地推理 | CPU/GPU 密集，独立 worker 线程或 `spawn_blocking` |
| ASR 远程 | `async` (tokio) + WebSocket/HTTP 流 |
| AI 后处理 | `async` (tokio) + HTTP |
| UI（egui） | 独立主线程，所有状态通过 `Arc<Mutex>` / `watch` 读取 |
| 注入 | 独立 worker 线程，使用 `enigo` 可能阻塞 |

**设计原则**：**音频和 UI 必须不能等 async runtime**。tokio 出问题时，录音还得继续，UI 还得画。所以 runtime 只围绕"网络 + AI + 插件"这个 blast radius 跑。

### 2.6 必须解耦

| 解耦对象 | 原因 |
|---|---|
| `AsrEngine` 和调用方 | 本地/远程/多引擎切换；测试用 mock |
| `TextPostProcessor` 和模式 | 不同模式 prompt 差异大；插件可替换 |
| `TextInjector` 和平台 | 三大 OS 各不相同；CLI 注入又是另一路 |
| `HotkeyManager` 和 UI | 后台模式无 UI；CLI-only 场景可用 |
| `ConfigRepository` 和业务 | 热更新、多 profile、测试固定值 |

---

## 3. 模块拆解

每个模块给出：**职责 / I/O / 数据结构 / trait / 并发 / 错误 / 可替换**。

### 3.1 音频采集模块 `audio_capture`

**职责**：打开设备、产生统一格式（16 kHz mono f32）、提供启停控制。

**I/O**：
- in：`AudioCmd::Start { device, sample_rate }` / `Stop` / `Abort`
- out：`mpsc::Sender<AudioFrame>`（每 10 ms 一帧）

**数据结构**：
```rust
pub struct AudioFrame {
    pub samples: Arc<[f32]>,   // 16000 Hz, mono, 10 ms = 160 samples
    pub t_monotonic: Instant,  // 采集时间戳（低抖动）
    pub rms: f32,              // 预计算 RMS，省得下游重算
}
```

**trait**：
```rust
pub trait AudioSource: Send + 'static {
    fn start(&mut self, cfg: &AudioConfig) -> Result<mpsc::Receiver<AudioFrame>>;
    fn stop(&mut self);
    fn devices() -> Vec<AudioDeviceInfo>;
}
```

**并发**：cpal 回调 → **无锁 ring buffer（`ringbuf::HeapRb`）** → 独立 worker 线程做 resample + 发 `AudioFrame`。禁止在 cpal 回调里分配、锁、发送到 channel（冷路径未知）。

**错误**：`AudioError::DeviceGone` / `FormatUnsupported` / `Overflow`（上层决定降级或重启设备）。

**可替换**：`MockAudioSource` 从 wav 文件回放；远程设备（后续扩展）。

---

### 3.2 VAD 与分段模块 `vad`

**职责**：把流式 `AudioFrame` 切成"有意义的语音段"，给 ASR 投喂。

**推荐**：**Silero VAD (ONNX)** via `ort`。比 WebRTC VAD 准（对音乐、背景噪声鲁棒），比 whisper 自带判断快 10 倍，不需要 GPU。

**策略**：
- 每帧计算 speech prob。
- 连续 200 ms `p>0.5` ⇒ 进入 speech 状态。
- 连续 700 ms `p<0.3` ⇒ 进入 silence，发射 segment 给 ASR。
- 最小段 400 ms（避免嗓子响一声被当 utterance）。
- 最大段 30 s（超长强制切，流式投喂）。

**数据结构**：
```rust
pub enum VadDecision { Continue, Segment(AudioSegment), Silence }
pub struct AudioSegment {
    pub samples: Vec<f32>,
    pub start: Duration,  // 相对 session
    pub end: Duration,
    pub is_final: bool,   // 语句结束还是 chunk-flush
}
```

**trait**：
```rust
pub trait VadEngine: Send {
    fn push(&mut self, frame: &AudioFrame) -> VadDecision;
    fn flush(&mut self) -> Option<AudioSegment>;
    fn reset(&mut self);
}
```

**平衡低延迟 vs 过早截断**：用"未定决策时投入 partial ASR"（ASR 拿 partial_no_context 模式）——用户可能看到实时预览，但最终 commit 以 final 为准。

---

### 3.3 识别引擎层 `asr`

**统一 trait**：
```rust
#[async_trait]
pub trait AsrEngine: Send + Sync {
    async fn start_stream(&self, cfg: AsrStreamConfig) -> Result<AsrStreamHandle>;
    async fn transcribe_file(&self, path: &Path, cfg: &AsrCfg) -> Result<Transcript>;
    fn capabilities(&self) -> AsrCapabilities;
    async fn health_check(&self) -> Result<()>;
}

pub struct AsrStreamHandle {
    pub push: mpsc::Sender<AudioSegment>,
    pub events: mpsc::Receiver<AsrEvent>, // Partial / Final / Error
    pub finish: oneshot::Sender<()>,
}
```

**实现矩阵**：
- `WhisperCppEngine`（本地，whisper-rs） — 阻塞 CPU，`spawn_blocking` 包裹
- `OnnxWhisperEngine`（本地，ort + faster-whisper ONNX 导出） — 更快，但部署体积大
- `RemoteHttpEngine`（通用 OpenAI Whisper 协议） — 生产环境兜底
- `WebSocketStreamingEngine`（真正流式，例如 Deepgram / 自建 funasr）
- `MockEngine`（测试）

**推荐**：MVP 用 `WhisperCppEngine`；进阶版并存 `RemoteHttpEngine`（云端更快更准，隐私可选）。

---

### 3.4 AI 文本后处理 `ai_postprocess`

**职责**：把原始 ASR 输出变成"可接受输出"。分三种模式。

**trait**：
```rust
#[async_trait]
pub trait TextPostProcessor: Send + Sync {
    async fn process(&self, input: RawTranscript, ctx: &ProcessCtx) -> Result<PolishedText>;
    fn name(&self) -> &'static str;
}

pub struct ProcessCtx<'a> {
    pub mode: Mode,
    pub focused_app: Option<String>,
    pub recent_history: &'a [String],
    pub selected_text: Option<&'a str>,
    pub user_glossary: &'a Glossary,
}
```

**实现顺序**（流水线，前面失败后面兜底）：
1. `PuncRestorer`（轻量本地模型或规则）— 加标点、大小写
2. `GlossaryFixer`（用户词典替换，例如"凯莱"→"Claude"）
3. `CorrectorLlm`（远程 / 本地 LLM）— 按模式走不同 prompt
4. `SafetyFilter`（命令模式时拦截危险指令）

**关键点**：**必须保留"原始文本"**，AI 改完的结果不能覆盖它——用户随时可以 Undo 到原文。

---

### 3.5 输出注入 `injector`

**trait**：
```rust
#[async_trait]
pub trait TextInjector: Send + Sync {
    async fn inject(&self, text: &str, target: InjectTarget) -> Result<()>;
    fn supports(&self, target: InjectTarget) -> bool;
}

pub enum InjectTarget {
    FocusedWindow,
    Terminal { pty_fd: Option<i32> },
    ClaudeCode { session_id: String },
    Codex { tty: PathBuf },
    CustomCmd { argv: Vec<String> },
    Clipboard,
}
```

**实现**：
- `ClipboardInjector` — `arboard::set_text` + 模拟 Ctrl+V（CJK 唯一可靠路径）
- `KeystrokeInjector` — `enigo`，对英文+代码快
- `TerminalPtyInjector` — 直接写 pty fd（非常规，需要从父进程找到当前 tty）
- `ClaudeCodeInjector` — 走 `claude` CLI 的 slash command 或 stdin 管道
- `CodexInjector` — 类似

**特殊字符处理**：
- Markdown 代码块、shell 特殊字符，按 target 做 escape
- 多行：剪贴板方式无压力；keystroke 要按行 split 并在每行后模拟 Enter

**是否预览**：**命令模式和 AI 模式必须预览**；普通文本模式可以直接。

---

### 3.6 快捷键与状态 `hotkey` + `state`

**trait**：
```rust
pub trait HotkeyManager: Send {
    fn register(&mut self, chord: &KeyChord, cb: Box<dyn Fn(KeyEvent) + Send>) -> HotkeyId;
    fn unregister(&mut self, id: HotkeyId);
    fn set_suppressed(&mut self, suppressed: bool); // 设置 UI 捕获期间抑制
}
```

**后端**：rdev（X11/Mac/Win）+ evdev（Wayland）+ WindowsHook（Windows 原生）+ CGEventTap（macOS）。

**状态机**（集中到 `SessionController`）：
```rust
pub enum SessionState {
    Idle,
    Arming { started_at: Instant },     // 防抖
    Recording { mode: Mode },
    Processing { stage: ProcessStage },  // Transcribe | PostProcess
    Injecting,
    Error { kind: ErrorKind, at: Instant },
}
```

UI 订阅 `tokio::sync::watch<SessionState>`。

---

### 3.7 配置系统 `config`

- **格式**：TOML（用户可读写）+ 必要时 SQLite（历史/日志/上下文）
- **profile 支持**：`config.toml` + `profiles/{name}.toml`（命令模式 profile、代码模式 profile...）
- **热更新**：
  - 所有读取通过 `ConfigRepository::snapshot()` → `Arc<Config>`（共享不可变快照）
  - 文件变更用 `notify` crate 监听，`tokio::sync::watch` 广播新 snapshot
  - 线程持 `watch::Receiver`，每轮 `borrow()` 拿当前最新

**分层**：
```rust
pub struct ConfigRepository {
    path: PathBuf,
    current: watch::Sender<Arc<Config>>,
}
```

**API Key 存储**：用 OS secure storage（`keyring` crate），**不要写 TOML**。

---

### 3.8 日志与诊断 `telemetry`

- **tracing** 作为统一接口
- **subscriber**：`tracing-subscriber` 文件 + stdout 分离
- **关键指标**：
  - 端到端延迟：record_end → inject_start
  - ASR 延迟：segment_push → transcript_ready
  - 音频丢帧率：ring buffer overflow 次数
  - 内存峰值：RSS 采样
- **诊断包**：`xsay diag bundle` 打包最近日志 + 配置（脱敏）+ 系统信息 → zip
- **崩溃上报**：`sentry-rust`（opt-in，商用版）

---

## 4. 技术选型对比

### 4.1 UI 框架

| 方案 | 开发难度 | 跨平台 | 性能 | 维护 | 推荐 |
|---|---|---|---|---|---|
| **egui + eframe** | 低 | ✅ | 高 (GPU) | 活跃 | **MVP 和长期** |
| Tauri | 中（HTML/CSS） | ✅ | 中 (WebView) | 成熟 | 备选（需复杂 UI 时） |
| iced | 中 | ✅ | 高 | 慢 | 不推荐（生态小） |
| Slint | 中 | ✅ | 高 | 商用限制 | 不推荐 |

**决策**：**egui**。声明式简单、无 WebView 开销、Rust 纯生态、动画/不规则窗口友好。

### 4.2 音频采集

| 方案 | 推荐 |
|---|---|
| **cpal** | ✅ 跨平台、低延迟、原始 PCM |
| rodio | 面向播放，不适合采集 |
| 自写平台 FFI | 不值 |

**决策**：**cpal**。

### 4.3 本地 ASR

| 方案 | 速度 | 精度 | 部署 | 推荐 |
|---|---|---|---|---|
| **whisper.cpp** (whisper-rs) | 中 | 高 (Large) | 单 .bin 文件 | **MVP** |
| faster-whisper (ONNX via ort) | 快 3–5× | 同上 | ONNX 模型 + ort DLL | **进阶版** |
| Candle (pure Rust) | 中 | 中 | 纯 Rust | 关注中 |
| Vosk | 快 | 中 | 多语言模型较小 | 备选（资源受限） |

**决策**：MVP whisper-rs，生产版 `ort + faster-whisper`（速度翻倍）。

### 4.4 AI 后处理

| 方案 | 推荐 |
|---|---|
| 远程 Claude / OpenAI | **主路径**（质量/速度/可选最佳） |
| 本地 LLM (llama.cpp, 7B) | 可选（隐私版） |
| 小模型专做纠错（BART/T5） | **加标点专用** (< 50 MB) |

**决策**：**混合**。标点/纠错走小本地模型（即时、免费）；高级整理（模式、prompt 整形）走远程 API，按需开启。

### 4.5 配置存储

- **TOML**（用户配置）— 人读友好、可 diff
- **SQLite**（历史、上下文、缓存）— 查询方便
- `sled/redb` — 不用，避开 binary 锁定格式

### 4.6 插件系统

| 方案 | 优 | 缺 |
|---|---|---|
| 动态库 (`.so/.dylib/.dll`) | 高性能 | ABI 脆弱，Rust 没稳定 ABI |
| **WASM (wasmtime)** | 沙箱安全、跨平台 | 编译、受限 |
| **外部进程 + stdio/RPC** | 解耦彻底、任意语言 | IPC 开销 |

**决策**：**WASM + 外部进程并存**：
- WASM：轻量后处理器（文本变换、glossary）
- 外部进程：重型插件（调用 Python、走 GPU）

---

### 4.7 推荐栈

| 场景 | 栈 |
|---|---|
| **MVP** | cpal + whisper-rs + eframe + rdev + enigo + arboard + TOML + tracing |
| **生产版** | cpal + ort(faster-whisper) + Silero VAD + eframe + platform-native hotkey + tokio + Claude API 后处理 + keyring + SQLite + WASM 插件 |

---

## 5. 项目目录结构

推荐 **workspace + 单二进制 + 多 crate**：

```
xsay/
├── Cargo.toml                  # workspace
├── README.md
├── docs/
│   ├── ARCHITECTURE.md
│   ├── MODULES.md
│   ├── UI_DESIGN_PROMPT.md
│   └── SYSTEM_DESIGN.md        # 本文件
├── apps/
│   ├── xsay-gui/               # 主 GUI app (bin)
│   │   └── src/main.rs
│   └── xsay-cli/               # 纯 CLI / 后台 (bin)
│       └── src/main.rs
├── crates/
│   ├── xsay-core/              # 领域模型 + trait 定义
│   │   └── src/{lib.rs, state.rs, session.rs}
│   ├── xsay-audio/             # cpal + ring buffer + resample
│   ├── xsay-vad/               # Silero VAD + segmenter
│   ├── xsay-asr/               # trait + impls
│   │   └── src/{trait.rs, whisper_cpp.rs, onnx.rs, remote_http.rs, mock.rs}
│   ├── xsay-postprocess/       # punct + glossary + LLM correct
│   ├── xsay-injector/          # trait + impls
│   │   └── src/{trait.rs, clipboard.rs, keystroke.rs, terminal.rs, claude_code.rs, codex.rs}
│   ├── xsay-hotkey/            # trait + rdev/evdev/win/mac
│   ├── xsay-config/            # load/save/watch + secret storage
│   ├── xsay-history/           # JSONL + SQLite
│   ├── xsay-telemetry/         # tracing setup + diag bundle
│   ├── xsay-platform/          # platform abstraction
│   │   └── src/{mod.rs, linux.rs, macos.rs, windows.rs}
│   └── xsay-plugin/            # WASM runtime + external process RPC
├── plugins/
│   └── example_glossary/       # 示例插件
├── configs/
│   ├── default.toml
│   ├── profiles/
│   │   ├── command.toml
│   │   └── coding.toml
│   └── glossary.example.toml
├── scripts/
│   ├── build.sh
│   ├── package-deb.sh
│   └── fetch-models.sh
├── assets/
│   ├── icons/
│   ├── models/                 # .gitignored
│   └── test_audio/
└── tests/
    ├── e2e/
    ├── fixtures/
    └── benchmarks/
```

**为什么这么拆**：
- `crates/xsay-*`：每个领域独立 crate，编译增量快，便于单元测试。
- `apps/*`：可以同时有 GUI 和纯 CLI 两个二进制，复用 core。
- `plugins/*`：用户能 clone 仓库看着例子写自己的插件。
- `configs/`：带示例 profile，便于新用户起步。

---

## 6. 核心 trait 与数据结构

### 6.1 trait 清单

```rust
// crates/xsay-core/src/traits.rs

pub trait AudioSource: Send + 'static {
    fn start(&mut self, cfg: &AudioConfig) -> Result<mpsc::Receiver<AudioFrame>>;
    fn stop(&mut self);
}

pub trait VadEngine: Send {
    fn push(&mut self, frame: &AudioFrame) -> VadDecision;
    fn flush(&mut self) -> Option<AudioSegment>;
    fn reset(&mut self);
}

#[async_trait]
pub trait AsrEngine: Send + Sync {
    async fn start_stream(&self, cfg: AsrStreamConfig) -> Result<AsrStreamHandle>;
    async fn transcribe_file(&self, path: &Path) -> Result<Transcript>;
    fn capabilities(&self) -> AsrCapabilities;
}

#[async_trait]
pub trait TextPostProcessor: Send + Sync {
    async fn process(&self, raw: RawTranscript, ctx: &ProcessCtx<'_>) -> Result<PolishedText>;
    fn name(&self) -> &'static str;
}

#[async_trait]
pub trait TextInjector: Send + Sync {
    async fn inject(&self, text: &str, target: InjectTarget) -> Result<()>;
    fn supports(&self, target: InjectTarget) -> bool;
}

pub trait HotkeyManager: Send {
    fn register(&mut self, chord: &KeyChord, cb: Box<dyn Fn(KeyEvent) + Send>) -> HotkeyId;
    fn unregister(&mut self, id: HotkeyId);
    fn set_suppressed(&mut self, yes: bool);
}

pub trait CommandAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn inject(&self, text: &str, mode: Mode) -> Result<()>;
    fn preflight(&self) -> Result<()>;  // 检查 CLI 在不在
}

pub trait SettingsRepository: Send + Sync {
    fn snapshot(&self) -> Arc<Config>;
    fn subscribe(&self) -> watch::Receiver<Arc<Config>>;
    fn update(&self, mut f: impl FnMut(&mut Config)) -> Result<()>;
}

pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn on_event(&self, ev: PluginEvent) -> PluginResponse;
}
```

### 6.2 关键结构

```rust
// AppState 是 UI 层呈现的简化状态；SessionState 是内核状态，更细
pub enum AppState { Idle, Recording, Processing, Injecting, Error(String) }

pub struct SessionState {
    pub app: AppState,
    pub mode: Mode,
    pub partial_text: String,         // 流式结果
    pub final_text: String,
    pub last_error: Option<XsayError>,
    pub session_id: Uuid,
}

pub enum Mode {
    Dictation,
    Command { preview: bool, danger_guard: bool },
    Code { lang: CodeLang },
    AiAssistant { target: AiTarget },
}

pub enum AiTarget {
    ClaudeCode { session: Option<String> },
    Codex,
    Generic { api_url: String },
    Clipboard, // 复制到剪贴板，不自动发
}

pub struct ProcessCtx<'a> {
    pub mode: Mode,
    pub focused_app: Option<String>,
    pub recent_history: &'a [HistoryEntry],
    pub selected_text: Option<&'a str>,
    pub user_glossary: &'a Glossary,
    pub locale: &'a str,
}

// 错误分层：thiserror 做类型化，anyhow 在顶层拼装
#[derive(thiserror::Error, Debug)]
pub enum XsayError {
    #[error("audio: {0}")] Audio(#[from] AudioError),
    #[error("vad: {0}")] Vad(#[from] VadError),
    #[error("asr: {0}")] Asr(#[from] AsrError),
    #[error("postprocess: {0}")] Post(#[from] PostError),
    #[error("injector: {0}")] Inject(#[from] InjectError),
    #[error("config: {0}")] Config(#[from] ConfigError),
    #[error("plugin: {0}")] Plugin(#[from] PluginError),
}
```

### 6.3 Arc / Mutex / RwLock / channel 使用建议

| 用法 | 选型 |
|---|---|
| 全局不可变配置快照 | `Arc<Config>` + `watch::Sender` 更新 |
| 低频写、高频读 | `Arc<ArcSwap<T>>`（arc-swap crate） |
| 单写多读（配置变）| `tokio::sync::watch` |
| 命令队列 | `tokio::sync::mpsc`（async）或 `crossbeam_channel`（同步/跨线程） |
| 实时音频 PCM | `ringbuf::HeapRb<f32>`（无锁 SPSC） |
| 一次性结果 | `oneshot` |
| 多订阅事件 | `tokio::sync::broadcast` |

**生命周期原则**：`'static` + `Arc` 而不是借用；所有跨线程/任务的数据都用 `Arc<T>`，避免生命周期地狱。

---

## 7. 异步并发模型

### 7.1 拓扑

```
┌──── main (sync) ──── eframe UI loop ────┐
│                                          │
│    tokio runtime (multi-thread, 4 核)   │
│    ├── tokio::task: SessionController   │
│    ├── tokio::task: AsrEngine (streaming)│
│    ├── tokio::task: TextPostProcessor   │
│    ├── tokio::task: ConfigWatcher       │
│    └── tokio::task: PluginHost          │
│                                          │
│    dedicated threads (std::thread):      │
│    ├── cpal input callback (lock-free)  │
│    ├── audio worker (resample + VAD)    │
│    ├── whisper_cpp inference (blocking) │
│    ├── injector (enigo blocking)        │
│    ├── hotkey listener (rdev/evdev)     │
│    └── tray (GTK main, Linux only)      │
│                                          │
│    channel topology:                     │
│    cpal cb ──ringbuf──► audio worker    │
│    audio worker ──mpsc(AudioFrame)──► VAD ──mpsc(Segment)──► ASR task│
│    ASR task ──mpsc(AsrEvent)──► SessionController                    │
│    SessionController ──oneshot──► PostProcess ──mpsc──► Injector     │
│    config ──watch<Arc<Config>>──► 所有订阅者                           │
│    state ──watch<SessionState>──► UI                                  │
└──────────────────────────────────────────┘
```

### 7.2 规则

1. **音频回调禁止 await / lock / alloc**。
2. **whisper 推理用 `spawn_blocking`**，它不能在 tokio worker 跑否则饿死别的任务。
3. **远程 ASR 用 tokio-native 流（tungstenite / reqwest）**。
4. **取消**：每个 session 分配一个 `CancellationToken`（`tokio-util`），Escape → `token.cancel()`，所有子任务 `select!` 监听。
5. **背压**：ASR 比音频慢 → VAD 的 mpsc 会填满 → 主动丢弃 partial 但保留 final。
6. **graceful shutdown**：main 收 Ctrl+C → `shutdown_tx.send(())` → 所有任务收到后 drain 并退出；cpal stream 主动 stop；whisper 线程在当前段结束后退出。

### 7.3 channel 选型总结

| 场景 | 选型 | 理由 |
|---|---|---|
| cpal → audio worker | `ringbuf` SPSC | 无锁、固定大小、实时安全 |
| audio → VAD → ASR | `tokio::mpsc(64)` | 背压、支持 `async` send |
| ASR → Session | `tokio::mpsc` | 同上 |
| 配置 | `tokio::watch` | 单写多读，总是看到最新 |
| UI 状态 | `tokio::watch` | UI 每帧 `borrow()` |
| Cancel | `CancellationToken` | 多方订阅 |
| 一次性结果 | `oneshot` | 避免分配 mpsc |

### 7.4 "录音中实时显示文字"

```rust
// 在 session controller
let handle = asr.start_stream(cfg).await?;
while let Some(ev) = handle.events.recv().await {
    match ev {
        AsrEvent::Partial(text) => state.send(SessionState {
            partial_text: text,
            ..current_snapshot
        }),
        AsrEvent::Final(text) => { finals.push(text); }
        AsrEvent::Error(e) => { ... }
    }
}
```

UI 订阅 `watch::Receiver<SessionState>`，每帧 `borrow().partial_text` 显示。

---

## 8. 四种模式设计

### 8.1 普通听写模式（Dictation）

- **目标**：自然语言，自动标点，自然分段。
- **Prompt**：
  ```
  You are a transcription cleaner. Add proper punctuation and capitalization.
  Do NOT change wording, do NOT translate, do NOT add content.
  If the text is in Chinese, use Chinese punctuation (，。！？).
  Return ONLY the cleaned text.
  ```
- **输出约束**：字符级 diff ≤ 20% 原文长度，否则回退到原始 ASR 输出。
- **示例**：
  - 输入：`今天 会议 讨论 了 Q1 的 okr 完成 情况 以及 下 季度 计划`
  - 输出：`今天会议讨论了 Q1 的 OKR 完成情况，以及下季度计划。`

### 8.2 命令模式（Command）

- **目标**：语音 → 可在当前 shell 执行的命令。
- **Prompt**：
  ```
  You convert natural-language commands into safe shell commands.
  - Platform: {{os}} ({{shell}})
  - Cwd: {{cwd}}
  - Output ONLY the command, no markdown fences, no explanation.
  - If the request is destructive (rm -rf, drop database, etc.), prefix with `# DANGER:\n`.
  - If the request is ambiguous, output `# AMBIGUOUS: <reason>`.
  ```
- **安全限制**：
  - 黑名单正则：`rm\s+-rf\s+/`, `mkfs\.`, `dd\s+if=.*of=/dev/`, `:(){ :|:& };:`
  - 命中黑名单强制走"预览 + 显式确认"
  - 所有命令模式输出**先到剪贴板 + 弹预览，不直接模拟回车**，除非用户勾了"自动执行"
- **示例**：
  - 输入：`查找当前目录下所有大于 10M 的文件`
  - 输出：`find . -type f -size +10M`

### 8.3 代码模式（Code）

- **目标**：口述代码片段。
- **Prompt**：
  ```
  You convert dictated pseudo-code into {{lang}} syntax.
  - Recognize Chinese tech words (for 循环 → for loop)
  - Keep variable names as the user said
  - Use standard formatting (rustfmt / prettier / black)
  - Output the code block ONLY, no markdown fences
  ```
- **符号转换表**：常用口述 → 符号
  - "赋值" → `=`  |  "等于" → `==`  |  "箭头" → `->` / `=>`
  - "左花括号/右花括号" → `{` `}`
  - "冒号" → `:`  |  "反引号块" → ``` ` ```
- **示例**：
  - 输入：`function getUser 接受一个 u32 id 返回 Option User`
  - 输出：`fn get_user(id: u32) -> Option<User> { }`

### 8.4 AI 助手模式（AI Assistant）

- **目标**：整理成适合喂给 Claude Code / Codex 的高质量 prompt。
- **Prompt**：
  ```
  You rewrite the user's dictated request into a precise, structured prompt
  for an AI coding assistant. Preserve intent exactly. Add:
  - Context about the current file / selection (if provided)
  - Specific file paths or identifiers mentioned
  - Expected output form (code, explanation, plan)
  Format: 2-3 short paragraphs, plain English/Chinese, no markdown fences.
  ```
- **风险点**：**不可以**帮用户"做决定"。只能把杂乱口述拍成结构化请求。
- **示例**：
  - 输入：`帮我看下那个 auth 模块 有 bug 昨天改了之后好像 token 过期不续签了`
  - 输出：
    > 请排查 auth 模块 token 续签 bug。昨天的改动导致 token 过期后不再自动刷新。请：
    >
    > 1. 对比上一次 commit 的 diff
    > 2. 检查 refresh flow 是否被短路
    > 3. 指出最可能的 regression 并给出最小修复 diff

---

## 9. Claude Code / Codex 集成

这是 **xsay 最核心的差异化价值**。

### 9.1 架构

```
用户按 Hotkey + Mode=AiAssistant
      ↓
录音 → ASR → PostProcess (AI 整理 prompt)
      ↓
    根据 AiTarget 路由：
    ├── Claude Code → ClaudeCodeInjector
    ├── Codex        → CodexInjector
    └── Clipboard    → 只复制，不注入
```

### 9.2 Claude Code 集成

Claude Code 是终端 CLI，用户通常在终端里运行 `claude`。集成方案：

**方案 A（推荐）— tty 注入**：
- xsay 监控用户最近前台进程；发现 `claude` 时记录其 tty
- 注入通过 `ioctl(tty, TIOCSTI, ...)` 把字符塞进该 tty 的输入队列
- 不经过剪贴板，不模拟键盘，用户无感
- **Linux 特定**：新内核禁用了 TIOCSTI 未授权调用，需要 `sudo setcap cap_sys_admin+ep xsay` 或走下面方案 B

**方案 B — 剪贴板 + 模拟回车**：
- 安全兜底方案，跨平台可行
- 把文本复制到剪贴板 → 模拟 Ctrl+Shift+V → 可选模拟 Enter（用户可禁用）

**方案 C — stdin 管道**：
- 用户用 `xsay say | claude` 模式启动
- xsay 作为语音输入源，claude 作为消费者
- 适合"语音对话"交互，不适合穿插键盘输入

**推荐**：默认方案 B（安全 + 跨平台），Linux 提供方案 A 作为可选特性。

### 9.3 Codex 集成

Codex (GitHub Copilot CLI / OpenAI codex-cli) 通常也是 tty 交互。与 Claude Code 同构，复用 `TerminalPtyInjector` + `CodexInjector` 包装差异。

### 9.4 通用 CLI Adapter

```rust
pub trait CommandAdapter {
    fn name(&self) -> &str;
    fn detect(&self) -> Option<CliHandle>;     // 扫 /proc 找对应进程+tty
    fn inject(&self, handle: &CliHandle, text: &str, mode: InjectStyle) -> Result<()>;
}

pub enum InjectStyle {
    AppendNewline,          // 文本+回车
    NoNewline,              // 只文本
    Clipboard,              // 只剪贴板，不模拟
    SlashCommand(String),   // 走 /prefix，例如 Claude Code 的 /compact
}
```

每种 CLI 写一个适配器（`ClaudeCodeAdapter`, `CodexAdapter`, `AiderAdapter`...）。

### 9.5 语音命令模板

用户在配置里预设短语 → 映射到 slash 命令：

```toml
[voice_templates.claude_code]
"开启计划模式" = "/plan"
"压缩上下文" = "/compact"
"清空" = "/clear"
```

识别出的文本如果精确匹配模板，直接发命令而不是当提示词。

### 9.6 历史上下文增强

AI 助手模式下，把**最近 3 条**发送给 Claude Code 的内容（或剪贴板 recent text）加到 prompt context：

```
# 最近对话上下文
{{recent_prompts}}

# 用户当前请求（语音转写）
{{transcript}}
```

### 9.7 安全红线

- **绝不**自动发送 destructive slash command（`/clear`, `/reset`, `/exit`）除非预览确认
- **默认关闭**"识别完自动按 Enter"——用户必须显式开
- **默认**命令模式只复制剪贴板
- **冷却期**：同一文本 5 s 内只能注入一次（防卡键重放）

---

## 10. 安全设计

| 风险 | 机制 |
|---|---|
| API Key 泄露 | `keyring` crate + OS secure storage (Keychain/KWallet/CredMan) |
| 本地/远程切换 | 切到远程模型前弹"你的音频会发送到 {provider}" 隐私提示，只提一次 |
| 危险命令误执行 | 黑名单正则 + 预览强制 + 冷却期 |
| 注入到错误窗口 | 注入前 200 ms 读焦点窗口标题，若与"录音开始"时不同 → 改为剪贴板 |
| 敏感词 | 可配置 glossary 过滤列表 |
| 审计日志 | 所有注入动作写 `~/.local/share/xsay/audit.jsonl`：时间、目标 app、前 40 字 |
| 插件权限 | WASM 默认无文件/网络；外部进程插件必须在配置里显式声明 capability |
| 崩溃恢复 | panic hook → 写 panic log → 重启主循环（而非整个进程）当可能 |
| 敏感音频 | 录音默认仅在内存；不落盘除非调试模式 |

---

## 11. 性能优化

### 11.1 延迟预算（目标）

| 阶段 | MVP | 生产版 |
|---|---|---|
| Hotkey 响应 → 开始录音 | 30 ms | 10 ms |
| VAD 判断一段结束 | 800 ms (配置) | 500 ms |
| ASR 转写（base 模型 / 5s 音频） | 1500 ms | 300 ms (ONNX+GPU) |
| AI 后处理 | - | 200 ms (小模型本地) / 600 ms (远程) |
| 注入 | 100 ms | 40 ms |
| **端到端** | **2.5 s** | **< 1 s** |

### 11.2 优化手段（按 ROI 排序）

1. **更快的 ASR**：ONNX + faster-whisper 替换 whisper.cpp → 2-4× 提速
2. **模型预热**：进程启动后立即做一次 dummy encode，触发 mmap + kernel cache
3. **VAD 门控**：不让无意义静音进入 ASR
4. **流式 chunk**：30 s 的音频切成 5 s chunk 并行送，Hunk 完就出 partial
5. **后处理异步化**：先注入原文，AI polish 完了做 silent replace（如果用户没改动）
6. **UI 限帧**：Idle 状态 ≤ 5 fps 动画；Recording 60 fps
7. **零分配热路径**：所有音频缓冲使用 `Arc<[f32]>` 或对象池
8. **并行加载**：app 启动时异步加载模型，UI 立即可交互
9. **避免 kernel 唤醒**：ring buffer 用 busy-wait + yield 而不是 park（采集线程）

### 11.3 不适合做的优化

- **批处理**：实时场景一次一个 segment；批会增加延迟
- **超大 CPU 模型 + GPU fallback**：复杂度爆炸，选一条路
- **预测性预录**：侵入用户隐私，不做

---

## 12. 跨平台适配

### 12.1 能力矩阵

| 能力 | Linux X11 | Linux Wayland | macOS | Windows |
|---|---|---|---|---|
| 全局热键 | rdev | evdev (input 组) | CGEventTap (辅助功能授权) | LowLevelKeyboardProc |
| 文本注入 | enigo (xdotool) | ydotool / 剪贴板 | enigo (辅助功能授权) | SendInput API |
| 剪贴板 | arboard (x11) | arboard (wayland) | arboard | arboard |
| 聚焦窗口查询 | _NET_ACTIVE_WINDOW | **受限**（各 compositor 不同） | AXUIElement | GetForegroundWindow |
| 托盘 | tray-icon (AppIndicator) | 同左 | NSStatusBar | Shell_NotifyIcon |
| 麦克风权限 | 无 | 无 | TCC.db (首次询问) | Privacy settings |
| tty 注入 | TIOCSTI (需 CAP_SYS_ADMIN) | 同左 | 禁用（SIP） | ConPTY API |

### 12.2 platform abstraction 层

```rust
// crates/xsay-platform/src/lib.rs
pub trait Platform: Send + Sync {
    fn hotkey_manager(&self) -> Box<dyn HotkeyManager>;
    fn injector(&self) -> Box<dyn TextInjector>;
    fn focused_window(&self) -> Option<WindowInfo>;
    fn tray(&self) -> Box<dyn TraySystem>;
    fn request_mic_permission(&self) -> Result<bool>;
    fn request_accessibility(&self) -> Result<bool>;
}

pub fn current() -> Box<dyn Platform> {
    #[cfg(target_os = "linux")] return Box::new(linux::LinuxPlatform::new());
    #[cfg(target_os = "macos")] return Box::new(macos::MacPlatform::new());
    #[cfg(target_os = "windows")] return Box::new(windows::WinPlatform::new());
}
```

每平台实现细节在 `crates/xsay-platform/src/{linux,macos,windows}.rs`。

### 12.3 关键平台特定工作

- **macOS**：首次运行要求用户去"系统设置 → 隐私与安全性 → 辅助功能/麦克风"授权。xsay 启动时检测并引导。
- **Linux Wayland**：无法通过标准 API 拿聚焦窗口 → 默认禁用"按 app 切换 profile"，或通过 `gnome-shell` DBus 扩展（可选）。
- **Windows**：DPI 感知（per-monitor V2），`xsay.exe.manifest` 必须声明。

---

## 13. 测试方案

### 13.1 测试金字塔

```
            ▲
            │ E2E (少)    ──── tests/e2e/
            │              完整流程：wav 文件 → 识别 → 剪贴板断言
            │
            │ 集成 (中)    ──── tests/integration/
            │              模块组合：VAD + Mock ASR + Injector
            │
            │ 单元 (多)    ──── src/**/tests.rs
            │              纯函数、状态机、parser
            ▼
```

### 13.2 Mock 策略

| 模块 | 推荐 Mock |
|---|---|
| `AsrEngine` | `MockAsrEngine` 返回预设文本 + 可配置延迟 |
| `AudioSource` | `FileReplayAudioSource` 从 wav 回放 |
| `TextInjector` | `InMemoryInjector` 把文本存到 `Vec<String>` 供断言 |
| `HotkeyManager` | `TriggerableHotkeyManager` 测试代码直接调 callback |
| `Platform` | `MockPlatform` 组合上述 |

### 13.3 必须端到端

- Hotkey → 录音 → 识别 → 注入（hot-path）
- Escape 取消（不能注入）
- 暂停下载的断点续传
- 配置热更新

### 13.4 测试音频素材

- `tests/fixtures/audio/zh_short.wav` 5 s 中文
- `tests/fixtures/audio/en_short.wav` 5 s 英文
- `tests/fixtures/audio/mixed_code.wav` "function main"
- `tests/fixtures/audio/silence_20s.wav` 纯静音（VAD 测试）
- `tests/fixtures/audio/noise_party.wav` 嘈杂背景
- `tests/fixtures/audio/pause_at_mid.wav` 中间 2s 停顿

### 13.5 性能测试

- `criterion` bench：VAD push/frame、resample、whisper encode（不同模型）
- 稳定性：`cargo run --example soak` 连续 24 h 自动录音-识别循环，检查内存/延迟不退化

### 13.6 自动化

- GitHub Actions：Linux / macOS / Windows 三路
- 每 PR：`cargo test --workspace` + fmt + clippy
- Nightly：benchmark + soak，结果回写到 wiki

---

## 14. 发布方案

### 14.1 发布构件

| 构件 | 受众 | 分发 |
|---|---|---|
| `xsay-gui_0.1.0_amd64.deb` | Ubuntu/Debian | cargo-deb + apt repo（可选） |
| `xsay-gui_0.1.0.dmg` | macOS | create-dmg + notarize |
| `xsay-gui_0.1.0_x64.msi` | Windows | wix-rs |
| `xsay-cli_0.1.0_<os>_<arch>` | headless | tarball, GitHub Releases |

### 14.2 模型分发

- **不内置模型**（体积问题）
- 首次运行 UI 引导下载（走 HuggingFace）
- 企业版可镜像到内部 S3/NAS，`config.hf_mirror` 支持

### 14.3 自动更新

- **MVP**：不做（手动升级）
- **生产版**：`self_update` crate，GitHub Releases 拉 manifest，增量更新
- **企业版**：内部更新服务器 + 签名校验

### 14.4 崩溃日志收集

- 默认：本地写 `~/.local/share/xsay/crash/*.log`
- 商用版：opt-in sentry 上报（带脱敏）

### 14.5 版本策略

- **SemVer**：`MAJOR.MINOR.PATCH`
- **配置向后兼容**：新字段必须 `#[serde(default)]`
- **插件 ABI**：WASM 插件声明 ABI 版本，主程序检查不兼容直接拒绝加载

### 14.6 开发/内测/商用

| 阶段 | 渠道 | 标识 |
|---|---|---|
| Dev | 本地 `cargo run` | 无更新检查、verbose 日志 |
| Beta | GitHub Releases prereleases | 开 sentry，有 "Beta" 水印 |
| GA | GitHub Releases stable | 完整功能，可选 sentry |

---

## 15. 开发路线图

### Phase 1 — MVP（4 周）

**目标**：能跑通"按键 → 识别 → 注入"。

**交付**：
- xsay-gui 二进制（Linux X11）
- whisper.cpp 本地识别
- 简单 eframe 悬浮窗 + 设置
- 剪贴板注入

**难点**：whisper-rs 首次构建；cpal 回调与业务线程解耦；Ubuntu 运行时依赖。

**风险**：Wayland 用户无法使用（降级到 X11 提示）。

**工期**：约 4 周（1 人）。✅ 本仓库已达此阶段。

### Phase 2 — 流式 + Wayland（4 周）

**目标**：流式识别 + 真正跨平台（Wayland / macOS / Windows）。

**交付**：
- evdev 后端（Wayland）
- AsrEngine trait + RemoteHttpEngine 备选
- VAD 从"RMS 门限"升级到 Silero
- Session controller 重构为 tokio async task

**难点**：WhisperContext 非 Send 在 async 里的处理；取消语义；VAD 参数调优。

**风险**：Silero ONNX 模型下载（100 MB）增加首次启动成本。

**工期**：约 4 周。

### Phase 3 — 模式 + AI 后处理（6 周）

**目标**：四模式上线，AI 整理文本。

**交付**：
- Mode enum + UI 切换
- TextPostProcessor trait + 3 种实现
- Claude / OpenAI API 集成
- 命令黑名单 + 预览 UI

**难点**：Prompt 稳定性；确认交互不打断心流。

**风险**：远程 API 费用不可控 → 加 budget guard。

**工期**：约 6 周。

### Phase 4 — Claude Code / Codex（3 周）

**目标**：一等公民级 AI CLI 集成。

**交付**：
- TerminalPtyInjector
- ClaudeCodeAdapter / CodexAdapter
- 语音命令模板
- tty 检测

**难点**：TIOCSTI 权限；进程识别；Wayland 下的 tty 追踪。

**风险**：macOS SIP 禁用 TIOCSTI → 必须提供兜底方案。

**工期**：3 周。

### Phase 5 — 插件 + 商业化（6 周）

**目标**：可扩展 + 可上架。

**交付**：
- WASM 插件 runtime
- 签名 + 更新机制
- 账号（可选）
- 官网 + 文档
- Mac/Win 包签名

**难点**：WASM 沙箱 I/O；签名证书采购；国区分发合规。

**风险**：苹果 notarize 审核；微软 Defender 误杀。

**工期**：6 周。

### 总时间线

```
Phase 1 ─────────█████ (已完成)
Phase 2        ───────████
Phase 3                ──────██████
Phase 4                           ────███
Phase 5                                ──────██████
总计                                              ~5 个月
```

---

## 附录 A · 术语表

- **VAD** — Voice Activity Detection（语音活动检测）
- **ASR** — Automatic Speech Recognition（自动语音识别）
- **PCM** — Pulse Code Modulation（未压缩音频）
- **GGML** — whisper.cpp 的本地模型格式
- **TIOCSTI** — Unix 的 terminal input injection ioctl
- **Utterance** — 一次连续说话（一段语音单元）
- **Push-to-talk** — 按住说话；**Toggle** — 点按切换

## 附录 B · 参考项目

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp)
- [whisper-rs](https://github.com/tazz4843/whisper-rs)
- [cpal](https://github.com/RustAudio/cpal)
- [silero-vad](https://github.com/snakers4/silero-vad)
- [Superwhisper](https://superwhisper.com/) — 商业对标（macOS only）
- [Whishper](https://github.com/pluja/whishper) — 开源对标（浏览器端）
- [VoiceInk](https://github.com/Beingpax/VoiceInk) — 开源 macOS 听写工具
