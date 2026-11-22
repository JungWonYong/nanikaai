use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use encoding_rs::SHIFT_JIS;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::agent_engine::AgentReply;
use crate::dialogue_parser::{parse_dialogue, DialogueAction};

pub fn parse_sstp_text(raw_text: &str) -> (String, String) {
    let mut sender = "external".to_string();
    let mut script = String::new();
    for line in raw_text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let kl = k.trim().to_ascii_lowercase();
        if kl == "sender" {
            sender = v.trim().to_string();
        } else if kl == "script" {
            script = v.trim().to_string();
        }
    }
    (sender, script)
}

pub trait ServerBackend: Send + Sync + 'static {
    fn get_health(&self) -> Value;
    fn get_skins(&self) -> Value;
    fn get_providers(&self) -> Value;
    fn on_speak(&self, script: &str) -> Vec<DialogueAction>;
    fn on_chat(&self, message: &str) -> AgentReply;
    fn on_event(&self, event: &str, detail: &str) -> AgentReply;
    fn on_switch_skin(&self, shell: Option<String>, balloon: Option<String>) -> Result<Value, String>;
    fn on_switch_provider(&self, provider: String, model: Option<String>) -> Result<Value, String>;
    fn get_voices(&self) -> Value {
        json!({"voices": [], "error": "voice configuration not supported"})
    }
    fn on_configure_voice(&self, _req: VoiceConfigReq) -> Result<Value, String> {
        Err("voice configuration not supported".to_string())
    }
    fn get_settings(&self) -> Value {
        json!({"error": "settings configuration not supported"})
    }
    fn get_backlog(&self) -> Value {
        json!({ "count": 0, "entries": [] })
    }
    fn on_update_settings(&self, _req: SettingsPatchReq) -> Result<Value, String> {
        Err("settings configuration not supported".to_string())
    }
    fn get_memos(&self) -> Value {
        json!({ "count": 0, "memos": [] })
    }
    fn on_memo_action(&self, _req: MemoActionReq) -> Result<Value, String> {
        Err("memos not supported".to_string())
    }
    fn get_calendar(&self) -> Value {
        json!({ "enabled": false, "connected": false, "count": 0, "events": [] })
    }
    fn on_calendar_action(&self, _req: CalendarConfigReq) -> Result<Value, String> {
        Err("calendar not supported".to_string())
    }
}

#[derive(Clone)]
struct AppState {
    backend: Arc<dyn ServerBackend>,
    http_port: u16,
    tcp_port: u16,
}

#[derive(Deserialize)]
struct SpeakReq {
    script: Option<String>,
}

#[derive(Deserialize)]
struct ChatReq {
    message: Option<String>,
}

#[derive(Deserialize)]
struct EventReq {
    event: Option<String>,
    detail: Option<String>,
}

#[derive(Deserialize)]
struct SkinReq {
    shell: Option<String>,
    balloon: Option<String>,
}

#[derive(Deserialize)]
struct ProviderReq {
    provider: Option<String>,
    model: Option<String>,
}

/// JSON payload for configuring TTS engine, speaker overrides, archetype, or AivisHub model install.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VoiceConfigReq {
    pub voice_tts: Option<bool>,
    pub tts_engine: Option<String>,
    pub sakura_speaker_id: Option<i64>,
    #[serde(default)]
    pub clear_sakura_speaker: bool,
    pub kero_speaker_id: Option<i64>,
    #[serde(default)]
    pub clear_kero_speaker: bool,
    pub kero_voice_archetype: Option<String>,
    pub install_aivm_uuid: Option<String>,
    pub uninstall_aivm_uuid: Option<String>,
    pub voice_speed: Option<f32>,
    pub voice_intonation: Option<f32>,
}

/// JSON payload for quick sticky memo actions (`add` / `toggle` / `delete` / `clear_done`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoActionReq {
    pub action: Option<String>,
    pub text: Option<String>,
    pub id: Option<u64>,
}

