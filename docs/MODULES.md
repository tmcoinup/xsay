# 模块说明

每个源文件的职责、对外接口、关键数据结构。按依赖从底到顶排列。

---

## `error.rs`

`XsayError` 统一错误枚举，`thiserror` 派生 `Display`。与 `anyhow::Result` 混用：库层用自定义错误（类型化），`main` 层用 `anyhow`（便于串联）。

---

## `config.rs`

- `Config`（根）→ `HotkeyConfig`、`AudioConfig`、`ModelConfig`、`TranscriptionConfig`、`OverlayConfig`、`InjectionConfig`
- `Config::load()`：读 `~/.config/xsay/config.toml`；不存在则用 `Default` 生成写盘
- `Config::config_path()`：返回配置文件路径

所有子结构都派生 `Serialize + Deserialize + Default + Clone`，字段 `pub`，以便 UI 直接读写。

---

## `state.rs`

```rust
pub enum AppState { Idle, Recording { started_at: Instant }, Transcribing, Injecting }
pub type SharedState = Arc<Mutex<AppState>>;
pub fn new_shared_state() -> SharedState;
```

只有这一个类型跨越所有线程。

---

## `history.rs`

JSONL 追加日志。公开：
- `append(&str)` — 每条 `HistoryEntry { timestamp: i64, text: String }` 写一行
- `load_recent(limit) -> Vec<HistoryEntry>` — 读尾部 N 条（newest first）
- `clear() -> io::Result<()>`
- `format_timestamp(ts) -> String` — Unix 秒 → 本地时间字符串（libc::localtime_r on Unix，UTC 回退）

存储位置：`~/.cache/xsay/history.jsonl`。

---

## `autostart.rs`

