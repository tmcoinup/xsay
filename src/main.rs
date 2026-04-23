mod audio;
mod autostart;
mod config;
mod download;
mod error;
mod hotkey;
#[cfg(target_os = "linux")]
mod hotkey_evdev;
mod inject;
mod model;
mod overlay;
mod settings_ui;
mod state;
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
    );
    log::info!("Hotkey backend: {}", hotkey_backend);

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
    let native_options = overlay::build_native_options(&cfg.overlay);
    eframe::run_native(
        "xsay",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(overlay::XsayOverlay::new(
                shared_state,
                shared_hotkey,
                shared_audio,
                shared_inject,
                shared_transcription,
                shared_position,
                capture_active,
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
            let snap = tx_cfg.lock().clone();
            let _ = transcribe_req_tx.send(TranscribeReq {
                samples: chunk.samples,
                language: snap.language,
                n_threads: snap.n_threads,
                translate: snap.translate,
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

    // Only inject if we're in a transcribing or still recording (pause-triggered) state
    if matches!(state, AppState::Transcribing | AppState::Recording { .. }) {
        let text = seg.text.trim().to_string();
        if text.is_empty() {
            let mut s = shared_state.lock();
            if matches!(*s, AppState::Transcribing) {
                *s = AppState::Idle;
            }
            return;
        }

        {
            let mut s = shared_state.lock();
            *s = AppState::Injecting;
        }
        let _ = inject_tx.send(InjectCmd::Type(text));
        log::debug!("State → Injecting");
    }
}

fn spawn_hotkey_backend(
    hotkey_tx: crossbeam_channel::Sender<AppEvent>,
    shared_hotkey: Arc<Mutex<config::HotkeyConfig>>,
    capture_active: Arc<std::sync::atomic::AtomicBool>,
) -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if hotkey_evdev::is_wayland_session() {
            match hotkey_evdev::spawn_hotkey_threads(
                hotkey_tx.clone(),
                Arc::clone(&shared_hotkey),
                Arc::clone(&capture_active),
            ) {
                Ok(n) => {
                    log::info!(
                        "Wayland detected. Using evdev for global hotkeys ({} keyboard(s))",
                        n
                    );
                    return "evdev (Wayland)";
                }
                Err(e) => {
                    log::warn!("evdev unavailable on Wayland: {}", e);
                    eprintln!("⚠ Wayland 下 evdev 不可用：{}", e);
                    eprintln!(
                        "   执行以下命令后重新登录（或重启）以生效：sudo usermod -aG input $USER"
                    );
                    eprintln!("   暂时回退到 rdev (仅 X11/XWayland 应用可用)");
                }
            }
        }
    }

    std::thread::spawn(move || hotkey::run_hotkey_thread(hotkey_tx, shared_hotkey, capture_active));
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
    println!("  --download-model   Download the Whisper model and exit");
    println!("  --list-devices     List available audio input devices");
    println!("  --config           Print config file path");
    println!("  --help             Show this help");
    println!();
    println!("CONFIG:");
    println!("  Edit ~/.config/xsay/config.toml to customize hotkey, model, etc.");
}
