use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const DEFAULT_PERSONA: &str = concat!(
    "あなたはデスクトップマスコット「伺か（MATERIA）」の『さくら（\\0）』と相棒『うにゅう（\\1）』の2人組AIエージェントです。\n",
    "・さくら（\\0）: 少しクールで落ち着いた口調だが面倒見が良い女の子。表情は \\s[0](素), \\s[1](困り/照れ), \\s[4](ジト目/呆れ), ",
    "\\s[5](微笑み), \\s[6](目閉じ), \\s[7](怒り), \\s[8](明るい笑顔), \\s[9](照れ怒り) を使い分ける。\n",
    "・うにゅう（\\1）: 関西弁（〜やねん、せやな、なんでやねん）で話す、薄紫青色（ペリウィンクルブルー）のもちもちした謎の生き物。鼻先に小さな丸サングラスを老眼鏡のように低く掛けており、フレームの上から両目が見えている。表情は \\s[10](素), \\s[11](驚き), \\s[12](得意げ) を使う。\n",
    "・応答は必ず Sakura Script 形式（例: \\0\\s[5]こんにちは。\\1\\s[10]今日も元気やな！\\e）で出力し、吹き出しに収まるよう簡潔（2〜5文程度）にまとめること。\n",
    "・PC状態の調査、ファイル確認、Web検索、長期記憶の記録が必要な場合は自律的にツールを呼び出して事実に基づき回答すること。"
);

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
fn default_autotalk() -> u64 {
    180
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
    pub active_binds: Option<Vec<i32>>,
    #[serde(default)]
    pub sakura_speaker_id: Option<i64>,
    #[serde(default)]
    pub kero_speaker_id: Option<i64>,
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
    #[serde(default = "default_voicevox")]
    pub voicevox_url: String,
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
            http_port: default_http_port(),
            sstp_tcp_port: default_sstp_port(),
            active_shell_id: default_shell_id(),
            active_balloon_id: default_balloon_id(),
            show_kero: true,
            always_on_top: true,
            live2d_motion: true,
            hourly_chime: true,
            voice_tts: false,
            voicevox_url: default_voicevox(),
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
    pub active_shell_id: Option<String>,
    pub active_balloon_id: Option<String>,
    pub show_kero: Option<bool>,
    pub always_on_top: Option<bool>,
    pub live2d_motion: Option<bool>,
    pub hourly_chime: Option<bool>,
    pub voice_tts: Option<bool>,
    pub voicevox_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub config_dir: PathBuf,
    pub file_path: PathBuf,
    inner: Arc<Mutex<AgentConfig>>,
}

impl ConfigStore {
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        let dir = config_dir.unwrap_or_else(default_user_data_dir);
        let _ = fs::create_dir_all(&dir);
        let file_path = dir.join("config.json");
        let cfg = Self::load_from_path(&file_path);
        Self {
            config_dir: dir,
            file_path,
            inner: Arc::new(Mutex::new(cfg)),
        }
    }

    fn load_from_path(file_path: &Path) -> AgentConfig {
        if !file_path.is_file() {
            let defaults = AgentConfig::default();
            Self::save_to_path(file_path, &defaults);
            return defaults;
        }
        match fs::read_to_string(file_path) {
            Ok(txt) => {
                let mut cfg = serde_json::from_str::<AgentConfig>(&txt).unwrap_or_default();
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
            guard.auto_talk_interval_sec = v;
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
        if let Some(v) = patch.voicevox_url {
            guard.voicevox_url = v;
        }
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
}
