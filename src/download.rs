use crossbeam_channel::{Receiver, Sender, TryRecvError};
use parking_lot::Mutex;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum DlState {
    NotStarted,
    Downloading,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

pub struct DownloadProgress {
    pub downloaded: AtomicU64,
    pub total: AtomicU64,
    pub state: Mutex<DlState>,
}

impl DownloadProgress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            downloaded: AtomicU64::new(0),
            total: AtomicU64::new(0),
            state: Mutex::new(DlState::NotStarted),
        })
    }
}

pub enum DownloadCmd {
    Pause,
    Cancel,
}

pub fn hf_url(repo: &str, filename: &str) -> String {
    format!(
        "https://huggingface.co/{}/resolve/main/{}",
        repo, filename
    )
}

pub fn partial_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".partial");
    PathBuf::from(s)
}

/// Start (or resume) a download. Returns a channel sender to pause/cancel.
pub fn start_download(
    url: String,
    dest_path: PathBuf,
    progress: Arc<DownloadProgress>,
) -> Sender<DownloadCmd> {
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<DownloadCmd>(4);

    std::thread::spawn(move || {
        *progress.state.lock() = DlState::Downloading;

        match run_download(&url, &dest_path, &progress, &cmd_rx) {
            Ok(()) => {
                *progress.state.lock() = DlState::Completed;
            }
            Err(e) if e == "paused" => {
                *progress.state.lock() = DlState::Paused;
            }
            Err(e) if e == "cancelled" => {
                let _ = std::fs::remove_file(partial_path(&dest_path));
                *progress.state.lock() = DlState::Cancelled;
            }
            Err(e) => {
                *progress.state.lock() = DlState::Failed(e);
            }
        }
    });

    cmd_tx
}

fn run_download(
    url: &str,
    dest: &Path,
    progress: &DownloadProgress,
    cmd_rx: &Receiver<DownloadCmd>,
) -> Result<(), String> {
    let pp = partial_path(dest);

    // Resume from partial file if it exists
    let existing = pp.metadata().map(|m| m.len()).unwrap_or(0);
    progress.downloaded.store(existing, Ordering::Relaxed);

    let response = if existing > 0 {
        ureq::get(url)
            .set("Range", &format!("bytes={}-", existing))
            .call()
            .map_err(|e| e.to_string())?
    } else {
        ureq::get(url).call().map_err(|e| e.to_string())?
    };

    let status = response.status();

    let (total, start_offset) = if status == 206 {
        let t = response
            .header("Content-Range")
            .and_then(|cr| cr.split('/').last())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        (t, existing)
    } else {
        // 200: full content, ignore partial file
        let t = response
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        (t, 0u64)
    };

    progress.total.store(total, Ordering::Relaxed);
    progress.downloaded.store(start_offset, Ordering::Relaxed);

    let mut file = if status == 206 && existing > 0 {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&pp)
            .map_err(|e| e.to_string())?
    } else {
        std::fs::File::create(&pp).map_err(|e| e.to_string())?
    };

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 65536];
    let mut cur = start_offset;

    loop {
        match cmd_rx.try_recv() {
            Ok(DownloadCmd::Pause) => {
                let _ = file.flush();
                return Err("paused".to_string());
            }
            Ok(DownloadCmd::Cancel) => return Err("cancelled".to_string()),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {}
        }

        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }

        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        cur += n as u64;
        progress.downloaded.store(cur, Ordering::Relaxed);
    }

    drop(file);
    std::fs::rename(&pp, dest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Spawn a thread that does a HEAD request and sends back (filename, Option<size_bytes>).
pub fn check_remote_size(
    url: String,
    result_tx: Sender<(String, Option<u64>)>,
    filename: String,
) {
    std::thread::spawn(move || {
        let size = ureq::request("HEAD", &url)
            .call()
            .ok()
            .and_then(|r| r.header("Content-Length").and_then(|s| s.parse().ok()));
        let _ = result_tx.send((filename, size));
    });
}
