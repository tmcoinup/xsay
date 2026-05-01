use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SpeechStats {
    pub(crate) duration_s: f32,
    pub(crate) peak_rms: f32,
    pub(crate) noise_floor: f32,
    pub(crate) active_ratio: f32,
    pub(crate) active_windows: usize,
    pub(crate) longest_active_run: usize,
    active_threshold: f32,
}

impl SpeechStats {
    pub(crate) fn looks_like_speech(&self) -> bool {
        self.duration_s >= 0.45
            && self.peak_rms >= 0.010
            && self.active_windows >= 3
            && self.longest_active_run >= 2
            && self.active_ratio >= 0.03
    }
}

pub(crate) fn speech_stats(samples: &[f32]) -> SpeechStats {
    let windows = rms_windows(samples);
    let duration_s = samples.len() as f32 / 16_000.0;
    if windows.is_empty() {
        return SpeechStats {
            duration_s,
            peak_rms: 0.0,
            noise_floor: 0.0,
            active_ratio: 0.0,
            active_windows: 0,
            longest_active_run: 0,
            active_threshold: 0.010,
        };
    }

    let mut sorted = windows.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor_idx = ((sorted.len().saturating_sub(1)) as f32 * 0.20).round() as usize;
    let noise_floor = sorted[floor_idx];
    let active_threshold = (noise_floor * 2.8).clamp(0.006, 0.025);
    let peak_rms = windows.iter().copied().fold(0.0, f32::max);

    let mut active_windows = 0;
    let mut longest_active_run = 0;
    let mut run = 0;
    for rms in windows.iter().copied() {
        if rms >= active_threshold {
            active_windows += 1;
            run += 1;
            longest_active_run = longest_active_run.max(run);
        } else {
            run = 0;
        }
    }

    SpeechStats {
        duration_s,
        peak_rms,
        noise_floor,
        active_ratio: active_windows as f32 / windows.len() as f32,
        active_windows,
        longest_active_run,
        active_threshold,
    }
}