跨平台开机自启：
- Linux：`~/.config/autostart/xsay.desktop`
- macOS：`~/Library/LaunchAgents/com.xsay.plist`（`RunAtLoad=true`）
- Windows：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\xsay.bat`

API：`is_enabled() / enable() -> Result / disable() -> Result`。Exec 路径用 `std::env::current_exe()`。

---

## `model.rs`

模型文件管理（只负责磁盘/下载触发，不涉及 whisper-rs）：
- `find_local(&ModelConfig) -> Option<PathBuf>` — 纯检查，不下载
- `ensure_model(&ModelConfig) -> Result<PathBuf>` — `--download-model` CLI 用的阻塞下载

平时不调用 `ensure_model`——lazy loading，由用户在设置里显式点"下载"。

---

## `download.rs`

带暂停/取消/恢复的 HTTP 下载：
- `DownloadProgress { downloaded: AtomicU64, total: AtomicU64, state: Mutex<DlState> }`
- `DlState = Running | Paused | Cancelled | Completed | Failed(String)`
- `start_download(url, dest, Arc<Progress>) -> Sender<DownloadCmd>` — 启动线程，返回控制通道
- `check_remote_size(url, tx, fname)` — HEAD 请求拿远端大小
- `partial_path(p)` — `.partial` 后缀路径，用于断点续传
- `hf_url(repo, filename)` — HuggingFace URL 构造

底层用 `ureq` 做 Range 请求。

---

## `audio.rs`

cpal 音频采集 + 降采样 + 静音检测。
- `run_audio_thread(cmd_rx, chunk_tx, Arc<Mutex<AudioConfig>>)` — 主循环
- `AudioCmd = StartRecording | StopRecording | Abort`
- `AudioChunk { samples: Vec<f32>, is_final: bool, triggered_by_pause: bool }`
- `input_device_names() -> Vec<String>` — UI 查询
- `list_devices()` — `--list-devices` CLI

所有样本统一转 16 kHz mono f32。静音用 RMS < threshold 连续 N 个 1024-样本 chunk 判定。

---

## `transcribe.rs`

whisper-rs 封装。
- `run_transcribe_thread(req_rx, reload_rx, transcript_tx, Option<PathBuf>)` — 单线程，`select!` 在请求和模型重载之间
- `TranscribeReq { samples, language, n_threads, translate }`
- `TranscriptSeg { text }`

`WhisperContext` 非 Send，所以只在本线程构造。重载路径到来时 drop 旧上下文。

---

## `inject.rs`

文本注入。
- `run_inject_thread(cmd_rx, done_tx, Arc<Mutex<InjectionConfig>>)`
- `InjectCmd::Type(String)`
- 两种方式：
  - `clipboard` - `arboard::Clipboard::set_text()` + 模拟 Ctrl+V（推荐 CJK）
  - `type` - `enigo` 逐字符模拟（对 CJK 不稳）

---

## `hotkey.rs` (rdev, X11/Mac/Win)

- `run_hotkey_thread(tx, Arc<Mutex<HotkeyConfig>>, Arc<AtomicBool>)`
- `AppEvent = HotkeyPressed | HotkeyReleased | EscapePressed`
- `parse_key(&str) -> rdev::Key` — 名字到枚举映射（F1-F12, a-z, 特殊键）

关键点：区分硬件重复（OS auto-repeat）和真正按下；用本地 `held_keys` HashSet 过滤。`capture_active=true` 时整体让位给 UI。

---

## `hotkey_evdev.rs` (Linux Wayland)

直接读 `/dev/input/event*`。
- `is_wayland_session() -> bool` — `WAYLAND_DISPLAY + XDG_SESSION_TYPE=wayland`
- `spawn_hotkey_threads(tx, cfg, capture) -> Result<usize, String>` — 每个支持 KEY_ESC 的设备起一条线程

需要 `input` 组权限（`sudo usermod -aG input $USER`）。

---

## `tray.rs`

系统托盘图标 + 右键菜单。
- `spawn_in_background()` — Linux 下单独线程 `gtk::init() + gtk::main()`，其他平台普通线程
- `TrayAction = ShowSettings | Quit`
- `poll_events() -> Vec<TrayAction>` — UI 每帧拉取

菜单项程序化生成，图标是代码画的 32×32 RGBA 红色圆+白色话筒。

---

## `overlay.rs`

eframe::App 实现，主线程跑。
- `XsayOverlay` — 持有所有 `Arc<Mutex<...>>`、动画相位、settings 子 state
- 根据 `AppState` 切不同尺寸：90×30 徽章 / 120×120 动画
- `ViewportCommand::OuterPosition` 根据 `monitor_size + shared_position` 重新定位
- `show_viewport_immediate` 弹出设置窗
- 每帧 `tray::poll_events()`

- `build_native_options(&OverlayConfig)` — 启动时的 `NativeOptions`

---

## `settings_ui/`

设置窗口，按标签页拆文件：

| 文件 | 职责 |
|---|---|
| `mod.rs` | `Tab`、`SettingsState`、顶层 `render()` |
| `models.rs` | 静态 `MODELS` 数组 |
| `model_tab.rs` | 模型列表、下载控制、更新检查 |
| `hotkey_tab.rs` | 快捷键捕获、模式、修饰键 |
| `general_tab.rs` | 语言、注入、VAD 参数、自启动、浮层位置 |
| `history_tab.rs` | 历史条目查看/复制/清空 |

所有 tab 都接受 `&mut SettingsState`，直接读写其中的 `shared_*` 做即时生效。

---

## `main.rs`

- 解析 CLI（`--download-model / --list-devices / --config / --help`）
- 加载 config，初始化共享 Arc、channels
- 按平台选热键后端（evdev → rdev 回退）
- spawn 4 个 worker 线程（audio / transcribe / inject / coordinator）
- spawn tray
- 主线程跑 `eframe::run_native`

`coordinator_loop` 是唯一修改 `AppState` 的地方（除了 Escape 快捷路径），用 `crossbeam select!` 路由 4 个 channel。
