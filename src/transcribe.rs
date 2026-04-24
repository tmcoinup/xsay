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
) {
    let mut ctx: Option<WhisperContext> =
        initial_model_path.as_ref().and_then(|p| load_model(p));
    if ctx.is_some() {
        log::info!("Whisper model loaded");
    } else {
        log::warn!("Starting without a Whisper model (transcribe requests will be ignored)");
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
                match &ctx {
                    Some(c) => process_request(c, req, &transcript_tx),
                    None => {
                        log::warn!("No model loaded — transcribe request returning empty text");
                        let _ = transcript_tx.send(TranscriptSeg { text: String::new() });
                    }
                }
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
    ctx: &WhisperContext,
    req: TranscribeReq,
    transcript_tx: &Sender<TranscriptSeg>,
) {
    if req.samples.is_empty() {
        return;
    }

    // Backend dispatch — pluggable so Whisper can coexist with ONNX-based
    // backends (SenseVoice, Paraformer, ...). Non-whisper backends are
    // feature-gated; when the feature is off we fall through to Whisper
    // rather than crashing, so a config pointing at an ONNX backend on a
    // binary built without that feature still produces output.
    if req.backend == "sensevoice" || req.backend == "paraformer" {
        if try_onnx_backend(&req, transcript_tx) {
            return;
        }
        log::warn!(
            "{} backend requested but unavailable (needs xsay built with \
             --features sensevoice + model installed); falling back to Whisper",
            req.backend
        );
    }

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
                if !trimmed.is_empty() && !is_silence_marker(trimmed) {
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
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || is_silence_marker(trimmed) {
        let _ = tx.send(TranscriptSeg { text: String::new() });
    } else {
        let _ = tx.send(TranscriptSeg {
            text: trimmed.to_string(),
        });
    }
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

/// Whisper emits sentinels like `[BLANK_AUDIO]`, `(silence)`, `[noise]`,
/// `[music]` when it thinks the audio contains no speech. They're not real
/// transcripts and shouldn't reach history or be injected into the user's
/// focused window. Treat any single bracketed/parenthesized token as a
/// silence marker — these are non-linguistic metadata by convention in
/// Whisper's training data.
///
/// Also filters the well-known *hallucinations* Whisper emits when audio
/// is unclear or very short. The model was trained on massive amounts of
/// YouTube captions and news clips, so under low signal-to-noise it
/// confidently outputs memorized closers like "謝謝大家收看" or
/// "Thanks for watching, please subscribe". These ruin history and paste
/// random phrases into the user's document, so we drop them outright.
fn is_silence_marker(segment: &str) -> bool {
    let s = segment.trim();
    if s.len() < 2 {
        return false;
    }
    let first = s.chars().next().unwrap();
    let last = s.chars().last().unwrap();
    if matches!(
        (first, last),
        ('[', ']') | ('(', ')') | ('*', '*') | ('<', '>')
    ) {
        return true;
    }
    is_known_hallucination(s)
}

/// Well-known phrases Whisper hallucinates on silent/short/noisy input.
/// Gathered from whisper.cpp / OpenAI Whisper issue trackers and real user
/// reports. Uses *substring* matching (not exact) so variants with stray
/// punctuation, extra words, or partial runs still get filtered.
///
/// Also catches repetition-based hallucinations: when Whisper gets stuck
/// (common on noisy/silent tail-ends) it loops the same 2-4 char token
/// several times — e.g. "打赏 打赏 打赏". Any 2-4 char substring that
/// appears 3+ times in a short segment is almost certainly hallucinated.
fn is_known_hallucination(s: &str) -> bool {
    let norm_full = s.to_lowercase();
    // Normalize trim for exact-ish comparison on whole-segment matches.
    let norm: String = norm_full
        .trim_end_matches(['.', '。', '!', '！', '?', '？', ',', '，', ' '])
        .to_string();

    const HALLUCINATIONS: &[&str] = &[
        // Chinese — YouTube / TV closers
        "謝謝大家收看",
        "谢谢大家收看",
        "謝謝觀看",
        "谢谢观看",
        "謝謝觀賞",
        "谢谢观赏",
        "請訂閱",
        "请订阅",
        "訂閱我的頻道",
        "订阅我的频道",
        "謝謝大家",
        "谢谢大家",
        "感謝觀看",
        "感谢观看",
        "多謝收看",
        "多谢收看",
        // Chinese — Bilibili / Douyin / video-creator closers
        "请不吝点赞",
        "請不吝點讚",
        "點贊訂閱",
        "点赞订阅",
        "一鍵三連",
        "一键三连",
        "点赞关注转发",
        "點贊關注轉發",
        "打赏",
        // Chinese — subtitle / translation credits (from Whisper training
        // on fansubbed TV / films). Matches anything containing these
        // substrings so attached names (e.g. "中文字幕志愿者 杨茜茜") get
        // filtered regardless of who the random hallucinated name is.
        "字幕志愿者",
        "字幕志願者",
        "字幕由",
        "字幕組",
        "字幕组",
        "字幕制作",
        "字幕製作",
        "翻译志愿者",
        "翻譯志願者",
        "中文字幕",
        "繁體字幕",
        "简体字幕",
        "字幕提供",
        "翻譯：",
        "翻译：",
        "校對：",
        "校对：",
        "mediaclub",
        // Our own initial-prompt echoes. Whisper occasionally parrots the
        // priming prompt verbatim (or with a minor swap like 以下→这些/下面)
        // when audio is silent or too short to seed real output. Catch all
        // variants so the priming trick still gives us Simplified Chinese
        // without letting the prompt leak into the user's text.
        "以下是普通话的简体中文内容",
        "这些是普通话的简体中文内容",
        "下面是普通话的简体中文内容",
        "普通话的简体中文内容",
        // English — YouTube / podcast closers
        "thanks for watching",
        "thank you for watching",
        "thanks for listening",
        "please subscribe",
        "subscribe to my channel",
        "like and subscribe",
        "see you next time",
        "thank you",
        "thanks",
    ];
    if HALLUCINATIONS.iter().any(|h| norm.contains(h)) {
        return true;
    }
    has_repetition(&norm)
}

/// Heuristic: any 2-, 3-, or 4-char substring that appears 3+ times in a
/// segment this short is almost always a Whisper stuck-in-a-loop
/// hallucination, not natural repetition. Examples caught:
///   "打赏 打赏 打赏"
///   "你在搞什麼,你在搞什麼,你在搞什麼"
///   "Huh? Huh? Huh?"
/// Skipped for long segments because a genuine transcript of a long
/// speech can legitimately use a word 3+ times.
fn has_repetition(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 6 || chars.len() > 60 {
        return false;
    }
    for window in 2..=4 {
        if chars.len() < window * 3 {
            continue;
        }
        let mut counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for start in 0..=chars.len() - window {
            let token: String = chars[start..start + window].iter().collect();
            // Skip tokens that are all whitespace / punctuation.
            if token.chars().all(|c| !c.is_alphanumeric() && !is_cjk(c)) {
                continue;
            }
            let n = counts.entry(token).or_insert(0);
            *n += 1;
            if *n >= 3 {
                return true;
            }
        }
    }
    false
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF   // CJK Ext A
      | 0x4E00..=0x9FFF   // CJK Unified
      | 0x20000..=0x2A6DF // CJK Ext B
    )
}
