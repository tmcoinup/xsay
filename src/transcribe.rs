use crossbeam_channel::{Receiver, Sender, select};
use std::path::PathBuf;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct TranscribeReq {
    pub samples: Vec<f32>,
    pub language: String,
    pub n_threads: i32,
    pub translate: bool,
    /// "whisper" | "sensevoice" — chosen per-request so live backend
    /// switches from the settings UI take effect on the next utterance
    /// without needing to restart the daemon.
    pub backend: String,
}

pub struct TranscriptSeg {
    pub text: String,
}

pub fn run_transcribe_thread(
    req_rx: Receiver<TranscribeReq>,
    reload_rx: Receiver<PathBuf>,
    transcript_tx: Sender<TranscriptSeg>,
    initial_model_path: Option<PathBuf>,
    initial_backend: String,
) {
    let mut ctx: Option<WhisperContext> = if initial_backend == "whisper" {
        initial_model_path.as_ref().and_then(load_model)
    } else {
        None
    };
    match (&ctx, initial_backend.as_str()) {
        (Some(_), "whisper") => log::info!("Whisper model loaded"),
        (None, "whisper") => {
            log::warn!("Starting without a Whisper model (Whisper requests will return empty text)")
        }
        _ => log::info!(
            "Starting with {} backend; Whisper model loading deferred",
            initial_backend
        ),
    }

    // Pre-warm the selected ONNX backend so the first F2 press doesn't
    // eat a 500ms–3s ONNX session-construction wait. Paraformer in
    // particular takes multiple seconds cold. Warmup runs in this same
    // transcribe thread so it doesn't race with incoming TranscribeReqs.
    #[cfg(any(feature = "sensevoice", feature = "sensevoice-cuda"))]
    if is_onnx_backend(&initial_backend) {
        let provider = if cfg!(feature = "sensevoice-cuda") {
            "cuda".to_string()
        } else {
            "cpu".to_string()
        };
        let opts = crate::sensevoice::OnnxOptions {
            language: "auto".into(),
            use_itn: true,
            provider,
            num_threads: 4,
        };
        crate::sensevoice::warmup(&initial_backend, &opts);
    }

    loop {
        select! {
            recv(reload_rx) -> new_path => {
                let new_path = match new_path { Ok(p) => p, Err(_) => break };
                log::info!("Loading Whisper model from {}", new_path.display());
                match load_model(&new_path) {
                    Some(new_ctx) => {
                        ctx = Some(new_ctx);
                        log::info!("Model loaded successfully");
                    }
                    None => log::error!("Model load failed; keeping previous state"),
                }
            }
            recv(req_rx) -> req => {
                let req = match req { Ok(r) => r, Err(_) => break };
                process_request(ctx.as_ref(), req, &transcript_tx);
            }
        }
    }
}

fn load_model(path: &PathBuf) -> Option<WhisperContext> {
    let s = path.to_string_lossy();
    if !path.exists() {
        log::warn!("Model file does not exist: {}", s);
        return None;
    }
    match WhisperContext::new_with_params(&s, WhisperContextParameters::default()) {
        Ok(c) => Some(c),
        Err(e) => {
            log::error!("Failed to load Whisper model at {}: {:?}", s, e);
            None
        }
    }
}

