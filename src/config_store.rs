use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const DEFAULT_PERSONA: &str = concat!(
    "あなたはデスクトップマスコット「伺か（MATERIA）」の『さくら（\\0）』と相棒『うにゅう（\\1）』の2人組AIエージェントです。\n",
    "・さくら（\\0）: 少しクールで落ち着いた口調だが面倒見が良い女の子。表情番号は 0=素, 1=困り・照れ, 4=ジト目・呆れ, ",
    "5=微笑み, 6=目閉じ, 7=怒り, 8=明るい笑顔, 9=照れ怒り を \\s[番号] タグのみで指定する。\n",
    "・うにゅう（\\1）: 関西弁（〜やねん、せやな、なんでやねん）で話す、薄紫青色（ペリウィンクルブルー）のもちもちした謎の生き物。鼻先に小さな丸サングラスを老眼鏡のように低く掛けており、フレームの上から両目が見えている。表情番号は 10=素, 11=驚き, 12=得意げ を \\s[番号] タグのみで指定する。\n",
    "・応答は必ず Sakura Script 形式（例: \\0\\s[5]こんにちは。\\1\\s[10]今日も元気やな！\\e）で出力し、吹き出しに収まるよう簡潔（2〜5文程度）にまとめること。(照れ) 等の感情名・ト書き括弧は本文に一切書かないこと。\n",
    "・PC状態の調査、ファイル確認、Web検索、長期記憶の記録が必要な場合は自律的にツールを呼び出して事実に基づき回答すること。"
);

/// Returns `true` when `raw` matches the built-in default persona (including older revisions).
pub fn is_stock_default_persona(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.is_empty()
        || trimmed == DEFAULT_PERSONA.trim()
        || (trimmed.starts_with("あなたはデスクトップマスコット「伺か（MATERIA）」の『さくら")
            && trimmed.contains("2人組AIエージェントです。"))
}

pub fn user_home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            if !p.trim().is_empty() {
                return PathBuf::from(p);
            }
        }
    }
    if let Ok(p) = std::env::var("HOME") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(".")
}

pub fn default_user_data_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("NANIKAAI_HOME") {
        let trimmed = override_dir.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    user_home_dir().join(".nanikaai")
}

pub fn legacy_os_nanikaai_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        return user_home_dir()
            .join("Library")
            .join("Application Support")
            .join("NanikaAi");
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home_dir().join("AppData").join("Roaming"));
        return base.join("NanikaAi");
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home_dir().join(".config"));
        xdg.join("nanikaai")
    }
}

pub fn legacy_user_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        return user_home_dir()
            .join("Library")
            .join("Application Support")
            .join("AIUkagaka");
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home_dir().join("AppData").join("Roaming"));
        return base.join("AIUkagaka");
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home_dir().join(".config"));
        xdg.join("ai-ukagaka")
    }
}

pub fn legacy_user_data_dirs() -> Vec<PathBuf> {
    let primary = default_user_data_dir();
    let mut dirs = Vec::new();
    for cand in [legacy_os_nanikaai_dir(), legacy_user_data_dir()] {
        if cand != primary && !dirs.contains(&cand) {
            dirs.push(cand);
        }
    }
    dirs
}

