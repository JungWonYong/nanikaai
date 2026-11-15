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
        .name("ukagaka-server".to_string())
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
                    .route("/api/speak", post(handle_speak))
                    .route("/api/chat", post(handle_chat))
                    .route("/api/event", post(handle_event))
                    .route("/api/skin", post(handle_switch_skin))
                    .route("/api/provider", post(handle_switch_provider))
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
