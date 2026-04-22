use crate::config::InjectionConfig;
use arboard::Clipboard;
use crossbeam_channel::{Receiver, Sender};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub enum InjectCmd {
    Type(String),
}

pub fn run_inject_thread(
    inject_rx: Receiver<InjectCmd>,
    done_tx: Sender<()>,
    shared_config: Arc<Mutex<InjectionConfig>>,
) {
    loop {
        let cmd = match inject_rx.recv() {
            Ok(c) => c,
            Err(_) => break,
        };

        match cmd {
            InjectCmd::Type(text) => {
                if text.is_empty() {
                    let _ = done_tx.send(());
                    continue;
                }
                log::debug!("Injecting text: {:?}", text);

                let cfg = shared_config.lock().clone();
                match cfg.method.as_str() {
                    "type" => inject_via_keystrokes(&text),
                    _ => inject_via_clipboard(&text, cfg.clipboard_delay_ms),
                }

                let _ = done_tx.send(());
            }
        }
    }
}

fn inject_via_clipboard(text: &str, delay_ms: u64) {
    // Save current clipboard content to restore later
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to open clipboard: {}", e);
            return;
        }
    };

    let prev_text = clipboard.get_text().ok();

    if let Err(e) = clipboard.set_text(text) {
        log::error!("Failed to set clipboard: {}", e);
        return;
    }

    // Brief delay so clipboard contents settle
    std::thread::sleep(Duration::from_millis(delay_ms));

    // Send Ctrl+V
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to create enigo: {}", e);
            return;
        }
    };

    let _ = enigo.key(Key::Control, Direction::Press);
    std::thread::sleep(Duration::from_millis(10));
    let _ = enigo.key(Key::Unicode('v'), Direction::Click);
    std::thread::sleep(Duration::from_millis(10));
    let _ = enigo.key(Key::Control, Direction::Release);

    // Small delay then restore clipboard
    std::thread::sleep(Duration::from_millis(100));
    if let Some(prev) = prev_text {
        let _ = clipboard.set_text(prev);
    }
}

fn inject_via_keystrokes(text: &str) {
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to create enigo: {}", e);
            return;
        }
    };

    if let Err(e) = enigo.text(text) {
        log::error!("Failed to type text: {}", e);
        // Fall back to clipboard
        inject_via_clipboard(text, 80);
    }
}
