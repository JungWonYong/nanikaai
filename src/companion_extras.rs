use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::agent_engine::{
    assert_allowed_model, is_forbidden_model, wrap_untrusted_data, PREFERRED_MODELS,
};
use crate::builtin_tools::{is_sensitive_path, query_ram_gb, validate_public_url};
use crate::config_store::{default_user_data_dir, user_home_dir};
use crate::dialogue_parser::{parse_dialogue, ActionKind, Scope};

pub const GITHUB_RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/JungWonYong/nanikaai/releases/latest";
pub const DEFAULT_AIVIS_URL: &str = "http://127.0.0.1:10101";
pub const DEFAULT_VOICEVOX_URL: &str = "http://127.0.0.1:50021";
pub const DEFAULT_COEIROINK_URL: &str = "http://127.0.0.1:50032";
pub const DEFAULT_BOYISH_AIVM_UUID: &str = "9f36ec0d-8dac-42dc-aff4-149c5f99faad";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeroVoiceArchetype {
    Boyish,
    FemalePartner,
    YouthMale,
}

impl KeroVoiceArchetype {
    pub fn short_code(self) -> &'static str {
        match self {
            Self::Boyish => "boy",
            Self::FemalePartner => "girl",
            Self::YouthMale => "male",
        }
    }

    pub fn label_ja(self) -> &'static str {
        match self {
            Self::Boyish => "少年ボイス",
            Self::FemalePartner => "女の子ボイス",
            Self::YouthMale => "青年/男性ボイス",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AivmCatalogModel {
    pub uuid: &'static str,
    pub name: &'static str,
    pub role_tag: &'static str,
    pub menu_label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledAivmModel {
    pub uuid: String,
    pub name: String,
    pub is_default_model: bool,
    pub speaker_names: Vec<String>,
}

pub const CURATED_AIVISHUB_MODELS: &[AivmCatalogModel] = &[
    AivmCatalogModel {
        uuid: "a670e6b8-0852-45b2-8704-1bc9862f2fe6",
        name: "花音",
        role_tag: "girl",
        menu_label: "👩 花音 · 自然な会話女性 (高評価 168★)",
    },
    AivmCatalogModel {
        uuid: "3328da9a-8124-4619-a853-f7fc2f37889f",
        name: "桜音",
        role_tag: "girl",
        menu_label: "👧 桜音 · 明るく自然な少女 (93★)",
    },
    AivmCatalogModel {
        uuid: "4f281e78-eba6-495a-8e50-5c322d02b5b1",
        name: "るな",
        role_tag: "girl",
        menu_label: "👧 るな · やわらか自然少女 (98★)",
    },
    AivmCatalogModel {
        uuid: "59f96896-64d2-4378-830a-4d5feb3d81aa",
        name: "ほのか",
        role_tag: "girl",
        menu_label: "👩 ほのか · リアル20代会話女性 (4感情)",
    },
    AivmCatalogModel {
        uuid: "470a41e2-863f-4bf2-8a20-bf7cde0d8f7b",
        name: "TANAKA",
        role_tag: "male",
        menu_label: "👦 TANAKA · 関西弁おとん/相棒 (4感情)",
    },
    AivmCatalogModel {
        uuid: "9f36ec0d-8dac-42dc-aff4-149c5f99faad",
        name: "らせつん",
        role_tag: "boyish",
        menu_label: "👦 らせつん · 少年/中性 (8感情・うにゅう推奨)",
    },
    AivmCatalogModel {
        uuid: "9e9f84bd-05cd-4476-b47e-ad34264b2a03",
        name: "亜空マオ",
        role_tag: "boyish",
        menu_label: "👦 亜空マオ · 優しい少年/青年 (3感情)",
    },
    AivmCatalogModel {
        uuid: "3dabea2b-cc10-41da-955b-4618ce84e716",
        name: "Lux",
        role_tag: "male",
        menu_label: "🧑 Lux · 爽やか青年 (高音質)",
    },
    AivmCatalogModel {
        uuid: "6d11c6c2-f4a4-4435-887e-23dd60f8b8dd",
        name: "にせ",
        role_tag: "male",
        menu_label: "🧑 にせ · ナチュラル青年",
    },
    AivmCatalogModel {
        uuid: "f5017410-fbb5-49e1-97cb-e785f42e15f5",
        name: "凛音エル",
        role_tag: "girl",
        menu_label: "👧 凛音エル · クール少女 (5感情)",
    },
    AivmCatalogModel {
        uuid: "1b2830f4-8cf1-4184-a0d9-3a1bace3a844",
        name: "みちのくあいり",
        role_tag: "girl",
        menu_label: "👧 みちのくあいり · 落ち着いた少女 (7感情)",
    },
    AivmCatalogModel {
        uuid: "baaae3c0-7b22-4605-8ba5-80c959b41a48",
        name: "morioki",
        role_tag: "girl",
        menu_label: "👩 morioki · お姉さん/女性",
    },
    AivmCatalogModel {
        uuid: "47e53151-a378-46f3-abee-ce13aa07feb1",
        name: "阿井田 茂",
        role_tag: "male",
        menu_label: "🧔 阿井田 茂 · 朗らかおじさん",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVoiceSelection {
    pub sakura_id: Option<i64>,
    pub kero_id: Option<i64>,
    pub sakura_label: String,
    pub kero_label: String,
    pub archetype: KeroVoiceArchetype,
    pub used_female_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsVoiceOption {
    pub style_id: i64,
    pub speaker_name: String,
    pub style_name: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsEngineStatus {
    pub engine_label: String,
    pub base_url: Option<String>,
    pub auto_spawned: bool,
    pub sakura_voice_label: String,
    pub kero_voice_label: String,
    pub voices: Vec<TtsVoiceOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_url: String,
    pub notes_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedPayload {
    pub file_name: String,
    pub kind_label: String,
    pub size_bytes: u64,
    pub wrapped_data: String,
    pub extracted_url: Option<String>,
    pub offline_summary: String,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpeechBatch {
    segments: Vec<(u8, String)>,
    preferred_url: String,
    tts_engine: String,
    kero_archetype: KeroVoiceArchetype,
    sakura_speaker_override: Option<i64>,
    kero_speaker_override: Option<i64>,
    voice_speed: f32,
    voice_intonation: f32,
}

struct ManagedTtsProcess {
    port: u16,
    child: Child,
}

#[derive(Default, Clone)]
struct CachedTtsState {
    engine_label: String,
    base_url: Option<String>,
    voices: Vec<TtsVoiceOption>,
    installed_aivm_uuids: Vec<String>,
    installed_aivm_models: Vec<InstalledAivmModel>,
}

fn tts_sender() -> &'static Mutex<Option<mpsc::Sender<SpeechBatch>>> {
    static SENDER: OnceLock<Mutex<Option<mpsc::Sender<SpeechBatch>>>> = OnceLock::new();
    SENDER.get_or_init(|| Mutex::new(None))
}

fn managed_tts_process() -> &'static Mutex<Option<ManagedTtsProcess>> {
    static PROC: OnceLock<Mutex<Option<ManagedTtsProcess>>> = OnceLock::new();
    PROC.get_or_init(|| Mutex::new(None))
}

fn tts_init_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn cached_tts_state() -> &'static Mutex<CachedTtsState> {
    static CACHE: OnceLock<Mutex<CachedTtsState>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(CachedTtsState {
            engine_label: "OS Native TTS".to_string(),
            base_url: None,
            voices: Vec::new(),
            installed_aivm_uuids: Vec::new(),
            installed_aivm_models: Vec::new(),
        })
    })
}

/// Synchronously read cached engine label and discovered character voice styles without blocking.
pub fn cached_tts_voices() -> (String, Vec<TtsVoiceOption>) {
    cached_tts_state()
        .lock()
        .map(|g| (g.engine_label.clone(), g.voices.clone()))
        .unwrap_or_else(|_| ("OS Native TTS".to_string(), Vec::new()))
}

/// Synchronously read cached engine base URL without network probing.
pub fn cached_tts_base_url() -> Option<String> {
    cached_tts_state()
        .lock()
        .ok()
        .and_then(|g| g.base_url.clone())
}

/// Synchronously read cached installed AIVM model UUIDs from the last AivisSpeech query.
pub fn cached_installed_aivm_uuids() -> Vec<String> {
    cached_tts_state()
        .lock()
        .map(|g| g.installed_aivm_uuids.clone())
        .unwrap_or_default()
}

/// Synchronously read cached installed AIVM model metadata (name, default flag, speakers).
pub fn cached_installed_aivm_models() -> Vec<InstalledAivmModel> {
    cached_tts_state()
        .lock()
        .map(|g| g.installed_aivm_models.clone())
        .unwrap_or_default()
}

/// Extract speakable `(scope_id, text)` segments (`0` = Sakura, `1` = Kero) from SakuraScript.
pub fn extract_speech_segments(script: &str) -> Vec<(u8, String)> {
    let actions = parse_dialogue(script);
    extract_speech_segments_from_actions(&actions)
}

/// Extract speakable `(scope_id, text)` segments from already-parsed `DialogueAction` items.
pub fn extract_speech_segments_from_actions(
    actions: &[crate::dialogue_parser::DialogueAction],
) -> Vec<(u8, String)> {
    let mut segments: Vec<(u8, String)> = Vec::new();
    let mut current_scope: u8 = 0;
    let mut buf = String::new();

    let flush = |scope_id: u8, buf: &mut String, out: &mut Vec<(u8, String)>| {
        let cleaned = buf.split_whitespace().collect::<Vec<_>>().join(" ");
        buf.clear();
        if !cleaned.is_empty() {
            let truncated: String = cleaned.chars().take(600).collect();
            out.push((scope_id, truncated));
        }
    };

    for action in actions {
        match action.kind {
            ActionKind::Speaker => {
                let next_scope = match action.scope {
                    Scope::Sakura => 0,
                    Scope::Kero => 1,
                };
                if next_scope != current_scope {
                    flush(current_scope, &mut buf, &mut segments);
                    current_scope = next_scope;
                }
            }
            ActionKind::Text => {
                buf.push_str(&action.text);
            }
            ActionKind::Clear => {
                flush(current_scope, &mut buf, &mut segments);
            }
            ActionKind::Link => {
                if !action.label.trim().is_empty() {
                    if !buf.ends_with(' ') && !buf.is_empty() {
                        buf.push(' ');
                    }
                    buf.push_str(action.label.trim());
                }
            }
            ActionKind::Surface
            | ActionKind::BalloonSize
            | ActionKind::Choice
            | ActionKind::Pause
            | ActionKind::QuickToggle => {}
        }
    }
    flush(current_scope, &mut buf, &mut segments);
    segments
}

/// Discover installed headless TTS engine binaries (AivisSpeech Engine first, then VOICEVOX vv-engine).
pub fn discover_tts_engine_binaries() -> Vec<(&'static str, PathBuf, u16)> {
    let mut candidates: Vec<(&'static str, PathBuf, u16)> = Vec::new();
    let home = user_home_dir();

    if let Some(custom) = std::env::var_os("NANIKAAI_TTS_ENGINE_BIN") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            let low = p.to_string_lossy().to_ascii_lowercase();
            let (lbl, port) = if low.contains("aivis") {
                ("AivisSpeech Engine", 10101)
            } else {
                ("VOICEVOX Engine", 50021)
            };
            candidates.push((lbl, p, port));
        }
    }

    let mut add_if_exists = |label: &'static str, p: PathBuf, port: u16| {
        if p.is_file() && !candidates.iter().any(|(_, existing, _)| existing == &p) {
            candidates.push((label, p, port));
        }
    };

    // 1. AivisSpeech Engine (Style-Bert-VITS2 natural anime voices, vv-engine compatible :10101)
    add_if_exists(
        "AivisSpeech Engine",
        home.join(".local/share/aivisspeech-engine/run"),
        10101,
    );
    add_if_exists(
        "AivisSpeech Engine",
        home.join(".local/share/aivisspeech-engine/run.exe"),
        10101,
    );
    add_if_exists(
        "AivisSpeech Engine",
        PathBuf::from("/Applications/AivisSpeech.app/Contents/Resources/AivisSpeech-Engine/run"),
        10101,
    );
    add_if_exists(
        "AivisSpeech Engine",
        home.join("Applications/AivisSpeech.app/Contents/Resources/AivisSpeech-Engine/run"),
        10101,
    );
    add_if_exists(
        "AivisSpeech Engine",
        PathBuf::from("/opt/aivisspeech-engine/run"),
        10101,
    );
    if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
        add_if_exists(
            "AivisSpeech Engine",
            PathBuf::from(&local_app).join(r"Programs\AivisSpeech\AivisSpeech-Engine\run.exe"),
            10101,
        );
    }
    if let Some(prog_files) = std::env::var_os("PROGRAMFILES") {
        add_if_exists(
            "AivisSpeech Engine",
            PathBuf::from(&prog_files).join(r"AivisSpeech\AivisSpeech-Engine\run.exe"),
            10101,
        );
    }
    add_if_exists(
        "AivisSpeech Engine",
        PathBuf::from(r"C:\usr\local\AivisSpeech\AivisSpeech-Engine\run.exe"),
        10101,
    );

    // 2. VOICEVOX vv-engine (classic character voices, :50021)
    add_if_exists(
        "VOICEVOX Engine",
        PathBuf::from("/Applications/VOICEVOX.app/Contents/Resources/vv-engine/run"),
        50021,
    );
    add_if_exists(
        "VOICEVOX Engine",
        home.join("Applications/VOICEVOX.app/Contents/Resources/vv-engine/run"),
        50021,
    );
    add_if_exists(
        "VOICEVOX Engine",
        home.join(".local/share/voicevox-engine/run"),
        50021,
    );
    add_if_exists(
        "VOICEVOX Engine",
        PathBuf::from("/opt/voicevox_engine/run"),
        50021,
    );
    add_if_exists(
        "VOICEVOX Engine",
        PathBuf::from(r"C:\usr\local\VOICEVOX\vv-engine\run.exe"),
        50021,
    );
    if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
        add_if_exists(
            "VOICEVOX Engine",
            PathBuf::from(&local_app).join(r"Programs\VOICEVOX\vv-engine\run.exe"),
            50021,
        );
    }
    if let Some(prog_files) = std::env::var_os("PROGRAMFILES") {
        add_if_exists(
            "VOICEVOX Engine",
            PathBuf::from(&prog_files).join(r"VOICEVOX\vv-engine\run.exe"),
            50021,
        );
    }

    candidates
}

