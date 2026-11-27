use crate::companion_extras::{
    ensure_tts_engine_ready_with_mode_blocking, fetch_voices_from_engine, is_blocked_drop_target,
    TtsVoiceOption,
};
use crate::dialogue_parser::{parse_dialogue, DialogueAction};
use crate::i18n::UiLang;
use regex::Regex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use url::Url;

/// Maximum uncompressed text bytes read from an EPUB or text file (DoS / ZIP bomb guard).
const MAX_NARRATION_TEXT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum utterances queued per narration session for responsive desktop playback.
const MAX_UTTERANCES_PER_SESSION: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Emotion {
    Neutral,
    Happy,
    Sad,
    Angry,
    Fear,
    Surprise,
    Disgust,
}

impl Emotion {
    pub fn as_str(self) -> &'static str {
        match self {
            Emotion::Neutral => "Neutral",
            Emotion::Happy => "Happy",
            Emotion::Sad => "Sad",
            Emotion::Angry => "Angry",
            Emotion::Fear => "Fear",
            Emotion::Surprise => "Surprise",
            Emotion::Disgust => "Disgust",
        }
    }

    /// Map detected emotion to Sakura (\0) facial expression surface ID (0..=9).
    pub fn sakura_surface(self) -> i32 {
        match self {
            Emotion::Neutral => 0,
            Emotion::Happy => 5,
            Emotion::Sad => 4,
            Emotion::Angry => 7,
            Emotion::Fear => 6,
            Emotion::Surprise => 3,
            Emotion::Disgust => 1,
        }
    }

    pub fn label(self, lang: UiLang) -> &'static str {
        match (lang, self) {
            (UiLang::Ko, Emotion::Neutral) => "차분",
            (UiLang::Ko, Emotion::Happy) => "기쁨",
            (UiLang::Ko, Emotion::Sad) => "슬픔",
            (UiLang::Ko, Emotion::Angry) => "분노",
            (UiLang::Ko, Emotion::Fear) => "긴장",
            (UiLang::Ko, Emotion::Surprise) => "놀람",
            (UiLang::Ko, Emotion::Disgust) => "짜증",
            (UiLang::Ja, Emotion::Neutral) => "平常",
            (UiLang::Ja, Emotion::Happy) => "喜び",
            (UiLang::Ja, Emotion::Sad) => "悲しみ",
            (UiLang::Ja, Emotion::Angry) => "怒り",
            (UiLang::Ja, Emotion::Fear) => "緊張",
            (UiLang::Ja, Emotion::Surprise) => "驚き",
            (UiLang::Ja, Emotion::Disgust) => "不快",
            (UiLang::En, _) => self.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UtteranceKind {
    Heading,
    Narration,
    Dialogue,
    Monologue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NarratorPreset {
    Auto,
    Mai,
    Shino,
    Sayo,
    Airi,
}

impl NarratorPreset {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
            "mai" | "まい" => NarratorPreset::Mai,
            "shino" | "天深シノ" => NarratorPreset::Shino,
            "sayo" | "小夜" | "小夜/sayo" => NarratorPreset::Sayo,
            "airi" | "みちのくあいり" => NarratorPreset::Airi,
            _ => NarratorPreset::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NarratorPreset::Auto => "auto",
            NarratorPreset::Mai => "mai",
            NarratorPreset::Shino => "shino",
            NarratorPreset::Sayo => "sayo",
            NarratorPreset::Airi => "airi",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utterance {
    pub index: usize,
    pub episode: String,
    pub kind: UtteranceKind,
    pub text: String,
    pub emotion: Emotion,
    pub intensity: f32,
    pub pause_after_ms: u64,
    pub has_emphasis: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EmotionProsody {
    pub speed_scale: f64,
    pub pitch_scale: f64,
    pub intonation_scale: f64,
    pub volume_scale: f64,
    pub tempo_dynamics_scale: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NarrationStatus {
    pub active: bool,
    pub title: String,
    pub source_kind: String,
    pub preset: String,
    pub speaker_name: String,
    pub current_index: usize,
    pub total_utterances: usize,
    pub current_emotion: String,
    pub current_text: String,
    pub last_error: String,
}

#[derive(Debug, Clone)]
pub struct NarrationDocument {
    pub title: String,
    pub source_kind: String,
    pub utterances: Vec<Utterance>,
}

static NARRATION_GEN: AtomicU64 = AtomicU64::new(1);
static NARRATION_STOP: AtomicBool = AtomicBool::new(false);

fn active_child_slot() -> &'static Mutex<Option<Child>> {
    static SLOT: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn status_slot() -> &'static Mutex<NarrationStatus> {
    static SLOT: OnceLock<Mutex<NarrationStatus>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(NarrationStatus::default()))
}

/// Snapshot current emotional narration state (for GUI menu & `GET /api/narrate`).
pub fn current_narration_status() -> NarrationStatus {
    status_slot()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// Returns true if an emotional narration session is currently playing.
pub fn is_narration_active() -> bool {
    status_slot().lock().map(|s| s.active).unwrap_or(false)
}

/// Immediately stop any active narration session and terminate the active audio player child.
pub fn stop_narration() -> bool {
    let was_active = is_narration_active();
    NARRATION_GEN.fetch_add(1, Ordering::SeqCst);
    NARRATION_STOP.store(true, Ordering::SeqCst);
    if let Ok(mut guard) = active_child_slot().lock() {
        if let Some(ref mut child) = *guard {
            let _ = child.kill();
            let _ = child.wait();
        }
        *guard = None;
    }
    if let Ok(mut st) = status_slot().lock() {
        st.active = false;
    }
    was_active
}

/// Returns true if the given file extension is supported for direct emotional narration.
pub fn is_narratable_extension(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "epub" | "txt" | "md" | "markdown")
}

fn decode_html_entities(input: &str) -> String {
    let mut out = input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x3000;", " ")
        .replace("&#12288;", " ");
    let num_re = Regex::new(r"&#(x[0-9a-fA-F]+|[0-9]+);").unwrap();
    out = num_re
        .replace_all(&out, |caps: &regex::Captures| {
            let raw = &caps[1];
            let code = if let Some(hex) = raw.strip_prefix('x').or_else(|| raw.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                raw.parse::<u32>().ok()
            };
            code.and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_default()
        })
        .into_owned();
    out
}

/// Normalize EPUB3 `<ruby>` and bouten emphasis tags for natural TTS reading.
/// Returns `(cleaned_plain_text, has_emphasis)`.
pub fn normalize_xhtml_block_for_tts(html: &str) -> (String, bool) {
    let mut has_emphasis = html.contains("bouten");
    let rp_re = Regex::new(r"(?is)<rp\b[^>]*>.*?</rp>").unwrap();
    let ruby_re = Regex::new(r"(?is)<ruby\b([^>]*)>(.*?)</ruby>").unwrap();
    let rt_re = Regex::new(r"(?is)<rt\b[^>]*>(.*?)</rt>").unwrap();
    let tag_re = Regex::new(r"(?is)<[^>]+>").unwrap();

    let no_rp = rp_re.replace_all(html, "");
    let replaced_ruby = ruby_re.replace_all(&no_rp, |caps: &regex::Captures| {
        let attrs = &caps[1];
        let inner = &caps[2];
        let mut rt_parts = Vec::new();
        for rt_cap in rt_re.captures_iter(inner) {
            let rt_plain = tag_re.replace_all(&rt_cap[1], "");
            rt_parts.push(rt_plain.trim().to_string());
        }
        let rt_joined = rt_parts.join("");
        let is_bouten = attrs.contains("bouten")
            || (!rt_joined.is_empty()
                && rt_joined
                    .chars()
                    .all(|ch| matches!(ch, '﹅' | '・' | '●' | '◦' | '丶' | '、' | '。')));
        let base_only = rt_re.replace_all(inner, "");
        let base_plain = tag_re.replace_all(&base_only, "").into_owned();
        if is_bouten {
            has_emphasis = true;
            base_plain
        } else if !rt_joined.is_empty() {
            rt_joined
        } else {
            base_plain
        }
    });

    let stripped = tag_re.replace_all(&replaced_ruby, "");
    let decoded = decode_html_entities(&stripped);
    (clean_text_for_tts(&decoded), has_emphasis)
}

/// Clean decorative lines, kaomoji, brackets, and excess ellipses for smooth TTS synthesis.
pub fn clean_text_for_tts(raw: &str) -> String {
    let trimmed = raw.replace('\u{3000}', " ").trim().to_string();
    if trimmed.is_empty() {
        return String::new();
    }
    let kaomoji_re =
        Regex::new(r"[（(]\s*[\^;；・ω｀´\-_~*°∀▽Д]+\s*[)）]").unwrap();
    let deco_re =
        Regex::new(r"^[＊*◆◇■□●○★☆▲△▼▽\-_=＝~～・─―—\s]+$").unwrap();
    let s = kaomoji_re.replace_all(&trimmed, "").trim().to_string();
    if s.is_empty() || deco_re.is_match(&s) {
        return String::new();
    }
    let b1 = Regex::new(r"【([^】]+)】(\S)").unwrap();
    let s = b1.replace_all(&s, "$1、$2");
    let b2 = Regex::new(r"【([^】]+)】").unwrap();
    let s = b2.replace_all(&s, "$1");
    let b3 = Regex::new(r"〈([^〉]+)〉").unwrap();
    let s = b3.replace_all(&s, "$1");
    let dash_re = Regex::new(r"[―—─]{2,}").unwrap();
    let s = dash_re.replace_all(&s, "、");
    let ell_re = Regex::new(r"[…]{2,}").unwrap();
    let s = ell_re.replace_all(&s, "…");
    let dot_re = Regex::new(r"[・]{3,}").unwrap();
    let s = dot_re.replace_all(&s, "…");
    let ws_re = Regex::new(r"\s{2,}").unwrap();
    let s = ws_re.replace_all(&s, " ");
    s.trim_start_matches('、').trim().to_string()
}

fn split_long_narration(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let mut sentences: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        cur.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n') {
            let t = cur.trim().to_string();
            if !t.is_empty() {
                sentences.push(t);
            }
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur.trim().to_string());
    }
    if sentences.is_empty() {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut buf = String::new();
    for sent in sentences {
        let combined_len = buf.chars().count() + sent.chars().count();
        if !buf.is_empty() && combined_len > max_chars {
            chunks.push(buf.clone());
            buf.clear();
        }
        buf.push_str(&sent);
    }
    if !buf.is_empty() {
        chunks.push(buf);
    }
    chunks
}

fn score_text_emotions(text: &str) -> (Emotion, f32) {
    let low = text.to_lowercase();
    const ANGRY: &[(&str, f32)] = &[
        ("叫", 1.8), ("怒鳴", 2.4), ("怒", 2.0), ("睨", 1.8), ("許さ", 2.0),
        ("ふざけ", 2.2), ("殺", 1.8), ("くそ", 2.0), ("クソ", 2.0), ("黙れ", 2.4),
        ("貴様", 1.8), ("てめえ", 2.2), ("激怒", 2.4), ("裏切", 1.6),
        ("화나", 2.2), ("분노", 2.4), ("소리쳤", 2.0), ("닥쳐", 2.4), ("용서", 1.8),
        ("furious", 2.2), ("angry", 2.0), ("shouted", 1.8), ("shut up", 2.2),
    ];
    const SAD: &[(&str, f32)] = &[
        ("泣", 2.2), ("涙", 2.2), ("悲", 2.2), ("辛", 2.0), ("苦し", 1.8),
        ("寂し", 1.8), ("絶望", 2.2), ("情けな", 1.8), ("虚し", 1.8), ("ごめん", 1.6),
        ("눈물", 2.2), ("슬프", 2.2), ("울었", 2.2), ("외롭", 1.8), ("절망", 2.2), ("미안", 1.6),
        ("tears", 2.2), ("cried", 2.2), ("sad", 2.0), ("despair", 2.2), ("lonely", 1.8),
    ];
    const HAPPY: &[(&str, f32)] = &[
        ("笑", 2.0), ("嬉", 2.4), ("楽", 1.8), ("歓声", 2.2), ("喜", 2.4),
        ("微笑", 1.8), ("笑顔", 2.2), ("幸せ", 2.2), ("ありがと", 1.8), ("大好き", 2.0),
        ("기쁘", 2.4), ("웃었", 2.0), ("행복", 2.2), ("미소", 1.8), ("고마워", 1.8), ("사랑", 1.8),
        ("smiled", 2.0), ("laughed", 2.0), ("happy", 2.2), ("delighted", 2.2), ("thank you", 1.6),
    ];
    const FEAR: &[(&str, f32)] = &[
        ("怖", 2.2), ("恐怖", 2.5), ("震", 2.0), ("怯", 2.3), ("恐ろ", 2.2),
        ("悲鳴", 2.2), ("危な", 1.8), ("絶体絶命", 2.2), ("凍りつ", 2.0), ("息を呑", 1.8),
        ("무섭", 2.2), ("공포", 2.5), ("떨렸", 2.0), ("비명", 2.2), ("얼어붙", 2.0),
        ("terrified", 2.5), ("scared", 2.2), ("trembled", 2.0), ("screamed", 2.2),
    ];
    const SURPRISE: &[(&str, f32)] = &[
        ("驚", 2.2), ("まさか", 2.0), ("えっ", 2.2), ("なっ", 1.8), ("信じられ", 1.8),
        ("目を見開", 2.2), ("！？", 2.2), ("!?", 2.2), ("嘘", 1.6),
        ("놀랐", 2.2), ("설마", 2.0), ("믿을 수 없", 1.8), ("눈을 크게", 2.2),
        ("surprised", 2.2), ("shocked", 2.2), ("unbelievable", 1.8),
    ];
    const DISGUST: &[(&str, f32)] = &[
        ("嫌", 1.8), ("呆", 2.0), ("うんざり", 2.3), ("最低", 2.0), ("気持ち悪", 2.2),
        ("ため息", 1.6), ("げんなり", 2.0), ("馬鹿", 1.5),
        ("짜증", 2.2), ("역겨", 2.3), ("최악", 2.0), ("한숨", 1.6), ("지긋지긋", 2.2),
        ("disgusting", 2.3), ("annoying", 1.8), ("sighed", 1.5),
    ];

    let buckets: [(Emotion, &[(&str, f32)]); 6] = [
        (Emotion::Angry, ANGRY),
        (Emotion::Sad, SAD),
        (Emotion::Happy, HAPPY),
        (Emotion::Fear, FEAR),
        (Emotion::Surprise, SURPRISE),
        (Emotion::Disgust, DISGUST),
    ];

    let mut best_emo = Emotion::Neutral;
    let mut best_score = 0.0f32;
    for (emo, cues) in buckets {
        let mut score = 0.0f32;
        for (kw, w) in cues {
            if low.contains(kw) {
                score += *w;
            }
        }
        if score > best_score {
            best_score = score;
            best_emo = emo;
        }
    }

    if best_score < 1.5 {
        if text.contains("！？") || text.contains("!?") {
            return (Emotion::Surprise, 1.35);
        }
        return (Emotion::Neutral, 1.0);
    }

    let excl_boost = if text.contains('！') || text.contains('!') {
        0.18
    } else {
        0.0
    };
    let intensity = (1.1 + best_score * 0.14 + excl_boost).clamp(1.0, 1.8);
    (best_emo, (intensity * 100.0).round() / 100.0)
}

/// Segment raw text (novel, essay, markdown) into emotion-tagged utterances.
pub fn build_utterances_from_text(episode: &str, raw_text: &str) -> Vec<Utterance> {
    let quote_re = Regex::new(r"(「[^」]+」|『[^』]+』|“[^”]+”|（[^）]{3,}）)").unwrap();
    let mut utterances = Vec::new();
    let mut idx = 0usize;

    for raw_line in raw_text.lines() {
        if utterances.len() >= MAX_UTTERANCES_PER_SESSION {
            break;
        }
        let line_trim = raw_line.trim();
        if line_trim.is_empty() {
            continue;
        }
        let is_heading = line_trim.starts_with('#')
            || (line_trim.starts_with('第')
                && (line_trim.contains('話') || line_trim.contains('章'))
                && line_trim.chars().count() <= 40);
        let stripped_heading = line_trim.trim_start_matches('#').trim();
        let cleaned_line = clean_text_for_tts(stripped_heading);
        if cleaned_line.is_empty() {
            continue;
        }

        if is_heading {
            utterances.push(Utterance {
                index: idx,
                episode: episode.to_string(),
                kind: UtteranceKind::Heading,
                text: cleaned_line,
                emotion: Emotion::Neutral,
                intensity: 1.0,
                pause_after_ms: 800,
                has_emphasis: false,
            });
            idx += 1;
            continue;
        }

        // Split quotes / monologues from surrounding narration
        let mut last = 0usize;
        let mut narr_buf = String::new();
        let flush_narr = |buf: &mut String, out: &mut Vec<Utterance>, idx_ref: &mut usize| {
            let t = clean_text_for_tts(buf);
            buf.clear();
            if t.is_empty() {
                return;
            }
            for chunk in split_long_narration(&t, 110) {
                if out.len() >= MAX_UTTERANCES_PER_SESSION {
                    break;
                }
                let (emo, intensity) = score_text_emotions(&chunk);
                out.push(Utterance {
                    index: *idx_ref,
                    episode: episode.to_string(),
                    kind: UtteranceKind::Narration,
                    text: chunk,
                    emotion: emo,
                    intensity,
                    pause_after_ms: 340,
                    has_emphasis: false,
                });
                *idx_ref += 1;
            }
        };

        for m in quote_re.find_iter(&cleaned_line) {
            if m.start() > last {
                narr_buf.push_str(&cleaned_line[last..m.start()]);
            }
            let seg = m.as_str();
            let inner = seg
                .trim_start_matches(['「', '『', '“', '（'])
                .trim_end_matches(['」', '』', '”', '）'])
                .trim();
            // Short inline terms like 「魔法」 inside narration stay part of narration
            let is_spoken = inner.chars().count() >= 5
                || inner.contains('。')
                || inner.contains('！')
                || inner.contains('？')
                || inner.contains('!')
                || inner.contains('?');
            if is_spoken && !inner.is_empty() {
                flush_narr(&mut narr_buf, &mut utterances, &mut idx);
                if utterances.len() >= MAX_UTTERANCES_PER_SESSION {
                    break;
                }
                let kind = if seg.starts_with('（') {
                    UtteranceKind::Monologue
                } else {
                    UtteranceKind::Dialogue
                };
                let cleaned_inner = clean_text_for_tts(inner);
                if !cleaned_inner.is_empty() {
                    let has_emph = inner.contains('！') || inner.contains('!');
                    for chunk in split_long_narration(&cleaned_inner, 110) {
                        if utterances.len() >= MAX_UTTERANCES_PER_SESSION {
                            break;
                        }
                        let (emo, intensity) = score_text_emotions(&chunk);
                        utterances.push(Utterance {
                            index: idx,
                            episode: episode.to_string(),
                            kind,
                            text: chunk,
                            emotion: emo,
                            intensity,
                            pause_after_ms: if kind == UtteranceKind::Monologue { 420 } else { 320 },
                            has_emphasis: has_emph,
                        });
                        idx += 1;
                    }
                }
            } else {
                narr_buf.push_str(seg);
            }
            last = m.end();
        }
        if last < cleaned_line.len() {
            narr_buf.push_str(&cleaned_line[last..]);
        }
        flush_narr(&mut narr_buf, &mut utterances, &mut idx);
    }

    utterances
}

/// Extract title and emotion-tagged utterances from an EPUB3/EPUB2 archive (100% Safe Rust).
pub fn load_epub_document(
    path: &Path,
    episode_spec: Option<&str>,
) -> Result<NarrationDocument, String> {
    if is_blocked_drop_target(path) {
        return Err("機密ファイルは朗読できません。".to_string());
    }
    let file = fs::File::open(path).map_err(|e| format!("EPUBを開けません: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("EPUB ZIP解析エラー: {e}"))?;

    let mut title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("EPUB")
        .to_string();
    let mut chapter_names: Vec<String> = Vec::new();
    let title_re = Regex::new(r"(?is)<(?:dc:)?title\b[^>]*>(.*?)</(?:dc:)?title>").unwrap();

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        if name.contains("..") || name.starts_with('/') {
            continue;
        }
        let low = name.to_ascii_lowercase();
        if low.ends_with(".opf") {
            let mut opf = String::new();
            if entry
                .by_ref()
                .take(256 * 1024)
                .read_to_string(&mut opf)
                .is_ok()
            {
                if let Some(cap) = title_re.captures(&opf) {
                    let t = decode_html_entities(cap[1].trim());
                    if !t.is_empty() {
                        title = t;
                    }
                }
            }
        } else if (low.ends_with(".xhtml") || low.ends_with(".html") || low.ends_with(".htm"))
            && !low.contains("nav.")
            && !low.contains("toc.")
            && !low.contains("cover.")
        {
            chapter_names.push(name);
        }
    }

    chapter_names.sort();
    // Prefer episodes/ep_*.xhtml if present (narou_epub convention)
    let ep_only: Vec<String> = chapter_names
        .iter()
        .filter(|n| n.contains("episodes/ep_") || n.contains("ep_"))
        .cloned()
        .collect();
    if !ep_only.is_empty() {
        chapter_names = ep_only;
    }

    if chapter_names.is_empty() {
        return Err("EPUB内に朗読可能な本文（XHTML/HTML）が見つかりませんでした。".to_string());
    }

    // Episode filter: default to first chapter if spec is None/empty; "all" reads up to MAX_UTTERANCES_PER_SESSION
    let selected_chapters: Vec<String> = match episode_spec.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(spec) if spec.eq_ignore_ascii_case("all") => chapter_names,
        Some(spec) => {
            if let Ok(n) = spec.parse::<usize>() {
                let idx = n.saturating_sub(1).min(chapter_names.len() - 1);
                vec![chapter_names[idx].clone()]
            } else {
                let matched: Vec<String> = chapter_names
                    .iter()
                    .filter(|c| c.to_ascii_lowercase().contains(&spec.to_ascii_lowercase()))
                    .cloned()
                    .collect();
                if matched.is_empty() {
                    vec![chapter_names[0].clone()]
                } else {
                    matched
                }
            }
        }
        None => chapter_names.into_iter().take(3).collect(),
    };

    let block_re = Regex::new(r"(?is)<(h[1-4]|p|div)\b[^>]*>(.*?)</(?:h[1-4]|p|div)>").unwrap();
    let mut total_bytes = 0usize;
    let mut all_utterances: Vec<Utterance> = Vec::new();

    for ch_name in selected_chapters {
        if all_utterances.len() >= MAX_UTTERANCES_PER_SESSION || total_bytes >= MAX_NARRATION_TEXT_BYTES {
            break;
        }
        let Ok(mut entry) = archive.by_name(&ch_name) else {
            continue;
        };
        let mut raw_html = String::new();
        let remaining = MAX_NARRATION_TEXT_BYTES.saturating_sub(total_bytes) as u64;
        if entry.by_ref().take(remaining).read_to_string(&mut raw_html).is_err() {
            continue;
        }
        total_bytes += raw_html.len();

        let ep_label = Path::new(&ch_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ep_1")
            .to_string();

        let mut ep_lines = Vec::new();
        for cap in block_re.captures_iter(&raw_html) {
            let tag = cap[1].to_ascii_lowercase();
            let (plain, _emph) = normalize_xhtml_block_for_tts(&cap[2]);
            if plain.is_empty() {
                continue;
            }
            if tag.starts_with('h') {
                ep_lines.push(format!("# {plain}"));
            } else {
                ep_lines.push(plain);
            }
        }
        if ep_lines.is_empty() {
            let (plain, _) = normalize_xhtml_block_for_tts(&raw_html);
            if !plain.is_empty() {
                ep_lines.push(plain);
            }
        }

        let mut ch_utts = build_utterances_from_text(&ep_label, &ep_lines.join("\n"));
        for u in &mut ch_utts {
            u.index = all_utterances.len();
            if all_utterances.len() < MAX_UTTERANCES_PER_SESSION {
                all_utterances.push(u.clone());
            }
        }
    }

    if all_utterances.is_empty() {
        return Err("EPUBから朗読テキストを抽出できませんでした。".to_string());
    }

    Ok(NarrationDocument {
        title,
        source_kind: "epub".to_string(),
        utterances: all_utterances,
    })
}

/// Load a plain text (`.txt`) or Markdown (`.md`) file into emotion-tagged utterances.
pub fn load_text_file_document(path: &Path) -> Result<NarrationDocument, String> {
    if is_blocked_drop_target(path) {
        return Err("機密ファイルは朗読できません。".to_string());
    }
    let raw_bytes = fs::read(path).map_err(|e| format!("ファイルを読めません: {e}"))?;
    let capped = if raw_bytes.len() > MAX_NARRATION_TEXT_BYTES {
        &raw_bytes[..MAX_NARRATION_TEXT_BYTES]
    } else {
        &raw_bytes[..]
    };
    let text = match String::from_utf8(capped.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(capped);
            decoded.into_owned()
        }
    };
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Text")
        .to_string();
    let utterances = build_utterances_from_text("text", &text);
    if utterances.is_empty() {
        return Err("ファイル内に朗読可能なテキストがありませんでした。".to_string());
    }
    Ok(NarrationDocument {
        title,
        source_kind: path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_ascii_lowercase(),
        utterances,
    })
}

/// Load either `.epub`, `.txt`, or `.md` automatically based on extension.
pub fn load_narration_document(
    path: &Path,
    episode_spec: Option<&str>,
) -> Result<NarrationDocument, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "epub" {
        load_epub_document(path, episode_spec)
    } else {
        load_text_file_document(path)
    }
}

/// Resolve the best AivisSpeech / VOICEVOX `(style_id, speaker_name)` for a given `(preset, kind, emotion)`.
/// Follows the user's preferred narrator ranking (`まい` -> `天深シノ` -> `小夜/SAYO` -> `みちのくあいり`).
pub fn resolve_emotional_speaker(
    voices: &[TtsVoiceOption],
    preset: NarratorPreset,
    kind: UtteranceKind,
    emotion: Emotion,
    fallback_speaker: Option<i64>,
) -> Option<(i64, String)> {
    resolve_emotional_speaker_with_override(voices, preset, kind, emotion, None, fallback_speaker)
}

/// Resolve `(style_id, speaker_name)` honoring an explicit user-selected speaker model (`explicit_speaker_id`)
/// when configured (e.g., for Kindle narration), while still adapting emotional styles within that speaker.
pub fn resolve_emotional_speaker_with_override(
    voices: &[TtsVoiceOption],
    preset: NarratorPreset,
    kind: UtteranceKind,
    emotion: Emotion,
    explicit_speaker_id: Option<i64>,
    fallback_speaker: Option<i64>,
) -> Option<(i64, String)> {
    if voices.is_empty() {
        return explicit_speaker_id
            .or(fallback_speaker)
            .map(|id| (id, format!("speaker:{id}")));
    }

    let explicit_match = explicit_speaker_id
        .and_then(|eid| voices.iter().find(|v| v.style_id == eid));

    let speaker_prefs: &[&str] = match preset {
        NarratorPreset::Mai => &["まい"],
        NarratorPreset::Shino => &["天深シノ"],
        NarratorPreset::Sayo => &["小夜/SAYO", "小夜"],
        NarratorPreset::Airi => &["みちのくあいり"],
        NarratorPreset::Auto => &["まい", "天深シノ", "小夜/SAYO", "みちのくあいり", "morioki", "まお"],
    };

    let chosen_speaker_name: String = explicit_match
        .map(|v| v.speaker_name.clone())
        .or_else(|| {
            speaker_prefs.iter().find_map(|kw| {
                voices
                    .iter()
                    .find(|v| v.speaker_name.contains(kw))
                    .map(|v| v.speaker_name.clone())
            })
        })
        .or_else(|| {
            fallback_speaker.and_then(|fid| {
                voices
                    .iter()
                    .find(|v| v.style_id == fid)
                    .map(|v| v.speaker_name.clone())
            })
        })
        .unwrap_or_else(|| voices[0].speaker_name.clone());

    let sp_styles: Vec<&TtsVoiceOption> = voices
        .iter()
        .filter(|v| v.speaker_name == chosen_speaker_name)
        .collect();

    let style_prefs: &[&str] = match (kind, emotion) {
        (UtteranceKind::Heading | UtteranceKind::Narration, _) => {
            &["ノーマル", "標準", "おちつき", "ふつー"]
        }
        (UtteranceKind::Monologue, _) => &["ささやき", "せつなめ", "おちつき", "ノーマル", "標準"],
        (UtteranceKind::Dialogue, Emotion::Happy) => {
            &["喜び", "じょうきげん", "あまあま", "たのしい", "ノーマル", "標準"]
        }
        (UtteranceKind::Dialogue, Emotion::Surprise) => {
            &["驚き", "じょうきげん", "喜び", "ノーマル", "標準"]
        }
        (UtteranceKind::Dialogue, Emotion::Angry) => {
            &["怒り", "ふきげん", "ツンギレ", "不機嫌", "ノーマル", "標準"]
        }
        (UtteranceKind::Dialogue, Emotion::Disgust) => {
            &["嫌悪", "ふきげん", "からかい", "ノーマル", "標準"]
        }
        (UtteranceKind::Dialogue, Emotion::Sad) => {
            &["悲しみ", "ささやき", "せつなめ", "かなしみ", "ノーマル", "標準"]
        }
        (UtteranceKind::Dialogue, Emotion::Fear) => {
            &["恐怖", "ささやき", "悲しみ", "せつなめ", "ノーマル", "標準"]
        }
        (UtteranceKind::Dialogue, Emotion::Neutral) => &["ノーマル", "標準", "ふつー", "おちつき"],
    };

    for kw in style_prefs {
        if let Some(v) = sp_styles.iter().find(|v| v.style_name.contains(kw)) {
            return Some((v.style_id, v.speaker_name.clone()));
        }
    }
    if let Some(em) = explicit_match {
        return Some((em.style_id, em.speaker_name.clone()));
    }
    sp_styles
        .first()
        .map(|v| (v.style_id, v.speaker_name.clone()))
}

/// Compute AivisSpeech / VOICEVOX emotional prosody parameters (`speedScale`, `pitchScale`,
/// `intonationScale`, `volumeScale`, `tempoDynamicsScale`) faithfully ported from `narrator.py`.
pub fn compute_emotional_prosody(
    utt: &Utterance,
    preset: NarratorPreset,
    user_speed: f32,
    user_intonation: f32,
) -> EmotionProsody {
    let speed_mul = crate::config_store::clamp_voice_speed(user_speed) as f64;
    let inton_mul = crate::config_store::clamp_voice_intonation(user_intonation) as f64;

    let (mut speed, pitch, mut intonation, mut volume): (f64, f64, f64, f64) = match utt.kind {
        UtteranceKind::Heading => (0.94, 0.0, 1.00, 1.0),
        UtteranceKind::Narration => match preset {
            NarratorPreset::Sayo => (0.96, 0.0, 1.05, 1.0),
            _ => (0.97, 0.0, 1.04, 1.0),
        },
        UtteranceKind::Dialogue | UtteranceKind::Monologue => {
            let delta = (utt.intensity - 1.0).max(0.0) as f64;
            match utt.emotion {
                Emotion::Happy => (1.00 + 0.02 * delta, 0.008 * delta, 1.12 + 0.10 * delta, 1.0),
                Emotion::Sad => (0.94 - 0.03 * delta, -0.010 * delta, 1.08 + 0.06 * delta, 0.96),
                Emotion::Angry => (1.01 + 0.03 * delta, 0.005 * delta, 1.16 + 0.14 * delta, 1.03),
                Emotion::Fear => (0.95 + 0.02 * delta, 0.005 * delta, 1.10 + 0.08 * delta, 0.94),
                Emotion::Surprise => (1.01 + 0.03 * delta, 0.010 * delta, 1.16 + 0.12 * delta, 1.02),
                Emotion::Disgust => (0.95, -0.008, 1.08 + 0.06 * delta, 0.98),
                Emotion::Neutral => (0.98, 0.0, 1.06, 1.0),
            }
        }
    };

    if utt.kind == UtteranceKind::Monologue {
        speed = speed.min(0.95);
        volume *= if preset == NarratorPreset::Shino { 0.92 } else { 0.88 };
    }
    if utt.has_emphasis {
        intonation += 0.05;
    }

    let final_speed = (speed * speed_mul).clamp(0.75, 1.30);
    let final_pitch = pitch.clamp(-0.04, 0.04);
    let final_inton = (intonation * inton_mul).clamp(0.85, 1.50);
    let final_vol = volume.clamp(0.75, 1.15);
    let tempo_dynamics = (0.96 + (final_inton - 1.0) * 0.65).clamp(0.75, 1.40);

    EmotionProsody {
        speed_scale: (final_speed * 1000.0).round() / 1000.0,
        pitch_scale: (final_pitch * 1000.0).round() / 1000.0,
        intonation_scale: (final_inton * 1000.0).round() / 1000.0,
        volume_scale: (final_vol * 1000.0).round() / 1000.0,
        tempo_dynamics_scale: (tempo_dynamics * 1000.0).round() / 1000.0,
    }
}

/// Synthesize a single utterance WAV via AivisSpeech / VOICEVOX with full emotional prosody.
pub fn synthesize_emotional_wav_bytes(
    base: &str,
    speaker: i64,
    text: &str,
    prosody: &EmotionProsody,
) -> Result<Vec<u8>, String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_millis(1200))
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(|e| e.to_string())?;
    let mut query_url =
        Url::parse(&format!("{}/audio_query", base.trim_end_matches('/')))
            .map_err(|e| e.to_string())?;
    query_url
        .query_pairs_mut()
        .append_pair("text", text)
        .append_pair("speaker", &speaker.to_string());

    let q_resp = client.post(query_url).send().map_err(|e| e.to_string())?;
    if !q_resp.status().is_success() {
        return Err(format!("audio_query HTTP {}", q_resp.status()));
    }
    let mut query_json = q_resp.json::<Value>().map_err(|e| e.to_string())?;
    if let Some(obj) = query_json.as_object_mut() {
        obj.insert("speedScale".to_string(), json!(prosody.speed_scale));
        obj.insert("pitchScale".to_string(), json!(prosody.pitch_scale));
        obj.insert("intonationScale".to_string(), json!(prosody.intonation_scale));
        obj.insert("volumeScale".to_string(), json!(prosody.volume_scale));
        obj.insert("prePhonemeLength".to_string(), json!(0.05));
        obj.insert("postPhonemeLength".to_string(), json!(0.08));
        if obj.contains_key("tempoDynamicsScale") {
            obj.insert(
                "tempoDynamicsScale".to_string(),
                json!(prosody.tempo_dynamics_scale),
            );
        }
    }

    let synth_url = format!("{}/synthesis?speaker={speaker}", base.trim_end_matches('/'));
    let s_resp = client
        .post(synth_url)
        .json(&query_json)
        .send()
        .map_err(|e| e.to_string())?;
    if !s_resp.status().is_success() {
        return Err(format!("synthesis HTTP {}", s_resp.status()));
    }
    let wav = s_resp.bytes().map_err(|e| e.to_string())?.to_vec();
    if wav.len() < 44 {
        return Err("synthesized audio payload too small".to_string());
    }
    Ok(wav)
}

