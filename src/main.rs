mod audio;
mod autostart;
mod config;
mod download;
mod error;
mod fonts;
mod history;
mod hotkey;
#[cfg(target_os = "linux")]
mod hotkey_evdev;
mod inject;
#[cfg(unix)]
mod ipc;
mod model;
mod overlay;
#[cfg(any(feature = "sensevoice", feature = "sensevoice-cuda"))]
mod sensevoice;
mod settings_ui;
mod state;
mod theme;
mod transcribe;
mod tray;

use audio::{AudioCmd, AudioChunk};
use config::Config;
use crossbeam_channel::select;
use hotkey::AppEvent;
use inject::InjectCmd;
use parking_lot::Mutex;
use state::{AppState, SharedState, new_shared_state};
use std::sync::{
    atomic::{AtomicBool},
    Arc,
};
use std::time::Instant;
use transcribe::{TranscribeReq, TranscriptSeg};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("xsay=info,warn"),
    )
    .init();

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "--help" | "-h" => {
            print_help();
            return Ok(());
        }
        "--config" => {
            let path = Config::config_path()?;
            println!("{}", path.display());
            return Ok(());
        }
        "--list-devices" => {
            audio::list_devices();
            return Ok(());
        }
        "--download-model" => {
            let cfg = Config::load()?;
            let path = model::ensure_model(&cfg.model)?;
            println!("Model ready at: {}", path.display());
            return Ok(());
        }
        // Flameshot-style IPC subcommands: short-lived invocations that
        // connect to the running daemon and trigger an action. This is the
        // recommended way to wire hotkeys on Wayland (bind
        // `xsay toggle` in GNOME Custom Shortcuts). Unix-only — Windows
        // doesn't have std::os::unix::net and wouldn't compile the module.
        #[cfg(unix)]
        "toggle" | "cancel" => {
            if let Err(e) = ipc::send_command(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            return Ok(());
        }
        _ => {}
    }

    // Wayland + X11 backends are both handled below; evdev is preferred on Wayland.

    let cfg = Config::load()?;
    log::info!("Config loaded. Hotkey: {}", cfg.hotkey.key);

    let model_path = model::find_local(&cfg.model);
    match &model_path {
        Some(p) => log::info!("Model ready: {}", p.display()),
        None => {
            log::info!("No model available at startup.");
            eprintln!("提示：当前没有可用模型，请点击右上角 xsay 图标打开设置下载。");
        }
    }

    let shared_state = new_shared_state();

    // Shared configs: hotkey thread, audio thread, inject thread, and coordinator
    // all read these on demand, while the settings UI writes to them for live updates.
    let shared_hotkey = Arc::new(Mutex::new(cfg.hotkey.clone()));
    let shared_audio = Arc::new(Mutex::new(cfg.audio.clone()));
    let shared_inject = Arc::new(Mutex::new(cfg.injection.clone()));
    let shared_transcription = Arc::new(Mutex::new(cfg.transcription.clone()));
    let shared_position = Arc::new(Mutex::new(cfg.overlay.position.clone()));
    let capture_active = Arc::new(AtomicBool::new(false));
    let capture_slot = hotkey::CaptureSlot::new();
    let backend_info = hotkey::BackendInfo::new();

    // Create all channels
    let (hotkey_tx, hotkey_rx) = crossbeam_channel::unbounded::<AppEvent>();
    let (audio_cmd_tx, audio_cmd_rx) = crossbeam_channel::unbounded::<AudioCmd>();
    let (audio_chunk_tx, audio_chunk_rx) = crossbeam_channel::unbounded::<AudioChunk>();
    let (transcribe_req_tx, transcribe_req_rx) = crossbeam_channel::unbounded::<TranscribeReq>();
    let (transcript_tx, transcript_rx) = crossbeam_channel::unbounded::<TranscriptSeg>();
    let (inject_tx, inject_rx) = crossbeam_channel::unbounded::<InjectCmd>();
    let (inject_done_tx, inject_done_rx) = crossbeam_channel::unbounded::<()>();
    let (model_reload_tx, model_reload_rx) =
        crossbeam_channel::unbounded::<std::path::PathBuf>();

    // Prefer evdev on Wayland, fall back to rdev (X11) on X11 or if evdev fails
    let hotkey_backend = spawn_hotkey_backend(
        hotkey_tx.clone(),
        Arc::clone(&shared_hotkey),
        Arc::clone(&capture_active),
        Arc::clone(&capture_slot),
        Arc::clone(&backend_info),
    );
    log::info!("Hotkey backend: {}", hotkey_backend);

    // IPC socket for `xsay toggle` / `xsay cancel` — lets users bind a
    // system-level shortcut (GNOME Custom Shortcuts, etc.) that spawns our
    // CLI, which hands the command to this daemon. Works regardless of
    // Wayland/X11 and doesn't need /dev/input access. Unix-only.
    #[cfg(unix)]
    ipc::spawn_server(hotkey_tx.clone(), Arc::clone(&shared_state));

    {
        let aud = Arc::clone(&shared_audio);
        std::thread::spawn(move || audio::run_audio_thread(audio_cmd_rx, audio_chunk_tx, aud));
    }

    {
        let mp = model_path.clone();
        std::thread::spawn(move || {
            transcribe::run_transcribe_thread(
                transcribe_req_rx,
                model_reload_rx,
                transcript_tx,
                mp,
            )
        });
    }

    {
        let inj = Arc::clone(&shared_inject);
        std::thread::spawn(move || inject::run_inject_thread(inject_rx, inject_done_tx, inj));
    }

    // Coordinator on a dedicated thread (main thread is reserved for eframe/GUI)
    {
        let coord_state = Arc::clone(&shared_state);
        let tx_cfg = Arc::clone(&shared_transcription);
        std::thread::spawn(move || {
            coordinator_loop(
                coord_state,
                tx_cfg,
                hotkey_rx,
                audio_cmd_tx,
                audio_chunk_rx,
                transcribe_req_tx,
                transcript_rx,
                inject_tx,
                inject_done_rx,
            );
        });
    }

    // Tray icon runs on its own GTK thread (Linux) / spawned thread (other OS).
    tray::spawn_in_background();

    eprintln!(
        "xsay running. Hold {} to record, release to transcribe, Escape to cancel.",
        cfg.hotkey.key
    );

    // Overlay on main thread (required by macOS and Windows)
    let mut native_options = overlay::build_native_options(&cfg.overlay);

    // On Linux + Wayland, GNOME/mutter silently ignores the Wayland protocol
    // extensions we rely on for positioning a transparent always-on-top
    // overlay — the feedback widget lands wherever the compositor decides
    // (usually top-left) instead of the configured corner. Force the GUI
    // event loop onto X11 so OuterPosition commands actually take effect.
    // Other subsystems (evdev, arboard, notify-send, the injection path)
    // keep reading WAYLAND_DISPLAY and run on their native Wayland code
    // paths — only eframe's window system is forced to XWayland.
    //
    // Override with `XSAY_GUI=wayland` for users on wlroots compositors
    // where OuterPosition actually works.
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let force_wayland = std::env::var("XSAY_GUI")
            .map(|v| v == "wayland")
            .unwrap_or(false);
        if !force_wayland {
            native_options.event_loop_builder =
                Some(Box::new(|builder| {
                    builder.with_x11();
                }));
            log::info!(
                "GUI: forcing X11 (XWayland) for accurate overlay positioning. \
                 Override with XSAY_GUI=wayland."
            );
        }
    }

    eframe::run_native(
        "xsay",
        native_options,
        Box::new(move |cc| {
            fonts::install(&cc.egui_ctx);
            Ok(Box::new(overlay::XsayOverlay::new(
                shared_state,
                shared_hotkey,
                shared_audio,
                shared_inject,
                shared_transcription,
                shared_position,
                capture_active,
                capture_slot,
                backend_info,
                model_reload_tx,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {:?}", e))?;

    Ok(())
}

fn coordinator_loop(
    shared_state: SharedState,
    tx_cfg: Arc<Mutex<config::TranscriptionConfig>>,
    hotkey_rx: crossbeam_channel::Receiver<AppEvent>,
    audio_cmd_tx: crossbeam_channel::Sender<AudioCmd>,
    audio_chunk_rx: crossbeam_channel::Receiver<AudioChunk>,
    transcribe_req_tx: crossbeam_channel::Sender<TranscribeReq>,
    transcript_rx: crossbeam_channel::Receiver<TranscriptSeg>,
    inject_tx: crossbeam_channel::Sender<InjectCmd>,
    inject_done_rx: crossbeam_channel::Receiver<()>,
) {
    loop {
        select! {
            recv(hotkey_rx) -> ev => {
                let ev = match ev { Ok(e) => e, Err(_) => break };
                handle_hotkey_event(ev, &shared_state, &audio_cmd_tx);
            }
            recv(audio_chunk_rx) -> chunk => {
                let chunk = match chunk { Ok(c) => c, Err(_) => break };
                handle_audio_chunk(chunk, &shared_state, &transcribe_req_tx, &tx_cfg);
            }
            recv(transcript_rx) -> seg => {
                let seg = match seg { Ok(s) => s, Err(_) => break };
                handle_transcript(seg, &shared_state, &inject_tx);
            }
            recv(inject_done_rx) -> _ => {
                let mut state = shared_state.lock();
                if matches!(*state, AppState::Injecting) {
                    *state = AppState::Idle;
                    log::debug!("Injection complete, back to Idle");
                }
            }
        }
    }
}

fn handle_hotkey_event(
    ev: AppEvent,
    shared_state: &SharedState,
    audio_cmd_tx: &crossbeam_channel::Sender<AudioCmd>,
) {
    match ev {
        AppEvent::HotkeyPressed => {
            let mut state = shared_state.lock();
            if matches!(*state, AppState::Idle) {
                *state = AppState::Recording { started_at: Instant::now() };
                drop(state);
                let _ = audio_cmd_tx.send(AudioCmd::StartRecording);
                log::debug!("State → Recording");
            }
        }
        AppEvent::HotkeyReleased => {
            let mut state = shared_state.lock();
            if matches!(*state, AppState::Recording { .. }) {
                *state = AppState::Transcribing;
                drop(state);
                let _ = audio_cmd_tx.send(AudioCmd::StopRecording);
                log::debug!("State → Transcribing (key release)");
            }
        }
        AppEvent::EscapePressed => {
            let mut state = shared_state.lock();
            let was_active = !matches!(*state, AppState::Idle);
            *state = AppState::Idle;
            drop(state);
            if was_active {
                let _ = audio_cmd_tx.send(AudioCmd::Abort);
                // Drain any pending transcription requests
                log::debug!("State → Idle (cancelled)");
            }
        }
    }
}

fn handle_audio_chunk(
    chunk: AudioChunk,
    shared_state: &SharedState,
    transcribe_req_tx: &crossbeam_channel::Sender<TranscribeReq>,
    tx_cfg: &Arc<Mutex<config::TranscriptionConfig>>,
) {
    let state = shared_state.lock().clone();

    let should_transcribe = match state {
        AppState::Transcribing => chunk.is_final,
        AppState::Recording { .. } => chunk.triggered_by_pause,
        _ => false,
    };

    if should_transcribe && !chunk.samples.is_empty() {
        // Minimum length check: at least 0.5s of audio at 16kHz
        if chunk.samples.len() >= 8000 {
            // Backpressure: if Whisper is slow (Medium/Large on CPU) the
            // pause-triggered chunks pile up faster than they're consumed.
            // Drop new pause chunks when anything is already in flight —
            // the final chunk on key-release always goes through.
            if chunk.triggered_by_pause && transcribe_req_tx.len() > 0 {
                log::debug!(
                    "Transcribe queue backed up ({} pending); dropping pause chunk",
                    transcribe_req_tx.len()
                );
                return;
            }
            let snap = tx_cfg.lock().clone();
            let _ = transcribe_req_tx.send(TranscribeReq {
                samples: chunk.samples,
                language: snap.language,
                n_threads: snap.n_threads,
                translate: snap.translate,
                backend: snap.backend,
            });

            // If triggered by pause, stay in Recording state (key still held)
            if !chunk.triggered_by_pause {
                // Stay in Transcribing state, waiting for result
            }
        } else {
            log::debug!("Audio too short, skipping transcription");
            let mut s = shared_state.lock();
            if matches!(*s, AppState::Transcribing) {
                *s = AppState::Idle;
            }
        }
    }
}

fn handle_transcript(
    seg: TranscriptSeg,
    shared_state: &SharedState,
    inject_tx: &crossbeam_channel::Sender<InjectCmd>,
) {
    let state = shared_state.lock().clone();

    // Drop transcripts only when the user has explicitly canceled (Idle).
    // In every other state (Recording with pause-trigger mid-session,
    // Transcribing on key release, Injecting while a previous part is
    // still being pasted) we want to queue this segment for injection —
    // a long dictation produces multiple chunks and they must all reach
    // the focused window in order. Previously we guarded with
    // `Transcribing | Recording` which silently dropped every segment
    // after the first one.
    if matches!(state, AppState::Idle) {
        return;
    }

    let text = seg.text.trim().to_string();
    if text.is_empty() {
        // No text to inject. If nothing is currently injecting, return to
        // Idle so a subsequent hotkey press starts a fresh session.
        let mut s = shared_state.lock();
        if matches!(*s, AppState::Transcribing) {
            *s = AppState::Idle;
        }
        return;
    }

    history::append(&text);

    // Transition to Injecting only if we're not already there — avoids a
    // redundant write that would stomp an in-progress Injecting state and
    // confuse the inject_done_rx handler's Idle flip.
    {
        let mut s = shared_state.lock();
        if !matches!(*s, AppState::Injecting) {
            *s = AppState::Injecting;
            log::debug!("State → Injecting");
        } else {
            log::debug!("Queuing additional inject while previous still running");
        }
    }
    let _ = inject_tx.send(InjectCmd::Type(text));
}

fn spawn_hotkey_backend(
    hotkey_tx: crossbeam_channel::Sender<AppEvent>,
    shared_hotkey: Arc<Mutex<config::HotkeyConfig>>,
    capture_active: Arc<std::sync::atomic::AtomicBool>,
    capture_slot: Arc<hotkey::CaptureSlot>,
    backend_info: Arc<hotkey::BackendInfo>,
) -> &'static str {
    #[cfg(target_os = "linux")]
    let mut evdev_error: Option<String> = None;

    #[cfg(target_os = "linux")]
    {
        if hotkey_evdev::is_wayland_session() {
            match hotkey_evdev::spawn_hotkey_threads(
                hotkey_tx.clone(),
                Arc::clone(&shared_hotkey),
                Arc::clone(&capture_active),
                Arc::clone(&capture_slot),
            ) {
                Ok(n) => {
                    log::info!(
                        "Wayland detected. Using evdev for global hotkeys ({} keyboard(s))",
                        n
                    );
                    *backend_info.backend.lock() =
                        Some(hotkey::Backend::EvdevWayland { devices: n });
                    return "evdev (Wayland)";
                }
                Err(e) => {
                    log::warn!("evdev unavailable on Wayland: {}", e);
                    eprintln!("⚠ Wayland 下 evdev 不可用：{}", e);
                    eprintln!(
                        "   执行以下命令后重新登录（或重启）以生效：sudo usermod -aG input $USER"
                    );
                    eprintln!("   暂时回退到 rdev (仅 X11/XWayland 应用可用)");
                    evdev_error = Some(e);
                }
            }
        }
    }

    std::thread::spawn(move || {
        hotkey::run_hotkey_thread(hotkey_tx, shared_hotkey, capture_active, capture_slot)
    });

    #[cfg(target_os = "linux")]
    {
        *backend_info.backend.lock() = if hotkey_evdev::is_wayland_session() {
            Some(hotkey::Backend::RdevWaylandFallback {
                evdev_error: evdev_error.unwrap_or_default(),
            })
        } else {
            Some(hotkey::Backend::RdevX11)
        };
    }
    #[cfg(not(target_os = "linux"))]
    {
        *backend_info.backend.lock() = Some(hotkey::Backend::RdevX11);
    }
    "rdev (X11)"
}

fn print_help() {
    println!("xsay - AI voice input tool");
    println!();
    println!("USAGE:");
    println!("  xsay [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("  (no args)          Start xsay (hold hotkey to record)");
    #[cfg(unix)]
    {
        println!("  toggle             Toggle recording on the running daemon (for custom shortcuts)");
        println!("  cancel             Abort an in-flight session on the running daemon");
    }
    println!("  --download-model   Download the Whisper model and exit");
    println!("  --list-devices     List available audio input devices");
    println!("  --config           Print config file path");
    println!("  --help             Show this help");
    println!();
    println!("CONFIG:");
    println!("  Edit ~/.config/xsay/config.toml to customize hotkey, model, etc.");
    #[cfg(unix)]
    {
        println!();
        println!("CUSTOM SHORTCUTS (推荐):");
        println!("  GNOME: Settings → Keyboard → Custom Shortcuts, 命令填 `xsay toggle`");
        println!("  绑定任意组合键（Super+Z 等），由系统派发，跨 X11/Wayland 都可用。");
    }
}