pub(crate) fn denoise_for_asr(samples: &[f32], stats: &SpeechStats) -> Vec<f32> {
    const WINDOW: usize = 320;
    const MARGIN_WINDOWS: usize = 5;

    let windows = rms_windows(samples);
    if windows.is_empty() {
        return Vec::new();
    }

    let first_active = windows
        .iter()
        .position(|&r| r >= stats.active_threshold)
        .unwrap_or(0)
        .saturating_sub(MARGIN_WINDOWS);
    let last_active = windows
        .iter()
        .rposition(|&r| r >= stats.active_threshold)
        .unwrap_or(windows.len().saturating_sub(1));

    let start = first_active * WINDOW;
    let end = ((last_active + 1 + MARGIN_WINDOWS) * WINDOW).min(samples.len());
    if start >= end {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(end - start);
    for (local_window_idx, chunk) in samples[start..end].chunks(WINDOW).enumerate() {
        let window_idx = first_active + local_window_idx;
        let rms = windows.get(window_idx).copied().unwrap_or(0.0);
        let gain = if rms < stats.active_threshold {
            0.08
        } else if rms < stats.active_threshold * 1.35 {
            0.45
        } else {
            1.0
        };
        out.extend(chunk.iter().map(|s| (s * gain).clamp(-1.0, 1.0)));
    }
    remove_dc(&mut out);
    out
}

pub(crate) fn finalize_transcript(raw: &str, language: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() || is_silence_marker(text, language) {
        None
    } else {
        Some(text.to_string())
    }
}

/// Whisper emits sentinels like `[BLANK_AUDIO]`, `(silence)`, `[noise]`,
/// `[music]` when it thinks the audio contains no speech. SenseVoice and
/// Paraformer more often emit short fillers or Latin words on near-silence.
/// None of those should reach history or be pasted into the user's document.
pub(crate) fn is_silence_marker(segment: &str, language: &str) -> bool {
    let s = segment.trim();
    if s.is_empty() || is_punctuation_or_symbol_only(s) {
        return true;
    }
    if looks_like_noise_transcript(s, language) {
        return true;
    }
    if s.len() >= 2 {
        let first = s.chars().next().unwrap();
        let last = s.chars().last().unwrap();
        if matches!(
            (first, last),
            ('[', ']') | ('(', ')') | ('*', '*') | ('<', '>')
        ) {
            return true;
        }
    }
    is_known_hallucination(s, language)
}

fn looks_like_noise_transcript(s: &str, language: &str) -> bool {
    let meaningful = s
        .chars()
        .filter(|c| c.is_alphanumeric() || is_cjk(*c))
        .count();
    if meaningful <= 1 {
        return true;
    }

    let cjk = s.chars().filter(|c| is_cjk(*c)).count();
    if cjk == 0 && meaningful <= 2 {
        return true;
    }

    language_prefers_chinese_filter(language) && cjk == 0 && is_short_latin_hallucination(s)
}

fn is_known_hallucination(s: &str, language: &str) -> bool {
    let norm = normalize_for_match(s);

    // Filter ONLY single-utterance fillers — SenseVoice's quiet-input
    // fallback always lands on these. Repeated forms ("好的好的", "嗯嗯")
    // were previously here too but they're indistinguishable from a user
    // genuinely saying "好的好的" twice; rejecting them silently swallows
    // real speech and the user has no idea why nothing was pasted.
    const EXACT_HALLUCINATIONS: &[&str] = &[
        // Single-letter / symbol fragments from ONNX backends on keyboard
        // taps, room noise, or clipped empty recordings.
        "o",
        "i",
        "l",
        "0",
        "1",
        // SenseVoice filler fallback set when audio is quiet-but-not-silent.
        "yeah",
        "no",
        "嗯",
        "啊",
        "哦",
        "唉",
        "噢",
        "呃",
        "额",
        "mm",
        "hmm",
    ];
    if EXACT_HALLUCINATIONS.iter().any(|h| norm == *h) {
        return true;
    }
    if language_prefers_chinese_filter(language) && is_short_latin_hallucination(&norm) {
        return true;
    }

    const SUBSTRING_HALLUCINATIONS: &[&str] = &[
        // Chinese video closers and subtitle credits.
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
        "请不吝点赞",
        "請不吝點讚",
        "點贊訂閱",
        "点赞订阅",
        "一鍵三連",
        "一键三连",
        "点赞关注转发",
        "點贊關注轉發",
        "打赏",
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
        // Initial-prompt echoes.
        "以下是普通话的简体中文内容",
        "这些是普通话的简体中文内容",
        "下面是普通话的简体中文内容",
        "普通话的简体中文内容",
        // English video/podcast closers.
        "thanks for watching",
        "thank you for watching",
        "thanks for listening",
        "please subscribe",
        "subscribe to my channel",
        "like and subscribe",
        "see you next time",
    ];
    if SUBSTRING_HALLUCINATIONS.iter().any(|h| norm.contains(h)) {
        return true;
    }

    has_repetition(&norm)
}

fn normalize_for_match(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .trim_end_matches(['.', '。', '!', '！', '?', '？', ',', '，', ' '])
        .to_string()
}

fn is_short_latin_hallucination(s: &str) -> bool {
    let norm = normalize_for_match(s);
    if norm.is_empty() || norm.chars().any(|c| is_cjk(c)) {
        return false;
    }

    let mut tokens = norm
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|token| !token.is_empty());
    let Some(token) = tokens.next() else {
        return false;
    };
    if tokens.next().is_some() {
        return false;
    }

    const SHORT_LATIN_HALLUCINATIONS: &[&str] =
        &["a", "an", "the", "uh", "um", "er", "ah", "oh", "you"];
    SHORT_LATIN_HALLUCINATIONS.iter().any(|h| token == *h)
}