/// JSON payload for calendar sync configuration & actions (Google / OS / Apple iCloud / Yahoo! Japan / Yahoo / iCal).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CalendarConfigReq {
    pub action: Option<String>,
    pub enabled: Option<bool>,
    pub use_os_native: Option<bool>,
    pub add_ics_url: Option<String>,
    #[serde(default)]
    pub clear_ics_urls: bool,
    pub home_location: Option<String>,
    pub default_travel_mins: Option<u32>,
    pub prep_mins: Option<u32>,
    pub remind_mins: Option<u32>,
    pub briefing_interval_mins: Option<u32>,
}

/// JSON payload for reading/updating skin opacity, scale, and OS convenience settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsPatchReq {
    pub skin_opacity: Option<f32>,
    pub balloon_opacity: Option<f32>,
    pub character_scale: Option<f32>,
    pub opaque_on_hover: Option<bool>,
    pub eye_tracking: Option<bool>,
    pub lock_position: Option<bool>,
    pub snap_to_bottom: Option<bool>,
    pub click_through_mode: Option<bool>,
    pub show_kero: Option<bool>,
    pub always_on_top: Option<bool>,
    pub live2d_motion: Option<bool>,
    pub hourly_chime: Option<bool>,
    pub auto_talk_interval_sec: Option<u64>,
    pub interest_news_weight: Option<f32>,
    pub interests: Option<Vec<String>>,
    #[serde(default)]
    pub clear_interests: bool,
    pub ui_language: Option<String>,
    pub quiet_mode: Option<bool>,
    pub auto_hide_on_fullscreen: Option<bool>,
    pub characters_hidden: Option<bool>,
    pub auto_start_at_login: Option<bool>,
    pub power_save_mode: Option<String>,
    pub low_battery_alert: Option<bool>,
    pub pomodoro_preset_idx: Option<usize>,
    pub pomodoro_action: Option<String>,
    pub voice_speed: Option<f32>,
    pub voice_intonation: Option<f32>,
    pub macos_accept_first_mouse: Option<bool>,
    pub macos_window_shadow: Option<bool>,
    pub macos_show_in_dock: Option<bool>,
    pub windows_skip_taskbar: Option<bool>,
    pub windows_undecorated_shadow: Option<bool>,
    pub linux_prefer_x11: Option<bool>,
    pub linux_respect_workarea: Option<bool>,
    #[serde(default)]
    pub reset_defaults: bool,
}