/// Stop any headless TTS engine child process that was auto-spawned by NanikaAi.
pub fn stop_managed_tts_engine() {
    if let Ok(mut guard) = managed_tts_process().lock() {
        if let Some(mut proc_info) = guard.take() {
            let _ = proc_info.child.kill();
            let _ = proc_info.child.wait();
        }
    }
    if let Ok(mut cache) = cached_tts_state().lock() {
        cache.engine_label = "OS Native TTS".to_string();
        cache.base_url = None;
        cache.voices.clear();
        cache.installed_aivm_uuids.clear();
    }
}

/// Parse `GET {base}/speakers` from a VOICEVOX / AivisSpeech compatible engine.
pub fn fetch_voices_from_engine(base: &str) -> Result<Vec<TtsVoiceOption>, String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let clean = base.trim_end_matches('/');
    let resp = client
        .get(format!("{clean}/speakers"))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let arr: Vec<serde_json::Value> = resp.json().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for sp in arr {
        let sp_name = sp
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Speaker")
            .trim()
            .to_string();
        if let Some(styles) = sp.get("styles").and_then(|v| v.as_array()) {
            for st in styles {
                let Some(id) = st.get("id").and_then(|v| v.as_i64()) else {
                    continue;
                };
                let st_name = st
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ノーマル")
                    .trim()
                    .to_string();
                let label = format!("{sp_name} ({st_name})");
                out.push(TtsVoiceOption {
                    style_id: id,
                    speaker_name: sp_name.clone(),
                    style_name: st_name,
                    label,
                });
            }
        }
    }
    Ok(out)
}

const BOYISH_SPEAKER_PRIORITY: &[&str] = &[
    "らせつん",
    "満別花丸",
    "猫使アル",
    "亜空マオ",
    "蒼月ハヤテ",
    "ずんだもん",
    "春歌ナナ",
    "雀松朱司",
    "にせ",
    "Lux",
];

const MALE_SPEAKER_PRIORITY: &[&str] = &[
    "Lux",
    "にせ",
    "玄野武宏",
    "青山龍星",
    "fumifumi",
    "剣崎雌雄",
    "雀松朱司",
    "亜空マオ",
    "蒼月ハヤテ",
    "阿井田 茂",
    "麒ヶ島宗麟",
    "M1",
    "M2",
    "yukyu",
    "Flow",
];

const FEMALE_SPEAKER_PRIORITY: &[&str] = &[
    "まお",
    "四国めたん",
    "Anneli",
    "凛音エル",
    "みちのくあいり",
    "春日部つむぎ",
    "雨晴はう",
    "冥鳴ひまり",
    "花音",
    "まい",
    "morioki",
    "コハク",
    "九州そら",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TtsProsodyHint {
    Default,
    BoyishNative,
    BoyishFallbackOnFemale,
    YouthMaleFallbackOnFemale,
}

/// Infer the partner character (`\1`) voice archetype from explicit override, kero name, and shell metadata.
pub fn infer_kero_voice_archetype(
    shell_id: &str,
    shell_name: &str,
    kero_name: &str,
    override_mode: Option<&str>,
) -> KeroVoiceArchetype {
    if let Some(ov) = override_mode {
        match ov.trim().to_ascii_lowercase().as_str() {
            "boy" | "boyish" => return KeroVoiceArchetype::Boyish,
            "girl" | "female" | "female_partner" => return KeroVoiceArchetype::FemalePartner,
            "male" | "youth_male" | "man" => return KeroVoiceArchetype::YouthMale,
            _ => {}
        }
    }

    let kn = kero_name.trim();
    let kn_low = kn.to_ascii_lowercase();
    if [
        "うにゅう",
        "ほねゅう",
        "まるちい",
        "unyu",
        "少年",
        "くん",
        "坊",
        "飯綱",
        "太郎",
        "小僧",
    ]
    .iter()
    .any(|kw| kn.contains(kw) || kn_low.contains(kw))
    {
        return KeroVoiceArchetype::Boyish;
    }

    let fem_suffix = kn.chars().last().is_some_and(|c| {
        matches!(c, '子' | '奈' | '花' | '美' | '香' | '音' | '乃' | '莉' | '華')
    }) && !matches!(kn, "帽子" | "息子" | "童子");
    if fem_suffix
        || ["ゆうか", "黒姉", "姉", "妹", "娘", "姫", "少女", "ちゃん", "魔女"]
            .iter()
            .any(|kw| kn.contains(kw))
    {
        return KeroVoiceArchetype::FemalePartner;
    }

    let male_suffix = kn
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '郎' | '介' | '助' | '司' | '男' | '夫' | '彦'));
    if male_suffix
        || ["斗和", "ただきち", "兄", "執事", "氏", "殿", "おじ"]
            .iter()
            .any(|kw| kn.contains(kw))
    {
        return KeroVoiceArchetype::YouthMale;
    }

    if kn.is_empty() {
        let meta = format!("{shell_id} {shell_name}");
        if meta.contains("ゆうか") || meta.contains("黒姉") {
            return KeroVoiceArchetype::FemalePartner;
        }
        if meta.contains("斗和") || meta.contains("ただきち") {
            return KeroVoiceArchetype::YouthMale;
        }
    }

    KeroVoiceArchetype::Boyish
}

/// Strict lowercase 8-4-4-4-12 UUID validator preventing path traversal or URL injection.
pub fn is_valid_aivm_uuid(raw: &str) -> bool {
    let s = raw.trim();
    if s.len() != 36 {
        return false;
    }
    s.bytes().enumerate().all(|(idx, b)| {
        if matches!(idx, 8 | 13 | 18 | 23) {
            b == b'-'
        } else {
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        }
    })
}

fn speaker_matches_list(speaker_name: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|kw| speaker_name == *kw || speaker_name.contains(kw))
}

/// Returns true when discovered speakers contain at least one boyish or male voice model.
pub fn has_non_female_voice(voices: &[TtsVoiceOption]) -> bool {
    voices.iter().any(|v| {
        speaker_matches_list(&v.speaker_name, BOYISH_SPEAKER_PRIORITY)
            || speaker_matches_list(&v.speaker_name, MALE_SPEAKER_PRIORITY)
    })
}

fn shares_voice_actor(a: &str, b: &str) -> bool {
    (a.contains("まお") && b.contains("コハク")) || (a.contains("コハク") && b.contains("まお"))
}

fn pick_preferred_style_for_speaker<'a>(
    voices: &'a [TtsVoiceOption],
    speaker: &str,
) -> Option<&'a TtsVoiceOption> {
    let mut cands = voices.iter().filter(|v| v.speaker_name == speaker);
    let first = cands.next()?;
    voices
        .iter()
        .filter(|v| v.speaker_name == speaker)
        .find(|v| v.style_name == "ノーマル" || v.style_name == "標準")
        .or_else(|| {
            voices.iter().filter(|v| v.speaker_name == speaker).find(|v| {
                v.style_name.contains("ふつ") || v.style_name.eq_ignore_ascii_case("normal")
            })
        })
        .or(Some(first))
}

fn pick_crisp_non_sweet_option(
    voices: &[TtsVoiceOption],
    sakura_id: i64,
    sakura_speaker: &str,
) -> Option<i64> {
    let is_crisp_style = |s: &str| {
        !(s.contains("あまあま") || s.contains("ねむたい") || s.contains("せつなめ") || s.contains("ささやき"))
    };
    let rank_style = |s: &str| -> u8 {
        if s.contains("ふつ") {
            0
        } else if s.contains("からかい") || s.contains("元気") {
            1
        } else if s == "ノーマル" || s == "標準" || s.eq_ignore_ascii_case("normal") {
            2
        } else if is_crisp_style(s) {
            3
        } else {
            4
        }
    };

    let other_speaker = voices
        .iter()
        .filter(|v| v.speaker_name != sakura_speaker && v.style_id != sakura_id)
        .min_by_key(|v| rank_style(&v.style_name));
    if let Some(opt) = other_speaker {
        return Some(opt.style_id);
    }

    voices
        .iter()
        .filter(|v| v.style_id != sakura_id)
        .min_by_key(|v| rank_style(&v.style_name))
        .map(|v| v.style_id)
}

