use crate::config::AudioConfig;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use std::sync::Arc;

pub enum AudioCmd {
    StartRecording,
    StopRecording,
    Abort,
}

pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub is_final: bool,
    pub triggered_by_pause: bool,
}

pub fn list_devices() {
    let host = cpal::default_host();
    println!("Available input devices:");
    for (i, name) in input_device_names().iter().enumerate() {
        println!("  [{}] {}", i, name);
    }
    let _ = host; // suppress unused warning when called via CLI
}

/// Returns the names of available input devices, or an empty Vec on failure.
pub fn input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|d| d.name().ok())
        .collect()
}

pub fn run_audio_thread(
    cmd_rx: Receiver<AudioCmd>,
    chunk_tx: Sender<AudioChunk>,
    shared_config: Arc<Mutex<AudioConfig>>,
) {
    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            log::error!("No audio input device found");
            return;
        }
    };

    let supported_config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Could not get default input config: {}", e);
            return;
        }
    };

    let device_sample_rate = supported_config.sample_rate().0;
    let device_channels = supported_config.channels() as usize;
    let sample_format = supported_config.sample_format();

    log::info!(
        "Audio device: {}, sample rate: {}, channels: {}, format: {:?}",
        device.name().unwrap_or_default(),
        device_sample_rate,
        device_channels,
        sample_format,
    );

    // Channel for raw samples from cpal callback
    let (raw_tx, raw_rx) = crossbeam_channel::bounded::<Vec<f32>>(256);

    let stream_config: cpal::StreamConfig = supported_config.clone().into();
    let channels = device_channels;

    let raw_tx_clone = raw_tx.clone();
    let err_fn = |e| log::error!("Audio stream error: {}", e);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                let _ = raw_tx_clone.send(data.to_vec());
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let tx = raw_tx.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let v: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let _ = tx.send(v);
                },
                err_fn,
                None,
            )
        }
        _ => {
            // Try f32 as fallback
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    let _ = raw_tx_clone.send(data.to_vec());
                },
                err_fn,
                None,
            )
        }
    };

    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            log::error!("Could not build audio input stream: {}", e);
            return;
        }
    };

    // Keep stream active but paused until recording starts
    if let Err(e) = stream.pause() {
        log::warn!("Could not pause stream initially: {}", e);
    }

    let mut recording = false;
    let mut accumulator: Vec<f32> = Vec::new();
    let mut silent_chunks: u32 = 0;
    let mut recording_start = std::time::Instant::now();
    // Peak RMS observed during the current recording session. Printed on
    // Stop/Pause so a quick log scan tells the user whether the mic is
    // actually picking up sound (values under ~0.005 almost always mean the
    // wrong input device or a muted/disconnected mic).
    let mut peak_rms: f32 = 0.0;

    loop {
        // Check for commands (non-blocking)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                AudioCmd::StartRecording => {
                    accumulator.clear();
                    silent_chunks = 0;
                    peak_rms = 0.0;
                    recording = true;
                    recording_start = std::time::Instant::now();
                    if let Err(e) = stream.play() {
                        log::error!("Could not start audio stream: {}", e);
                    }
                    log::debug!("Recording started");
                }
                AudioCmd::StopRecording => {
                    recording = false;
                    let _ = stream.pause();
                    log::info!(
                        "Recording stopped: {} samples, peak RMS {:.4} ({})",
                        accumulator.len(),
                        peak_rms,
                        rms_hint(peak_rms),
                    );
                    // Desktop notification for mic-unplugged / wrong-device
                    // scenarios. Recordings shorter than 0.4s are usually
                    // accidental key taps — don't nag in that case.
                    // Threshold 0.003 is below the silence threshold and
                    // well below normal speech (~0.03–0.1); hitting it
                    // generally means the device returned zeroes.
                    let duration_s = accumulator.len() as f32 / 16_000.0;
                    if duration_s >= 0.4 && peak_rms < 0.003 {
                        notify_mic_silent();
                    }
                    let samples = std::mem::take(&mut accumulator);
                    let _ = chunk_tx.send(AudioChunk {
                        samples,
                        is_final: true,
                        triggered_by_pause: false,
                    });
                    silent_chunks = 0;
                }
                AudioCmd::Abort => {
                    recording = false;
                    let _ = stream.pause();
                    accumulator.clear();
                    silent_chunks = 0;
                    log::debug!("Recording aborted");
                }
            }
        }

        if !recording {
            // Drain raw_rx so buffer doesn't fill up
            while raw_rx.try_recv().is_ok() {}
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }

        // Snapshot live-configurable values for this loop iteration
        let cfg = shared_config.lock().clone();

        // Check max duration
        if recording_start.elapsed().as_secs() >= cfg.max_record_seconds as u64 {
            log::debug!("Max duration reached, stopping recording");
            recording = false;
            let _ = stream.pause();
            let samples = std::mem::take(&mut accumulator);
            let _ = chunk_tx.send(AudioChunk {
                samples,
                is_final: true,
                triggered_by_pause: false,
            });
            silent_chunks = 0;
            continue;
        }

        // Drain all available raw samples
        let mut got_samples = false;
        while let Ok(raw) = raw_rx.try_recv() {
            // Mix channels to mono
            let mono = mix_to_mono(&raw, channels);
            // Resample to 16kHz
            let resampled = resample_to_16k(&mono, device_sample_rate);
            accumulator.extend_from_slice(&resampled);
            got_samples = true;

            // Check RMS of this chunk for silence detection
            let chunk_rms = rms(&resampled);
            if chunk_rms > peak_rms {
                peak_rms = chunk_rms;
            }
            if chunk_rms < cfg.silence_threshold {
                silent_chunks += 1;
            } else {
                silent_chunks = 0;
            }

            // Pause detected: send chunk for transcription and keep recording
            if silent_chunks >= cfg.silence_frames && accumulator.len() > 16000 {
                silent_chunks = 0;
                let samples = std::mem::take(&mut accumulator);
                log::debug!(
                    "Pause detected, sending {} samples for transcription",
                    samples.len()
                );
                let _ = chunk_tx.send(AudioChunk {
                    samples,
                    is_final: false,
                    triggered_by_pause: true,
                });
            }
        }

        if !got_samples {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

fn mix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_to_16k(samples: &[f32], from_rate: u32) -> Vec<f32> {
    const TARGET_RATE: u32 = 16000;
    if from_rate == TARGET_RATE {
        return samples.to_vec();
    }
    let ratio = TARGET_RATE as f64 / from_rate as f64;
    let out_len = (samples.len() as f64 * ratio) as usize;
    if out_len == 0 {
        return vec![];
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;
        let s0 = samples.get(src_idx).copied().unwrap_or(0.0);
        let s1 = samples.get(src_idx + 1).copied().unwrap_or(s0);
        out.push(s0 + frac * (s1 - s0));
    }
    out
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Human-readable diagnostic for a recording's peak RMS. Thresholds chosen
/// by eye from observed normal-speech levels on a laptop mic (~0.05–0.15)
/// and the default `silence_threshold = 0.01`.
fn rms_hint(peak: f32) -> &'static str {
    if peak < 0.002 {
        "几乎无信号 — 麦克风可能未工作或选错设备"
    } else if peak < 0.01 {
        "极弱 — 低于静音阈值，Whisper 会输出空白"
    } else if peak < 0.03 {
        "偏弱 — 靠近麦克风或提高增益"
    } else {
        "正常"
    }
}

/// Desktop notification surfaced when a recording returns near-zero
/// audio energy over a non-trivial duration. The usual cause is the
/// system default input device being a monitor / muted mic / unplugged
/// headset — situations the user can only diagnose visually (or via
/// log inspection) unless xsay tells them.
///
/// Throttled by a static AtomicU64 timestamp so a long stretch of
/// silent attempts doesn't spam a notification per hotkey press.
fn notify_mic_silent() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_NOTIFY_EPOCH: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = LAST_NOTIFY_EPOCH.load(Ordering::Relaxed);
    // 60s cooldown — plenty of time for the user to react, not so long
    // that a plug-in/plug-out cycle gets silenced.
    if now - last < 60 {
        return;
    }
    LAST_NOTIFY_EPOCH.store(now, Ordering::Relaxed);

    let _ = std::process::Command::new("notify-send")
        .args([
            "-a",
            "xsay",
            "-u",
            "normal",
            "-t",
            "6000",
            "xsay: 麦克风无信号",
            "录音时没有采集到声音。请检查：系统声音设置里的输入设备是不是你想用的麦克风，\
             权限是否允许，物理麦克是不是静音了。",
        ])
        .status();
}