pub fn validate_settings_patch(req: &SettingsPatchReq) -> Result<(), String> {
    if let Some(v) = req.skin_opacity {
        if !v.is_finite() || !(0.15..=1.0).contains(&v) {
            return Err("Invalid skin_opacity: must be finite and within 0.15..=1.0".to_string());
        }
    }
    if let Some(v) = req.balloon_opacity {
        if !v.is_finite() || !(0.25..=1.0).contains(&v) {
            return Err("Invalid balloon_opacity: must be finite and within 0.25..=1.0".to_string());
        }
    }
    if let Some(v) = req.character_scale {
        if !v.is_finite() || !(0.50..=1.50).contains(&v) {
            return Err("Invalid character_scale: must be finite and within 0.50..=1.50".to_string());
        }
    }
    if let Some(v) = req.auto_talk_interval_sec {
        if v != 0 && !(30..=1800).contains(&v) {
            return Err(
                "Invalid auto_talk_interval_sec: must be 0 (OFF) or within 30..=1800".to_string(),
            );
        }
    }
    if let Some(v) = req.interest_news_weight {
        if !v.is_finite() || !(0.0..=0.80).contains(&v) {
            return Err(
                "Invalid interest_news_weight: must be finite and within 0.0..=0.80".to_string(),
            );
        }
    }
    if let Some(ref lang) = req.ui_language {
        if !crate::i18n::is_valid_ui_language_setting(lang) {
            return Err("Invalid ui_language: must be one of auto, ja, ko, en".to_string());
        }
    }
    if let Some(ref ps) = req.power_save_mode {
        if !crate::accessory_utils::is_valid_power_save_mode(ps) {
            return Err("Invalid power_save_mode: must be one of auto, on, off".to_string());
        }
    }
    if let Some(idx) = req.pomodoro_preset_idx {
        if idx >= crate::accessory_utils::POMODORO_PRESETS.len() {
            return Err(format!(
                "Invalid pomodoro_preset_idx: must be 0..{}",
                crate::accessory_utils::POMODORO_PRESETS.len() - 1
            ));
        }
    }
    if let Some(ref act) = req.pomodoro_action {
        let low = act.trim().to_ascii_lowercase();
        if !matches!(low.as_str(), "start" | "pause" | "toggle" | "stop" | "skip") {
            return Err(
                "Invalid pomodoro_action: must be one of start, pause, toggle, stop, skip"
                    .to_string(),
            );
        }
    }
    if let Some(v) = req.voice_speed {
        if !v.is_finite() || !(0.75..=1.35).contains(&v) {
            return Err("Invalid voice_speed: must be finite and within 0.75..=1.35".to_string());
        }
    }
    if let Some(v) = req.voice_intonation {
        if !v.is_finite() || !(0.70..=1.50).contains(&v) {
            return Err(
                "Invalid voice_intonation: must be finite and within 0.70..=1.50".to_string(),
            );
        }
    }
    Ok(())
}

async fn handle_health(State(st): State<AppState>) -> Json<Value> {
    let mut obj = st.backend.get_health();
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
        map.insert("httpPort".to_string(), json!(st.http_port));
        map.insert("sstpTcpPort".to_string(), json!(st.tcp_port));
    }
    Json(obj)
}

async fn handle_skins(State(st): State<AppState>) -> Json<Value> {
    let mut obj = st.backend.get_skins();
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

async fn handle_providers(State(st): State<AppState>) -> Json<Value> {
    let b = st.backend.clone();
    let mut obj = tokio::task::spawn_blocking(move || b.get_providers())
        .await
        .unwrap_or_else(|_| json!({}));
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

async fn handle_speak(State(st): State<AppState>, Json(req): Json<SpeakReq>) -> Response {
    let script = req.script.unwrap_or_default();
    if script.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "script required"})),
        )
            .into_response();
    }
    let acts = st.backend.on_speak(&script);
    (StatusCode::OK, Json(json!({"ok": true, "actions": acts}))).into_response()
}