/// Lightweight read-only probe used before GUI initialization on Linux to read `linux_prefer_x11`.
pub fn read_linux_prefer_x11_hint() -> bool {
    let primary = default_user_data_dir().join("config.json");
    let target = if primary.is_file() {
        Some(primary)
    } else {
        legacy_user_data_dirs()
            .into_iter()
            .map(|d| d.join("config.json"))
            .find(|p| p.is_file())
    };
    let Some(path) = target else {
        return true;
    };
    let Ok(txt) = fs::read_to_string(path) else {
        return true;
    };
    serde_json::from_str::<serde_json::Value>(&txt)
        .ok()
        .and_then(|v| v.get("linux_prefer_x11").and_then(|b| b.as_bool()))
        .unwrap_or(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryItem {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_provider() -> String {
    "ollama".to_string()
}
fn default_endpoint() -> String {
    "http://127.0.0.1:11434".to_string()
}
fn default_remote_endpoint() -> String {
    "http://127.0.0.1:21435".to_string()
}
fn default_model() -> String {
    "auto".to_string()
}
fn default_true() -> bool {
    true
}
pub const MIN_AUTO_TALK_INTERVAL_SEC: u64 = 30;
pub const MAX_AUTO_TALK_INTERVAL_SEC: u64 = 1800;
pub const DEFAULT_INTEREST_NEWS_WEIGHT: f32 = 0.50;
pub const MAX_INTEREST_NEWS_WEIGHT: f32 = 0.80;
pub const MAX_INTEREST_TOPICS: usize = 16;

fn default_autotalk() -> u64 {
    180
}
fn default_interest_news_weight() -> f32 {
    DEFAULT_INTEREST_NEWS_WEIGHT
}
fn default_http_port() -> u16 {
    9821
}
fn default_sstp_port() -> u16 {
    9801
}
fn default_shell_id() -> String {
    "neo_default".to_string()
}
fn default_balloon_id() -> String {
    "clean_default".to_string()
}
fn default_persona() -> String {
    DEFAULT_PERSONA.to_string()
}

fn default_voicevox() -> String {
    "http://127.0.0.1:50021".to_string()
}

fn default_tts_engine() -> String {
    "auto".to_string()
}

fn default_ui_language() -> String {
    "auto".to_string()
}

fn default_power_save_mode() -> String {
    "auto".to_string()
}

pub fn clamp_pomodoro_preset_idx(v: usize) -> usize {
    v.min(crate::accessory_utils::POMODORO_PRESETS.len().saturating_sub(1))
}

fn default_one_f32() -> f32 {
    1.0
}

fn round_pct(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

pub fn clamp_auto_talk_interval_sec(v: u64) -> u64 {
    if v == 0 {
        0
    } else {
        v.clamp(MIN_AUTO_TALK_INTERVAL_SEC, MAX_AUTO_TALK_INTERVAL_SEC)
    }
}

pub fn clamp_interest_news_weight(v: f32) -> f32 {
    if v.is_finite() {
        let clamped = v.clamp(0.0, MAX_INTEREST_NEWS_WEIGHT);
        (clamped * 20.0).round() / 20.0
    } else {
        DEFAULT_INTEREST_NEWS_WEIGHT
    }
}

/// Sanitize a conversational user interest topic before storing or using it in news queries.
pub fn sanitize_interest_topic(raw: &str) -> Option<String> {
    if raw.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                '\\' | '/' | '<' | '>' | '@' | ':' | ';' | '|' | '`' | '"' | '\''
            )
    }) {
        return None;
    }
    let trim_chars: &[char] = &[
        ' ', '\t', '\r', '\n', ',', '、', '。', '.', '!', '?', '！', '？', '(', ')', '[', ']',
        '「', '」', '『', '』', '“', '”',
    ];
    let trimmed = raw
        .trim_matches(trim_chars)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let char_count = trimmed.chars().count();
    if char_count == 0 || char_count > 32 {
        return None;
    }
    if char_count == 1 {
        let ch = trimmed.chars().next()?;
        if !('\u{4e00}'..='\u{9fff}').contains(&ch) {
            return None;
        }
    }
    if trimmed.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    const STOPWORDS: &[&str] = &[
        "너", "나", "우리", "당신", "이거", "그거", "저거", "뭐", "무엇", "아무것", "관심",
        "관심사", "취미", "뉴스", "소식", "잡담", "これ", "それ", "あれ", "何", "なに", "私",
        "僕", "俺", "あなた", "君", "興味", "関心", "趣味", "ニュース", "話題", "雑談", "今日",
        "天気", "you", "me", "it", "this", "that", "what", "anything", "nothing", "news",
        "interest", "interests", "hobby", "hobbies",
    ];
    if STOPWORDS.iter().any(|&sw| trimmed == sw || lower == sw) {
        return None;
    }
    Some(trimmed)
}

fn sanitize_and_dedup_interests(raw: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();
    for item in raw {
        if let Some(topic) = sanitize_interest_topic(item) {
            if !cleaned.iter().any(|t| t.eq_ignore_ascii_case(&topic)) {
                cleaned.push(topic);
            }
        }
    }
    if cleaned.len() > MAX_INTEREST_TOPICS {
        let drop_n = cleaned.len() - MAX_INTEREST_TOPICS;
        cleaned.drain(0..drop_n);
    }
    cleaned
}

pub fn clamp_skin_opacity(v: f32) -> f32 {
    if v.is_finite() {
        round_pct(v.clamp(0.15, 1.0))
    } else {
        1.0
    }
}

pub fn clamp_balloon_opacity(v: f32) -> f32 {
    if v.is_finite() {
        round_pct(v.clamp(0.25, 1.0))
    } else {
        1.0
    }
}

pub fn clamp_character_scale(v: f32) -> f32 {
    if v.is_finite() {
        round_pct(v.clamp(0.50, 1.50))
    } else {
        1.0
    }
}

pub fn normalize_tts_engine(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "aivisspeech" | "aivis" => "aivisspeech".to_string(),
        "voicevox" | "vv" => "voicevox".to_string(),
        "coeiroink" => "coeiroink".to_string(),
        "os" | "native" => "os".to_string(),
        _ => "auto".to_string(),
    }
}