fn process_request(
    ctx: Option<&WhisperContext>,
    mut req: TranscribeReq,
    transcript_tx: &Sender<TranscriptSeg>,
) {
    if req.samples.is_empty() {
        return;
    }

    // Noise gate — runs before any backend. Every ASR model we
    // support (Whisper, SenseVoice, Paraformer) hallucinates confidently
    // on near-silent audio: Whisper outputs training fanfic (字幕志愿者,
    // 謝謝大家收看), SenseVoice falls back to conversational fillers
    // ("Okay.", "Yes.", "嗯。", "好的。"). Gate at the audio level and
    // attenuate low-energy windows so the model sees less room noise.
    let stats = crate::speech_quality::speech_stats(&req.samples);
    if !stats.looks_like_speech() {
        log::info!(
            "Skipping transcribe: likely noise/silence \
             (duration {:.2}s, peak {:.4}, floor {:.4}, active {:.0}%, run {} frames)",
            stats.duration_s,
            stats.peak_rms,
            stats.noise_floor,
            stats.active_ratio * 100.0,
            stats.longest_active_run,
        );
        let _ = transcript_tx.send(TranscriptSeg {
            text: String::new(),
        });
        return;
    }
    req.samples = crate::speech_quality::denoise_for_asr(&req.samples, &stats);
    if req.samples.len() < 8000 {
        log::debug!("Skipping transcribe: trimmed speech shorter than 0.5s");
        let _ = transcript_tx.send(TranscriptSeg {
            text: String::new(),
        });
        return;
    }

    // Backend dispatch — pluggable so Whisper can coexist with ONNX-based
    // backends (SenseVoice, Paraformer, ...). Non-whisper backends are
    // feature-gated; when the feature is off we fall through to Whisper
    // rather than crashing, so a config pointing at an ONNX backend on a
    // binary built without that feature still produces output.
    if is_onnx_backend(&req.backend) {
        if try_onnx_backend(&req, transcript_tx) {
            return;
        }
        log::warn!(
            "{} backend requested but unavailable (needs xsay built with \
             --features sensevoice + model installed); falling back to Whisper",
            req.backend
        );
    }

    let Some(ctx) = ctx else {
        log::warn!("No Whisper model loaded — transcribe request returning empty text");
        let _ = transcript_tx.send(TranscriptSeg {
            text: String::new(),
        });
        return;
    };

    // Escalate from debug → info so this always appears in release logs —
    // previously a stuck inference produced a completely silent gap between
    // "Recording stopped" and nothing, which was impossible to diagnose.
    let secs = req.samples.len() as f32 / 16000.0;
    log::info!(
        "Whisper start: {} samples ({:.2}s), lang={}, threads={}, translate={}",
        req.samples.len(),
        secs,
        req.language,
        req.n_threads,
        req.translate,
    );
    let start = std::time::Instant::now();

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    // whisper-rs refuses n_threads <= 0 by silently misbehaving; clamp
    // defensively so a mis-edited config can't take down the pipeline.
    let n_threads = req.n_threads.max(1);
    params.set_n_threads(n_threads);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_translate(req.translate);

    // Aggressive hallucination suppression:
    //   - no_speech_thold 0.6 (default) → 0.8: segments where Whisper
    //     thinks there's >80% chance of silence return empty. Silent/short
    //     clips that would otherwise trigger "中文字幕志愿者 XXX" or "请
    //     不吝点赞" get skipped.
    //   - logprob_thold -1.0 (default) → -0.7: low-confidence decodings
    //     are rejected. Reduces confident-but-wrong transcripts on
    //     noisy input.
    //   - suppress_blank true (default) kept, complements above.
    //   - suppress_non_speech_tokens true: Whisper's special tokens like
    //     [music], (applause) get stripped at the sampler level too.
    params.set_no_speech_thold(0.8);
    params.set_logprob_thold(-0.7);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);

    if req.language == "auto" || req.language.is_empty() {
        params.set_language(None);
    } else {
        params.set_language(Some(req.language.as_str()));
    }

    // Whisper's zh training data leans Traditional Chinese. For mainland
    // users, push the decoder toward Simplified via a priming prompt —
    // this is the canonical whisper.cpp trick and costs nothing at
    // inference time. Only applies when language is explicitly zh so
    // English / other-language sessions aren't primed with Chinese tokens.
    if req.language == "zh" {
        params.set_initial_prompt("以下是普通话的简体中文内容。");
    }

    let mut state = match ctx.create_state() {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to create whisper state: {:?}", e);
            return;
        }
    };

    if let Err(e) = state.full(params, &req.samples) {
        log::error!(
            "Whisper transcription failed after {:?}: {:?}",
            start.elapsed(),
            e
        );
        return;
    }
    log::info!("Whisper done in {:?}", start.elapsed());

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
                if !trimmed.is_empty()
                    && !crate::speech_quality::is_silence_marker(trimmed, &req.language)
                {
                    text.push_str(trimmed);
                    text.push(' ');
                }
            }
            Err(e) => log::warn!("Failed to get segment {}: {:?}", i, e),
        }
    }

    let text = crate::speech_quality::finalize_transcript(&text, &req.language).unwrap_or_default();
    log::debug!("Transcription result: {:?}", text);

    let _ = transcript_tx.send(TranscriptSeg { text });
}

fn is_onnx_backend(backend: &str) -> bool {
    backend.starts_with("sensevoice") || backend == "paraformer"
}

/// ONNX backend dispatch (SenseVoice, Paraformer). Returns `true` if we
/// attempted the backend and produced output — caller should then skip
/// the Whisper codepath. Returns `false` if the backend isn't compiled
/// in or the model isn't installed, so the caller falls back to Whisper.
#[cfg(any(feature = "sensevoice", feature = "sensevoice-cuda"))]
fn try_onnx_backend(req: &TranscribeReq, tx: &Sender<TranscriptSeg>) -> bool {
    if !crate::sensevoice::is_installed(&req.backend) {
        return false;
    }
    let provider = if cfg!(feature = "sensevoice-cuda") {
        "cuda".to_string()
    } else {
        "cpu".to_string()
    };
    let opts = crate::sensevoice::OnnxOptions {
        language: req.language.clone(),
        use_itn: true,
        provider,
        num_threads: req.n_threads.max(1),
    };
    let secs = req.samples.len() as f32 / 16000.0;
    log::info!(
        "{} start: {} samples ({:.2}s), lang={}, threads={}",
        req.backend,
        req.samples.len(),
        secs,
        req.language,
        req.n_threads,
    );
    let start = std::time::Instant::now();
    let Some(raw) = crate::sensevoice::transcribe(&req.backend, &req.samples, &opts) else {
        return false;
    };
    log::info!("{} done in {:?}", req.backend, start.elapsed());
    // Strip SenseVoice-style <|language|>/<|emotion|> markers. Paraformer
    // doesn't emit these so the scan is a no-op — cheap either way.
    let cleaned = strip_markers(&raw);
    let text =
        crate::speech_quality::finalize_transcript(&cleaned, &req.language).unwrap_or_default();
    let _ = tx.send(TranscriptSeg { text });
    true
}

#[cfg(not(any(feature = "sensevoice", feature = "sensevoice-cuda")))]
fn try_onnx_backend(_req: &TranscribeReq, _tx: &Sender<TranscriptSeg>) -> bool {
    false
}

#[cfg(any(feature = "sensevoice", feature = "sensevoice-cuda"))]
fn strip_markers(s: &str) -> String {
    // Cheap left-to-right scan: drop everything between '<' and '>'
    // (inclusive). We don't need a real XML parser — SenseVoice emits
    // single-token markers, never nested.
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}