/// Pick distinct `\0` and `\1` speaker style IDs tailored for the requested partner archetype.
/// Returns `(sakura_id, kero_id, used_female_fallback_for_nonfemale)`.
pub fn pick_speaker_pair_for_archetype(
    voices: &[TtsVoiceOption],
    archetype: KeroVoiceArchetype,
) -> (Option<i64>, Option<i64>, bool) {
    if voices.is_empty() {
        return (None, None, false);
    }

    let sakura_opt = FEMALE_SPEAKER_PRIORITY
        .iter()
        .find_map(|pat| {
            voices
                .iter()
                .find(|v| v.speaker_name.contains(pat))
                .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
        })
        .or_else(|| {
            voices
                .iter()
                .find(|v| {
                    !speaker_matches_list(&v.speaker_name, BOYISH_SPEAKER_PRIORITY)
                        && !speaker_matches_list(&v.speaker_name, MALE_SPEAKER_PRIORITY)
                })
                .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
        })
        .or_else(|| pick_preferred_style_for_speaker(voices, &voices[0].speaker_name))
        .unwrap_or(&voices[0]);

    let sakura_id = sakura_opt.style_id;
    let sakura_sp = sakura_opt.speaker_name.as_str();

    let pick_by_priority = |list: &[&str]| -> Option<i64> {
        list.iter().find_map(|pat| {
            voices
                .iter()
                .find(|v| v.speaker_name.contains(pat) && v.speaker_name != sakura_sp)
                .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
                .map(|v| v.style_id)
        })
    };

    match archetype {
        KeroVoiceArchetype::Boyish => {
            if let Some(kid) = pick_by_priority(BOYISH_SPEAKER_PRIORITY)
                .or_else(|| pick_by_priority(MALE_SPEAKER_PRIORITY))
            {
                return (Some(sakura_id), Some(kid), false);
            }
            let fb = pick_crisp_non_sweet_option(voices, sakura_id, sakura_sp)
                .unwrap_or(sakura_id);
            (Some(sakura_id), Some(fb), true)
        }
        KeroVoiceArchetype::YouthMale => {
            if let Some(kid) = pick_by_priority(MALE_SPEAKER_PRIORITY)
                .or_else(|| pick_by_priority(BOYISH_SPEAKER_PRIORITY))
            {
                return (Some(sakura_id), Some(kid), false);
            }
            let fb = pick_crisp_non_sweet_option(voices, sakura_id, sakura_sp)
                .unwrap_or(sakura_id);
            (Some(sakura_id), Some(fb), true)
        }
        KeroVoiceArchetype::FemalePartner => {
            let distinct_diff_cv = FEMALE_SPEAKER_PRIORITY
                .iter()
                .find_map(|pat| {
                    voices
                        .iter()
                        .find(|v| {
                            v.speaker_name.contains(pat)
                                && v.speaker_name != sakura_sp
                                && !shares_voice_actor(&v.speaker_name, sakura_sp)
                        })
                        .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
                        .map(|v| v.style_id)
                })
                .or_else(|| {
                    voices
                        .iter()
                        .find(|v| {
                            v.speaker_name != sakura_sp
                                && !shares_voice_actor(&v.speaker_name, sakura_sp)
                                && !speaker_matches_list(&v.speaker_name, BOYISH_SPEAKER_PRIORITY)
                                && !speaker_matches_list(&v.speaker_name, MALE_SPEAKER_PRIORITY)
                        })
                        .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
                        .map(|v| v.style_id)
                });
            if let Some(kid) = distinct_diff_cv {
                return (Some(sakura_id), Some(kid), false);
            }
            let any_diff = pick_by_priority(FEMALE_SPEAKER_PRIORITY)
                .or_else(|| {
                    voices
                        .iter()
                        .find(|v| {
                            v.speaker_name != sakura_sp
                                && !speaker_matches_list(&v.speaker_name, BOYISH_SPEAKER_PRIORITY)
                                && !speaker_matches_list(&v.speaker_name, MALE_SPEAKER_PRIORITY)
                        })
                        .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
                        .map(|v| v.style_id)
                })
                .or_else(|| {
                    voices
                        .iter()
                        .find(|v| v.speaker_name != sakura_sp)
                        .and_then(|v| pick_preferred_style_for_speaker(voices, &v.speaker_name))
                        .map(|v| v.style_id)
                })
                .or_else(|| {
                    voices
                        .iter()
                        .find(|v| v.style_id != sakura_id)
                        .map(|v| v.style_id)
                })
                .unwrap_or(sakura_id);
            (Some(sakura_id), Some(any_diff), false)
        }
    }
}

/// Choose distinct default `\0` and `\1` speaker style IDs from discovered speakers (boyish `\1` default).
pub fn pick_default_speaker_pair(voices: &[TtsVoiceOption]) -> (Option<i64>, Option<i64>) {
    let (s0, s1, _) = pick_speaker_pair_for_archetype(voices, KeroVoiceArchetype::Boyish);
    (s0, s1)
}

pub fn resolve_effective_voice_selection(
    voices: &[TtsVoiceOption],
    archetype: KeroVoiceArchetype,
    sakura_override: Option<i64>,
    kero_override: Option<i64>,
) -> ResolvedVoiceSelection {
    let (def_s0, def_s1, auto_fallback) = pick_speaker_pair_for_archetype(voices, archetype);
    let sakura_id = sakura_override.or(def_s0);
    let kero_id = kero_override.or(def_s1);
    let used_female_fallback = kero_override.is_none() && auto_fallback;
    ResolvedVoiceSelection {
        sakura_id,
        kero_id,
        sakura_label: voice_label_for_id(voices, sakura_id),
        kero_label: voice_label_for_id(voices, kero_id),
        archetype,
        used_female_fallback,
    }
}

