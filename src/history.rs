//! Append-only transcription history stored as JSONL in the user's cache dir.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix seconds
    pub timestamp: i64,
    pub text: String,
}

pub fn path() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("xsay").join("history.jsonl"))
}

pub fn append(text: &str) {
    let Some(p) = path() else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let entry = HistoryEntry {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        text: text.to_string(),
    };

    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("history serialize failed: {}", e);
            return;
        }
    };

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", line);
        }
        Err(e) => log::warn!("history append failed: {}", e),
    }
}

/// Load the most recent `limit` entries, newest first.
pub fn load_recent(limit: usize) -> Vec<HistoryEntry> {
    let Some(p) = path() else { return Vec::new() };
    let Ok(f) = std::fs::File::open(&p) else {
        return Vec::new();
    };

    let mut all: Vec<HistoryEntry> = BufReader::new(f)
        .lines()
        .filter_map(|r| r.ok())
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();

    all.reverse();
    all.truncate(limit);
    all
}

pub fn clear() -> std::io::Result<()> {
    if let Some(p) = path() {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

/// Format unix seconds as "YYYY-MM-DD HH:MM" in local time.
pub fn format_timestamp(ts: i64) -> String {
    #[cfg(unix)]
    {
        unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            let t = ts as libc::time_t;
            if !libc::localtime_r(&t, &mut tm).is_null() {
                return format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}",
                    tm.tm_year + 1900,
                    tm.tm_mon + 1,
                    tm.tm_mday,
                    tm.tm_hour,
                    tm.tm_min
                );
            }
        }
    }
    // Windows / fallback: UTC only.
    let d = ts.max(0);
    let mins = ((d / 60) % 60) as u32;
    let hours = ((d / 3600) % 24) as u32;
    let days = d / 86400;
    let (y, m, dm) = civil_from_days(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, m, dm, hours, mins)
}

// Howard Hinnant's days → civil conversion.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + (m <= 2) as i64, m, d)
}