async fn handle_chat(State(st): State<AppState>, Json(req): Json<ChatReq>) -> Response {
    let msg = req.message.unwrap_or_default().trim().to_string();
    if msg.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "message required"})),
        )
            .into_response();
    }
    let b = st.backend.clone();
    match tokio::task::spawn_blocking(move || b.on_chat(&msg)).await {
        Ok(rep) => {
            let mut val = serde_json::to_value(rep).unwrap_or_else(|_| json!({}));
            if let Some(m) = val.as_object_mut() {
                m.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(val)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn handle_event(State(st): State<AppState>, Json(req): Json<EventReq>) -> Response {
    let ev = req
        .event
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ExternalEvent".to_string());
    let detail = req.detail.unwrap_or_default();
    let b = st.backend.clone();
    match tokio::task::spawn_blocking(move || b.on_event(&ev, &detail)).await {
        Ok(rep) => {
            let mut val = serde_json::to_value(rep).unwrap_or_else(|_| json!({}));
            if let Some(m) = val.as_object_mut() {
                m.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(val)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn handle_switch_skin(State(st): State<AppState>, Json(req): Json<SkinReq>) -> Response {
    let b = st.backend.clone();
    let res = tokio::task::spawn_blocking(move || b.on_switch_skin(req.shell, req.balloon)).await;
    match res {
        Ok(Ok(mut val)) => {
            if let Some(m) = val.as_object_mut() {
                m.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(val)).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn handle_switch_provider(
    State(st): State<AppState>,
    Json(req): Json<ProviderReq>,
) -> Response {
    let provider = req.provider.unwrap_or_default().trim().to_string();
    if provider.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "provider required"})),
        )
            .into_response();
    }
    let b = st.backend.clone();
    let res =
        tokio::task::spawn_blocking(move || b.on_switch_provider(provider, req.model)).await;
    match res {
        Ok(Ok(mut val)) => {
            if let Some(m) = val.as_object_mut() {
                m.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(val)).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn handle_get_voices(State(st): State<AppState>) -> Json<Value> {
    let b = st.backend.clone();
    let mut obj = tokio::task::spawn_blocking(move || b.get_voices())
        .await
        .unwrap_or_else(|_| json!({}));
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

/// Validate `/api/voice` request fields before invoking TTS engine discovery or model install/uninstall.
pub fn validate_voice_config_req(req: &VoiceConfigReq) -> Result<(), String> {
    if let Some(ref eng) = req.tts_engine {
        let low = eng.trim().to_ascii_lowercase();
        if !matches!(
            low.as_str(),
            "auto" | "aivisspeech" | "aivis" | "voicevox" | "vv" | "coeiroink" | "os" | "native"
        ) {
            return Err(format!(
                "Invalid tts_engine: '{eng}' (allowed: auto, aivisspeech, voicevox, coeiroink, os)"
            ));
        }
    }
    if let Some(ref arch) = req.kero_voice_archetype {
        let low = arch.trim().to_ascii_lowercase();
        if !low.is_empty()
            && !matches!(
                low.as_str(),
                "auto" | "default" | "boy" | "boyish" | "girl" | "female" | "female_partner" | "male" | "youth_male" | "man"
            )
        {
            return Err(format!("Invalid kero_voice_archetype: '{arch}'"));
        }
    }
    if let Some(ref uuid) = req.install_aivm_uuid {
        let trimmed = uuid.trim();
        if !trimmed.is_empty() && !crate::companion_extras::is_valid_aivm_uuid(trimmed) {
            return Err(format!("Invalid install_aivm_uuid: '{uuid}'"));
        }
    }
    if let Some(ref uuid) = req.uninstall_aivm_uuid {
        let trimmed = uuid.trim();
        if !trimmed.is_empty() && !crate::companion_extras::is_valid_aivm_uuid(trimmed) {
            return Err(format!("Invalid uninstall_aivm_uuid: '{uuid}'"));
        }
    }
    if let Some(v) = req.voice_speed {
        if !v.is_finite() || !(0.75..=1.35).contains(&v) {
            return Err("Invalid voice_speed: must be finite and within 0.75..=1.35".to_string());
        }
    }
    if let Some(v) = req.voice_intonation {
        if !v.is_finite() || !(0.70..=1.50).contains(&v) {
            return Err(
                "Invalid voice_intonation: must be finite and within 0.70..=1.50".to_string(),
            );
        }
    }
    Ok(())
}

async fn handle_post_voice(
    State(st): State<AppState>,
    Json(req): Json<VoiceConfigReq>,
) -> Response {
    if let Err(err) = validate_voice_config_req(&req) {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": err}))).into_response();
    }
    let b = st.backend.clone();
    let res = tokio::task::spawn_blocking(move || b.on_configure_voice(req)).await;
    match res {
        Ok(Ok(mut val)) => {
            if let Some(m) = val.as_object_mut() {
                m.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(val)).into_response()
        }
        Ok(Err(e)) => {
            let status = if e.contains("Invalid") || e.contains("不正") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({"ok": false, "error": e}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn handle_get_settings(State(st): State<AppState>) -> Json<Value> {
    let b = st.backend.clone();
    let mut obj = tokio::task::spawn_blocking(move || b.get_settings())
        .await
        .unwrap_or_else(|_| json!({}));
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

async fn handle_get_backlog(State(st): State<AppState>) -> Json<Value> {
    let b = st.backend.clone();
    let mut obj = tokio::task::spawn_blocking(move || b.get_backlog())
        .await
        .unwrap_or_else(|_| json!({ "count": 0, "entries": [] }));
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

async fn handle_get_memos(State(st): State<AppState>) -> Json<Value> {
    let b = st.backend.clone();
    let mut obj = tokio::task::spawn_blocking(move || b.get_memos())
        .await
        .unwrap_or_else(|_| json!({ "count": 0, "memos": [] }));
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

async fn handle_post_memos(
    State(st): State<AppState>,
    Json(req): Json<MemoActionReq>,
) -> Response {
    let b = st.backend.clone();
    let res = tokio::task::spawn_blocking(move || b.on_memo_action(req))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(mut obj) => {
            if let Some(map) = obj.as_object_mut() {
                map.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(obj)).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": err})),
        )
            .into_response(),
    }
}

async fn handle_get_calendar(State(st): State<AppState>) -> Json<Value> {
    let b = st.backend.clone();
    let mut obj = tokio::task::spawn_blocking(move || b.get_calendar())
        .await
        .unwrap_or_else(|_| json!({ "enabled": false, "count": 0, "events": [] }));
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
    }
    Json(obj)
}

async fn handle_post_calendar(
    State(st): State<AppState>,
    Json(req): Json<CalendarConfigReq>,
) -> Response {
    let b = st.backend.clone();
    let res = tokio::task::spawn_blocking(move || b.on_calendar_action(req))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(mut obj) => {
            if let Some(map) = obj.as_object_mut() {
                map.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(obj)).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": err})),
        )
            .into_response(),
    }
}

async fn handle_post_settings(
    State(st): State<AppState>,
    Json(req): Json<SettingsPatchReq>,
) -> Response {
    if let Err(err) = validate_settings_patch(&req) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": err})),
        )
            .into_response();
    }
    let b = st.backend.clone();
    let res = tokio::task::spawn_blocking(move || b.on_update_settings(req)).await;
    match res {
        Ok(Ok(mut val)) => {
            if let Some(m) = val.as_object_mut() {
                m.insert("ok".to_string(), json!(true));
            }
            (StatusCode::OK, Json(val)).into_response()
        }
        Ok(Err(e)) => {
            let status = if e.contains("Invalid") || e.contains("range") || e.contains("不正") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({"ok": false, "error": e}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn handle_http_sstp(State(st): State<AppState>, body: String) -> Response {
    let (_, script) = parse_sstp_text(&body);
    if script.is_empty() {
        (StatusCode::BAD_REQUEST, "SSTP/1.4 400 Bad Request\r\n\r\n").into_response()
    } else {
        st.backend.on_speak(&script);
        let n_acts = parse_dialogue(&script).len();
        (
            StatusCode::OK,
            format!("SSTP/1.4 200 OK\r\nCharset: UTF-8\r\nX-Actions: {n_acts}\r\n\r\n"),
        )
            .into_response()
    }
}

async fn bind_loopback_listener(preferred_port: u16) -> std::io::Result<TcpListener> {
    if preferred_port != 0 {
        let addr: SocketAddr = ([127, 0, 0, 1], preferred_port).into();
        if let Ok(listener) = TcpListener::bind(addr).await {
            return Ok(listener);
        }
    }
    let fallback: SocketAddr = ([127, 0, 0, 1], 0).into();
    TcpListener::bind(fallback).await
}

pub struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub http_port: u16,
    pub tcp_port: u16,
}

impl ServerHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start_dual_server(
    backend: Arc<dyn ServerBackend>,
    preferred_http_port: u16,
    preferred_tcp_port: u16,
) -> Result<ServerHandle, String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(u16, u16), String>>();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    std::thread::Builder::new()
        .name("nanikaai-server".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(4)
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };

            rt.block_on(async move {
                let http_listener = match bind_loopback_listener(preferred_http_port).await {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("HTTP bind failed: {e}")));
                        return;
                    }
                };
                let tcp_listener = match bind_loopback_listener(preferred_tcp_port).await {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("SSTP TCP bind failed: {e}")));
                        return;
                    }
                };

                let http_port = http_listener.local_addr().map(|a| a.port()).unwrap_or(0);
                let tcp_port = tcp_listener.local_addr().map(|a| a.port()).unwrap_or(0);
                let _ = ready_tx.send(Ok((http_port, tcp_port)));

                let state = AppState {
                    backend: backend.clone(),
                    http_port,
                    tcp_port,
                };
                let router = Router::new()
                    .route("/api/health", get(handle_health))
                    .route("/api/skins", get(handle_skins))
                    .route("/api/providers", get(handle_providers))
                    .route("/api/voices", get(handle_get_voices))
                    .route("/api/backlog", get(handle_get_backlog))
                    .route("/api/memos", get(handle_get_memos).post(handle_post_memos))
                    .route("/api/calendar", get(handle_get_calendar).post(handle_post_calendar))
                    .route("/api/settings", get(handle_get_settings).post(handle_post_settings))
                    .route("/api/speak", post(handle_speak))
                    .route("/api/chat", post(handle_chat))
                    .route("/api/event", post(handle_event))
                    .route("/api/skin", post(handle_switch_skin))
                    .route("/api/provider", post(handle_switch_provider))
                    .route("/api/voice", post(handle_post_voice))
                    .route("/sstp", post(handle_http_sstp))
                    .with_state(state);

                let tcp_backend = backend.clone();
                let tcp_task = tokio::spawn(async move {
                    loop {
                        let Ok((mut stream, _)) = tcp_listener.accept().await else {
                            break;
                        };
                        let b = tcp_backend.clone();
                        tokio::spawn(async move {
                            let mut buf = Vec::with_capacity(4096);
                            let mut tmp = [0u8; 4096];
                            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                                loop {
                                    match stream.read(&mut tmp).await {
                                        Ok(0) | Err(_) => break,
                                        Ok(n) => {
                                            buf.extend_from_slice(&tmp[..n]);
                                            if buf.windows(4).any(|w| w == b"\r\n\r\n")
                                                || buf.windows(2).any(|w| w == b"\n\n")
                                                || buf.len() > 65536
                                            {
                                                break;
                                            }
                                        }
                                    }
                                }
                            })
                            .await;
                            let txt = match std::str::from_utf8(&buf) {
                                Ok(s) => s.to_string(),
                                Err(_) => SHIFT_JIS.decode(&buf).0.into_owned(),
                            };
                            let (_, script) = parse_sstp_text(&txt);
                            if script.is_empty() {
                                let _ = stream
                                    .write_all(b"SSTP/1.4 400 Bad Request\r\n\r\n")
                                    .await;
                            } else {
                                b.on_speak(&script);
                                let _ = stream
                                    .write_all(b"SSTP/1.4 200 OK\r\nCharset: UTF-8\r\n\r\n")
                                    .await;
                            }
                        });
                    }
                });

                let _ = axum::serve(http_listener, router)
                    .with_graceful_shutdown(async move {
                        let _ = (&mut shutdown_rx).await;
                    })
                    .await;
                tcp_task.abort();
            });
        })
        .map_err(|e| e.to_string())?;

    let (http_port, tcp_port) = ready_rx
        .recv()
        .map_err(|e| format!("Server thread failed to start: {e}"))??;
    Ok(ServerHandle {
        shutdown_tx: Some(shutdown_tx),
        http_port,
        tcp_port,
    })
}
