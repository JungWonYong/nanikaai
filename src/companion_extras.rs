use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::agent_engine::wrap_untrusted_data;
use crate::builtin_tools::{is_sensitive_path, validate_public_url};
use crate::dialogue_parser::{parse_dialogue, ActionKind, Scope};

pub const GITHUB_RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/JungWonYong/nanikaai/releases/latest";
pub const DEFAULT_VOICEVOX_URL: &str = "http://127.0.0.1:50021";

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

#[derive(Debug, Serialize, Deserialize)]
struct SpeechJob {
    scope_id: u8,
    text: String,
    voicevox_url: String,
}

fn tts_sender() -> &'static Mutex<Option<mpsc::Sender<SpeechJob>>> {
    static SENDER: OnceLock<Mutex<Option<mpsc::Sender<SpeechJob>>>> = OnceLock::new();
    SENDER.get_or_init(|| Mutex::new(None))
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

/// Speak an utterance asynchronously via local VOICEVOX (if reachable) or OS native TTS.
pub fn speak_utterance_async(scope_id: u8, text: &str, voicevox_url: &str) {
    let clean: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        return;
    }
    let truncated: String = clean.chars().take(600).collect();
    let base_url = if voicevox_url.trim().is_empty() {
        DEFAULT_VOICEVOX_URL.to_string()
    } else {
        voicevox_url.trim().trim_end_matches('/').to_string()
    };

    let Ok(mut guard) = tts_sender().lock() else {
        return;
    };
    if guard.is_none() {
        let (tx, rx) = mpsc::channel::<SpeechJob>();
        if std::thread::Builder::new()
            .name("nanikaai-tts".to_string())
            .spawn(move || {
                while let Ok(mut job) = rx.recv() {
                    // Drain stale backlog to speak only the most recent lines without piling up.
                    while let Ok(newer) = rx.try_recv() {
                        job = newer;
                    }
                    speak_blocking(&job);
                }
            })
            .is_ok()
        {
            *guard = Some(tx);
        }
    }
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(SpeechJob {
            scope_id,
            text: truncated,
            voicevox_url: base_url,
        });
    }
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

fn speak_blocking(job: &SpeechJob) {
    if let Some(base) = validate_local_tts_url(&job.voicevox_url) {
        if try_voicevox_blocking(job.scope_id, &job.text, &base) {
            return;
        }
    }
    try_os_tts_blocking(job.scope_id, &job.text);
}

fn try_voicevox_blocking(scope_id: u8, text: &str, base: &str) -> bool {
    let Ok(client) = Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return false;
    };
    // VOICEVOX default speaker IDs: 2 = Shikoku Metan (Normal), 3 = Zundamon (Normal)
    let speaker = if scope_id == 0 { 2 } else { 3 };
    let mut query_url = match Url::parse(&format!("{base}/audio_query")) {
        Ok(u) => u,
        Err(_) => return false,
    };
    query_url
        .query_pairs_mut()
        .append_pair("text", text)
        .append_pair("speaker", &speaker.to_string());

    let Ok(q_resp) = client.post(query_url).send() else {
        return false;
    };
    if !q_resp.status().is_success() {
        return false;
    }
    let Ok(query_json) = q_resp.json::<serde_json::Value>() else {
        return false;
    };

    let synth_url = format!("{base}/synthesis?speaker={speaker}");
    let Ok(s_resp) = client.post(synth_url).json(&query_json).send() else {
        return false;
    };
    if !s_resp.status().is_success() {
        return false;
    }
    let Ok(wav_bytes) = s_resp.bytes() else {
        return false;
    };
    if wav_bytes.len() < 44 {
        return false;
    }
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

fn try_os_tts_blocking(scope_id: u8, text: &str) {
    #[cfg(target_os = "macos")]
    {
        let voice = if scope_id == 0 { "Kyoko" } else { "Otoya" };
        let primary = Command::new("say")
            .args(["-v", voice, "--", text])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !primary {
            let _ = Command::new("say").args(["--", text]).status();
        }
    }
    #[cfg(target_os = "windows")]
    {
        let role = if scope_id == 0 { "0" } else { "1" };
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "Add-Type -AssemblyName System.Speech; $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Rate=if($env:NANIKAAI_TTS_ROLE -eq '1'){-1}else{1}; $s.Speak($env:NANIKAAI_TTS_TEXT)",
            ])
            .env("NANIKAAI_TTS_ROLE", role)
            .env("NANIKAAI_TTS_TEXT", text)
            .status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let pitch = if scope_id == 0 { "20" } else { "-25" };
        let ok = Command::new("spd-say")
            .args(["-p", pitch, "--", text])
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

fn is_blocked_drop_target(path: &Path) -> bool {
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
    f.by_ref()
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
}