/// Query installed AIVM models (with name, default flag, speakers) from `GET {base}/aivm_models`.
pub fn fetch_installed_aivm_models(base: &str) -> Vec<InstalledAivmModel> {
    let Ok(client) = Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(3))
        .build()
    else {
        return Vec::new();
    };
    let url = format!("{}/aivm_models", base.trim_end_matches('/'));
    let Ok(resp) = client.get(url).send() else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(val) = resp.json::<serde_json::Value>() else {
        return Vec::new();
    };
    let mut models: Vec<InstalledAivmModel> = val
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter(|(k, _)| is_valid_aivm_uuid(k))
                .map(|(k, v)| {
                    let man = v.get("manifest");
                    let name = man
                        .and_then(|m| m.get("name"))
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(k)
                        .to_string();
                    let is_default_model = v
                        .get("is_default_model")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false);
                    let speaker_names = man
                        .and_then(|m| m.get("speakers"))
                        .and_then(|x| x.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|sp| sp.get("name").and_then(|n| n.as_str()))
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();
                    InstalledAivmModel {
                        uuid: k.clone(),
                        name,
                        is_default_model,
                        speaker_names,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    models.sort_by(|a, b| {
        b.is_default_model
            .cmp(&a.is_default_model)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let ids: Vec<String> = models.iter().map(|m| m.uuid.clone()).collect();
    if let Ok(mut cache) = cached_tts_state().lock() {
        cache.installed_aivm_uuids = ids;
        cache.installed_aivm_models = models.clone();
    }
    models
}

/// Query installed AIVM model UUIDs from `GET {base}/aivm_models`.
pub fn fetch_installed_aivm_uuids(base: &str) -> Vec<String> {
    fetch_installed_aivm_models(base)
        .into_iter()
        .map(|m| m.uuid)
        .collect()
}

/// Uninstall a user-installed AIVM voice model from local AivisSpeech Engine (`DELETE /aivm_models/{uuid}/uninstall`).
pub fn uninstall_aivm_model_blocking(
    base: &str,
    aivm_uuid: &str,
) -> Result<(String, Vec<TtsVoiceOption>), String> {
    let clean_uuid = aivm_uuid.trim();
    if !is_valid_aivm_uuid(clean_uuid) {
        return Err(format!("不正な AIVM UUID です: {aivm_uuid}"));
    }
    let clean_base = validate_local_tts_url(base)
        .ok_or_else(|| "ローカル AivisSpeech URL のみ許可されます".to_string())?;

    let installed = fetch_installed_aivm_models(&clean_base);
    let target = installed
        .iter()
        .find(|m| m.uuid.eq_ignore_ascii_case(clean_uuid))
        .ok_or_else(|| "対象の音声モデルが見つかりません".to_string())?;
    if target.is_default_model {
        return Err(format!("標準モデル「{}」は削除できません", target.name));
    }
    let removed_name = target.name.clone();

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let _ = client
        .post(format!("{clean_base}/aivm_models/{clean_uuid}/unload"))
        .send();

    let del_resp = client
        .delete(format!("{clean_base}/aivm_models/{clean_uuid}/uninstall"))
        .send()
        .map_err(|e| format!("音声モデル削除通信エラー: {e}"))?;
    if !del_resp.status().is_success() {
        let st = del_resp.status();
        let err_txt = del_resp.text().unwrap_or_default();
        return Err(format!("AIVM 削除失敗 (HTTP {st}): {err_txt}"));
    }

    let voices = fetch_voices_from_engine(&clean_base).unwrap_or_default();
    let models = fetch_installed_aivm_models(&clean_base);
    let uuids: Vec<String> = models.iter().map(|m| m.uuid.clone()).collect();
    if let Ok(mut cache) = cached_tts_state().lock() {
        cache.engine_label = label_for_engine_url(&clean_base, &voices);
        cache.base_url = Some(clean_base);
        cache.voices = voices.clone();
        cache.installed_aivm_uuids = uuids;
        cache.installed_aivm_models = models;
    }
    Ok((removed_name, voices))
}

/// Download & load a curated AivisHub `.aivmx` voice model into local AivisSpeech Engine cleanly via multipart URL upload.
pub fn install_aivm_model_blocking(
    base: &str,
    aivm_uuid: &str,
) -> Result<Vec<TtsVoiceOption>, String> {
    let clean_uuid = aivm_uuid.trim();
    if !is_valid_aivm_uuid(clean_uuid) {
        return Err(format!("不正な AIVM UUID です: {aivm_uuid}"));
    }
    let clean_base = validate_local_tts_url(base)
        .ok_or_else(|| "ローカル AivisSpeech URL のみ許可されます".to_string())?;

    let existing = fetch_installed_aivm_uuids(&clean_base);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;

    if !existing.iter().any(|u| u == clean_uuid) {
        let dl_url = format!(
            "https://api.aivis-project.com/v1/aivm-models/{clean_uuid}/download?model_type=AIVMX"
        );
        let boundary = "----NanikaAiAivmInstallBoundary7A9C";
        let multipart_body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"url\"\r\n\r\n{dl_url}\r\n--{boundary}--\r\n"
        );
        let inst_resp = client
            .post(format!("{clean_base}/aivm_models/install"))
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(multipart_body)
            .send()
            .map_err(|e| format!("音声モデル導入通信エラー: {e}"))?;
        if !inst_resp.status().is_success() {
            let st = inst_resp.status();
            let err_txt = inst_resp.text().unwrap_or_default();
            return Err(format!("AIVM 導入失敗 (HTTP {st}): {err_txt}"));
        }
    }

    let _ = client
        .post(format!("{clean_base}/aivm_models/{clean_uuid}/load"))
        .send();

    let voices = fetch_voices_from_engine(&clean_base)?;
    let uuids = fetch_installed_aivm_uuids(&clean_base);
    if let Ok(mut cache) = cached_tts_state().lock() {
        cache.engine_label = label_for_engine_url(&clean_base, &voices);
        cache.base_url = Some(clean_base);
        cache.voices = voices.clone();
        cache.installed_aivm_uuids = uuids;
    }
    Ok(voices)
}

/// Automatically install the default boyish voice (`らせつん`) once per process when AivisSpeech has no boyish/male model.
pub fn maybe_auto_install_boyish_aivm_async(
    base_url: String,
    archetype: KeroVoiceArchetype,
    on_done: impl FnOnce(Result<Vec<TtsVoiceOption>, String>) + Send + 'static,
) {
    static ATTEMPTED: AtomicBool = AtomicBool::new(false);
    if archetype == KeroVoiceArchetype::FemalePartner {
        return;
    }
    let (_, current_voices) = cached_tts_voices();
    let is_aivis = base_url.contains(":10101") || current_voices.iter().any(|v| v.style_id > 100_000);
    if !is_aivis || has_non_female_voice(&current_voices) {
        return;
    }
    if ATTEMPTED
        .compare_exchange(false, true, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let res = install_aivm_model_blocking(&base_url, DEFAULT_BOYISH_AIVM_UUID);
        on_done(res);
    });
}

fn voice_label_for_id(voices: &[TtsVoiceOption], id: Option<i64>) -> String {
    match id {
        Some(target) => voices
            .iter()
            .find(|v| v.style_id == target)
            .map(|v| v.label.clone())
            .unwrap_or_else(|| format!("Speaker #{target}")),
        None => "標準".to_string(),
    }
}

fn probe_tts_version(base: &str) -> bool {
    let Ok(client) = Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_millis(900))
        .build()
    else {
        return false;
    };
    client
        .get(format!("{}/version", base.trim_end_matches('/')))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

fn label_for_engine_url(base: &str, voices: &[TtsVoiceOption]) -> String {
    if base.contains(":10101") {
        "AivisSpeech Engine (:10101)".to_string()
    } else if base.contains(":50032") {
        "COEIROINK (:50032)".to_string()
    } else if voices.iter().any(|v| v.style_id > 100_000) {
        "AivisSpeech Engine".to_string()
    } else {
        "VOICEVOX Engine (:50021)".to_string()
    }
}

/// Probe running local character TTS engines or headlessly launch an installed engine matching `tts_engine`.
pub fn ensure_tts_engine_ready_with_mode_blocking(
    preferred_url: &str,
    tts_engine: &str,
    allow_autostart: bool,
) -> TtsEngineStatus {
    let _init_guard = tts_init_lock().lock().ok();
    let mode = tts_engine.trim().to_ascii_lowercase();

    if mode == "os" || mode == "native" {
        if let Ok(mut cache) = cached_tts_state().lock() {
            cache.engine_label = "OS Native TTS".to_string();
            cache.base_url = None;
            cache.voices.clear();
            cache.installed_aivm_uuids.clear();
            cache.installed_aivm_models.clear();
        }
        return TtsEngineStatus {
            engine_label: "OS Native TTS".to_string(),
            base_url: None,
            auto_spawned: false,
            sakura_voice_label: "OS標準 (女性 · Kyoko)".to_string(),
            kero_voice_label: "OS標準 (少年/男性 · Eddy)".to_string(),
            voices: Vec::new(),
        };
    }

    let mut probe_urls: Vec<String> = Vec::new();
    let trimmed_pref = preferred_url.trim();
    let pref_is_default = trimmed_pref.is_empty()
        || trimmed_pref.eq_ignore_ascii_case("auto")
        || trimmed_pref.trim_end_matches('/') == DEFAULT_VOICEVOX_URL;
    if mode == "auto" && !pref_is_default {
        if let Some(valid) = validate_local_tts_url(trimmed_pref) {
            probe_urls.push(valid);
        }
    }
    let ordered_std: &[&str] = match mode.as_str() {
        "aivisspeech" | "aivis" => &[DEFAULT_AIVIS_URL],
        "voicevox" | "vv" => &[DEFAULT_VOICEVOX_URL],
        "coeiroink" => &[DEFAULT_COEIROINK_URL],
        _ => &[DEFAULT_AIVIS_URL, DEFAULT_VOICEVOX_URL, DEFAULT_COEIROINK_URL],
    };
    for std_url in ordered_std {
        if !probe_urls.iter().any(|u| u == std_url) {
            probe_urls.push((*std_url).to_string());
        }
    }

    let populate_cache_and_status = |url: String, auto_spawned: bool, lbl_override: Option<String>| -> TtsEngineStatus {
        let voices = fetch_voices_from_engine(&url).unwrap_or_default();
        let (s0, s1, _) = pick_speaker_pair_for_archetype(&voices, KeroVoiceArchetype::Boyish);
        let engine_label = lbl_override.unwrap_or_else(|| label_for_engine_url(&url, &voices));
        let sakura_voice_label = voice_label_for_id(&voices, s0);
        let kero_voice_label = voice_label_for_id(&voices, s1);
        let is_aivis = url.contains(":10101") || voices.iter().any(|v| v.style_id > 100_000);
        if let Ok(mut cache) = cached_tts_state().lock() {
            cache.engine_label = engine_label.clone();
            cache.base_url = Some(url.clone());
            cache.voices = voices.clone();
            if !is_aivis {
                cache.installed_aivm_uuids.clear();
            }
        }
        if is_aivis {
            let _ = fetch_installed_aivm_uuids(&url);
        }
        TtsEngineStatus {
            engine_label,
            base_url: Some(url),
            auto_spawned,
            sakura_voice_label,
            kero_voice_label,
            voices,
        }
    };

    for url in &probe_urls {
        if probe_tts_version(url) {
            return populate_cache_and_status(url.clone(), false, None);
        }
    }

    if allow_autostart {
        let target_port: Option<u16> = match mode.as_str() {
            "aivisspeech" | "aivis" => Some(10101),
            "voicevox" | "vv" => Some(50021),
            "coeiroink" => Some(50032),
            _ => None,
        };
        if let Ok(mut proc_guard) = managed_tts_process().lock() {
            if let Some(ref mut existing) = *proc_guard {
                if let Ok(Some(_)) = existing.child.try_wait() {
                    *proc_guard = None;
                } else if target_port.is_some_and(|tp| existing.port != tp) {
                    if let Some(mut prev) = proc_guard.take() {
                        let _ = prev.child.kill();
                        let _ = prev.child.wait();
                    }
                }
            }
            if proc_guard.is_none() {
                for (lbl, bin_path, port) in discover_tts_engine_binaries() {
                    if target_port.is_some_and(|tp| tp != port) {
                        continue;
                    }
                    let mut cmd = Command::new(&bin_path);
                    if let Some(parent) = bin_path.parent() {
                        cmd.current_dir(parent);
                    }
                    cmd.args(["--host", "127.0.0.1", "--port", &port.to_string()])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x0800_0000);
                    }
                    if let Ok(mut child) = cmd.spawn() {
                        let base = format!("http://127.0.0.1:{port}");
                        let start = Instant::now();
                        let mut ready = false;
                        while start.elapsed() < Duration::from_secs(12) {
                            if let Ok(Some(_)) = child.try_wait() {
                                break;
                            }
                            if probe_tts_version(&base) {
                                ready = true;
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(200));
                        }
                        if ready {
                            *proc_guard = Some(ManagedTtsProcess { port, child });
                            return populate_cache_and_status(base, true, Some(format!("{lbl} (:{port})")));
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
        }
    }

    if let Ok(mut cache) = cached_tts_state().lock() {
        cache.engine_label = "OS Native TTS".to_string();
        cache.base_url = None;
        cache.voices.clear();
        cache.installed_aivm_uuids.clear();
    }
    TtsEngineStatus {
        engine_label: "OS Native TTS".to_string(),
        base_url: None,
        auto_spawned: false,
        sakura_voice_label: "OS標準 (女性 · Kyoko)".to_string(),
        kero_voice_label: "OS標準 (少年/男性 · Eddy)".to_string(),
        voices: Vec::new(),
    }
}

/// Probe running local character TTS engines or headlessly launch an installed AivisSpeech / VOICEVOX engine.
pub fn ensure_tts_engine_ready_blocking(
    preferred_url: &str,
    allow_autostart: bool,
) -> TtsEngineStatus {
    ensure_tts_engine_ready_with_mode_blocking(preferred_url, "auto", allow_autostart)
}

/// Speak an ordered batch of `(scope_id, text)` utterances asynchronously with explicit prosody controls.
pub fn speak_dialogue_batch_with_prosody_async(
    segments: Vec<(u8, String)>,
    preferred_url: &str,
    tts_engine: &str,
    kero_archetype: KeroVoiceArchetype,
    sakura_speaker_override: Option<i64>,
    kero_speaker_override: Option<i64>,
    voice_speed: f32,
    voice_intonation: f32,
) {
    let cleaned_segments: Vec<(u8, String)> = segments
        .into_iter()
        .filter_map(|(sc, txt)| {
            let clean: String = txt.split_whitespace().collect::<Vec<_>>().join(" ");
            if clean.is_empty() {
                None
            } else {
                Some((sc, clean.chars().take(600).collect()))
            }
        })
        .collect();
    if cleaned_segments.is_empty() {
        return;
    }
    let pref = preferred_url.trim().trim_end_matches('/').to_string();
    let eng = tts_engine.trim().to_ascii_lowercase();
    let spd = crate::config_store::clamp_voice_speed(voice_speed);
    let inton = crate::config_store::clamp_voice_intonation(voice_intonation);

    let Ok(mut guard) = tts_sender().lock() else {
        return;
    };
    if guard.is_none() {
        let (tx, rx) = mpsc::channel::<SpeechBatch>();
        if std::thread::Builder::new()
            .name("nanikaai-tts".to_string())
            .spawn(move || {
                while let Ok(mut batch) = rx.recv() {
                    // Drain stale older turns while keeping all segments within the newest turn intact.
                    while let Ok(newer) = rx.try_recv() {
                        batch = newer;
                    }
                    speak_batch_blocking(&batch);
                }
            })
            .is_ok()
        {
            *guard = Some(tx);
        }
    }
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(SpeechBatch {
            segments: cleaned_segments,
            preferred_url: pref,
            tts_engine: eng,
            kero_archetype,
            sakura_speaker_override,
            kero_speaker_override,
            voice_speed: spd,
            voice_intonation: inton,
        });
    }
}

/// Speak an ordered batch of `(scope_id, text)` utterances asynchronously with explicit engine & partner archetype.
pub fn speak_dialogue_batch_with_options_async(
    segments: Vec<(u8, String)>,
    preferred_url: &str,
    tts_engine: &str,
    kero_archetype: KeroVoiceArchetype,
    sakura_speaker_override: Option<i64>,
    kero_speaker_override: Option<i64>,
) {
    speak_dialogue_batch_with_prosody_async(
        segments,
        preferred_url,
        tts_engine,
        kero_archetype,
        sakura_speaker_override,
        kero_speaker_override,
        crate::config_store::default_voice_speed(),
        crate::config_store::default_voice_intonation(),
    );
}

/// Speak an ordered batch of `(scope_id, text)` utterances asynchronously via AivisSpeech / VOICEVOX or OS TTS.
pub fn speak_dialogue_batch_async(
    segments: Vec<(u8, String)>,
    preferred_url: &str,
    sakura_speaker_override: Option<i64>,
    kero_speaker_override: Option<i64>,
) {
    speak_dialogue_batch_with_options_async(
        segments,
        preferred_url,
        "auto",
        KeroVoiceArchetype::Boyish,
        sakura_speaker_override,
        kero_speaker_override,
    );
}

/// Speak a single utterance asynchronously (convenience wrapper over batch speech).
pub fn speak_utterance_async(scope_id: u8, text: &str, voicevox_url: &str) {
    speak_dialogue_batch_async(vec![(scope_id, text.to_string())], voicevox_url, None, None);
}

fn validate_local_tts_url(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "[::1]" {
        Some(raw.trim_end_matches('/').to_string())
    } else {
        None
    }
}

fn speak_batch_blocking(batch: &SpeechBatch) {
    let status = ensure_tts_engine_ready_with_mode_blocking(&batch.preferred_url, &batch.tts_engine, true);
    let resolved = resolve_effective_voice_selection(
        &status.voices,
        batch.kero_archetype,
        batch.sakura_speaker_override,
        batch.kero_speaker_override,
    );

    for (scope_id, text) in &batch.segments {
        let mut spoken_by_engine = false;
        if let Some(ref base) = status.base_url {
            let (chosen_id, hint) = if *scope_id == 0 {
                (resolved.sakura_id.unwrap_or(2), TtsProsodyHint::Default)
            } else {
                let h = if resolved.used_female_fallback {
                    match batch.kero_archetype {
                        KeroVoiceArchetype::YouthMale => TtsProsodyHint::YouthMaleFallbackOnFemale,
                        _ => TtsProsodyHint::BoyishFallbackOnFemale,
                    }
                } else if batch.kero_archetype == KeroVoiceArchetype::Boyish
                    && batch.kero_speaker_override.is_none()
                {
                    TtsProsodyHint::BoyishNative
                } else {
                    TtsProsodyHint::Default
                };
                (resolved.kero_id.unwrap_or(3), h)
            };
            spoken_by_engine = try_tts_api_synthesis(
                base,
                chosen_id,
                text,
                hint,
                batch.voice_speed,
                batch.voice_intonation,
            );
        }
        if !spoken_by_engine {
            try_os_tts_blocking(*scope_id, batch.kero_archetype, text);
        }
    }
}

/// Synthesize speech WAV bytes via a local VOICEVOX / AivisSpeech endpoint without playing audio.
pub fn synthesize_tts_wav_bytes(
    base: &str,
    speaker: i64,
    text: &str,
) -> Result<Vec<u8>, String> {
    synthesize_tts_wav_bytes_with_hint(
        base,
        speaker,
        text,
        TtsProsodyHint::Default,
        crate::config_store::default_voice_speed(),
        crate::config_store::default_voice_intonation(),
    )
}

fn synthesize_tts_wav_bytes_with_hint(
    base: &str,
    speaker: i64,
    text: &str,
    hint: TtsProsodyHint,
    voice_speed: f32,
    voice_intonation: f32,
) -> Result<Vec<u8>, String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_millis(400))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let mut query_url =
        Url::parse(&format!("{}/audio_query", base.trim_end_matches('/'))).map_err(|e| e.to_string())?;
    query_url
        .query_pairs_mut()
        .append_pair("text", text)
        .append_pair("speaker", &speaker.to_string());

    let q_resp = client.post(query_url).send().map_err(|e| e.to_string())?;
    if !q_resp.status().is_success() {
        return Err(format!("audio_query HTTP {}", q_resp.status()));
    }
    let mut query_json = q_resp
        .json::<serde_json::Value>()
        .map_err(|e| e.to_string())?;

    if let Some(obj) = query_json.as_object_mut() {
        let base_speed = crate::config_store::clamp_voice_speed(voice_speed) as f64;
        let base_inton = crate::config_store::clamp_voice_intonation(voice_intonation) as f64;
        obj.insert("speedScale".to_string(), serde_json::json!(base_speed));
        obj.insert("intonationScale".to_string(), serde_json::json!(base_inton));
        if obj.contains_key("tempoDynamicsScale") {
            let td = (0.95 + (base_inton - 1.0) * 0.65).clamp(0.75, 1.40);
            obj.insert("tempoDynamicsScale".to_string(), serde_json::json!(td));
        }
        match hint {
            TtsProsodyHint::Default => {}
            TtsProsodyHint::BoyishNative => {
                obj.insert(
                    "speedScale".to_string(),
                    serde_json::json!((base_speed * 1.02).clamp(0.75, 1.40)),
                );
            }
            TtsProsodyHint::BoyishFallbackOnFemale => {
                obj.insert(
                    "speedScale".to_string(),
                    serde_json::json!((base_speed * 1.05).clamp(0.75, 1.40)),
                );
                obj.insert("tempoDynamicsScale".to_string(), serde_json::json!(1.18));
                obj.insert("pitchScale".to_string(), serde_json::json!(-0.04));
            }
            TtsProsodyHint::YouthMaleFallbackOnFemale => {
                obj.insert(
                    "speedScale".to_string(),
                    serde_json::json!((base_speed * 0.96).clamp(0.75, 1.40)),
                );
                obj.insert("pitchScale".to_string(), serde_json::json!(-0.07));
            }
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
    let wav_bytes = s_resp.bytes().map_err(|e| e.to_string())?.to_vec();
    if wav_bytes.len() < 44 {
        return Err("synthesized audio payload too small".to_string());
    }
    Ok(wav_bytes)
}

fn try_tts_api_synthesis(
    base: &str,
    speaker: i64,
    text: &str,
    hint: TtsProsodyHint,
    voice_speed: f32,
    voice_intonation: f32,
) -> bool {
    let Ok(wav_bytes) =
        synthesize_tts_wav_bytes_with_hint(base, speaker, text, hint, voice_speed, voice_intonation)
    else {
        return false;
    };
    let tmp_wav = std::env::temp_dir().join(format!("nanikaai_tts_{}.wav", std::process::id()));
    if fs::write(&tmp_wav, &wav_bytes).is_err() {
        return false;
    }
    let played = play_wav_file(&tmp_wav);
    let _ = fs::remove_file(&tmp_wav);
    played
}

fn play_wav_file(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        return Command::new("afplay")
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    #[cfg(target_os = "windows")]
    {
        return Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "(New-Object Media.SoundPlayer $env:NANIKAAI_WAV).PlaySync()",
            ])
            .env("NANIKAAI_WAV", path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if Command::new("aplay")
            .args(["-q"])
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
        return Command::new("paplay")
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
}

fn try_os_tts_blocking(scope_id: u8, archetype: KeroVoiceArchetype, text: &str) {
    #[cfg(target_os = "macos")]
    {
        let candidates: &[(&str, Option<&str>)] = if scope_id == 0 {
            &[("Kyoko", Some("195"))]
        } else {
            match archetype {
                KeroVoiceArchetype::Boyish => {
                    &[("Eddy", Some("208")), ("Reed", Some("198")), ("Otoya", None)]
                }
                KeroVoiceArchetype::YouthMale => {
                    &[("Reed", Some("182")), ("Eddy", Some("182")), ("Otoya", None)]
                }
                KeroVoiceArchetype::FemalePartner => {
                    &[("Flo", Some("196")), ("Sandy", Some("196")), ("Shelley", None)]
                }
            }
        };
        for (voice, rate) in candidates {
            let mut cmd = Command::new("say");
            cmd.args(["-v", voice]);
            if let Some(r) = rate {
                cmd.args(["-r", r]);
            }
            cmd.args(["--", text]);
            if cmd.status().map(|s| s.success()).unwrap_or(false) {
                return;
            }
        }
        let _ = Command::new("say").args(["--", text]).status();
    }
    #[cfg(target_os = "windows")]
    {
        let role = if scope_id == 0 {
            "female"
        } else {
            archetype.short_code()
        };
        let ps = r#"
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$role = $env:NANIKAAI_TTS_ROLE
$txt = $env:NANIKAAI_TTS_TEXT
$defVoice = $s.Voice.Name
if ($role -eq 'female') {
  try { $s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Female) } catch {}
  $s.Speak($txt)
} elseif ($role -eq 'girl') {
  try { $s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Female, [System.Speech.Synthesis.VoiceAge]::Teen) } catch {}
  $esc = [System.Security.SecurityElement]::Escape($txt)
  $s.SpeakSsml("<speak version='1.0' xml:lang='ja-JP'><prosody pitch='+8%' rate='+5%'>$esc</prosody></speak>")
} elseif ($role -eq 'male') {
  try { $s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Male, [System.Speech.Synthesis.VoiceAge]::Adult) } catch {}
  $esc = [System.Security.SecurityElement]::Escape($txt)
  $p = if ($s.Voice.Name -eq $defVoice) { '-22%' } else { '-4%' }
  $s.SpeakSsml("<speak version='1.0' xml:lang='ja-JP'><prosody pitch='$p' rate='-2%'>$esc</prosody></speak>")
} else {
  try { $s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Male, [System.Speech.Synthesis.VoiceAge]::Child) } catch {
    try { $s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Male) } catch {}
  }
  $esc = [System.Security.SecurityElement]::Escape($txt)
  $p = if ($s.Voice.Name -eq $defVoice) { '-18%' } else { '+10%' }
  $s.SpeakSsml("<speak version='1.0' xml:lang='ja-JP'><prosody pitch='$p' rate='+9%'>$esc</prosody></speak>")
}
"#;
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", ps])
            .env("NANIKAAI_TTS_ROLE", role)
            .env("NANIKAAI_TTS_TEXT", text)
            .status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let (vtype, pitch, rate) = if scope_id == 0 {
            ("female1", "15", "0")
        } else {
            match archetype {
                KeroVoiceArchetype::Boyish => ("child_male", "5", "10"),
                KeroVoiceArchetype::YouthMale => ("male1", "-20", "0"),
                KeroVoiceArchetype::FemalePartner => ("female2", "-5", "0"),
            }
        };
        let ok = Command::new("spd-say")
            .args(["-t", vtype, "-p", pitch, "-r", rate, "--", text])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            let _ = Command::new("espeak-ng").args(["--", text]).status();
        }
    }
}

