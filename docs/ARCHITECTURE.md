# xsay 架构总览

本文描述 xsay v0.1 的线程模型、数据流和模块依赖。面向想深入了解 / 二次开发的工程师。

## 一图看懂

```
                    ┌─────────────────────────────────────┐
                    │  main thread: eframe / egui         │
                    │  ┌─────────────────────────────────┐│
                    │  │ XsayOverlay (主 viewport = 浮层)││
                    │  │  ├── Idle PNG / 识别中动画      ││
                    │  │  └── Settings 子 viewport       ││
                    │  │      (按需 show_viewport_imm.)  ││
                    │  └─────────────────────────────────┘│
                    └──────────────────▲──────────────────┘
                                       │ SharedState (Arc<Mutex<AppState>>)
                                       │ Arc<Mutex<各 Config>>
                                       │ tray::ACTIONS (mpsc)
                                       │
        ┌──────────────┐   hotkey_rx   │
        │ hotkey 线程  │ ────────────► │
        │ Linux: evdev │               │
        │ macOS/Win:rdev               │
        └──────────────┘               │
                                       ▼
                        ┌──────────────────────────────┐
                        │  coordinator 线程            │
                        │  crossbeam select! 路由      │
                        │   - hotkey_rx                │
                        │   - audio_chunk_rx           │
                        │   - transcript_rx            │
                        │   - inject_done_rx           │
                        └─┬────┬────────┬────────┬─────┘
            audio_cmd_tx  │    │        │        │ inject_tx
                          ▼    │        │        ▼
                   ┌────────┐  │        │   ┌────────┐
                   │ audio  │  │        │   │ inject │
                   │ 线程   │──┘        │   │ 线程   │
                   │ cpal   │ audio_chunk│  │ enigo/ │──► 光标处文本
                   └────────┘            │  │ uinput │
                                         │  └────────┘
                          transcribe_req │
                                  ▼      │
                              ┌────────┐ │
                              │transcr.│─┘
                              │ 线程   │   transcript_tx
                              │sherpa+ │
                              │whisper │
                              └────────┘

                    ┌──────────────────────┐
                    │ tray 线程 (Linux:    │ ─► tray::ACTIONS
                    │   ksni / SNI / zbus) │    (ShowSettings | Quit)
                    │ Win: Shell_NotifyIcon│
                    └──────────────────────┘
                       (no GTK / no main thread requirement)
```

## 线程清单

| 线程 | 启动位置 | 阻塞行为 | 通信 |
|---|---|---|---|
| main | `fn main` | eframe 事件循环 | 持有所有 Arc；轮询 tray::poll_events() |
| coordinator | `main.rs` 里 `thread::spawn` | `select!` 多路复用 | 桥接所有业务通道 |
| hotkey (evdev) | Linux：每个键盘设备一条 | `device.fetch_events()` 阻塞 | 发 `AppEvent` |
| hotkey (rdev) | macOS：CGEventTap / Windows：SetWindowsHookEx | `rdev::listen` 阻塞 | 同上 |
| audio | `main.rs` | `crossbeam::recv` + cpal 回调 | 收 `AudioCmd`，发 `AudioChunk` |
| transcribe | `main.rs` | `select!` (req + reload) | sherpa-onnx / whisper-rs 推理，CPU 密集 |
| inject | `main.rs` | `crossbeam::recv` | 收 `InjectCmd`，发 `()` done |
| tray (Linux) | `tray.rs` 子线程 | ksni 内部 zbus 事件循环 | `tray::ACTIONS` mpsc |
| tray (Windows) | `tray.rs` 主线程 build + 事件 worker | tray-icon 自管 WM_USER | 同上 |
| ipc | `ipc.rs` | UnixListener::accept() 阻塞 | 接 `xsay toggle/press/release/cancel/show/quit` |
| download | 临时，按需 | ureq 阻塞 IO | 状态通过 `DownloadProgress` 共享 |

## 共享状态

| 名字 | 类型 | 用途 |
|---|---|---|
| `shared_state` | `Arc<Mutex<AppState>>` | Idle / Recording / Transcribing / Injecting |
| `shared_hotkey` | `Arc<Mutex<HotkeyConfig>>` | 快捷键/修饰键/模式，UI 写 → hotkey 线程读 |
| `shared_audio` | `Arc<Mutex<AudioConfig>>` | 静音阈值等，UI 写 → audio 线程读 |
| `shared_inject` | `Arc<Mutex<InjectionConfig>>` | 注入方式、剪贴板延迟 |
| `shared_transcription` | `Arc<Mutex<TranscriptionConfig>>` | 语言、n_threads、translate |
| `shared_position` | `Arc<Mutex<String>>` | 浮层角落位置 |
| `capture_active` | `Arc<AtomicBool>` | UI 捕获新快捷键时屏蔽 hotkey 线程 |

