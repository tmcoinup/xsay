use crossbeam_channel::{Receiver, Sender, select};
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
    reload_rx: Receiver<PathBuf>,
    transcript_tx: Sender<TranscriptSeg>,
    initial_model_path: PathBuf,
) {
    let mut ctx = match load_model(&initial_model_path) {
        Some(c) => c,
        None => return,
    };

    loop {
        select! {
            recv(reload_rx) -> new_path => {
                let new_path = match new_path { Ok(p) => p, Err(_) => break };
                log::info!("Reloading Whisper model from {}", new_path.display());
                match load_model(&new_path) {
                    Some(new_ctx) => {
                        ctx = new_ctx;
                        log::info!("Model reloaded successfully");
                    }
                    None => log::error!("Model reload failed; keeping previous model"),
                }
            }
            recv(req_rx) -> req => {
                let req = match req { Ok(r) => r, Err(_) => break };
                process_request(&ctx, req, &transcript_tx);
            }
        }
    }
}

fn load_model(path: &PathBuf) -> Option<WhisperContext> {
    let s = path.to_string_lossy();
    match WhisperContext::new_with_params(&s, WhisperContextParameters::default()) {
        Ok(c) => Some(c),
        Err(e) => {
            log::error!("Failed to load Whisper model at {}: {:?}", s, e);
            None
        }
    }
}

fn process_request(
    ctx: &WhisperContext,
    req: TranscribeReq,
    transcript_tx: &Sender<TranscriptSeg>,
) {
    if req.samples.is_empty() {
        return;
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
            return;
        }
    };

    if let Err(e) = state.full(params, &req.samples) {
        log::error!("Whisper transcription failed: {:?}", e);
        return;
    }

    let n_segments = match state.full_n_segments() {
        Ok(n) => n,
        Err(e) => {
            log::error!("Failed to get segment count: {:?}", e);
            return;
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