fn parse_semver_parts(raw: &str) -> ([u64; 3], bool) {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let (core, has_pre) = match trimmed.split_once(['-', '+']) {
        Some((c, rest)) => (c, !rest.is_empty() && trimmed.contains('-')),
        None => (trimmed, false),
    };
    let mut nums = [0u64; 3];
    for (idx, part) in core.split('.').take(3).enumerate() {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        nums[idx] = digits.parse::<u64>().unwrap_or(0);
    }
    (nums, has_pre)
}

/// Compare two SemVer strings (`candidate` relative to `current`).
pub fn compare_semver(current: &str, candidate: &str) -> Ordering {
    let (cur_nums, cur_pre) = parse_semver_parts(current);
    let (cand_nums, cand_pre) = parse_semver_parts(candidate);
    match cand_nums.cmp(&cur_nums) {
        Ordering::Equal => match (cur_pre, cand_pre) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => Ordering::Equal,
        },
        other => other,
    }
}

/// Query GitHub Releases latest endpoint for newer NanikaAi versions.
pub fn check_github_release_update(current_version: &str) -> Result<UpdateCheckInfo, String> {
    check_github_release_update_at(current_version, GITHUB_RELEASES_LATEST_API)
}

pub fn check_github_release_update_at(
    current_version: &str,
    api_url: &str,
) -> Result<UpdateCheckInfo, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent(format!("NanikaAi/{current_version}"))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let resp = client
        .get(api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("Network error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API returned status {}", resp.status()));
    }
    let parsed: GitHubReleaseResponse = resp
        .json()
        .map_err(|e| format!("Invalid GitHub JSON: {e}"))?;
    let latest_clean = parsed.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let current_clean = current_version.trim().trim_start_matches(['v', 'V']).to_string();
    let has_update = compare_semver(&current_clean, &latest_clean) == Ordering::Greater;
    let notes_preview: String = parsed
        .body
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect();
    let safe_url = validate_public_url(&parsed.html_url)
        .unwrap_or_else(|_| "https://github.com/JungWonYong/nanikaai/releases".to_string());
    Ok(UpdateCheckInfo {
        current_version: current_clean,
        latest_version: latest_clean,
        has_update,
        release_url: safe_url,
        notes_preview,
    })
}