fn is_punctuation_or_symbol_only(s: &str) -> bool {
    let mut saw_any = false;
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        saw_any = true;
        if c.is_alphanumeric() || is_cjk(c) {
            return false;
        }
    }
    saw_any
}

fn rms_windows(samples: &[f32]) -> Vec<f32> {
    const WINDOW: usize = 320;
    if samples.is_empty() {
        return Vec::new();
    }
    samples.chunks(WINDOW).map(rms_block).collect()
}

fn remove_dc(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    if mean.abs() < 0.0001 {
        return;
    }
    for sample in samples {
        *sample = (*sample - mean).clamp(-1.0, 1.0);
    }
}

fn has_repetition(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 8 || chars.len() > 60 {
        return false;
    }
    // Require a high repetition count before declaring "this is a model
    // hallucination loop". Earlier we caught any 2-4-char substring that
    // appeared ≥3 times, which silently dropped genuine repeated speech
    // like "好的好的好的好的" (the user said "好的好的" twice and got nothing
    // back). 6 occurrences is high enough to still catch the pathological
    // "嗯嗯嗯嗯嗯嗯嗯嗯嗯嗯嗯嗯" filler loops without snagging conversational
    // emphasis.
    for window in 2..=4 {
        if chars.len() < window * 6 {
            continue;
        }
        let mut counts: HashMap<String, u32> = HashMap::new();
        for start in 0..=chars.len() - window {
            let token: String = chars[start..start + window].iter().collect();
            if token.chars().all(|c| !c.is_alphanumeric() && !is_cjk(c)) {
                continue;
            }
            let n = counts.entry(token).or_insert(0);
            *n += 1;
            if *n >= 6 {
                return true;
            }
        }
    }
    false
}

fn rms_block(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF   // CJK Ext A
      | 0x4E00..=0x9FFF   // CJK Unified
      | 0x20000..=0x2A6DF // CJK Ext B
    )
}

fn language_prefers_chinese_filter(language: &str) -> bool {
    matches!(
        language.trim().to_ascii_lowercase().as_str(),
        "" | "auto" | "zh" | "zh-cn" | "cmn" | "yue"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_short_latin_hallucinations_in_chinese_mode() {
        assert_eq!(finalize_transcript("The.", "zh"), None);
        assert_eq!(finalize_transcript("O.", "zh"), None);
        assert_eq!(finalize_transcript("l.", "zh"), None);
        assert_eq!(finalize_transcript("you.", "auto"), None);
    }

    #[test]
    fn drops_symbol_only_and_common_fillers() {
        assert_eq!(finalize_transcript(".", "zh"), None);
        assert_eq!(finalize_transcript("°", "zh"), None);
        assert_eq!(finalize_transcript("好的好的。", "zh"), None);
    }

    #[test]
    fn keeps_mixed_chinese_and_latin_content() {
        assert_eq!(
            finalize_transcript("这个 API 怎么部署。", "zh"),
            Some("这个 API 怎么部署。".to_string())
        );
        assert_eq!(finalize_transcript("API", "zh"), Some("API".to_string()));
    }

    #[test]
    fn silence_audio_does_not_look_like_speech() {
        let samples = vec![0.0; 16_000];
        assert!(!speech_stats(&samples).looks_like_speech());
    }

    #[test]
    fn synthetic_voice_burst_passes_gate_and_denoises() {
        let mut samples = vec![0.0; 16_000];
        for (i, sample) in samples.iter_mut().enumerate().skip(4_000).take(4_800) {
            let phase = (i as f32 / 16_000.0) * 440.0 * std::f32::consts::TAU;
            *sample = phase.sin() * 0.05;
        }

        let stats = speech_stats(&samples);
        assert!(stats.looks_like_speech(), "{stats:?}");
        let denoised = denoise_for_asr(&samples, &stats);
        assert!(!denoised.is_empty());
    }
}
