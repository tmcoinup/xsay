//! Unix-socket IPC between the running xsay daemon and short-lived CLI
//! invocations like `xsay toggle`. This is the "Flameshot pattern": instead
//! of xsay listening for global hotkeys itself (which is unreliable on
//! Wayland without /dev/input access), the user registers `xsay toggle` as
//! a system-level custom shortcut — the compositor handles the chord and
//! spawns the command, which we route to the daemon over this socket.
//!
//! Protocol: one line per connection, UTF-8, no framing. Known commands:
//!   - `toggle`  — flip Recording ↔ Idle (via HotkeyPressed / HotkeyReleased
//!                 depending on current state). In `hold` mode this still
//!                 effectively toggles because GNOME shortcuts fire once per
//!                 chord press.
//!   - `press`   — unconditionally send HotkeyPressed (start recording).
//!                 Pair with `release` if your binding tool can fire on key
//!                 down + key up separately (xbindkeys, evdev). GNOME's
//!                 stock custom shortcuts can't, which is why hold-to-talk
//!                 over plain `xsay toggle` is fundamentally a tap-toggle.
//!   - `release` — unconditionally send HotkeyReleased (stop & transcribe).
//!   - `cancel`  — send EscapePressed (abort any in-flight session).
//!   - `show`    — focus the already-running settings window.
//!   - `quit`    — graceful shutdown of the daemon.
//!   - `ping`    — no-op health check used by background autostart.

use crate::hotkey::AppEvent;
use crate::state::{AppState, SharedState};
use crossbeam_channel::Sender;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

pub struct InstanceGuard {
    #[allow(dead_code)]
    file: File,
}

pub fn socket_path() -> PathBuf {
    // XDG_RUNTIME_DIR is the canonical per-user runtime path on Linux
    // (systemd-logind creates /run/user/$UID and exports it). Fall back to
    // /tmp on distros that don't set it — the socket file permissions will
    // be u=rw only, so co-tenancy on /tmp is acceptable.
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join("xsay.sock")
}

pub fn lock_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join("xsay.lock")
}

/// Remove per-session IPC files after a normal shutdown.
///
/// The advisory flock itself is released by the OS when the process exits,
/// but leaving the socket file around makes the next launch attempt an
/// unnecessary stale IPC connection before it can become the daemon.
pub fn cleanup_runtime_files() {
    let _ = std::fs::remove_file(socket_path());
    let _ = std::fs::remove_file(lock_path());
}

/// Try to become the single long-lived xsay daemon for this user.
///
/// The lock is advisory and tied to the process file descriptor, so it is
/// released automatically on crash/exit. This is deliberately separate from
/// the IPC socket: old versions unconditionally removed the socket at startup,
/// which let a second process steal the command endpoint and create duplicate
/// tray icons/hotkey listeners.
pub fn acquire_instance_lock() -> Result<Option<InstanceGuard>, String> {
    let path = lock_path();
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("无法打开实例锁 {}：{}", path.display(), e))?;

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        file.set_len(0)
            .map_err(|e| format!("无法写入实例锁 {}：{}", path.display(), e))?;
        writeln!(&file, "{}", std::process::id())
            .map_err(|e| format!("无法写入实例锁 {}：{}", path.display(), e))?;
        return Ok(Some(InstanceGuard { file }));
    }

    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) || err.raw_os_error() == Some(libc::EAGAIN) {
        Ok(None)
    } else {
        Err(format!("无法锁定实例锁 {}：{}", path.display(), err))
    }
}

/// Client side — called from the short-lived `xsay toggle` / `xsay cancel`
/// invocation. Connects to the daemon, sends the command, exits. Returns
/// Err with a user-readable message if the daemon isn't running.
pub fn send_command(cmd: &str) -> Result<(), String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| {
        format!(
            "xsay 未运行（连接 {} 失败：{}）。先启动 xsay，再执行快捷键。",
            path.display(),
            e
        )
    })?;
    // Short write timeout — the daemon either acks instantly or something
    // is deeply wrong; don't let a user's hotkey press hang forever.
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    stream
        .write_all(cmd.as_bytes())
        .map_err(|e| format!("发送命令失败：{}", e))?;
    stream
        .write_all(b"\n")
        .map_err(|e| format!("发送命令失败：{}", e))?;
    Ok(())
}

/// Server side — spawn on daemon startup. Binds the socket (removing any
/// stale file from a prior unclean shutdown) and routes received commands
/// through `event_tx`. Runs forever; on bind failure logs and returns so
/// the rest of the app can continue without IPC.
pub fn spawn_server(event_tx: Sender<AppEvent>, shared_state: SharedState) {
    std::thread::spawn(move || {
        let path = socket_path();
        // Stale socket from a prior crash? Remove so bind can succeed.
        // If another instance is actually running, it will already hold the
        // socket file busy and a subsequent bind would still fail — harmless.
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::error!("IPC socket bind failed at {}: {}", path.display(), e);
                return;
            }
        };
        log::info!("IPC listening on {}", path.display());

        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = String::new();
            // Read a short command; cap to protect against a malicious
            // client piping endless data into our daemon.
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = (&mut stream).take(128).read_to_string(&mut buf);
            let cmd = buf.trim();
            if cmd.is_empty() {
                continue;
            }
            log::debug!("IPC received: {:?}", cmd);
            dispatch(cmd, &event_tx, &shared_state);
        }
    });
}

fn dispatch(cmd: &str, event_tx: &Sender<AppEvent>, shared_state: &SharedState) {
    match cmd {
        "toggle" => {
            let state = shared_state.lock().clone();
            let ev = match state {
                AppState::Idle => AppEvent::HotkeyPressed,
                AppState::Recording { .. } => AppEvent::HotkeyReleased,
                // Already transcribing or injecting — the user's press was
                // probably impatient; drop it rather than double-handle.
                AppState::Transcribing | AppState::Injecting => return,
            };
            let _ = event_tx.send(ev);
        }
        // Unconditional press / release — the IPC half of hold-to-talk.
        // Idempotent against the current state so re-sends from a flaky
        // binding don't get the daemon stuck. Press from non-idle is
        // silently dropped; release from non-recording is also dropped.
        "press" => {
            if matches!(*shared_state.lock(), AppState::Idle) {
                let _ = event_tx.send(AppEvent::HotkeyPressed);
            }
        }
        "release" => {
            if matches!(*shared_state.lock(), AppState::Recording { .. }) {
                let _ = event_tx.send(AppEvent::HotkeyReleased);
            }
        }
        "cancel" => {
            let _ = event_tx.send(AppEvent::EscapePressed);
        }
        "show" => {
            crate::tray::request_show_settings();
        }
        "quit" => {
            crate::tray::request_quit();
        }
        "ping" => {}
        other => {
            log::warn!("Unknown IPC command: {:?}", other);
        }
    }
}