fn validate_url_lexically(raw_url: &str) -> Result<String, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err("URL is required.".to_string());
    }
    let parsed = Url::parse(trimmed).map_err(|_| "Invalid URL syntax.".to_string())?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err("Only public http:// or https:// URLs are allowed.".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL missing host.".to_string())?
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || !host.contains('.')
    {
        return Err("Blocked local or unqualified hostname.".to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        let non_public = match ip {
            IpAddr::V4(v4) => {
                v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_broadcast()
                    || v4.is_documentation()
                    || v4.is_unspecified()
                    || v4.is_multicast()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified() || v6.is_multicast(),
        };
        if non_public {
            return Err(format!("Blocked non-public IP address: {ip}"));
        }
    }
    Ok(trimmed.to_string())
}

/// Validate a public URL and open it in the user's browser (or validate only when `dry_run = true`).
pub fn open_validated_browser_url(raw_url: &str, dry_run: bool) -> Result<String, String> {
    let clean_url = match validate_public_url(raw_url) {
        Ok(u) => u,
        Err(err) if dry_run && err.starts_with("Could not resolve host:") => {
            validate_url_lexically(raw_url)?
        }
        Err(err) => return Err(err),
    };
    if dry_run {
        return Ok(clean_url);
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["--", &clean_url])
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "Start-Process -FilePath $env:NANIKAAI_URL",
            ])
            .env("NANIKAAI_URL", &clean_url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(&clean_url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    Ok(clean_url)
}

pub fn is_blocked_drop_target(path: &Path) -> bool {
    if is_sensitive_path(path) {
        return true;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "pem" | "key" | "p12" | "pfx" | "kdbx" | "keystore" | "jks"
    ) || file_name.starts_with("id_rsa")
        || file_name.starts_with("id_ed25519")
        || file_name.starts_with("id_ecdsa")
        || file_name == ".netrc"
        || file_name == ".npmrc"
        || file_name == ".pypirc"
}

fn is_text_extension(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "md"
            | "markdown"
            | "rs"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "log"
            | "csv"
            | "html"
            | "htm"
            | "css"
            | "sh"
            | "zsh"
            | "ps1"
            | "bat"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "java"
            | "go"
            | "sql"
            | "xml"
            | "ini"
            | "cfg"
            | "url"
            | "webloc"
            | "desktop"
    )
}

fn extract_shortcut_url(ext: &str, text: &str) -> Option<String> {
    if ext == "url" || ext == "desktop" {
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed
                .strip_prefix("URL=")
                .or_else(|| trimmed.strip_prefix("url="))
            {
                let url = rest.trim();
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Some(url.to_string());
                }
            }
        }
    }
    if ext == "webloc" {
        static WEBLOC_RE: OnceLock<Regex> = OnceLock::new();
        let re = WEBLOC_RE
            .get_or_init(|| Regex::new(r"<string>\s*(https?://[^<\s]+)\s*</string>").unwrap());
        if let Some(cap) = re.captures(text) {
            return Some(cap[1].to_string());
        }
    }
    None
}