pub fn normalize_kero_archetype_str(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "boy" | "boyish" => Some("boy".to_string()),
        "girl" | "female" | "female_partner" => Some("girl".to_string()),
        "male" | "youth_male" | "man" => Some("male".to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReminderItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub fire_at_unix: i64,
    #[serde(default)]
    pub shell_id: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellGhostProfile {
    #[serde(default)]
    pub system_persona: String,
    #[serde(default)]
    pub memories: Vec<MemoryItem>,
    #[serde(default)]
    pub interests: Vec<String>,
    #[serde(default)]
    pub active_binds: Option<Vec<i32>>,
    #[serde(default)]
    pub sakura_speaker_id: Option<i64>,
    #[serde(default)]
    pub kero_speaker_id: Option<i64>,
    #[serde(default)]
    pub kero_voice_archetype: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_endpoint")]
    pub endpoint_url: String,
    #[serde(
        rename = "linux_endpoint_url",
        alias = "remote_endpoint_url",
        default = "default_remote_endpoint"
    )]
    pub remote_endpoint_url: String,
    #[serde(default)]
    pub ssh_tunnel_target: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_true")]
    pub enable_tools: bool,
    #[serde(default = "default_true")]
    pub enable_clipboard_tool: bool,
    #[serde(default = "default_autotalk")]
    pub auto_talk_interval_sec: u64,
    #[serde(default = "default_interest_news_weight")]
    pub interest_news_weight: f32,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_sstp_port")]
    pub sstp_tcp_port: u16,
    #[serde(default = "default_shell_id")]
    pub active_shell_id: String,
    #[serde(default = "default_balloon_id")]
    pub active_balloon_id: String,
    #[serde(default = "default_true")]
    pub show_kero: bool,
    #[serde(default = "default_true")]
    pub always_on_top: bool,
    #[serde(default = "default_true")]
    pub live2d_motion: bool,
    #[serde(default = "default_true")]
    pub hourly_chime: bool,
    #[serde(default)]
    pub voice_tts: bool,
    #[serde(default = "default_tts_engine")]
    pub tts_engine: String,
    #[serde(default = "default_voicevox")]
    pub voicevox_url: String,
    #[serde(default = "default_one_f32")]
    pub skin_opacity: f32,
    #[serde(default = "default_one_f32")]
    pub balloon_opacity: f32,
    #[serde(default = "default_one_f32")]
    pub character_scale: f32,
    #[serde(default = "default_true")]
    pub opaque_on_hover: bool,
    #[serde(default = "default_true")]
    pub eye_tracking: bool,
    #[serde(default)]
    pub lock_position: bool,
    #[serde(default = "default_true")]
    pub snap_to_bottom: bool,
    #[serde(default)]
    pub click_through_mode: bool,
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    #[serde(default)]
    pub quiet_mode: bool,
    #[serde(default = "default_true")]
    pub auto_hide_on_fullscreen: bool,
    #[serde(default)]
    pub auto_start_at_login: bool,
    #[serde(default = "default_power_save_mode")]
    pub power_save_mode: String,
    #[serde(default = "default_true")]
    pub low_battery_alert: bool,
    #[serde(default)]
    pub pomodoro_preset_idx: usize,
    #[serde(default = "default_true")]
    pub macos_accept_first_mouse: bool,
    #[serde(default)]
    pub macos_window_shadow: bool,
    #[serde(default)]
    pub macos_show_in_dock: bool,
    #[serde(default = "default_true")]
    pub windows_skip_taskbar: bool,
    #[serde(default)]
    pub windows_undecorated_shadow: bool,
    #[serde(default = "default_true")]
    pub linux_prefer_x11: bool,
    #[serde(default = "default_true")]
    pub linux_respect_workarea: bool,
    #[serde(default = "default_persona")]
    pub system_persona: String,
    #[serde(default)]
    pub memories: Vec<MemoryItem>,
    #[serde(default)]
    pub ghost_profiles: BTreeMap<String, ShellGhostProfile>,
    #[serde(default)]
    pub reminders: Vec<ReminderItem>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            endpoint_url: default_endpoint(),
            remote_endpoint_url: default_remote_endpoint(),
            ssh_tunnel_target: String::new(),
            api_key: String::new(),
            model: default_model(),
            enable_tools: true,
            enable_clipboard_tool: true,
            auto_talk_interval_sec: default_autotalk(),
            interest_news_weight: default_interest_news_weight(),
            http_port: default_http_port(),
            sstp_tcp_port: default_sstp_port(),
            active_shell_id: default_shell_id(),
            active_balloon_id: default_balloon_id(),
            show_kero: true,
            always_on_top: true,
            live2d_motion: true,
            hourly_chime: true,
            voice_tts: false,
            tts_engine: default_tts_engine(),
            voicevox_url: default_voicevox(),
            skin_opacity: 1.0,
            balloon_opacity: 1.0,
            character_scale: 1.0,
            opaque_on_hover: true,
            eye_tracking: true,
            lock_position: false,
            snap_to_bottom: true,
            click_through_mode: false,
            ui_language: default_ui_language(),
            quiet_mode: false,
            auto_hide_on_fullscreen: true,
            auto_start_at_login: false,
            power_save_mode: default_power_save_mode(),
            low_battery_alert: true,
            pomodoro_preset_idx: 0,
            macos_accept_first_mouse: true,
            macos_window_shadow: false,
            macos_show_in_dock: false,
            windows_skip_taskbar: true,
            windows_undecorated_shadow: false,
            linux_prefer_x11: true,
            linux_respect_workarea: true,
            system_persona: default_persona(),
            memories: Vec::new(),
            ghost_profiles: BTreeMap::new(),
            reminders: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ConfigPatch {
    pub provider: Option<String>,
    pub endpoint_url: Option<String>,
    pub remote_endpoint_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub enable_tools: Option<bool>,
    pub enable_clipboard_tool: Option<bool>,
    pub auto_talk_interval_sec: Option<u64>,
    pub interest_news_weight: Option<f32>,
    pub active_shell_id: Option<String>,
    pub active_balloon_id: Option<String>,
    pub show_kero: Option<bool>,
    pub always_on_top: Option<bool>,
    pub live2d_motion: Option<bool>,
    pub hourly_chime: Option<bool>,
    pub voice_tts: Option<bool>,
    pub tts_engine: Option<String>,
    pub voicevox_url: Option<String>,
    pub skin_opacity: Option<f32>,
    pub balloon_opacity: Option<f32>,
    pub character_scale: Option<f32>,
    pub opaque_on_hover: Option<bool>,
    pub eye_tracking: Option<bool>,
    pub lock_position: Option<bool>,
    pub snap_to_bottom: Option<bool>,
    pub click_through_mode: Option<bool>,
    pub ui_language: Option<String>,
    pub quiet_mode: Option<bool>,
    pub auto_hide_on_fullscreen: Option<bool>,
    pub auto_start_at_login: Option<bool>,
    pub power_save_mode: Option<String>,
    pub low_battery_alert: Option<bool>,
    pub pomodoro_preset_idx: Option<usize>,
    pub macos_accept_first_mouse: Option<bool>,
    pub macos_window_shadow: Option<bool>,
    pub macos_show_in_dock: Option<bool>,
    pub windows_skip_taskbar: Option<bool>,
    pub windows_undecorated_shadow: Option<bool>,
    pub linux_prefer_x11: Option<bool>,
    pub linux_respect_workarea: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub config_dir: PathBuf,
    pub file_path: PathBuf,
    inner: Arc<Mutex<AgentConfig>>,
}

impl ConfigStore {
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        match config_dir {
            Some(dir) => Self::new_with_migration(dir, &[]),
            None => Self::new_with_migration(default_user_data_dir(), &legacy_user_data_dirs()),
        }
    }

    pub fn new_with_migration(dir: PathBuf, legacy_dirs: &[PathBuf]) -> Self {
        let _ = fs::create_dir_all(&dir);
        let file_path = dir.join("config.json");
        let cfg = Self::load_or_migrate(&file_path, legacy_dirs);
        Self {
            config_dir: dir,
            file_path,
            inner: Arc::new(Mutex::new(cfg)),
        }
    }

    fn normalize_loaded_config(cfg: &mut AgentConfig) -> bool {
        let mut changed = false;
        if is_stock_default_persona(&cfg.system_persona) && cfg.system_persona != DEFAULT_PERSONA {
            cfg.system_persona = DEFAULT_PERSONA.to_string();
            changed = true;
        }
        if !cfg.memories.is_empty() {
            let shell_id = if cfg.active_shell_id.trim().is_empty() {
                default_shell_id()
            } else {
                cfg.active_shell_id.clone()
            };
            let profile = cfg.ghost_profiles.entry(shell_id).or_default();
            if profile.memories.is_empty() {
                profile.memories = std::mem::take(&mut cfg.memories);
            } else {
                cfg.memories.clear();
            }
            changed = true;
        }
        let clamped_skin = clamp_skin_opacity(cfg.skin_opacity);
        if (clamped_skin - cfg.skin_opacity).abs() > 1e-4 {
            cfg.skin_opacity = clamped_skin;
            changed = true;
        }
        let clamped_bal = clamp_balloon_opacity(cfg.balloon_opacity);
        if (clamped_bal - cfg.balloon_opacity).abs() > 1e-4 {
            cfg.balloon_opacity = clamped_bal;
            changed = true;
        }
        let clamped_scale = clamp_character_scale(cfg.character_scale);
        if (clamped_scale - cfg.character_scale).abs() > 1e-4 {
            cfg.character_scale = clamped_scale;
            changed = true;
        }
        let clamped_interval = clamp_auto_talk_interval_sec(cfg.auto_talk_interval_sec);
        if clamped_interval != cfg.auto_talk_interval_sec {
            cfg.auto_talk_interval_sec = clamped_interval;
            changed = true;
        }
        let clamped_weight = clamp_interest_news_weight(cfg.interest_news_weight);
        if !cfg.interest_news_weight.is_finite()
            || (clamped_weight - cfg.interest_news_weight).abs() > 1e-4
        {
            cfg.interest_news_weight = clamped_weight;
            changed = true;
        }
        let norm_lang = crate::i18n::normalize_ui_language(&cfg.ui_language);
        if norm_lang != cfg.ui_language {
            cfg.ui_language = norm_lang;
            changed = true;
        }
        let norm_ps = crate::accessory_utils::normalize_power_save_mode(&cfg.power_save_mode);
        if norm_ps != cfg.power_save_mode {
            cfg.power_save_mode = norm_ps;
            changed = true;
        }
        let clamped_pomo = clamp_pomodoro_preset_idx(cfg.pomodoro_preset_idx);
        if clamped_pomo != cfg.pomodoro_preset_idx {
            cfg.pomodoro_preset_idx = clamped_pomo;
            changed = true;
        }
        for profile in cfg.ghost_profiles.values_mut() {
            let cleaned = sanitize_and_dedup_interests(&profile.interests);
            if cleaned != profile.interests {
                profile.interests = cleaned;
                changed = true;
            }
        }
        changed
    }

    fn load_or_migrate(file_path: &Path, legacy_dirs: &[PathBuf]) -> AgentConfig {
        if !file_path.is_file() {
            for leg_dir in legacy_dirs {
                let leg_file = leg_dir.join("config.json");
                if leg_file.is_file() {
                    if let Ok(txt) = fs::read_to_string(&leg_file) {
                        let mut cfg = serde_json::from_str::<AgentConfig>(&txt).unwrap_or_default();
                        Self::normalize_loaded_config(&mut cfg);
                        Self::save_to_path(file_path, &cfg);
                        return cfg;
                    }
                }
            }
            let defaults = AgentConfig::default();
            Self::save_to_path(file_path, &defaults);
            return defaults;
        }
        match fs::read_to_string(file_path) {
            Ok(txt) => {
                let mut cfg = serde_json::from_str::<AgentConfig>(&txt).unwrap_or_default();
                if Self::normalize_loaded_config(&mut cfg) {
                    Self::save_to_path(file_path, &cfg);
                }
                cfg
            }
            Err(_) => AgentConfig::default(),
        }
    }

    fn save_to_path(file_path: &Path, cfg: &AgentConfig) {
        if let Some(parent) = file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(cfg) {
            let _ = fs::write(file_path, json);
        }
    }

    pub fn get(&self) -> AgentConfig {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn update(&self, patch: ConfigPatch) -> AgentConfig {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        if let Some(v) = patch.provider {
            guard.provider = v;
        }
        if let Some(v) = patch.endpoint_url {
            guard.endpoint_url = v;
        }
        if let Some(v) = patch.remote_endpoint_url {
            guard.remote_endpoint_url = v;
        }
        if let Some(v) = patch.api_key {
            guard.api_key = v;
        }
        if let Some(v) = patch.model {
            guard.model = v;
        }
        if let Some(v) = patch.enable_tools {
            guard.enable_tools = v;
        }
        if let Some(v) = patch.enable_clipboard_tool {
            guard.enable_clipboard_tool = v;
        }
        if let Some(v) = patch.auto_talk_interval_sec {
            guard.auto_talk_interval_sec = clamp_auto_talk_interval_sec(v);
        }
        if let Some(v) = patch.interest_news_weight {
            guard.interest_news_weight = clamp_interest_news_weight(v);
        }
        if let Some(v) = patch.active_shell_id {
            guard.active_shell_id = v;
        }
        if let Some(v) = patch.active_balloon_id {
            guard.active_balloon_id = v;
        }
        if let Some(v) = patch.show_kero {
            guard.show_kero = v;
        }
        if let Some(v) = patch.always_on_top {
            guard.always_on_top = v;
        }
        if let Some(v) = patch.live2d_motion {
            guard.live2d_motion = v;
        }
        if let Some(v) = patch.hourly_chime {
            guard.hourly_chime = v;
        }
        if let Some(v) = patch.voice_tts {
            guard.voice_tts = v;
        }
        if let Some(v) = patch.tts_engine {
            guard.tts_engine = normalize_tts_engine(&v);
        }
        if let Some(v) = patch.voicevox_url {
            guard.voicevox_url = v;
        }
        if let Some(v) = patch.skin_opacity {
            guard.skin_opacity = clamp_skin_opacity(v);
        }
        if let Some(v) = patch.balloon_opacity {
            guard.balloon_opacity = clamp_balloon_opacity(v);
        }
        if let Some(v) = patch.character_scale {
            guard.character_scale = clamp_character_scale(v);
        }
        if let Some(v) = patch.opaque_on_hover {
            guard.opaque_on_hover = v;
        }
        if let Some(v) = patch.eye_tracking {
            guard.eye_tracking = v;
        }
        if let Some(v) = patch.lock_position {
            guard.lock_position = v;
        }
        if let Some(v) = patch.snap_to_bottom {
            guard.snap_to_bottom = v;
        }
        if let Some(v) = patch.click_through_mode {
            guard.click_through_mode = v;
        }
        if let Some(v) = patch.ui_language {
            guard.ui_language = crate::i18n::normalize_ui_language(&v);
        }
        if let Some(v) = patch.quiet_mode {
            guard.quiet_mode = v;
        }
        if let Some(v) = patch.auto_hide_on_fullscreen {
            guard.auto_hide_on_fullscreen = v;
        }
        if let Some(v) = patch.auto_start_at_login {
            guard.auto_start_at_login = v;
        }
        if let Some(v) = patch.power_save_mode {
            guard.power_save_mode = crate::accessory_utils::normalize_power_save_mode(&v);
        }
        if let Some(v) = patch.low_battery_alert {
            guard.low_battery_alert = v;
        }
        if let Some(v) = patch.pomodoro_preset_idx {
            guard.pomodoro_preset_idx = clamp_pomodoro_preset_idx(v);
        }
        if let Some(v) = patch.macos_accept_first_mouse {
            guard.macos_accept_first_mouse = v;
        }
        if let Some(v) = patch.macos_window_shadow {
            guard.macos_window_shadow = v;
        }
        if let Some(v) = patch.macos_show_in_dock {
            guard.macos_show_in_dock = v;
        }
        if let Some(v) = patch.windows_skip_taskbar {
            guard.windows_skip_taskbar = v;
        }
        if let Some(v) = patch.windows_undecorated_shadow {
            guard.windows_undecorated_shadow = v;
        }
        if let Some(v) = patch.linux_prefer_x11 {
            guard.linux_prefer_x11 = v;
        }
        if let Some(v) = patch.linux_respect_workarea {
            guard.linux_respect_workarea = v;
        }
        Self::save_to_path(&self.file_path, &guard);
        guard.clone()
    }

    pub fn reset_convenience_and_appearance_defaults(&self) -> AgentConfig {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let defs = AgentConfig::default();
        guard.show_kero = defs.show_kero;
        guard.always_on_top = defs.always_on_top;
        guard.live2d_motion = defs.live2d_motion;
        guard.hourly_chime = defs.hourly_chime;
        guard.skin_opacity = defs.skin_opacity;
        guard.balloon_opacity = defs.balloon_opacity;
        guard.character_scale = defs.character_scale;
        guard.auto_talk_interval_sec = defs.auto_talk_interval_sec;
        guard.interest_news_weight = defs.interest_news_weight;
        guard.opaque_on_hover = defs.opaque_on_hover;
        guard.eye_tracking = defs.eye_tracking;
        guard.lock_position = defs.lock_position;
        guard.snap_to_bottom = defs.snap_to_bottom;
        guard.click_through_mode = defs.click_through_mode;
        guard.ui_language = defs.ui_language;
        guard.quiet_mode = defs.quiet_mode;
        guard.auto_hide_on_fullscreen = defs.auto_hide_on_fullscreen;
        guard.power_save_mode = defs.power_save_mode;
        guard.low_battery_alert = defs.low_battery_alert;
        guard.pomodoro_preset_idx = defs.pomodoro_preset_idx;
        guard.macos_accept_first_mouse = defs.macos_accept_first_mouse;
        guard.macos_window_shadow = defs.macos_window_shadow;
        guard.macos_show_in_dock = defs.macos_show_in_dock;
        guard.windows_skip_taskbar = defs.windows_skip_taskbar;
        guard.windows_undecorated_shadow = defs.windows_undecorated_shadow;
        guard.linux_prefer_x11 = defs.linux_prefer_x11;
        guard.linux_respect_workarea = defs.linux_respect_workarea;
        Self::save_to_path(&self.file_path, &guard);
        guard.clone()
    }

    pub fn active_memories(&self) -> Vec<MemoryItem> {
        let guard = self.inner.lock().expect("config mutex poisoned");
        Self::memories_slice(&guard, &guard.active_shell_id).to_vec()
    }

    pub fn memories_for_shell(&self, shell_id: &str) -> Vec<MemoryItem> {
        let guard = self.inner.lock().expect("config mutex poisoned");
        Self::memories_slice(&guard, shell_id).to_vec()
    }

    fn memories_slice<'a>(cfg: &'a AgentConfig, shell_id: &str) -> &'a [MemoryItem] {
        cfg.ghost_profiles
            .get(shell_id)
            .map(|p| p.memories.as_slice())
            .unwrap_or(&[])
    }

    fn resolve_shell_key(cfg: &AgentConfig, shell_id: Option<&str>) -> String {
        let raw = shell_id.unwrap_or(&cfg.active_shell_id).trim();
        if raw.is_empty() {
            default_shell_id()
        } else {
            raw.to_string()
        }
    }

    pub fn active_interests(&self) -> Vec<String> {
        let guard = self.inner.lock().expect("config mutex poisoned");
        let shell_id = Self::resolve_shell_key(&guard, None);
        guard
            .ghost_profiles
            .get(&shell_id)
            .map(|p| p.interests.clone())
            .unwrap_or_default()
    }

    pub fn interests_for_shell(&self, shell_id: &str) -> Vec<String> {
        let guard = self.inner.lock().expect("config mutex poisoned");
        let key = Self::resolve_shell_key(&guard, Some(shell_id));
        guard
            .ghost_profiles
            .get(&key)
            .map(|p| p.interests.clone())
            .unwrap_or_default()
    }

    pub fn add_interests(&self, topics: &[String]) -> Vec<String> {
        let active = self.get().active_shell_id;
        self.add_interests_for_shell(&active, topics)
    }

    pub fn add_interests_for_shell(&self, shell_id: &str, topics: &[String]) -> Vec<String> {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let key = Self::resolve_shell_key(&guard, Some(shell_id));
        let mut changed = false;
        let snapshot = {
            let profile = guard.ghost_profiles.entry(key).or_default();
            for raw in topics {
                if let Some(clean) = sanitize_interest_topic(raw) {
                    profile
                        .interests
                        .retain(|existing| !existing.eq_ignore_ascii_case(&clean));
                    profile.interests.push(clean);
                    changed = true;
                }
            }
            if profile.interests.len() > MAX_INTEREST_TOPICS {
                let drop_n = profile.interests.len() - MAX_INTEREST_TOPICS;
                profile.interests.drain(0..drop_n);
                changed = true;
            }
            profile.interests.clone()
        };
        if changed {
            Self::save_to_path(&self.file_path, &guard);
        }
        snapshot
    }

    pub fn set_interests(&self, topics: &[String]) -> Vec<String> {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let key = Self::resolve_shell_key(&guard, None);
        let cleaned = sanitize_and_dedup_interests(topics);
        let profile = guard.ghost_profiles.entry(key).or_default();
        profile.interests = cleaned.clone();
        Self::save_to_path(&self.file_path, &guard);
        cleaned
    }

    pub fn remove_interest(&self, topic: &str) -> Vec<String> {
        let target = topic.trim();
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let key = Self::resolve_shell_key(&guard, None);
        let mut changed = false;
        let out = if let Some(profile) = guard.ghost_profiles.get_mut(&key) {
            let before = profile.interests.len();
            profile
                .interests
                .retain(|existing| !existing.eq_ignore_ascii_case(target));
            changed = profile.interests.len() != before;
            profile.interests.clone()
        } else {
            Vec::new()
        };
        if changed {
            Self::save_to_path(&self.file_path, &guard);
        }
        out
    }

    pub fn clear_interests(&self) -> Vec<String> {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let key = Self::resolve_shell_key(&guard, None);
        if let Some(profile) = guard.ghost_profiles.get_mut(&key) {
            if !profile.interests.is_empty() {
                profile.interests.clear();
                Self::save_to_path(&self.file_path, &guard);
            }
        }
        Vec::new()
    }

    pub fn persona_override_for_shell(&self, shell_id: &str) -> Option<String> {
        let guard = self.inner.lock().ok()?;
        let custom = guard
            .ghost_profiles
            .get(shell_id)
            .map(|p| p.system_persona.trim())
            .unwrap_or("");
        if custom.is_empty() {
            None
        } else {
            Some(custom.to_string())
        }
    }

    pub fn set_persona_override_for_shell(&self, shell_id: &str, persona: &str) {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let profile = guard
            .ghost_profiles
            .entry(shell_id.to_string())
            .or_default();
        profile.system_persona = persona.trim().to_string();
        Self::save_to_path(&self.file_path, &guard);
    }

    pub fn get_active_binds(&self, shell_id: &str, default_binds: &[i32]) -> Vec<i32> {
        let guard = self.inner.lock().expect("config mutex poisoned");
        if let Some(Some(saved)) = guard
            .ghost_profiles
            .get(shell_id)
            .map(|p| &p.active_binds)
        {
            return saved.clone();
        }
        let mut out = default_binds.to_vec();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn toggle_shell_bind(
        &self,
        shell_id: &str,
        bind_id: i32,
        default_binds: &[i32],
    ) -> (Vec<i32>, bool) {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let profile = guard
            .ghost_profiles
            .entry(shell_id.to_string())
            .or_default();
        let mut current = match &profile.active_binds {
            Some(list) => list.clone(),
            None => default_binds.to_vec(),
        };
        let now_enabled = if let Some(pos) = current.iter().position(|&id| id == bind_id) {
            current.remove(pos);
            false
        } else {
            current.push(bind_id);
            current.sort_unstable();
            current.dedup();
            true
        };
        profile.active_binds = Some(current.clone());
        Self::save_to_path(&self.file_path, &guard);
        (current, now_enabled)
    }

    pub fn get_shell_voice_speakers(&self, shell_id: &str) -> (Option<i64>, Option<i64>) {
        let guard = self.inner.lock().expect("config mutex poisoned");
        guard
            .ghost_profiles
            .get(shell_id)
            .map(|p| (p.sakura_speaker_id, p.kero_speaker_id))
            .unwrap_or((None, None))
    }

    pub fn get_shell_voice_profile(
        &self,
        shell_id: &str,
    ) -> (Option<i64>, Option<i64>, Option<String>) {
        let guard = self.inner.lock().expect("config mutex poisoned");
        guard
            .ghost_profiles
            .get(shell_id)
            .map(|p| {
                (
                    p.sakura_speaker_id,
                    p.kero_speaker_id,
                    p.kero_voice_archetype.clone(),
                )
            })
            .unwrap_or((None, None, None))
    }

    pub fn set_shell_kero_archetype(&self, shell_id: &str, archetype: Option<String>) {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let profile = guard
            .ghost_profiles
            .entry(shell_id.to_string())
            .or_default();
        profile.kero_voice_archetype = archetype
            .as_deref()
            .and_then(normalize_kero_archetype_str);
        Self::save_to_path(&self.file_path, &guard);
    }

    pub fn set_shell_voice_speaker(
        &self,
        shell_id: &str,
        scope: u8,
        speaker_id: Option<i64>,
    ) {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let profile = guard
            .ghost_profiles
            .entry(shell_id.to_string())
            .or_default();
        if scope == 1 {
            profile.kero_speaker_id = speaker_id;
        } else {
            profile.sakura_speaker_id = speaker_id;
        }
        Self::save_to_path(&self.file_path, &guard);
    }

    pub fn add_reminder(
        &self,
        message: &str,
        fire_at_unix: i64,
        shell_id: Option<&str>,
    ) -> ReminderItem {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let target_shell = shell_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| guard.active_shell_id.clone());
        let id = format!(
            "rem_{}_{}",
            Utc::now().timestamp_millis(),
            guard.reminders.len() + 1
        );
        let item = ReminderItem {
            id,
            message: message.trim().chars().take(240).collect(),
            fire_at_unix,
            shell_id: target_shell,
            done: false,
        };
        guard.reminders.push(item.clone());
        // Retain at most 64 recent reminders to keep config.json lean.
        if guard.reminders.len() > 64 {
            let drain_count = guard.reminders.len() - 64;
            guard.reminders.drain(0..drain_count);
        }
        Self::save_to_path(&self.file_path, &guard);
        item
    }

    pub fn list_reminders(&self, include_done: bool) -> Vec<ReminderItem> {
        let guard = self.inner.lock().expect("config mutex poisoned");
        guard
            .reminders
            .iter()
            .filter(|r| include_done || !r.done)
            .cloned()
            .collect()
    }

    pub fn take_due_reminders(&self, now_unix: i64) -> Vec<ReminderItem> {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let mut due = Vec::new();
        let mut changed = false;
        for rem in &mut guard.reminders {
            if !rem.done && rem.fire_at_unix <= now_unix {
                rem.done = true;
                due.push(rem.clone());
                changed = true;
            }
        }
        if changed {
            Self::save_to_path(&self.file_path, &guard);
        }
        due
    }

    pub fn upsert_memory(&self, title: &str, content: &str) -> MemoryItem {
        let active = self.get().active_shell_id;
        self.upsert_memory_for_shell(&active, title, content)
    }

    pub fn upsert_memory_for_shell(
        &self,
        shell_id: &str,
        title: &str,
        content: &str,
    ) -> MemoryItem {
        let clean_title: String = title.trim().chars().take(120).collect();
        let clean_title = if clean_title.is_empty() {
            "note".to_string()
        } else {
            clean_title
        };
        let clean_content: String = content.trim().chars().take(4000).collect();
        let entry = MemoryItem {
            title: clean_title.clone(),
            content: clean_content,
            updated_at: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        };
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        let profile = guard
            .ghost_profiles
            .entry(shell_id.to_string())
            .or_default();
        if let Some(existing) = profile
            .memories
            .iter_mut()
            .find(|m| m.title.eq_ignore_ascii_case(&clean_title))
        {
            *existing = entry.clone();
        } else {
            profile.memories.push(entry.clone());
        }
        Self::save_to_path(&self.file_path, &guard);
        entry
    }

    pub fn search_memories(&self, keyword: &str) -> Vec<MemoryItem> {
        let active = self.get().active_shell_id;
        self.search_memories_for_shell(&active, keyword)
    }

    pub fn search_memories_for_shell(&self, shell_id: &str, keyword: &str) -> Vec<MemoryItem> {
        let q = keyword.trim().to_lowercase();
        let guard = self.inner.lock().expect("config mutex poisoned");
        let mems = Self::memories_slice(&guard, shell_id);
        if q.is_empty() {
            let start = mems.len().saturating_sub(10);
            return mems[start..].to_vec();
        }
        mems.iter()
            .filter(|m| m.title.to_lowercase().contains(&q) || m.content.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_and_memory() {
        let dir = std::env::temp_dir().join(format!("nanikaai_cfg_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = ConfigStore::new(Some(dir.clone()));
        store.update(ConfigPatch {
            provider: Some("linux-ollama".to_string()),
            model: Some("gemma4:latest".to_string()),
            ..Default::default()
        });
        let saved = store.upsert_memory("Tea", "Earl Grey hot");
        assert_eq!(saved.title, "Tea");

        let reloaded = ConfigStore::new(Some(dir.clone()));
        let cfg = reloaded.get();
        assert_eq!(cfg.provider, "linux-ollama");
        assert_eq!(cfg.remote_endpoint_url, "http://127.0.0.1:21435");
        assert_eq!(reloaded.search_memories("grey").len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_clamping_reset_and_legacy_migration() {
        assert!((clamp_skin_opacity(0.02) - 0.15).abs() < 1e-4);
        assert!((clamp_balloon_opacity(0.05) - 0.25).abs() < 1e-4);
        assert!((clamp_character_scale(2.5) - 1.50).abs() < 1e-4);

        let root = std::env::temp_dir().join(format!("nanikaai_mig_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let leg = root.join("legacy");
        let target = root.join(".nanikaai");
        fs::create_dir_all(&leg).unwrap();
        fs::write(
            leg.join("config.json"),
            r#"{"provider":"openai","model":"gpt-4o","api_key":"secret-keep","skin_opacity":0.68}"#,
        )
        .unwrap();

        let store = ConfigStore::new_with_migration(target.clone(), std::slice::from_ref(&leg));
        assert!(target.join("config.json").is_file());
        let cfg = store.get();
        assert_eq!(cfg.provider, "openai");
        assert_eq!(cfg.api_key, "secret-keep");
        assert!((cfg.skin_opacity - 0.68).abs() < 1e-3);

        store.update(ConfigPatch {
            lock_position: Some(true),
            click_through_mode: Some(true),
            character_scale: Some(1.25),
            auto_talk_interval_sec: Some(30),
            interest_news_weight: Some(0.80),
            ..Default::default()
        });
        let saved_interests = store.add_interests(&["Rust".to_string(), "AIエージェント".to_string()]);
        assert_eq!(saved_interests.len(), 2);

        let reset = store.reset_convenience_and_appearance_defaults();
        assert!(!reset.lock_position);
        assert!(!reset.click_through_mode);
        assert!((reset.skin_opacity - 1.0).abs() < 1e-4);
        assert!((reset.character_scale - 1.0).abs() < 1e-4);
        assert_eq!(reset.auto_talk_interval_sec, 180);
        assert!((reset.interest_news_weight - 0.50).abs() < 1e-4);
        assert_eq!(store.active_interests(), vec!["Rust", "AIエージェント"]);
        // Preserves AI credentials & provider
        assert_eq!(reset.provider, "openai");
        assert_eq!(reset.api_key, "secret-keep");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn autotalk_interval_weight_clamping_and_interest_storage() {
        assert_eq!(clamp_auto_talk_interval_sec(0), 0);
        assert_eq!(clamp_auto_talk_interval_sec(10), 30);
        assert_eq!(clamp_auto_talk_interval_sec(30), 30);
        assert_eq!(clamp_auto_talk_interval_sec(9999), 1800);

        assert!((clamp_interest_news_weight(-0.2) - 0.0).abs() < 1e-4);
        assert!((clamp_interest_news_weight(0.47) - 0.45).abs() < 1e-4);
        assert!((clamp_interest_news_weight(0.95) - 0.80).abs() < 1e-4);
        assert!((clamp_interest_news_weight(f32::NAN) - 0.50).abs() < 1e-4);

        assert_eq!(sanitize_interest_topic("  반도체  뉴스?! ").as_deref(), Some("반도체 뉴스"));
        assert_eq!(sanitize_interest_topic("猫").as_deref(), Some("猫"));
        assert_eq!(sanitize_interest_topic("a"), None);
        assert_eq!(sanitize_interest_topic("관심사"), None);
        assert_eq!(sanitize_interest_topic("https://example.com"), None);
        assert_eq!(sanitize_interest_topic("/etc/passwd"), None);

        let dir = std::env::temp_dir().join(format!("nanikaai_int_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = ConfigStore::new(Some(dir.clone()));
        store.add_interests(&["宇宙開発".to_string(), "Rust".to_string()]);
        store.add_interests_for_shell("other_shell", &["料理".to_string()]);

        let reloaded = ConfigStore::new(Some(dir.clone()));
        assert_eq!(reloaded.active_interests(), vec!["宇宙開発", "Rust"]);
        assert_eq!(reloaded.interests_for_shell("other_shell"), vec!["料理"]);
        reloaded.remove_interest("rust");
        assert_eq!(reloaded.active_interests(), vec!["宇宙開発"]);
        reloaded.clear_interests();
        assert!(reloaded.active_interests().is_empty());
        assert_eq!(reloaded.interests_for_shell("other_shell"), vec!["料理"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
