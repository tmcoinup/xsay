// Embed the Windows application icon into xsay.exe so Explorer and the
// taskbar show our logo instead of the default Rust binary icon. No-op
// on every non-Windows platform.

fn main() {
    #[cfg(target_os = "windows")]
    {
        // Look for the icon at its canonical path and quietly skip if
        // the file is missing (keeps `cargo check` green in unusual
        // checkouts where windows/ hasn't been populated yet).
        let ico = std::path::Path::new("windows").join("xsay.ico");
        if ico.exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico.to_str().unwrap());
            if let Err(e) = res.compile() {
                eprintln!("cargo:warning=winres icon embed failed: {}", e);
            }
        }
    }
}