fn read_prefix_utf8(path: &Path, max_bytes: u64) -> Result<(String, bool), String> {
    let mut f = File::open(path).map_err(|e| format!("読み込みに失敗しました: {e}"))?;
    let mut buf = Vec::new();
    std::io::Read::by_ref(&mut f)
        .take(max_bytes + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("読み込みに失敗しました: {e}"))?;
    let truncated = buf.len() > max_bytes as usize;
    if truncated {
        buf.truncate(max_bytes as usize);
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

/// Inspect a user-dropped file or folder safely BEFORE handing wrapped metadata to AI or offline handlers.
pub fn inspect_dropped_path(path: &Path) -> Result<DroppedPayload, String> {
    if is_blocked_drop_target(path) {
        return Err(
            "セキュリティ保護のため、認証情報や秘密鍵に関わるファイルは読み込まないよ。".to_string(),
        );
    }
    if !path.exists() {
        return Err("ドロップされたパスが見つかりませんでした。".to_string());
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string());

    if path.is_dir() {
        let mut names = Vec::new();
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten().take(20) {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        let summary_raw = format!(
            "Directory: {}\nItems shown ({}):\n{}",
            file_name,
            names.len(),
            names.join("\n")
        );
        let offline_summary = format!(
            "フォルダ「{}」を確認したよ。先頭 {} 件の項目が見つかりました。",
            file_name,
            names.len()
        );
        return Ok(DroppedPayload {
            file_name,
            kind_label: "folder".to_string(),
            size_bytes: 0,
            wrapped_data: wrap_untrusted_data("dropped_folder", &summary_raw),
            extracted_url: None,
            offline_summary,
        });
    }

    let meta = fs::metadata(path).map_err(|e| format!("ファイル情報の取得に失敗しました: {e}"))?;
    let size_bytes = meta.len();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp") {
        let dims = image::image_dimensions(path).ok();
        let dim_str = dims
            .map(|(w, h)| format!("{w}x{h}px"))
            .unwrap_or_else(|| "unknown".to_string());
        let info = format!(
            "Image file: {}\nDimensions: {}\nBytes: {}",
            file_name, dim_str, size_bytes
        );
        let offline_summary = format!(
            "画像「{}」（{}, {} bytes）を受け取ったよ！",
            file_name, dim_str, size_bytes
        );
        return Ok(DroppedPayload {
            file_name,
            kind_label: "image".to_string(),
            size_bytes,
            wrapped_data: wrap_untrusted_data("dropped_image", &info),
            extracted_url: None,
            offline_summary,
        });
    }

    if is_text_extension(&ext) || size_bytes <= 16_384 && ext.is_empty() {
        let (text, truncated) = read_prefix_utf8(path, 12_288)?;
        let line_count = text.lines().count();
        let extracted_url = extract_shortcut_url(&ext, &text);
        let preview: String = text.chars().take(3200).collect();
        let header = format!(
            "File: {}\nType: {}\nBytes: {}\nLines(sampled): {}{}\n---\n{}",
            file_name,
            if ext.is_empty() { "text" } else { ext.as_str() },
            size_bytes,
            line_count,
            if truncated { " (truncated)" } else { "" },
            preview
        );
        let first_line = text
            .lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(64)
            .collect::<String>();
        let offline_summary = if let Some(ref url) = extracted_url {
            format!(
                "URLショートカット「{}」を読み取ったよ！\\_a[{}]リンクを開く\\_a",
                file_name, url
            )
        } else if first_line.is_empty() {
            format!(
                "「{}」（{} bytes, {}行）を受け取ったよ。",
                file_name, size_bytes, line_count
            )
        } else {
            format!(
                "「{}」（{}行, {} bytes）を受け取ったよ。冒頭:「{}」",
                file_name, line_count, size_bytes, first_line
            )
        };
        return Ok(DroppedPayload {
            file_name,
            kind_label: if ext.is_empty() {
                "text".to_string()
            } else {
                ext
            },
            size_bytes,
            wrapped_data: wrap_untrusted_data("dropped_file", &header),
            extracted_url,
            offline_summary,
        });
    }

    let info = format!(
        "Binary or archive file: {}\nExtension: {}\nBytes: {}",
        file_name, ext, size_bytes
    );
    let offline_summary = format!(
        "ファイル「{}」（{} / {} bytes）を受け取ったよ。",
        file_name,
        if ext.is_empty() { "binary" } else { &ext },
        size_bytes
    );
    Ok(DroppedPayload {
        file_name,
        kind_label: if ext.is_empty() {
            "binary".to_string()
        } else {
            ext
        },
        size_bytes,
        wrapped_data: wrap_untrusted_data("dropped_file", &info),
        extracted_url: None,
        offline_summary,
    })
}

/// Normalize a temp path helper if ever needed by tests.
pub fn sanitize_dropped_preview_path(p: &Path) -> PathBuf {
    p.file_name().map(PathBuf::from).unwrap_or_default()
}

fn managed_ollama_process() -> &'static Mutex<Option<Child>> {
    static CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    CHILD.get_or_init(|| Mutex::new(None))
}

static LOCAL_AI_SETUP_RUNNING: AtomicBool = AtomicBool::new(false);

struct LocalAiSetupGuard;

impl Drop for LocalAiSetupGuard {
    fn drop(&mut self) {
        LOCAL_AI_SETUP_RUNNING.store(false, AtomicOrdering::Release);
    }
}

/// Returns whether a 1-click local Ollama setup task is currently running.
pub fn is_local_ai_setup_running() -> bool {
    LOCAL_AI_SETUP_RUNNING.load(AtomicOrdering::Acquire)
}

/// Stop any local Ollama serve daemon spawned by NanikaAi during the current session.
pub fn stop_managed_ollama_engine() {
    if let Ok(mut guard) = managed_ollama_process().lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Recommend safe lightweight free local Ollama models ordered best-first for the detected system RAM.
pub fn recommend_local_ollama_candidates(ram_total_gb: f64) -> Vec<&'static str> {
    let raw = if ram_total_gb > 0.1 && ram_total_gb < 12.0 {
        vec!["gemma4:e2b", "llama3.2:3b"]
    } else if ram_total_gb >= 12.0 && ram_total_gb < 28.0 {
        vec!["gemma4:e4b", "llama3.2:3b"]
    } else if ram_total_gb >= 28.0 {
        vec!["gemma4:e4b", "llama3.1:8b", "llama3.2:3b"]
    } else {
        vec!["llama3.2:3b", "gemma4:e2b"]
    };
    raw.into_iter()
        .filter(|m| assert_allowed_model(m).is_ok())
        .collect()
}

/// Format a concise user-facing hardware recommendation hint for Settings Tab 3 (cached).
pub fn recommended_local_ai_summary() -> String {
    static SUMMARY: OnceLock<String> = OnceLock::new();
    SUMMARY
        .get_or_init(|| {
            let (ram_gb, _) = query_ram_gb();
            let cands = recommend_local_ollama_candidates(ram_gb);
            let primary = cands.first().copied().unwrap_or("gemma4:e4b");
            let alt = cands.get(1).copied().unwrap_or("llama3.2:3b");
            if ram_gb > 0.1 {
                format!("RAM {:.0}GB → {primary} / {alt}", ram_gb)
            } else {
                format!("標準軽量構成 → {primary} / {alt}")
            }
        })
        .clone()
}

fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let p = dir.join(bin);
        if p.is_file() {
            return Some(p);
        }
        #[cfg(target_os = "windows")]
        {
            let exe = dir.join(format!("{bin}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// Locate an installed `ollama` binary on macOS, Windows, or Linux.
pub fn discover_ollama_binary() -> Option<PathBuf> {
    if let Some(p) = find_in_path("ollama") {
        return Some(p);
    }
    let home = user_home_dir();
    #[cfg(target_os = "macos")]
    let candidates = vec![
        PathBuf::from("/opt/homebrew/bin/ollama"),
        PathBuf::from("/usr/local/bin/ollama"),
        PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"),
        home.join("Applications/Ollama.app/Contents/Resources/ollama"),
    ];
    #[cfg(target_os = "windows")]
    let mut candidates = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join(r"Programs\Ollama\ollama.exe"));
        }
        if let Some(pf) = std::env::var_os("PROGRAMFILES") {
            candidates.push(PathBuf::from(pf).join(r"Ollama\ollama.exe"));
        }
        candidates.push(home.join(r"AppData\Local\Programs\Ollama\ollama.exe"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    let candidates = vec![
        PathBuf::from("/usr/local/bin/ollama"),
        PathBuf::from("/usr/bin/ollama"),
        PathBuf::from("/snap/bin/ollama"),
        home.join(".local/bin/ollama"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "macos")]
fn discover_ollama_macos_app() -> Option<PathBuf> {
    let home = user_home_dir();
    [
        PathBuf::from("/Applications/Ollama.app"),
        home.join("Applications/Ollama.app"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn ollama_endpoint_tags(base: &str, timeout_ms: u64) -> Option<Vec<String>> {
    let clean = base.trim().trim_end_matches('/');
    let client = Client::builder()
        .connect_timeout(Duration::from_millis(timeout_ms))
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .ok()?;
    let resp = client.get(format!("{clean}/api/tags")).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let val: serde_json::Value = resp.json().ok()?;
    let models = val
        .get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|s| s.as_str()))
                .filter(|n| !is_forbidden_model(n))
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(models)
}

#[derive(Debug, Clone)]
pub struct LocalAiSetupResult {
    pub installed_ready: bool,
    pub chosen_model: String,
    pub ram_gb: f64,
    pub opened_installer_url: Option<String>,
    pub summary_script: String,
}

/// Parse one streaming NDJSON status line returned by Ollama `POST /api/pull`.
pub fn parse_ollama_pull_progress_line(
    line: &str,
) -> Result<Option<(String, Option<u8>, String)>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let val: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
        if !err.trim().is_empty() {
            return Err(err.trim().to_string());
        }
    }
    let status = val
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("preparing")
        .trim()
        .to_string();
    let completed = val.get("completed").and_then(|v| v.as_u64()).unwrap_or(0);
    let total = val.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
    if total > 0 && completed <= total {
        let pct = ((completed as f64 / total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let human = if total >= 1_073_741_824 {
            format!(
                "{pct}% ({:.1}/{:.1}GB)",
                completed as f64 / 1_073_741_824.0,
                total as f64 / 1_073_741_824.0
            )
        } else {
            format!(
                "{pct}% ({:.0}/{:.0}MB)",
                completed as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0
            )
        };
        Ok(Some((status, Some(pct), human)))
    } else {
        Ok(Some((status.clone(), None, status)))
    }
}

fn pull_ollama_model_streaming<F>(
    base: &str,
    model: &str,
    on_progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(String),
{
    assert_allowed_model(model)?;
    let clean = base.trim().trim_end_matches('/');
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{clean}/api/pull"))
        .json(&serde_json::json!({
            "name": model,
            "stream": true
        }))
        .send()
        .map_err(|e| format!("Ollama /api/pull 接続失敗: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Ollama /api/pull HTTP {}", resp.status()));
    }

    on_progress(format!("⬇️ 無料AI {model} をダウンロード準備中…"));
    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut saw_success = false;

    loop {
        line.clear();
        let read_bytes = reader
            .read_line(&mut line)
            .map_err(|e| format!("ダウンロードストリーム読取エラー: {e}"))?;
        if read_bytes == 0 {
            break;
        }
        if let Some((status, pct_opt, human)) = parse_ollama_pull_progress_line(&line)? {
            if status.eq_ignore_ascii_case("success") {
                saw_success = true;
            }
            if last_emit.elapsed() >= Duration::from_millis(350) || pct_opt == Some(100) || saw_success {
                on_progress(format!("⬇️ {model} 準備中: {human}"));
                last_emit = Instant::now();
            }
        }
    }

    if let Some(tags) = ollama_endpoint_tags(clean, 1500) {
        let model_base = model.split(':').next().unwrap_or(model);
        if tags
            .iter()
            .any(|t| t == model || t.starts_with(&format!("{model_base}:")))
            || (saw_success && !tags.is_empty())
        {
            return Ok(());
        }
    }
    if saw_success {
        Ok(())
    } else {
        Err(format!("モデル「{model}」の展開確認ができませんでした。"))
    }
}

#[cfg(target_os = "macos")]
fn try_auto_install_ollama_macos<F>(on_progress: &mut F) -> Result<(), String>
where
    F: FnMut(String),
{
    for brew_path in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
        let bp = Path::new(brew_path);
        if bp.is_file() {
            on_progress("📦 Homebrew で Ollama を自動導入中…".to_string());
            let ok_cask = Command::new(bp)
                .args(["install", "--cask", "ollama"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok_cask && (discover_ollama_binary().is_some() || discover_ollama_macos_app().is_some()) {
                return Ok(());
            }
            let ok_formula = Command::new(bp)
                .args(["install", "ollama"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok_formula && (discover_ollama_binary().is_some() || discover_ollama_macos_app().is_some()) {
                return Ok(());
            }
        }
    }

    let dl_url = "https://ollama.com/download/Ollama-darwin.zip";
    validate_public_url(dl_url)?;
    on_progress("⬇️ Ollama for macOS を公式サーバーから取得中…".to_string());

    let cache_dir = default_user_data_dir().join("cache");
    fs::create_dir_all(&cache_dir).map_err(|e| format!("キャッシュ作成失敗: {e}"))?;
    let zip_path = cache_dir.join("Ollama-darwin.zip");
    let apps_dir = user_home_dir().join("Applications");
    let _ = fs::create_dir_all(&apps_dir);

    let dl_res = (|| -> Result<(), String> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| e.to_string())?;
        let mut resp = client
            .get(dl_url)
            .send()
            .map_err(|e| format!("Ollama ダウンロード失敗: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Ollama ダウンロード HTTP {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(0);
        let mut f = File::create(&zip_path).map_err(|e| format!("一時ZIP作成失敗: {e}"))?;
        let mut buf = [0u8; 65536];
        let mut got = 0u64;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        loop {
            let n = resp
                .read(&mut buf)
                .map_err(|e| format!("ダウンロード読取失敗: {e}"))?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n])
                .map_err(|e| format!("一時ZIP書込失敗: {e}"))?;
            got += n as u64;
            if last_emit.elapsed() >= Duration::from_millis(350) {
                if total > 0 {
                    let pct = ((got as f64 / total as f64) * 100.0).round().clamp(0.0, 100.0);
                    on_progress(format!(
                        "⬇️ Ollama 本体取得中… {pct:.0}% ({:.0}/{:.0}MB)",
                        got as f64 / 1_048_576.0,
                        total as f64 / 1_048_576.0
                    ));
                } else {
                    on_progress(format!(
                        "⬇️ Ollama 本体取得中… {:.0}MB",
                        got as f64 / 1_048_576.0
                    ));
                }
                last_emit = Instant::now();
            }
        }
        drop(f);
        on_progress("📂 ~/Applications へ Ollama.app を展開中…".to_string());
        let ok = Command::new("ditto")
            .args(["-x", "-k"])
            .arg(&zip_path)
            .arg(&apps_dir)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err("Ollama-darwin.zip の展開に失敗しました。".to_string());
        }
        Ok(())
    })();

    let _ = fs::remove_file(&zip_path);
    dl_res
}

fn ensure_ollama_daemon_running<F>(base: &str, on_progress: &mut F) -> Result<(), String>
where
    F: FnMut(String),
{
    if ollama_endpoint_tags(base, 800).is_some() {
        return Ok(());
    }
    on_progress("🚀 ローカル Ollama エンジンを起動中…".to_string());

    #[cfg(target_os = "macos")]
    {
        if let Some(app_path) = discover_ollama_macos_app() {
            let _ = Command::new("open")
                .args(["-g", "-a"])
                .arg(&app_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(6) {
                if ollama_endpoint_tags(base, 500).is_some() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(350));
            }
        }
    }

    let Some(bin) = discover_ollama_binary() else {
        return Err("Ollama 実行ファイルが見つかりませんでした。".to_string());
    };
    let mut cmd = Command::new(&bin);
    cmd.arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(child) = cmd.spawn() {
        if let Ok(mut guard) = managed_ollama_process().lock() {
            *guard = Some(child);
        }
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(12) {
            if ollama_endpoint_tags(base, 500).is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(350));
        }
    }
    if ollama_endpoint_tags(base, 800).is_some() {
        Ok(())
    } else {
        Err("Ollama サービスの起動がタイムアウトしました。".to_string())
    }
}

/// Prepare local Ollama (install/start if possible, or open official installer) and pull a
/// lightweight safe free model matched to the machine's physical RAM.
pub fn prepare_local_ollama_blocking<F>(
    endpoint_url: &str,
    mut on_progress: F,
) -> Result<LocalAiSetupResult, String>
where
    F: FnMut(String),
{
    if LOCAL_AI_SETUP_RUNNING
        .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
        .is_err()
    {
        return Err("すでに無料ローカルAIの自動セットアップが進行中です。".to_string());
    }
    let _guard = LocalAiSetupGuard;

    let clean_base = if endpoint_url.trim().is_empty() {
        "http://127.0.0.1:11434"
    } else {
        endpoint_url.trim().trim_end_matches('/')
    };
    let (ram_gb, _) = query_ram_gb();
    let ram_label = if ram_gb > 0.1 {
        format!("{:.0}GB", ram_gb)
    } else {
        "標準".to_string()
    };
    on_progress(format!("🔍 PCメモリ ({ram_label}) と Ollama 状態を確認中…"));

    if ollama_endpoint_tags(clean_base, 800).is_none() {
        let mut has_local = discover_ollama_binary().is_some();
        #[cfg(target_os = "macos")]
        {
            has_local = has_local || discover_ollama_macos_app().is_some();
        }
        if !has_local {
            #[cfg(target_os = "macos")]
            {
                if try_auto_install_ollama_macos(&mut on_progress).is_err() {
                    let url = open_validated_browser_url("https://ollama.com/download", false)
                        .unwrap_or_else(|_| "https://ollama.com/download".to_string());
                    return Ok(LocalAiSetupResult {
                        installed_ready: false,
                        chosen_model: String::new(),
                        ram_gb,
                        opened_installer_url: Some(url.clone()),
                        summary_script: format!(
                            "\\0\\s[3]Ollama 本体のダウンロードページを開いたよ！（{_u}）\\nインストールして起動したあと、下のボタンでもう一度モデル自動取得を押してね。\\n\\q[🔄 インストール後に再試行する,__setup_local_ai__]\\1\\s[10]あと1クリックで無料AIモデルまで自動で入るで！\\e",
                            _u = url
                        ),
                    });
                }
            }
            #[cfg(target_os = "windows")]
            {
                on_progress("📦 winget で Ollama を自動導入中…".to_string());
                let mut wcmd = Command::new("winget");
                wcmd.args([
                    "install",
                    "-e",
                    "--id",
                    "Ollama.Ollama",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    "--silent",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
                {
                    use std::os::windows::process::CommandExt;
                    wcmd.creation_flags(0x0800_0000);
                }
                let _ = wcmd.status();
                if discover_ollama_binary().is_none() {
                    let url = open_validated_browser_url("https://ollama.com/download/windows", false)
                        .unwrap_or_else(|_| "https://ollama.com/download/windows".to_string());
                    return Ok(LocalAiSetupResult {
                        installed_ready: false,
                        chosen_model: String::new(),
                        ram_gb,
                        opened_installer_url: Some(url.clone()),
                        summary_script: format!(
                            "\\0\\s[3]Windows 版 Ollama の公式ダウンロードページを開いたよ！({url})\\n導入が終わったら下のボタンで無料軽量モデルを自動取得できるよ。\\n\\q[🔄 インストール後に再試行する,__setup_local_ai__]\\1\\s[10]入れてから押せば自動接続やで！\\e"
                        ),
                    });
                }
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let url = open_validated_browser_url("https://ollama.com/download/linux", false)
                    .unwrap_or_else(|_| "https://ollama.com/download/linux".to_string());
                return Ok(LocalAiSetupResult {
                    installed_ready: false,
                    chosen_model: String::new(),
                    ram_gb,
                    opened_installer_url: Some(url.clone()),
                    summary_script: format!(
                        "\\0\\s[3]Linux 用 Ollama 導入ガイドを開いたよ！({url})\\nターミナルで公式コマンド実行後、下のボタンで推奨モデル（{cand}）を自動取得できるよ。\\n\\q[🔄 Ollama起動後にモデル自動取得,__setup_local_ai__]\\1\\s[10]待ってるで！\\e",
                        cand = recommend_local_ollama_candidates(ram_gb)
                            .first()
                            .copied()
                            .unwrap_or("gemma4:e2b")
                    ),
                });
            }
        }
        ensure_ollama_daemon_running(clean_base, &mut on_progress)?;
    }

    let existing_tags = ollama_endpoint_tags(clean_base, 1200).unwrap_or_default();
    for &pref in PREFERRED_MODELS {
        let pref_base = pref.split(':').next().unwrap_or(pref);
        if let Some(found) = existing_tags
            .iter()
            .find(|m| m.as_str() == pref || m.starts_with(&format!("{pref_base}:")))
        {
            return Ok(LocalAiSetupResult {
                installed_ready: true,
                chosen_model: found.clone(),
                ram_gb,
                opened_installer_url: None,
                summary_script: format!(
                    "\\0\\s[5]導入済みのローカルAI「{found}」（Ollama）に接続したよ！さっそくお話ししようね！\\1\\s[10]待ち時間ゼロで準備OKやで！\\e"
                ),
            });
        }
    }

    let cands = recommend_local_ollama_candidates(ram_gb);
    let mut last_err = String::new();
    for cand in &cands {
        match pull_ollama_model_streaming(clean_base, cand, &mut on_progress) {
            Ok(()) => {
                return Ok(LocalAiSetupResult {
                    installed_ready: true,
                    chosen_model: (*cand).to_string(),
                    ram_gb,
                    opened_installer_url: None,
                    summary_script: format!(
                        "\\0\\s[5]無料ローカルAI（Ollama + {cand}）の自動準備が完了したよ！これからはオフラインでも会話できるね。\\1\\s[10]ダブルクリックや入力バーから何でも話しかけてな！\\e"
                    ),
                });
            }
            Err(e) => {
                last_err = e;
            }
        }
    }

    Err(if last_err.is_empty() {
        "推奨無料AIモデルの自動取得に失敗しました。".to_string()
    } else {
        last_err
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_ordering_handles_tags_and_prereleases() {
        assert_eq!(compare_semver("0.1.3", "0.1.4"), Ordering::Greater);
        assert_eq!(compare_semver("0.1.4", "v0.1.5"), Ordering::Greater);
        assert_eq!(compare_semver("0.1.4", "0.1.4"), Ordering::Equal);
        assert_eq!(compare_semver("0.2.0-rc1", "0.2.0"), Ordering::Greater);
        assert_eq!(compare_semver("0.2.0", "0.1.9"), Ordering::Less);
    }

    #[test]
    fn extract_speech_groups_by_speaker() {
        let script = r"\0こんにちは！\_w[200]元気？\n今日は晴れだよ。\1せやな！\e";
        let segs = extract_speech_segments(script);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].0, 0);
        assert!(segs[0].1.contains("こんにちは！"));
        assert_eq!(segs[1], (1, "せやな！".to_string()));
    }

    #[test]
    fn sensitive_drop_is_blocked_before_read() {
        let dir = std::env::temp_dir().join(format!("nanikaai_drop_sec_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let env_file = dir.join(".env");
        fs::write(&env_file, "SAMPLE_SETTING=placeholder_value").unwrap();
        let res = inspect_dropped_path(&env_file);
        assert!(res.is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_browser_url_blocks_loopback_and_allows_public_https() {
        assert!(open_validated_browser_url("http://127.0.0.1:8080/admin", true).is_err());
        assert!(open_validated_browser_url("file:///etc/passwd", true).is_err());
        assert!(open_validated_browser_url("https://github.com/JungWonYong/nanikaai", true).is_ok());
    }

    fn vopt(style_id: i64, speaker_name: &str, style_name: &str) -> TtsVoiceOption {
        TtsVoiceOption {
            style_id,
            speaker_name: speaker_name.to_string(),
            style_name: style_name.to_string(),
            label: format!("{speaker_name} ({style_name})"),
        }
    }

    #[test]
    fn infer_kero_voice_archetype_matches_skins_and_overrides() {
        assert_eq!(
            infer_kero_voice_archetype("neo_default", "さくら＆うにゅう", "うにゅう", None),
            KeroVoiceArchetype::Boyish
        );
        assert_eq!(
            infer_kero_voice_archetype("mayura", "まゆら", "黒うにゅう", None),
            KeroVoiceArchetype::Boyish
        );
        assert_eq!(
            infer_kero_voice_archetype("naru", "奈留", "ゆうか", None),
            KeroVoiceArchetype::FemalePartner
        );
        assert_eq!(
            infer_kero_voice_archetype("mayura_santa", "まゆらサンタ", "黒姉", None),
            KeroVoiceArchetype::FemalePartner
        );
        assert_eq!(
            infer_kero_voice_archetype("futaba", "双葉", "ただきちさん", None),
            KeroVoiceArchetype::YouthMale
        );
        assert_eq!(
            infer_kero_voice_archetype("kikka_master", "橘花", "斗和", None),
            KeroVoiceArchetype::YouthMale
        );
        assert_eq!(
            infer_kero_voice_archetype("neo_default", "さくら＆うにゅう", "うにゅう", Some("girl")),
            KeroVoiceArchetype::FemalePartner
        );
        assert_eq!(
            infer_kero_voice_archetype("naru", "奈留", "ゆうか", Some("boy")),
            KeroVoiceArchetype::Boyish
        );
        assert_eq!(
            infer_kero_voice_archetype("naru", "奈留", "ゆうか", Some("male")),
            KeroVoiceArchetype::YouthMale
        );
    }

    #[test]
    fn pick_speaker_pair_selects_distinct_boyish_male_and_female_partners() {
        let aivis_voices = vec![
            vopt(888753760, "まお", "ノーマル"),
            vopt(2200001, "コハク", "ノーマル"),
            vopt(1500001, "らせつん", "標準"),
            vopt(1600001, "Lux", "ノーマル"),
            vopt(1700001, "凛音エル", "ノーマル"),
        ];
        assert_eq!(
            pick_speaker_pair_for_archetype(&aivis_voices, KeroVoiceArchetype::Boyish),
            (Some(888753760), Some(1500001), false)
        );
        assert_eq!(
            pick_speaker_pair_for_archetype(&aivis_voices, KeroVoiceArchetype::YouthMale),
            (Some(888753760), Some(1600001), false)
        );
        assert_eq!(
            pick_speaker_pair_for_archetype(&aivis_voices, KeroVoiceArchetype::FemalePartner),
            (Some(888753760), Some(1700001), false)
        );

        let only_two_female = vec![
            vopt(888753760, "まお", "ノーマル"),
            vopt(2200001, "コハク", "ノーマル"),
        ];
        assert_eq!(
            pick_speaker_pair_for_archetype(&only_two_female, KeroVoiceArchetype::Boyish),
            (Some(888753760), Some(2200001), true)
        );
        assert_eq!(
            pick_speaker_pair_for_archetype(&only_two_female, KeroVoiceArchetype::FemalePartner),
            (Some(888753760), Some(2200001), false)
        );

        let mao_rasetsun_kohaku = vec![
            vopt(888753760, "まお", "ノーマル"),
            vopt(1500001, "らせつん", "標準"),
            vopt(2200001, "コハク", "ノーマル"),
        ];
        assert_eq!(
            pick_speaker_pair_for_archetype(&mao_rasetsun_kohaku, KeroVoiceArchetype::Boyish),
            (Some(888753760), Some(1500001), false)
        );
        assert_eq!(
            pick_speaker_pair_for_archetype(&mao_rasetsun_kohaku, KeroVoiceArchetype::FemalePartner),
            (Some(888753760), Some(2200001), false)
        );

        let voicevox_voices = vec![
            vopt(2, "四国めたん", "ノーマル"),
            vopt(3, "ずんだもん", "ノーマル"),
            vopt(69, "満別花丸", "ノーマル"),
            vopt(8, "春日部つむぎ", "ノーマル"),
            vopt(11, "玄野武宏", "ノーマル"),
        ];
        assert_eq!(
            pick_speaker_pair_for_archetype(&voicevox_voices, KeroVoiceArchetype::Boyish),
            (Some(2), Some(69), false)
        );
        assert_eq!(
            pick_speaker_pair_for_archetype(&voicevox_voices, KeroVoiceArchetype::YouthMale),
            (Some(2), Some(11), false)
        );
        assert_eq!(
            pick_speaker_pair_for_archetype(&voicevox_voices, KeroVoiceArchetype::FemalePartner),
            (Some(2), Some(8), false)
        );
    }

    #[test]
    fn aivm_uuid_validation_rejects_malformed_or_traversal_inputs() {
        assert!(is_valid_aivm_uuid("9f36ec0d-8dac-42dc-aff4-149c5f99faad"));
        assert!(!is_valid_aivm_uuid("not-a-uuid"));
        assert!(!is_valid_aivm_uuid("9f36ec0d-8dac-42dc-aff4-149c5f99faa"));
        assert!(!is_valid_aivm_uuid("../etc/passwd"));
    }

    #[test]
    fn recommend_local_ollama_candidates_scale_with_ram_and_allow_safe_models() {
        for ram in [8.0_f64, 16.0, 48.0, 0.0] {
            let cands = recommend_local_ollama_candidates(ram);
            assert!(!cands.is_empty(), "RAM={ram} must yield candidates");
            for m in cands {
                assert!(assert_allowed_model(m).is_ok(), "restricted model {m}");
            }
        }
        assert_eq!(recommend_local_ollama_candidates(8.0)[0], "gemma4:e2b");
        assert_eq!(recommend_local_ollama_candidates(16.0)[0], "gemma4:e4b");
        assert_eq!(recommend_local_ollama_candidates(48.0)[0], "gemma4:e4b");
        assert_eq!(recommend_local_ollama_candidates(0.0)[0], "llama3.2:3b");
    }

    #[test]
    fn parse_ollama_pull_progress_handles_bytes_and_errors() {
        let p = parse_ollama_pull_progress_line(
            r#"{"status":"downloading digest","completed":1073741824,"total":2147483648}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.1, Some(50));
        assert!(p.2.contains("50%"));

        let err = parse_ollama_pull_progress_line(r#"{"error":"pull model manifest: file does not exist"}"#);
        assert!(err.is_err());
    }
}