fn synthesize_emotional_wav_with_retry(
    base: &str,
    speaker: i64,
    text: &str,
    prosody: &EmotionProsody,
    my_gen: u64,
) -> Result<Vec<u8>, String> {
    match synthesize_emotional_wav_bytes(base, speaker, text, prosody) {
        Ok(wav) => Ok(wav),
        Err(first_err) => {
            if NARRATION_STOP.load(Ordering::SeqCst)
                || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
            {
                return Err(first_err);
            }
            std::thread::sleep(Duration::from_millis(350));
            if NARRATION_STOP.load(Ordering::SeqCst)
                || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
            {
                return Err(first_err);
            }
            synthesize_emotional_wav_bytes(base, speaker, text, prosody).map_err(|e| {
                format!("{first_err} (retry: {e})")
            })
        }
    }
}

fn play_wav_interruptible(path: &Path, my_gen: u64) -> bool {
    let mut cmd = {
        #[cfg(target_os = "macos")]
        {
            let mut c = Command::new("afplay");
            c.arg(path);
            c
        }
        #[cfg(target_os = "windows")]
        {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "(New-Object Media.SoundPlayer $env:NANIKAAI_WAV).PlaySync()",
            ]);
            c.env("NANIKAAI_WAV", path);
            c
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut c = Command::new("aplay");
            c.args(["-q"]).arg(path);
            c
        }
    };

    let Ok(child) = cmd.spawn() else {
        return false;
    };
    if let Ok(mut guard) = active_child_slot().lock() {
        *guard = Some(child);
    }

    loop {
        if NARRATION_STOP.load(Ordering::SeqCst) || NARRATION_GEN.load(Ordering::SeqCst) != my_gen {
            if let Ok(mut guard) = active_child_slot().lock() {
                if let Some(ref mut ch) = *guard {
                    let _ = ch.kill();
                    let _ = ch.wait();
                }
                *guard = None;
            }
            return false;
        }
        let finished = {
            if let Ok(mut guard) = active_child_slot().lock() {
                if let Some(ref mut ch) = *guard {
                    match ch.try_wait() {
                        Ok(Some(st)) => {
                            *guard = None;
                            Some(st.success())
                        }
                        Ok(None) => None,
                        Err(_) => {
                            *guard = None;
                            Some(false)
                        }
                    }
                } else {
                    Some(false)
                }
            } else {
                Some(false)
            }
        };
        if let Some(ok) = finished {
            return ok;
        }
        std::thread::sleep(Duration::from_millis(45));
    }
}