## 状态机

```
Idle ──按/触发──▶ Recording
Recording ──松开 / 停顿 1.5s──▶ Transcribing
Recording ──Esc──▶ Idle (丢弃音频)
Transcribing ──识别完──▶ Injecting
Transcribing ──Esc──▶ Idle
Injecting ──完成──▶ Idle
```

`Recording` 状态下若触发停顿（chunk.triggered_by_pause=true），会送 `TranscribeReq` 但不切状态——允许"边说边出字"。

## 数据流（典型一轮）

1. 用户按 F2 → evdev/rdev 发 `AppEvent::HotkeyPressed`
2. coordinator 收到 → 改 `AppState = Recording` → 发 `AudioCmd::StartRecording`
3. audio 线程启动 cpal 流，把 f32 样本累积到 16 kHz mono buffer
4. UI 线程读 `AppState`，绘制脉动话筒动画
5. 用户松开 → `HotkeyReleased` → coordinator 改 state 为 `Transcribing` → 发 `AudioCmd::StopRecording`
6. audio 线程发 `AudioChunk { is_final: true, samples }`
7. coordinator 用当前 `TranscriptionConfig` snapshot 构造 `TranscribeReq` 发给 whisper 线程
8. whisper 线程跑 `state.full(params, samples)`，发 `TranscriptSeg { text }`
9. coordinator 收到非空文本 → 追加到 history → 改 state 为 `Injecting` → 发 `InjectCmd::Type(text)`
10. inject 线程剪贴板设文本 → 模拟 Ctrl+V → 发 `inject_done_tx`
11. coordinator 收 done → state 回 `Idle`

## 模块依赖图

```
main ─┬─► config
      ├─► model (初始文件查找)
      ├─► autostart
      ├─► history
      ├─► state
      ├─► audio
      ├─► hotkey (+ hotkey_evdev on Linux)
      ├─► transcribe
      ├─► inject
      ├─► tray
      └─► overlay ─┬─► settings_ui ─► (各 tab)
                   │                    └─► download
                   └─► state

settings_ui 各子模块 ─► config / history / autostart / download / audio
```

没有循环依赖。`config` 是叶子模块（除 error 外不依赖任何其他本项目模块）。

## 为什么这么分层

- **eframe 必须主线程**：macOS/Cocoa 要求 UI 在主线程。所有业务逻辑都必须在其他线程。
- **WhisperContext 非 Send**：whisper-rs 的上下文不能跨线程移动，所以在 transcribe 线程内部构造，通过 channel 跨线程。
- **Linux 上只用 evdev，不用 rdev**：rdev 在 Linux 实现里通过 X11 XRecord 扩展抓事件，和 eframe winit 同进程的 X11 窗口抢全局锁，会让 mutter 间歇性死锁（见 commit 2a23853）。evdev 直接读 `/dev/input/event*`，绕开 X server，对 X11 / Wayland 都通用。代价是用户得在 `input` 组里——`.deb` 的 postinst 自动加。
- **tray 不依赖 GTK**：旧版本 `tray-icon` 在 Linux 必须 `gtk::init() + gtk::main()` 在专属线程，和 eframe winit 抢 X11 全局锁会偶发卡死整个进程（commit 8942a1f）。0.1.30 起 Linux 走 ksni，纯 zbus 实现 StatusNotifierItem 协议；Windows 仍用 tray-icon（在那边底层是 Shell_NotifyIcon，和 GTK 无关）。
- **coordinator 单点路由**：所有状态转换只发生在这一个线程里，避免竞态。UI 和 worker 线程只读/写 shared config，永远不直接改 `AppState`（Escape 的快捷路径除外，但它只写 Idle，不会与 coordinator 起冲突）。

## 配置即时生效

UI 改动写回两处：
1. `Arc<Mutex<Config 结构>>`：worker 线程每轮/每个事件重新 `lock().clone()`，拿到最新值
2. `config.toml`：持久化到磁盘

所以改静音阈值、快捷键、注入方式、语言、线程数等不需要重启。唯一例外：模型切换——必须通过 `model_reload_tx` 发路径给 transcribe 线程重新 `load_model()`。

## 非目标

- 不做流式识别（Whisper ggml 本身支持但延迟收益不明显，复杂度高）
- 不做 GPU 推理（whisper-rs CPU 后端够用；GPU 需 bindgen + CUDA/Metal，构建复杂）
- 不做 IPC（单实例进程；多窗口合并到单进程）
