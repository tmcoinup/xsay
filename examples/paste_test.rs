//! Isolated paste-pipeline test. Reproduces what xsay's inject thread does
//! on Wayland: create a persistent uinput virtual keyboard, wait, then emit
//! Ctrl+V followed by Ctrl+Shift+V so we can see which one actually lands
//! in the focused window.
//!
//! Run after focusing a target text field:
//!   cargo run --example paste_test
//!
//! The test sets the clipboard to "XSAY-PASTE-TEST-<timestamp>" so you can
//! distinguish what our pipeline produced from any pre-existing clipboard
//! content.

use std::time::{Duration, SystemTime};

fn main() {
    let stamp = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = format!("XSAY-PASTE-TEST-{}", stamp);

    eprintln!("[1/5] setting clipboard to: {}", payload);
    let mut cb = arboard::Clipboard::new().expect("open clipboard");
    cb.set_text(&payload).expect("set clipboard");

    eprintln!("[2/5] creating uinput virtual keyboard");
    let mut keys = evdev::AttributeSet::<evdev::KeyCode>::new();
    keys.insert(evdev::KeyCode::KEY_LEFTCTRL);
    keys.insert(evdev::KeyCode::KEY_LEFTSHIFT);
    keys.insert(evdev::KeyCode::KEY_V);
    let mut dev = evdev::uinput::VirtualDeviceBuilder::new()
        .expect("uinput open")
        .name("xsay-paste-test")
        .with_keys(&keys)
        .expect("with_keys")
        .build()
        .expect("build");

    eprintln!("[3/5] waiting 4s — FOCUS A TEXT FIELD NOW");
    std::thread::sleep(Duration::from_millis(4000));

    eprintln!("[4/5] emitting Ctrl+V");
    let ctrl_v = [
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_LEFTCTRL, 1),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_V, 1),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_V, 0),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_LEFTCTRL, 0),
    ];
    dev.emit(&ctrl_v).expect("emit ctrl+v");

    std::thread::sleep(Duration::from_millis(500));

    eprintln!("[5/5] emitting Ctrl+Shift+V");
    let ctrl_shift_v = [
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_LEFTCTRL, 1),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_LEFTSHIFT, 1),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_V, 1),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_V, 0),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_LEFTSHIFT, 0),
        *evdev::KeyEvent::new(evdev::KeyCode::KEY_LEFTCTRL, 0),
    ];
    dev.emit(&ctrl_shift_v).expect("emit ctrl+shift+v");

    eprintln!("done. Your focused text field should contain:");
    eprintln!("  {}  (once from Ctrl+V)", payload);
    eprintln!("  {}  (once from Ctrl+Shift+V)", payload);
    eprintln!("Note: Ctrl+V outputs once in GUI apps. Ctrl+Shift+V outputs once");
    eprintln!("in terminals and some GUI apps' 'paste special'. Both could fire");
    eprintln!("in apps that accept both → two copies in the field.");
}