/// Start emotional AivisSpeech narration in the background, syncing mascot facial expressions
/// and subtitle balloons in real time without polluting chat history.
#[allow(clippy::too_many_arguments)]
pub fn start_narration_async(
    doc: NarrationDocument,
    preset: NarratorPreset,
    voicevox_url: String,
    tts_engine: String,
    fallback_speaker: Option<i64>,
    user_speed: f32,
    user_intonation: f32,
    ui_lang: UiLang,
    dry_run: bool,
    on_balloon: Arc<dyn Fn(Vec<DialogueAction>) + Send + Sync>,
    on_status: Option<Arc<dyn Fn(String) + Send + Sync>>,
) -> NarrationStatus {
    stop_narration();
    let my_gen = NARRATION_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    NARRATION_STOP.store(false, Ordering::SeqCst);

    let total = doc.utterances.len();
    let initial = NarrationStatus {
        active: !dry_run,
        title: doc.title.clone(),
        source_kind: doc.source_kind.clone(),
        preset: preset.as_str().to_string(),
        speaker_name: String::new(),
        current_index: 0,
        total_utterances: total,
        current_emotion: doc
            .utterances
            .first()
            .map(|u| u.emotion.as_str().to_string())
            .unwrap_or_else(|| "Neutral".to_string()),
        current_text: doc
            .utterances
            .first()
            .map(|u| u.text.clone())
            .unwrap_or_default(),
        last_error: String::new(),
    };
    if let Ok(mut st) = status_slot().lock() {
        *st = initial.clone();
    }
    if dry_run {
        return initial;
    }

    std::thread::spawn(move || {
        let ready = ensure_tts_engine_ready_with_mode_blocking(&voicevox_url, &tts_engine, true);
        let Some(base_url) = ready.base_url else {
            let err_msg = match ui_lang {
                UiLang::Ko => "AivisSpeech / VOICEVOX 엔진을 찾지 못해 감정 낭독을 시작할 수 없어요.",
                UiLang::Ja => "AivisSpeech / VOICEVOX エンジンが見つからないため感情朗読を開始できません。",
                UiLang::En => "AivisSpeech / VOICEVOX engine was not found for emotional narration.",
            };
            if let Ok(mut st) = status_slot().lock() {
                st.active = false;
                st.last_error = err_msg.to_string();
            }
            on_balloon(parse_dialogue(&format!("\\0\\s[3]{err_msg}\\e")));
            return;
        };

        let voices = fetch_voices_from_engine(&base_url).unwrap_or_default();
        let stop_label = match ui_lang {
            UiLang::Ko => "⏹ 낭독 중지",
            UiLang::Ja => "⏹ 朗読停止",
            UiLang::En => "⏹ Stop Reading",
        };

        for (i, utt) in doc.utterances.iter().enumerate() {
            if NARRATION_STOP.load(Ordering::SeqCst) || NARRATION_GEN.load(Ordering::SeqCst) != my_gen {
                break;
            }
            let Some((speaker_id, sp_name)) =
                resolve_emotional_speaker(&voices, preset, utt.kind, utt.emotion, fallback_speaker)
            else {
                break;
            };
            let prosody = compute_emotional_prosody(utt, preset, user_speed, user_intonation);

            if let Ok(mut st) = status_slot().lock() {
                st.active = true;
                st.speaker_name = sp_name.clone();
                st.current_index = i + 1;
                st.current_emotion = utt.emotion.as_str().to_string();
                st.current_text = utt.text.clone();
            }
            if let Some(ref cb) = on_status {
                cb(format!(
                    "📖 {} [{}/{} · {}]",
                    doc.title,
                    i + 1,
                    total,
                    utt.emotion.label(ui_lang)
                ));
            }

            let preview: String = utt.text.chars().take(90).collect();
            let surf = utt.emotion.sakura_surface();
            let emo_lbl = utt.emotion.label(ui_lang);
            let script = format!(
                "\\0\\s[{surf}]\\_q📖 [{}/{} · {emo_lbl}]\\n{preview}\\n\\q[{stop_label},__narrate_stop__]\\_q\\e",
                i + 1,
                total
            );
            on_balloon(parse_dialogue(&script));

            match synthesize_emotional_wav_bytes(&base_url, speaker_id, &utt.text, &prosody) {
                Ok(wav) => {
                    if NARRATION_STOP.load(Ordering::SeqCst)
                        || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                    {
                        break;
                    }
                    let tmp_wav = std::env::temp_dir().join(format!(
                        "nanikaai_narrate_{}_{}.wav",
                        std::process::id(),
                        my_gen
                    ));
                    if fs::write(&tmp_wav, &wav).is_ok() {
                        let _ = play_wav_interruptible(&tmp_wav, my_gen);
                        let _ = fs::remove_file(&tmp_wav);
                    }
                }
                Err(e) => {
                    if let Ok(mut st) = status_slot().lock() {
                        st.last_error = e;
                    }
                    continue;
                }
            }

            // Interruptible natural inter-utterance pause
            let mut waited = 0u64;
            while waited < utt.pause_after_ms {
                if NARRATION_STOP.load(Ordering::SeqCst)
                    || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(45));
                waited += 45;
            }
        }

        let stopped_early =
            NARRATION_STOP.load(Ordering::SeqCst) || NARRATION_GEN.load(Ordering::SeqCst) != my_gen;
        if let Ok(mut st) = status_slot().lock() {
            if NARRATION_GEN.load(Ordering::SeqCst) == my_gen {
                st.active = false;
            }
        }
        if !stopped_early {
            let done_msg = match ui_lang {
                UiLang::Ko => format!("\\0\\s[5]「{}」 낭독을 마쳤어요!\\e", doc.title),
                UiLang::Ja => format!("\\0\\s[5]「{}」の朗読が終わりましたよ！\\e", doc.title),
                UiLang::En => format!("\\0\\s[5]Finished reading \"{}\" aloud!\\e", doc.title),
            };
            on_balloon(parse_dialogue(&done_msg));
        }
    });

    initial
}

