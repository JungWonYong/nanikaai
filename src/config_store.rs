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
    "・うにゅう（\\1）: 関西弁（〜やねん、せやな、なんでやねん）で話す謎の白い生き物。表情は \\s[10](素), \\s[11](驚き) を使う。\n",
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
    #[serde(default = "default_persona")]
    pub system_persona: String,
    #[serde(default)]
    pub memories: Vec<MemoryItem>,
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
            system_persona: default_persona(),
            memories: Vec::new(),
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
            Ok(txt) => serde_json::from_str::<AgentConfig>(&txt).unwrap_or_default(),
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
        Self::save_to_path(&self.file_path, &guard);
        guard.clone()
    }

    pub fn upsert_memory(&self, title: &str, content: &str) -> MemoryItem {
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
        if let Some(existing) = guard
            .memories
            .iter_mut()
            .find(|m| m.title.eq_ignore_ascii_case(&clean_title))
        {
            *existing = entry.clone();
        } else {
            guard.memories.push(entry.clone());
        }
        Self::save_to_path(&self.file_path, &guard);
        entry
    }

    pub fn search_memories(&self, keyword: &str) -> Vec<MemoryItem> {
        let q = keyword.trim().to_lowercase();
        let guard = self.inner.lock().expect("config mutex poisoned");
        if q.is_empty() {
            let start = guard.memories.len().saturating_sub(10);
            return guard.memories[start..].to_vec();
        }
        guard
            .memories
            .iter()
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
        let dir = std::env::temp_dir().join(format!("ukagaka_cfg_test_{}", std::process::id()));
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
