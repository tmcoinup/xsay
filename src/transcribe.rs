use crossbeam_channel::{Receiver, Sender};
use std::path::PathBuf;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct TranscribeReq {
    pub samples: Vec<f32>,
    pub language: String,
    pub n_threads: i32,
    pub translate: bool,
}

pub struct TranscriptSeg {
    pub text: String,
}

pub fn run_transcribe_thread(
    req_rx: Receiver<TranscribeReq>,
    transcript_tx: Sender<TranscriptSeg>,
    model_path: PathBuf,
) {
    let model_str = model_path.to_string_lossy().to_string();
    log::info!("Loading Whisper model from {}", model_str);

    let ctx = match WhisperContext::new_with_params(&model_str, WhisperContextParameters::default())
    {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to load Whisper model: {:?}", e);
            eprintln!("Error: Failed to load Whisper model: {:?}", e);
            return;
        }
    };

    log::info!("Whisper model loaded");

    loop {
        let req = match req_rx.recv() {
            Ok(r) => r,
            Err(_) => break,
        };

        if req.samples.is_empty() {
            continue;
        }

        log::debug!("Transcribing {} samples", req.samples.len());

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(req.n_threads);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_translate(req.translate);

        if req.language == "auto" || req.language.is_empty() {
            params.set_language(None);
        } else {
            params.set_language(Some(req.language.as_str()));
        }

        let mut state = match ctx.create_state() {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to create whisper state: {:?}", e);
                continue;
            }
        };

        if let Err(e) = state.full(params, &req.samples) {
            log::error!("Whisper transcription failed: {:?}", e);
            continue;
        }

        let n_segments = match state.full_n_segments() {
            Ok(n) => n,
            Err(e) => {
                log::error!("Failed to get segment count: {:?}", e);
                continue;
            }
        };

        let mut text = String::new();
        for i in 0..n_segments {
            match state.full_get_segment_text(i) {
                Ok(seg) => {
                    let trimmed = seg.trim();
                    if !trimmed.is_empty() {
                        text.push_str(trimmed);
                        text.push(' ');
                    }
                }
                Err(e) => log::warn!("Failed to get segment {}: {:?}", i, e),
            }
        }

        let text = text.trim().to_string();
        log::debug!("Transcription result: {:?}", text);

        let _ = transcript_tx.send(TranscriptSeg { text });
    }
}