// ---------------------------------------------------------------------------
// Kindle Cloud Reader Browser Automation (Open -> Detect Login -> OCR -> Read -> Turn Page)
// ---------------------------------------------------------------------------

pub const DEFAULT_KINDLE_URL: &str = "https://read.amazon.co.jp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KindlePageDirection {
    /// Japanese vertical / right-to-left books (Left Arrow advances to next page).
    Rtl,
    /// Horizontal / left-to-right books (Right Arrow advances to next page).
    Ltr,
}

impl KindlePageDirection {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("rtl").trim().to_ascii_lowercase().as_str() {
            "ltr" | "right" | "horizontal" | "横" | "横書き" => KindlePageDirection::Ltr,
            _ => KindlePageDirection::Rtl,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            KindlePageDirection::Rtl => "rtl",
            KindlePageDirection::Ltr => "ltr",
        }
    }
}

#[derive(Debug, Clone)]
pub struct KindleSessionConfig {
    pub kindle_url: String,
    pub direction: KindlePageDirection,
    pub max_pages: usize,
    pub page_turn_delay_ms: u64,
    pub preset: NarratorPreset,
    pub speaker_id: Option<i64>,
    pub open_browser: bool,
    pub dry_run: bool,
}

impl Default for KindleSessionConfig {
    fn default() -> Self {
        Self {
            kindle_url: DEFAULT_KINDLE_URL.to_string(),
            direction: KindlePageDirection::Rtl,
            max_pages: 50,
            page_turn_delay_ms: 1300,
            preset: NarratorPreset::Auto,
            speaker_id: None,
            open_browser: true,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindleTabState {
    Ready {
        browser: String,
        url: String,
        bounds: Option<(i32, i32, i32, i32)>,
    },
    Library {
        browser: String,
        url: String,
        bounds: Option<(i32, i32, i32, i32)>,
    },
    NeedsLogin {
        browser: String,
        url: String,
    },
    NotFound,
}

/// Strictly validate that a Kindle URL points only to official Amazon Kindle Cloud Reader hosts.
pub fn validate_kindle_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let candidate = if trimmed.is_empty() {
        DEFAULT_KINDLE_URL
    } else {
        trimmed
    };
    let parsed = Url::parse(candidate).map_err(|e| format!("無効なKindle URL: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("Kindle URL は https:// のみ許可されています。".to_string());
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let allowed = host == "read.amazon.co.jp"
        || host == "read.amazon.com"
        || host == "read.amazon.co.uk"
        || host == "read.amazon.de"
        || host == "read.amazon.fr"
        || host == "read.amazon.ca"
        || host == "read.amazon.com.au";
    if !allowed {
        return Err(format!(
            "許可されていないホストです（read.amazon.* のみ許可）: {host}"
        ));
    }
    Ok(parsed.to_string())
}

/// Classify a detected browser tab URL into Ready (open book) / Library / NeedsLogin / NotFound.
pub fn classify_kindle_tab_url(browser: &str, url: &str, bounds: Option<(i32, i32, i32, i32)>) -> KindleTabState {
    let low = url.to_ascii_lowercase();
    if low.contains("amazon.")
        && (low.contains("/ap/signin")
            || low.contains("/ap/mfa")
            || low.contains("/ap/cvf")
            || low.contains("openid.ns"))
    {
        return KindleTabState::NeedsLogin {
            browser: browser.to_string(),
            url: url.to_string(),
        };
    }
    if low.contains("read.amazon.") {
        if low.contains("/kindle-library") || low.contains("landing") {
            return KindleTabState::Library {
                browser: browser.to_string(),
                url: url.to_string(),
                bounds,
            };
        }
        return KindleTabState::Ready {
            browser: browser.to_string(),
            url: url.to_string(),
            bounds,
        };
    }
    KindleTabState::NotFound
}

/// Reflow wrapped physical Kindle OCR lines into complete sentences/paragraphs
/// so vertical column wraps do not chop sentences or dialogues in half.
pub fn reflow_kindle_ocr_lines(cleaned: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_jp_quote = false;
    let mut in_corner_quote = false;

    for raw_line in cleaned.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let starts_new_block = line.starts_with('「')
            || line.starts_with('『')
            || line.starts_with('（')
            || line.starts_with('#')
            || (line.starts_with('第')
                && (line.contains('話') || line.contains('章'))
                && line.chars().count() <= 40);

        if buf.is_empty() {
            buf.push_str(line);
        } else {
            let last_ch = buf.chars().last().unwrap_or(' ');
            let ends_sentence = matches!(
                last_ch,
                '。' | '！' | '？' | '!' | '?' | '」' | '』' | '”' | '）' | '.'
            );
            let should_break =
                (!in_jp_quote && !in_corner_quote && (ends_sentence || starts_new_block))
                    || (starts_new_block && ends_sentence);
            if should_break {
                paragraphs.push(std::mem::take(&mut buf));
                buf.push_str(line);
            } else {
                let first_ch = line.chars().next().unwrap_or(' ');
                if last_ch.is_ascii_alphanumeric() && first_ch.is_ascii_alphanumeric() {
                    buf.push(' ');
                }
                buf.push_str(line);
            }
        }

        for ch in line.chars() {
            match ch {
                '「' => in_jp_quote = true,
                '」' => in_jp_quote = false,
                '『' => in_corner_quote = true,
                '』' => in_corner_quote = false,
                _ => {}
            }
        }
    }
    if !buf.is_empty() {
        paragraphs.push(buf);
    }
    paragraphs.join("\n")
}

/// Strip Kindle Cloud Reader UI chrome (progress, location numbers, footer hints) from OCR output
/// and reflow wrapped lines into continuous sentences.
pub fn clean_kindle_ocr_text(raw: &str) -> String {
    let loc_re = Regex::new(
        r"(?i)^(位置\s*No\.?\s*\d.*|Location\s*\d.*|Page\s*\d.*|ページ\s*\d.*|章の残り.*|本の中の位置.*|残り時間.*|残りの時間.*|Kindle|\d+\s*/\s*\d+|\d+\s*%|\d+\s*分\s*(未満|残り|で読了)?|\d+\s*mins?\s*(left.*)?)$",
    )
    .unwrap();
    let mut kept = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if loc_re.is_match(t) {
            continue;
        }
        if t.chars().count() == 1 && t.chars().all(|c| c.is_ascii_digit() || c.is_ascii_punctuation()) {
            continue;
        }
        let cleaned = clean_text_for_tts(t);
        if !cleaned.is_empty() {
            kept.push(cleaned);
        }
    }
    reflow_kindle_ocr_lines(&kept.join("\n"))
}

/// Open official Kindle Cloud Reader in the user's default browser (uses existing Amazon login).
pub fn open_kindle_in_browser(raw_url: &str, dry_run: bool) -> Result<String, String> {
    let valid = validate_kindle_url(raw_url)?;
    crate::companion_extras::open_validated_browser_url(&valid, dry_run)
}

/// Detect an already-open Kindle tab in the user's running browser (Chrome / Safari) and bring it to front.
/// Pass 1 prioritizes an open book reader tab (`read.amazon.` without `/kindle-library`);
/// Pass 2 falls back to `/kindle-library` or `/ap/signin`.
pub fn find_and_focus_kindle_browser_tab(activate_if_library: bool) -> KindleTabState {
    #[cfg(target_os = "macos")]
    {
        let act_lib = if activate_if_library { "true" } else { "false" };
        let script = format!(
            r#"
set activateLib to {act_lib}
set fallbackHit to ""
if application "Google Chrome" is running then
    try
        tell application "Google Chrome"
            repeat with w in windows
                set tIdx to 0
                repeat with t in tabs of w
                    set tIdx to tIdx + 1
                    set u to URL of t
                    if u contains "read.amazon." and u does not contain "kindle-library" and u does not contain "/ap/signin" then
                        set active tab index of w to tIdx
                        set index of w to 1
                        activate
                        set b to bounds of w
                        return "Google Chrome||" & u & "||" & (item 1 of b) & "," & (item 2 of b) & "," & (item 3 of b) & "," & (item 4 of b)
                    else if fallbackHit is "" and (u contains "read.amazon." or (u contains "amazon." and u contains "/ap/signin")) then
                        set b to bounds of w
                        set fallbackHit to "Google Chrome||" & u & "||" & (item 1 of b) & "," & (item 2 of b) & "," & (item 3 of b) & "," & (item 4 of b)
                        if activateLib then
                            set active tab index of w to tIdx
                            set index of w to 1
                            activate
                        end if
                    end if
                end repeat
            end repeat
        end tell
    end try
end if
if application "Safari" is running then
    try
        tell application "Safari"
            repeat with w in windows
                repeat with t in tabs of w
                    set u to URL of t
                    if u contains "read.amazon." and u does not contain "kindle-library" and u does not contain "/ap/signin" then
                        set current tab of w to t
                        set index of w to 1
                        activate
                        set b to bounds of w
                        return "Safari||" & u & "||" & (item 1 of b) & "," & (item 2 of b) & "," & (item 3 of b) & "," & (item 4 of b)
                    else if fallbackHit is "" and (u contains "read.amazon." or (u contains "amazon." and u contains "/ap/signin")) then
                        set b to bounds of w
                        set fallbackHit to "Safari||" & u & "||" & (item 1 of b) & "," & (item 2 of b) & "," & (item 3 of b) & "," & (item 4 of b)
                        if activateLib then
                            set current tab of w to t
                            set index of w to 1
                            activate
                        end if
                    end if
                end repeat
            end repeat
        end tell
    end try
end if
if fallbackHit is not "" then
    return fallbackHit
end if
return "NOT_FOUND"
"#
        );
        if let Ok(out) = Command::new("osascript").args(["-e", &script]).output() {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !stdout.is_empty() && stdout != "NOT_FOUND" {
                let parts: Vec<&str> = stdout.split("||").collect();
                if parts.len() >= 2 {
                    let bounds = parts.get(2).and_then(|bs| {
                        let nums: Vec<i32> = bs
                            .split(',')
                            .filter_map(|s| s.trim().parse::<i32>().ok())
                            .collect();
                        if nums.len() == 4 {
                            Some((nums[0], nums[1], nums[2], nums[3]))
                        } else {
                            None
                        }
                    });
                    return classify_kindle_tab_url(parts[0], parts[1], bounds);
                }
            }
        }
        KindleTabState::NotFound
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = activate_if_library;
        KindleTabState::NotFound
    }
}

/// Turn one page in the focused Kindle browser window (`Rtl` -> Left Arrow, `Ltr` -> Right Arrow).
/// Strategy 1 (macOS): Execute JavaScript directly inside Chrome/Safari Kindle tab (requires NO Accessibility permission).
/// Strategy 2 (macOS): Fallback to System Events key code 123/124; if denied, opens Privacy > Accessibility directly.
pub fn turn_kindle_page(browser: &str, direction: KindlePageDirection) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let (key_code, key_name, key_num, chevron_sel) = match direction {
            KindlePageDirection::Rtl => (
                123,
                "ArrowLeft",
                37,
                "#kr-chevron-left, button[aria-label*='次のページ'], button[aria-label*='Next'], .kr-chevron-left",
            ),
            KindlePageDirection::Ltr => (
                124,
                "ArrowRight",
                39,
                "#kr-chevron-right, button[aria-label*='次のページ'], button[aria-label*='Next'], .kr-chevron-right",
            ),
        };
        let safe_browser = browser.replace('"', "");

        // Strategy 1: Direct JavaScript page turn in Chrome/Safari (zero Accessibility permission needed!)
        // Dispatch keyboard event strictly ONCE (never both document + activeElement, which double-turns pages).
        let js_payload = format!(
            "(function(){{var el=document.querySelector(\"{chevron_sel}\");if(el){{el.click();return \"CLICKED\";}}var ev=new KeyboardEvent(\"keydown\",{{key:\"{key_name}\",code:\"{key_name}\",keyCode:{key_num},which:{key_num},bubbles:true,cancelable:true}});var target=(document.activeElement&&document.activeElement!==document.body)?document.activeElement:document;target.dispatchEvent(ev);return \"KEY_DISPATCHED\";}})();"
        );
        let js_escaped = js_payload.replace('\\', "\\\\").replace('"', "\\\"");
        let js_script = if safe_browser == "Safari" {
            format!(
                "tell application \"Safari\"\nactivate\ndo JavaScript \"{js_escaped}\" in current tab of window 1\nend tell"
            )
        } else {
            format!(
                "tell application \"{safe_browser}\"\nactivate\nexecute active tab of window 1 javascript \"{js_escaped}\"\nend tell"
            )
        };
        if let Ok(out) = Command::new("osascript").args(["-e", &js_script]).output() {
            if out.status.success() {
                return Ok(());
            }
        }

        // Strategy 2: System Events Arrow key fallback
        let script = format!(
            "tell application \"{safe_browser}\" to activate\ndelay 0.12\ntell application \"System Events\" to key code {key_code}"
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| format!("ページ送り実行エラー: {e}"))?;
        if !status.success() {
            // Open the EXACT Privacy > Accessibility pane (not top-level Accessibility!)
            let _ = Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .status();
            let browser_hint = if safe_browser == "Safari" {
                "Safari メニュー「開発 > Apple Events からの JavaScript を許可」を ON にするか、"
            } else {
                "Chrome メニュー「表示 > デベロッパー > Apple Events からの JavaScript を許可」を ON にするか、"
            };
            return Err(format!(
                "ページ送りに権限が必要です。一番簡単な方法: {browser_hint}開いた「プライバシーとセキュリティ > アクセシビリティ」下部の「+」から NanikaAi.app を追加してください。"
            ));
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = browser;
        let key = match direction {
            KindlePageDirection::Rtl => "{LEFT}",
            KindlePageDirection::Ltr => "{RIGHT}",
        };
        let ps = format!(
            "$ws = New-Object -ComObject WScript.Shell; $ws.SendKeys('{key}')"
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &ps])
            .status()
            .map_err(|e| format!("SendKeys error: {e}"))?;
        if !status.success() {
            return Err("Kindleページ送りに失敗しました。".to_string());
        }
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = browser;
        let key = match direction {
            KindlePageDirection::Rtl => "Left",
            KindlePageDirection::Ltr => "Right",
        };
        let status = Command::new("xdotool")
            .args(["key", key])
            .status()
            .map_err(|e| format!("xdotool error: {e}"))?;
        if !status.success() {
            return Err("xdotool によるページ送りに失敗しました。".to_string());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn run_macos_vision_ocr(image_path: &Path, direction: KindlePageDirection) -> Result<String, String> {
    let dir_arg = direction.as_str();
    let swift_code = r#"
import Foundation
import Vision
import AppKit

guard CommandLine.arguments.count >= 3 else { exit(2) }
let imgPath = CommandLine.arguments[1]
let dirMode = CommandLine.arguments[2]
guard let img = NSImage(contentsOfFile: imgPath),
      let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    fputs("Failed to load captured image\n", stderr)
    exit(3)
}
let req = VNRecognizeTextRequest()
req.recognitionLevel = .accurate
req.recognitionLanguages = ["ja-JP", "ko-KR", "en-US"]
req.usesLanguageCorrection = true
let handler = VNImageRequestHandler(cgImage: cg, options: [:])
try handler.perform([req])
let obs = req.results ?? []
struct Item { let text: String; let minX: CGFloat; let midX: CGFloat; let midY: CGFloat; let w: CGFloat; let h: CGFloat }
let items: [Item] = obs.compactMap { o in
    guard let top = o.topCandidates(1).first else { return nil }
    let b = o.boundingBox
    return Item(text: top.string, minX: b.minX, midX: b.midX, midY: b.midY, w: b.width, h: b.height)
}
let sorted: [Item]
if dirMode == "rtl" {
    // Vertical Japanese columns: cluster by midX (tolerance 0.032) right-to-left, then top-to-bottom within each column
    let byX = items.sorted { $0.midX > $1.midX }
    var cols: [[Item]] = []
    for it in byX {
        if var last = cols.last, let anchor = last.first, abs(anchor.midX - it.midX) <= 0.032 {
            last.append(it)
            cols[cols.count - 1] = last
        } else {
            cols.append([it])
        }
    }
    sorted = cols.flatMap { col in col.sorted { $0.midY > $1.midY } }
} else {
    // Horizontal lines: cluster by midY (tolerance 0.018) top-to-bottom, then left-to-right within each row
    let byY = items.sorted { $0.midY > $1.midY }
    var rows: [[Item]] = []
    for it in byY {
        if var last = rows.last, let anchor = last.first, abs(anchor.midY - it.midY) <= 0.018 {
            last.append(it)
            rows[rows.count - 1] = last
        } else {
            rows.append([it])
        }
    }
    sorted = rows.flatMap { row in row.sorted { $0.minX < $1.minX } }
}
for it in sorted {
    print(it.text)
}
"#;
    let out = Command::new("swift")
        .args(["-e", swift_code, &image_path.to_string_lossy(), dir_arg])
        .output()
        .map_err(|e| format!("Vision OCR起動エラー: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("Vision OCRエラー: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Detect a clean vertical whitespace gutter near the horizontal center of a landscape
/// 2-page Kindle spread so each page can be OCR'd independently without slicing characters
/// or causing small VLMs to skip columns across wide spreads.
pub fn detect_spread_gutter_x(img: &image::DynamicImage) -> Option<u32> {
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    if w < 640 || h < 240 || w * 10 < h * 11 {
        return None;
    }

    // Estimate page background luminance from top/bottom center margin samples
    let mut bg_samples: Vec<u8> = Vec::with_capacity(128);
    let y_top = (h / 25).min(h - 1);
    let y_bot = h.saturating_sub((h / 25).max(1));
    let step_x = (w / 32).max(1);
    let mut sx = w / 8;
    while sx < w * 7 / 8 {
        bg_samples.push(gray.get_pixel(sx, y_top)[0]);
        bg_samples.push(gray.get_pixel(sx, y_bot)[0]);
        sx += step_x;
    }
    bg_samples.sort_unstable();
    let bg_luma = bg_samples.get(bg_samples.len() / 2).copied().unwrap_or(250);

    let y0 = h / 10;
    let y1 = h * 9 / 10;
    let span_y = y1.saturating_sub(y0).max(1);
    let col_ink_ratio = |x: u32| -> f32 {
        let mut ink = 0u32;
        for y in y0..y1 {
            let lum = gray.get_pixel(x, y)[0];
            if lum.abs_diff(bg_luma) > 28 {
                ink += 1;
            }
        }
        ink as f32 / span_y as f32
    };

    let band_start = w * 42 / 100;
    let band_end = w * 58 / 100;
    let mut best_start = 0u32;
    let mut best_len = 0u32;
    let mut cur_start = 0u32;
    let mut cur_len = 0u32;

    for x in band_start..band_end {
        if col_ink_ratio(x) < 0.005 {
            if cur_len == 0 {
                cur_start = x;
            }
            cur_len += 1;
            if cur_len > best_len {
                best_len = cur_len;
                best_start = cur_start;
            }
        } else {
            cur_len = 0;
        }
    }

    let min_gutter_w = (w / 100).max(12);
    if best_len < min_gutter_w {
        return None;
    }
    let gutter_mid = best_start + best_len / 2;

    // Confirm both left and right halves actually contain ink (real 2-page spread, not 1-page centered)
    let has_ink_in_range = |x_start: u32, x_end: u32| -> bool {
        let step = ((x_end - x_start) / 60).max(1);
        let mut x = x_start;
        while x < x_end {
            if col_ink_ratio(x) >= 0.02 {
                return true;
            }
            x += step;
        }
        false
    };

    let left_ok = has_ink_in_range(w / 20, best_start.saturating_sub(4));
    let right_ok = has_ink_in_range((best_start + best_len + 4).min(w), w * 19 / 20);
    if left_ok && right_ok {
        Some(gutter_mid)
    } else {
        None
    }
}

/// Transcribe a single Kindle page image using local Ollama VLM (`gemma4:e4b` by default,
/// overridable via `NANIKAAI_KINDLE_VLM_MODEL`). Uses `Lanczos3` crisp scaling (`<= 1600px`),
/// JPEG quality 92, greedy decoding (`temperature: 0.0`), and explicit no-skip instructions.
fn run_local_vlm_kindle_ocr_image(
    img: &image::DynamicImage,
    direction: KindlePageDirection,
) -> Result<String, String> {
    let resized = if img.width() > 1600 || img.height() > 1600 {
        img.resize(1600, 1600, image::imageops::FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let mut jpg_buf = Vec::new();
    let rgb = resized.to_rgb8();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg_buf, 92);
    encoder
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("JPEG変換エラー: {e}"))?;
    let b64 = crate::agent_engine::encode_base64(&jpg_buf);

    let prompt = match direction {
        KindlePageDirection::Rtl => {
            "このKindle縦書きページの本文テキストを、右端の1列目から左端の最終列まで、1列も1文も飛ばさず省略せず、各列は上から下の正しい読む順番で一字一句そのまま書き起こしてください。上部の書名ヘッダーや下部の位置No・ページ数・%・残り時間は書かず、要約や解説も一切付けずに本文のみ出力してください。"
        }
        KindlePageDirection::Ltr => {
            "Transcribe EVERY SINGLE LINE AND SENTENCE of the main book body text in this Kindle page image verbatim from top to bottom without skipping, omitting, or summarizing any line. Exclude headers, book titles at the top, location numbers, percentages, and do not add any commentary."
        }
    };

    let vlm_model = std::env::var("NANIKAAI_KINDLE_VLM_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "gemma4:e4b".to_string());

    let client = Client::builder()
        .connect_timeout(Duration::from_millis(1200))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({
        "model": vlm_model,
        "messages": [{
            "role": "user",
            "content": prompt,
            "images": [b64]
        }],
        "stream": false,
        "think": false,
        "options": { "temperature": 0.0, "num_ctx": 6144, "num_predict": 2048 }
    });
    let resp = client
        .post("http://127.0.0.1:11434/api/chat")
        .json(&payload)
        .send()
        .map_err(|e| format!("ローカルVLM接続エラー: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ローカルVLM HTTP {}", resp.status()));
    }
    let data: Value = resp.json().map_err(|e| format!("ローカルVLM JSONエラー: {e}"))?;
    let text = data
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("ローカルVLMから本文テキストを取得できませんでした。".to_string());
    }
    Ok(text)
}

/// Transcribe a captured Kindle page image using local Ollama VLM (`gemma4:e4b`).
/// Automatically detects 2-page spreads via center whitespace gutter and OCRs each half
/// in reading order (Right->Left for RTL, Left->Right for LTR) to prevent column skipping.
fn run_local_vlm_kindle_ocr(
    image_path: &Path,
    direction: KindlePageDirection,
) -> Result<String, String> {
    let raw_bytes = fs::read(image_path).map_err(|e| format!("OCR画像読込エラー: {e}"))?;
    let img = image::load_from_memory(&raw_bytes).map_err(|e| format!("画像デコードエラー: {e}"))?;
    if let Some(gutter_x) = detect_spread_gutter_x(&img) {
        let (w, h) = (img.width(), img.height());
        let left_half = img.crop_imm(0, 0, gutter_x, h);
        let right_half = img.crop_imm(gutter_x, 0, w.saturating_sub(gutter_x), h);
        let ordered = match direction {
            KindlePageDirection::Rtl => [right_half, left_half],
            KindlePageDirection::Ltr => [left_half, right_half],
        };
        let mut parts = Vec::with_capacity(2);
        for half in &ordered {
            if let Ok(t) = run_local_vlm_kindle_ocr_image(half, direction) {
                if !t.trim().is_empty() {
                    parts.push(t);
                }
            }
        }
        if !parts.is_empty() {
            return Ok(parts.join("\n"));
        }
    }
    run_local_vlm_kindle_ocr_image(&img, direction)
}

/// Capture the visible Kindle browser content region into a short-lived temp PNG, run local OCR
/// (local VLM `gemma4:e4b` for vertical Japanese `Rtl` books; Apple Vision OCR with VLM fallback for `Ltr`),
/// and delete the temp PNG immediately (zero persistent book text/image storage).
pub fn capture_and_ocr_kindle_page(
    bounds: Option<(i32, i32, i32, i32)>,
    direction: KindlePageDirection,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let tmp_png = std::env::temp_dir().join(format!(
            "nanikaai_kindle_page_{}_{}.png",
            std::process::id(),
            NARRATION_GEN.load(Ordering::SeqCst)
        ));
        let mut cmd = Command::new("screencapture");
        cmd.arg("-x"); // silent
        if let Some((x1, y1, x2, y2)) = bounds {
            let w = (x2 - x1).max(320);
            let h = (y2 - y1).max(320);
            // Crop out browser toolbar/tabs + Kindle title header (top 160px), Kindle footer (bottom 75px), and side margins
            let cx = x1 + 56;
            let cy = y1 + 160;
            let cw = (w - 112).max(240);
            let ch = (h - 235).max(200);
            cmd.args(["-R", &format!("{cx},{cy},{cw},{ch}")]);
        }
        cmd.arg(&tmp_png);
        let status = cmd
            .status()
            .map_err(|e| format!("画面キャプチャ実行エラー: {e}"))?;
        if !status.success() || !tmp_png.exists() {
            let _ = fs::remove_file(&tmp_png);
            let _ = Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
                .status();
            return Err(
                "Kindle画面の取得に失敗しました（プライバシーとセキュリティ > 画面収録 で NanikaAi を許可してください）。"
                    .to_string(),
            );
        }
        // Vertical Japanese (`Rtl`) MUST use local VLM (`gemma4:e4b`) because Apple Vision OCR ignores vertical columns.
        let ocr_res = match direction {
            KindlePageDirection::Rtl => run_local_vlm_kindle_ocr(&tmp_png, direction)
                .or_else(|_| run_macos_vision_ocr(&tmp_png, direction)),
            KindlePageDirection::Ltr => match run_macos_vision_ocr(&tmp_png, direction) {
                Ok(t) if t.trim().chars().count() >= 12 => Ok(t),
                _ => run_local_vlm_kindle_ocr(&tmp_png, direction),
            },
        };
        let _ = fs::remove_file(&tmp_png);
        let raw = ocr_res?;
        Ok(clean_kindle_ocr_text(&raw))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (bounds, direction);
        Err("ブラウザ画面OCR朗読は現在 macOS で対応しています。".to_string())
    }
}

/// Start automated Kindle Cloud Reader narration:
/// 1) Detects logged-in Kindle tab (or opens `https://read.amazon.co.jp` if not open / asks user to log in if on sign-in page)
/// 2) Captures visible page -> local Vision OCR -> AivisSpeech emotional narration with mascot expression sync
/// 3) Turns page automatically (`Left` for RTL vertical books, `Right` for LTR horizontal books) until stopped or end-of-book.
#[allow(clippy::too_many_arguments)]
pub fn start_kindle_narration_async(
    kcfg: KindleSessionConfig,
    voicevox_url: String,
    tts_engine: String,
    fallback_speaker: Option<i64>,
    user_speed: f32,
    user_intonation: f32,
    ui_lang: UiLang,
    on_balloon: Arc<dyn Fn(Vec<DialogueAction>) + Send + Sync>,
    on_status: Option<Arc<dyn Fn(String) + Send + Sync>>,
) -> Result<NarrationStatus, String> {
    let valid_url = validate_kindle_url(&kcfg.kindle_url)?;
    stop_narration();
    let my_gen = NARRATION_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    NARRATION_STOP.store(false, Ordering::SeqCst);

    let max_pages = kcfg.max_pages.clamp(1, 200);
    let delay_ms = kcfg.page_turn_delay_ms.clamp(600, 5000);
    let initial = NarrationStatus {
        active: !kcfg.dry_run,
        title: "Kindle Cloud Reader".to_string(),
        source_kind: format!("kindle:{}", kcfg.direction.as_str()),
        preset: kcfg.preset.as_str().to_string(),
        speaker_name: String::new(),
        current_index: 0,
        total_utterances: max_pages,
        current_emotion: "Neutral".to_string(),
        current_text: valid_url.clone(),
        last_error: String::new(),
    };
    if let Ok(mut st) = status_slot().lock() {
        *st = initial.clone();
    }
    if kcfg.dry_run {
        return Ok(initial);
    }

    std::thread::spawn(move || {
        let stop_label = match ui_lang {
            UiLang::Ko => "⏹ 낭독 중지",
            UiLang::Ja => "⏹ 朗読停止",
            UiLang::En => "⏹ Stop Reading",
        };

        let mut tab = find_and_focus_kindle_browser_tab(true);
        let mut opened_once = false;
        let mut notified_wait = false;
        let wait_start = std::time::Instant::now();

        let (browser, bounds) = loop {
            if NARRATION_STOP.load(Ordering::SeqCst)
                || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
            {
                if let Ok(mut st) = status_slot().lock() {
                    st.active = false;
                }
                return;
            }
            match tab {
                KindleTabState::Ready { browser, bounds, .. } => {
                    if notified_wait {
                        // Give the freshly opened Kindle book page 1.6s to finish rendering
                        let mut w = 0u64;
                        while w < 1600 {
                            if NARRATION_STOP.load(Ordering::SeqCst)
                                || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                            {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                            w += 50;
                        }
                    }
                    break (browser, bounds);
                }
                KindleTabState::Library { .. } => {
                    if !notified_wait {
                        notified_wait = true;
                        let msg = match ui_lang {
                            UiLang::Ko => format!("\\0\\s[5]Kindle 라이브러리를 찾았어요! 읽을 책을 클릭해서 열면 자동으로 감정 낭독을 시작할게요…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                            UiLang::Ja => format!("\\0\\s[5]Kindle ライブラリを検出しました！読みたい本を開くと自動で感情朗読を開始しますね…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                            UiLang::En => format!("\\0\\s[5]Found your Kindle Library! Open any book and I'll automatically start reading it aloud…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                        };
                        on_balloon(parse_dialogue(&msg));
                    }
                }
                KindleTabState::NeedsLogin { .. } => {
                    if !notified_wait {
                        notified_wait = true;
                        let msg = match ui_lang {
                            UiLang::Ko => format!("\\0\\s[3]브라우저에서 Amazon 로그인 후 읽을 책을 열어 주세요! 본문이 열리면 자동으로 낭독을 시작할게요…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                            UiLang::Ja => format!("\\0\\s[3]ブラウザで Amazon にログインして読みたい本を開いてくださいね！本が開いたら自動で朗読を開始します…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                            UiLang::En => format!("\\0\\s[3]Please sign in to Amazon and open a book in your browser—I'll start reading automatically once the book opens!\\n\\q[{stop_label},__narrate_stop__]\\e"),
                        };
                        on_balloon(parse_dialogue(&msg));
                    }
                }
                KindleTabState::NotFound => {
                    if kcfg.open_browser && !opened_once {
                        opened_once = true;
                        let _ = open_kindle_in_browser(&valid_url, false);
                    }
                    if !notified_wait {
                        notified_wait = true;
                        let msg = match ui_lang {
                            UiLang::Ko => format!("\\0\\s[5]브라우저에서 Kindle을 열었어요! 읽을 책을 열면 자동으로 감정 낭독을 시작할게요…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                            UiLang::Ja => format!("\\0\\s[5]ブラウザで Kindle を開きました！読みたい本を開くと自動で感情朗読を開始しますね…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                            UiLang::En => format!("\\0\\s[5]Opened Kindle in your browser! Open a book and I'll start reading automatically…\\n\\q[{stop_label},__narrate_stop__]\\e"),
                        };
                        on_balloon(parse_dialogue(&msg));
                    }
                }
            }

            if wait_start.elapsed() >= Duration::from_secs(180) {
                if let Ok(mut st) = status_slot().lock() {
                    st.active = false;
                }
                let msg = match ui_lang {
                    UiLang::Ko => "\\0\\s[3]Kindle 책 열림 대기가 시간 초과되었어요. 책을 연 뒤 다시 시작해 주세요.\\e",
                    UiLang::Ja => "\\0\\s[3]Kindle の本が開くのを待機していましたがタイムアウトしました。本を開いてから再度開始してくださいね。\\e",
                    UiLang::En => "\\0\\s[3]Timed out waiting for a Kindle book to open. Please try again after opening a book.\\e",
                };
                on_balloon(parse_dialogue(msg));
                return;
            }

            let mut w = 0u64;
            while w < 1200 {
                if NARRATION_STOP.load(Ordering::SeqCst)
                    || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                {
                    return;
                }
                std::thread::sleep(Duration::from_millis(60));
                w += 60;
            }
            tab = find_and_focus_kindle_browser_tab(false);
        };

        let ready = ensure_tts_engine_ready_with_mode_blocking(&voicevox_url, &tts_engine, true);
        let Some(base_url) = ready.base_url else {
            let err_msg = match ui_lang {
                UiLang::Ko => "AivisSpeech / VOICEVOX 엔진을 찾지 못해 Kindle 낭독을 시작할 수 없어요.",
                UiLang::Ja => "AivisSpeech / VOICEVOX エンジンが見つからないため Kindle 朗読を開始できません。",
                UiLang::En => "AivisSpeech / VOICEVOX engine was not found for Kindle narration.",
            };
            if let Ok(mut st) = status_slot().lock() {
                st.active = false;
                st.last_error = err_msg.to_string();
            }
            on_balloon(parse_dialogue(&format!("\\0\\s[3]{err_msg}\\e")));
            return;
        };

        let voices = fetch_voices_from_engine(&base_url).unwrap_or_default();

        let mut prev_page_text = String::new();
        let mut same_page_streak = 0usize;

        for page in 1..=max_pages {
            if NARRATION_STOP.load(Ordering::SeqCst) || NARRATION_GEN.load(Ordering::SeqCst) != my_gen {
                break;
            }
            let page_text = match capture_and_ocr_kindle_page(bounds, kcfg.direction) {
                Ok(t) => t,
                Err(e) => {
                    if let Ok(mut st) = status_slot().lock() {
                        st.active = false;
                        st.last_error = e.clone();
                    }
                    on_balloon(parse_dialogue(&format!("\\0\\s[3]{e}\\e")));
                    return;
                }
            };

            if page_text.trim().is_empty() {
                let msg = match ui_lang {
                    UiLang::Ko => "\\0\\s[3]Kindle 페이지에서 본문 텍스트를 찾지 못했어요. 책 본문 화면이 열려 있는지 확인해 주세요.\\e",
                    UiLang::Ja => "\\0\\s[3]Kindle ページから本文を読み取れませんでした。本の本文ページが開いているか確認してくださいね。\\e",
                    UiLang::En => "\\0\\s[3]Could not read book text from the Kindle page. Make sure a book page is open!\\e",
                };
                on_balloon(parse_dialogue(msg));
                break;
            }

            if page_text == prev_page_text {
                same_page_streak += 1;
                if same_page_streak == 1 {
                    // Slow page-turn render guard: wait once and re-capture before assuming end-of-book
                    let mut w = 0u64;
                    while w < 1200 {
                        if NARRATION_STOP.load(Ordering::SeqCst)
                            || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                        {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                        w += 50;
                    }
                    continue;
                }
                if same_page_streak >= 2 {
                    let done_msg = match ui_lang {
                        UiLang::Ko => "\\0\\s[5]더 이상 넘어갈 페이지가 없어 Kindle 낭독을 마쳤어요!\\e",
                        UiLang::Ja => "\\0\\s[5]ページが変わらなくなったため、Kindle 朗読を終了しましたよ！\\e",
                        UiLang::En => "\\0\\s[5]Reached the end of the Kindle pages—narration finished!\\e",
                    };
                    on_balloon(parse_dialogue(done_msg));
                    break;
                }
            } else {
                same_page_streak = 0;
                prev_page_text = page_text.clone();
            }

            let utts = build_utterances_from_text(&format!("kindle_p{page}"), &page_text);
            let total_in_page = utts.len();
            for (i, utt) in utts.iter().enumerate() {
                if NARRATION_STOP.load(Ordering::SeqCst)
                    || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                {
                    break;
                }
                let Some((speaker_id, sp_name)) = resolve_emotional_speaker_with_override(
                    &voices,
                    kcfg.preset,
                    utt.kind,
                    utt.emotion,
                    kcfg.speaker_id,
                    fallback_speaker,
                ) else {
                    break;
                };
                let prosody =
                    compute_emotional_prosody(utt, kcfg.preset, user_speed, user_intonation);

                if let Ok(mut st) = status_slot().lock() {
                    st.active = true;
                    st.speaker_name = sp_name;
                    st.current_index = page;
                    st.total_utterances = max_pages;
                    st.current_emotion = utt.emotion.as_str().to_string();
                    st.current_text = utt.text.clone();
                }
                if let Some(ref cb) = on_status {
                    cb(format!(
                        "📚 Kindle P.{page} [{}/{} · {}]",
                        i + 1,
                        total_in_page,
                        utt.emotion.label(ui_lang)
                    ));
                }

                let preview: String = utt.text.chars().take(90).collect();
                let surf = utt.emotion.sakura_surface();
                let emo_lbl = utt.emotion.label(ui_lang);
                let script = format!(
                    "\\0\\s[{surf}]\\_q📚 Kindle P.{page} [{}/{} · {emo_lbl}]\\n{preview}\\n\\q[{stop_label},__narrate_stop__]\\_q\\e",
                    i + 1,
                    total_in_page
                );
                on_balloon(parse_dialogue(&script));

                match synthesize_emotional_wav_with_retry(
                    &base_url, speaker_id, &utt.text, &prosody, my_gen,
                ) {
                    Ok(wav) => {
                        if NARRATION_STOP.load(Ordering::SeqCst)
                            || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                        {
                            break;
                        }
                        let tmp_wav = std::env::temp_dir().join(format!(
                            "nanikaai_kindle_{}_{}.wav",
                            std::process::id(),
                            my_gen
                        ));
                        if fs::write(&tmp_wav, &wav).is_ok() {
                            let _ = play_wav_interruptible(&tmp_wav, my_gen);
                            let _ = fs::remove_file(&tmp_wav);
                        }
                    }
                    Err(e) => {
                        if let Ok(mut st) = status_slot().lock() {
                            st.last_error = e;
                        }
                        continue;
                    }
                }

                let mut waited = 0u64;
                while waited < utt.pause_after_ms {
                    if NARRATION_STOP.load(Ordering::SeqCst)
                        || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(45));
                    waited += 45;
                }
            }

            if NARRATION_STOP.load(Ordering::SeqCst)
                || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                || page >= max_pages
            {
                break;
            }

            if let Err(e) = turn_kindle_page(&browser, kcfg.direction) {
                if let Ok(mut st) = status_slot().lock() {
                    st.active = false;
                    st.last_error = e.clone();
                }
                on_balloon(parse_dialogue(&format!("\\0\\s[3]{e}\\e")));
                break;
            }

            let mut waited = 0u64;
            while waited < delay_ms {
                if NARRATION_STOP.load(Ordering::SeqCst)
                    || NARRATION_GEN.load(Ordering::SeqCst) != my_gen
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
                waited += 50;
            }
        }

        if let Ok(mut st) = status_slot().lock() {
            if NARRATION_GEN.load(Ordering::SeqCst) == my_gen {
                st.active = false;
            }
        }
    });

    Ok(initial)
}
