use std::sync::OnceLock;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiLang {
    Ja,
    Ko,
    En,
}

impl UiLang {
    pub fn code(self) -> &'static str {
        match self {
            UiLang::Ja => "ja",
            UiLang::Ko => "ko",
            UiLang::En => "en",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            UiLang::Ja => "日本語",
            UiLang::Ko => "한국어",
            UiLang::En => "English",
        }
    }
}

pub const UI_LANGUAGE_CHOICES: &[&str] = &["auto", "ja", "ko", "en"];

pub fn normalize_ui_language(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "auto".to_string();
    }
    let low = trimmed.to_ascii_lowercase();
    match low.as_str() {
        "auto" | "os" | "system" => "auto".to_string(),
        "ja" | "jp" | "jpn" | "japanese" => "ja".to_string(),
        "ko" | "kr" | "kor" | "korean" => "ko".to_string(),
        "en" | "eng" | "english" => "en".to_string(),
        _ if trimmed == "日本語" => "ja".to_string(),
        _ if trimmed == "한국어" => "ko".to_string(),
        _ => "auto".to_string(),
    }
}

pub fn is_valid_ui_language_setting(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "auto" | "ja" | "ko" | "en"
    )
}

/// Map any OS locale tag (`ja-JP`, `ko_KR.UTF-8`, `en-US`, `fr-FR`, `C.UTF-8`, etc.) to `UiLang`.
/// Unsupported or empty locales fall back cleanly to `UiLang::En`.
pub fn parse_locale_to_ui_lang(raw_locale: &str) -> UiLang {
    let primary = raw_locale
        .trim()
        .split(['-', '_', '.', '@', ':', ';', ' '])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match primary.as_str() {
        "ja" | "jp" | "jpn" => UiLang::Ja,
        "ko" | "kr" | "kor" => UiLang::Ko,
        "en" | "eng" => UiLang::En,
        _ => UiLang::En,
    }
}

fn is_usable_locale_candidate(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() {
        return false;
    }
    let low = t.to_ascii_lowercase();
    !matches!(low.as_str(), "c" | "c.utf-8" | "c.utf8" | "posix")
}

fn first_usable_env_locale(keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Ok(val) = std::env::var(k) {
            for seg in val.split(':') {
                let s = seg.trim();
                if is_usable_locale_candidate(s) {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn extract_first_quoted_tag(raw: &str) -> Option<String> {
    if let Some(q1) = raw.find('"') {
        let rest = &raw[q1 + 1..];
        if let Some(q2) = rest.find('"') {
            let tag = rest[..q2].trim();
            if is_usable_locale_candidate(tag) {
                return Some(tag.to_string());
            }
        }
    }
    for line in raw.lines() {
        let t = line.trim().trim_matches(&['(', ')', ',', '"', ' '][..]);
        if is_usable_locale_candidate(t) {
            return Some(t.to_string());
        }
    }
    None
}

fn probe_os_locale_uncached() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            if out.status.success() {
                let txt = String::from_utf8_lossy(&out.stdout);
                if let Some(tag) = extract_first_quoted_tag(&txt) {
                    return tag;
                }
            }
        }
        if let Ok(out) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
        {
            if out.status.success() {
                let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if is_usable_locale_candidate(&tag) {
                    return tag;
                }
            }
        }
        if let Some(loc) = first_usable_env_locale(&["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]) {
            return loc;
        }
        return "en-US".to_string();
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(out) = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[System.Globalization.CultureInfo]::CurrentUICulture.Name",
            ])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
        {
            if out.status.success() {
                let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if is_usable_locale_candidate(&tag) {
                    return tag;
                }
            }
        }
        if let Some(loc) = first_usable_env_locale(&["LC_ALL", "LC_MESSAGES", "LANG"]) {
            return loc;
        }
        return "en-US".to_string();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(loc) = first_usable_env_locale(&["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]) {
            return loc;
        }
        "en-US".to_string()
    }
}

pub fn detect_os_locale_raw() -> &'static str {
    static OS_LOCALE: OnceLock<String> = OnceLock::new();
    OS_LOCALE.get_or_init(probe_os_locale_uncached).as_str()
}

pub fn resolve_ui_lang_with_os(setting: &str, os_locale: &str) -> UiLang {
    match normalize_ui_language(setting).as_str() {
        "ja" => UiLang::Ja,
        "ko" => UiLang::Ko,
        "en" => UiLang::En,
        _ => parse_locale_to_ui_lang(os_locale),
    }
}

pub fn resolve_ui_lang(setting: &str) -> UiLang {
    resolve_ui_lang_with_os(setting, detect_os_locale_raw())
}

pub fn format_ui_language_label(setting: &str, effective: UiLang) -> String {
    match normalize_ui_language(setting).as_str() {
        "ja" => "日本語".to_string(),
        "ko" => "한국어".to_string(),
        "en" => "English".to_string(),
        _ => match effective {
            UiLang::Ja => "OS 自動 (日本語)".to_string(),
            UiLang::Ko => "OS 자동 (한국어)".to_string(),
            UiLang::En => "OS Auto (English)".to_string(),
        },
    }
}

pub struct UiStrings {
    pub settings_title: &'static str,
    pub tab_appearance: &'static str,
    pub tab_os: &'static str,
    pub tab_voice: &'static str,
    pub tab_ai: &'static str,
    pub hdr_opacity_scale: &'static str,
    pub sld_skin_opacity_title: &'static str,
    pub sld_skin_opacity_sub: &'static str,
    pub sld_balloon_opacity_title: &'static str,
    pub sld_balloon_opacity_sub: &'static str,
    pub sld_scale_title: &'static str,
    pub sld_scale_sub: &'static str,
    pub hdr_motion_skins: &'static str,
    pub tog_opaque_hover: &'static str,
    pub tog_live2d: &'static str,
    pub tog_eye_track: &'static str,
    pub tog_kero: &'static str,
    pub lbl_shell: &'static str,
    pub lbl_balloon: &'static str,
    pub hdr_desktop_behavior: &'static str,
    pub lbl_ui_language: &'static str,
    pub tog_always_on_top: &'static str,
    pub tog_click_through: &'static str,
    pub tog_lock_pos: &'static str,
    pub tog_snap_bottom: &'static str,
    pub tog_hourly_chime: &'static str,
    pub btn_reset_pos: &'static str,
    pub os_hdr_macos_cur: &'static str,
    pub os_hdr_macos_shared: &'static str,
    pub os_hdr_win_cur: &'static str,
    pub os_hdr_win_shared: &'static str,
    pub os_hdr_linux_cur: &'static str,
    pub os_hdr_linux_shared: &'static str,
    pub tog_mac_first_mouse: &'static str,
    pub tog_mac_shadow: &'static str,
    pub tog_mac_dock: &'static str,
    pub tog_win_skip_taskbar: &'static str,
    pub tog_win_shadow: &'static str,
    pub tog_linux_x11: &'static str,
    pub tog_linux_workarea: &'static str,
    pub hdr_voice_tts: &'static str,
    pub tog_voice_tts: &'static str,
    pub lbl_tts_engine: &'static str,
    pub lbl_sakura_voice: &'static str,
    pub lbl_kero_voice: &'static str,
    pub lbl_kero_arch: &'static str,
    pub lbl_kindle_tts_engine: &'static str,
    pub lbl_kindle_voice: &'static str,
    pub lbl_note: &'static str,
    pub voice_note_value: &'static str,
    pub hdr_ai_tools: &'static str,
    pub lbl_provider: &'static str,
    pub lbl_model_api: &'static str,
    pub lbl_router_model: &'static str,
    pub lbl_coding_model: &'static str,
    pub lbl_reasoning_model: &'static str,
    pub lbl_cloud_fallback: &'static str,
    pub btn_setup_local_ai: &'static str,
    pub btn_setup_local_ai_busy: &'static str,
    pub lbl_best_free_ai: &'static str,
    pub hdr_autotalk_news: &'static str,
    pub sld_autotalk_title: &'static str,
    pub sld_autotalk_sub: &'static str,
    pub sld_weight_title: &'static str,
    pub sld_weight_sub: &'static str,
    pub lbl_saved_interests: &'static str,
    pub interests_empty: &'static str,
    pub btn_clear_interests: &'static str,
    pub tog_tools: &'static str,
    pub tog_clipboard: &'static str,
    pub hdr_storage: &'static str,
    pub lbl_json_path: &'static str,
    pub btn_open_cfg_dir: &'static str,
    pub btn_clear_hist: &'static str,
    pub footer_autosave: &'static str,
    pub btn_reset_defaults: &'static str,
    pub comm_header: &'static str,
    pub comm_placeholder: &'static str,
    pub menu_chat: &'static str,
    pub tray_chat: &'static str,
    pub menu_settings: &'static str,
    pub menu_default_skin: &'static str,
    pub menu_sec_skins_prefix: &'static str,
    pub menu_install_nar: &'static str,
    pub menu_sec_shells: &'static str,
    pub tray_sub_shells: &'static str,
    pub menu_sec_balloons: &'static str,
    pub tray_sub_balloons: &'static str,
    pub menu_sec_bind: &'static str,
    pub tray_sub_bind: &'static str,
    pub menu_sec_voice_prefix: &'static str,
    pub menu_voice_tts: &'static str,
    pub menu_voices_hdr_prefix: &'static str,
    pub menu_voice_sakura_auto_prefix: &'static str,
    pub menu_voice_kero_auto_prefix: &'static str,
    pub menu_voice_discovering: &'static str,
    pub menu_voice_arch_hdr: &'static str,
    pub tray_sub_arch: &'static str,
    pub menu_voice_engine_hdr: &'static str,
    pub tray_sub_engine: &'static str,
    pub tray_sub_sak_voice: &'static str,
    pub tray_sub_ker_voice: &'static str,
    pub menu_aivis_hdr: &'static str,
    pub tray_sub_aivis: &'static str,
    pub suffix_installed: &'static str,
    pub menu_sec_ai_prefix: &'static str,
    pub menu_setup_local_ai: &'static str,
    pub menu_sec_tools: &'static str,
    pub menu_hourly_chime: &'static str,
    pub menu_l2d_studio: &'static str,
    pub menu_l2d_folder: &'static str,
    pub menu_check_updates: &'static str,
    pub menu_clear_history: &'static str,
    pub tray_sec_view_tools: &'static str,
    pub menu_kero: &'static str,
    pub menu_always_on_top: &'static str,
    pub menu_live2d_motion: &'static str,
    pub menu_click_through: &'static str,
    pub menu_reset_pos: &'static str,
    pub menu_open_cfg_folder: &'static str,
    pub menu_quit: &'static str,
    pub voice_auto_prefix: &'static str,
    pub status_thinking: &'static str,
    pub status_local_ai_dry_run: &'static str,
    pub status_local_ai_busy: &'static str,
    pub status_local_ai_checking: &'static str,
    pub status_clear_interests: &'static str,
    pub status_link_opened_prefix: &'static str,
    pub status_url_blocked_prefix: &'static str,
    pub status_checking_updates: &'static str,
    pub status_tts_preparing: &'static str,
    pub hdr_focus_accessories: &'static str,
    pub tog_quiet_mode: &'static str,
    pub tog_auto_hide_fs: &'static str,
    pub tog_autostart: &'static str,
    pub tog_low_batt_alert: &'static str,
    pub lbl_pomodoro_preset: &'static str,
    pub lbl_power_save: &'static str,
    pub btn_hide_chars: &'static str,
    pub btn_show_chars: &'static str,
    pub btn_next_monitor: &'static str,
    pub btn_pomo_stop: &'static str,
    pub btn_show_backlog: &'static str,
    pub btn_copy_last: &'static str,
    pub menu_quiet_mode: &'static str,
    pub menu_hide_chars: &'static str,
    pub menu_show_chars: &'static str,
    pub menu_pomo_stop: &'static str,
    pub menu_show_backlog: &'static str,
    pub menu_copy_last: &'static str,
    pub menu_next_monitor: &'static str,
    pub status_quiet_on: &'static str,
    pub status_quiet_off: &'static str,
    pub status_chars_hidden: &'static str,
    pub status_chars_shown: &'static str,
    pub status_copied_reply: &'static str,
    pub status_copy_empty: &'static str,
    pub status_autostart_on: &'static str,
    pub status_autostart_off: &'static str,
    pub status_autostart_err: &'static str,
    pub status_monitor_moved_prefix: &'static str,
    pub status_dock_edge_left: &'static str,
    pub status_dock_edge_right: &'static str,
    pub status_power_save_prefix: &'static str,
    pub menu_ask_clipboard: &'static str,
    pub menu_quick_memos: &'static str,
    pub menu_test_sakura_voice: &'static str,
    pub menu_test_kero_voice: &'static str,
    pub menu_manage_voices: &'static str,
    pub menu_pomo_skip: &'static str,
    pub btn_pomo_skip: &'static str,
    pub sld_voice_speed_title: &'static str,
    pub sld_voice_speed_sub: &'static str,
    pub sld_voice_inton_title: &'static str,
    pub sld_voice_inton_sub: &'static str,
    pub btn_preview_sakura: &'static str,
    pub btn_preview_kero: &'static str,
    pub hdr_installed_models: &'static str,
    pub hdr_recommended_voices: &'static str,
    pub badge_builtin: &'static str,
    pub btn_delete_model: &'static str,
    pub btn_install_model: &'static str,
    pub lbl_no_aivm_models: &'static str,
    pub menu_calendar_agenda: &'static str,
    pub sec_calendar_hdr: &'static str,
    pub tgl_cal_enabled: &'static str,
    pub tgl_cal_os_native: &'static str,
    pub cyc_cal_briefing: &'static str,
    pub cyc_cal_travel: &'static str,
    pub btn_cal_paste_ics: &'static str,
    pub btn_cal_sync_now: &'static str,
    pub btn_cal_show_agenda: &'static str,
    pub btn_attach_image: &'static str,
}

static STRINGS_JA: UiStrings = UiStrings {
    settings_title: "⚙️ 何か藍 設定パネル · Settings",
    tab_appearance: "🎨 外観・透過",
    tab_os: "🖥 OS・機能",
    tab_voice: "🎤 音声・声色",
    tab_ai: "🤖 AI・保存先",
    hdr_opacity_scale: "不透明度・表示倍率 · Opacity & Scale",
    sld_skin_opacity_title: "キャラ透明度 (15%-100%)",
    sld_skin_opacity_sub: "Live2D 24層・クラシック共通 / 内部透け防止",
    sld_balloon_opacity_title: "吹き出し透明度 (25%-100%)",
    sld_balloon_opacity_sub: "会話バルーンウィンドウ全体の透過率",
    sld_scale_title: "表示サイズ倍率 (50%-150%)",
    sld_scale_sub: "さくら・うにゅう共通の表示スケール",
    hdr_motion_skins: "モーション・スキン選択 · Motion & Skins",
    tog_opaque_hover: "ホバー時に 100% 不透明化",
    tog_live2d: "✨ Live2D モーション・物理揺れ",
    tog_eye_track: "👀 カーソル視線追従",
    tog_kero: "🐣 パートナー (\\1) 表示",
    lbl_shell: "シェル · Shell",
    lbl_balloon: "バルーン · Balloon",
    hdr_desktop_behavior: "言語・デスクトップ共通便利機能",
    lbl_ui_language: "🌐 UI 言語",
    tog_always_on_top: "📌 常に手前に表示",
    tog_click_through: "👻 すり抜けモード (クリック透過 · トレイ/設定で解除)",
    tog_lock_pos: "🔒 ドラッグ位置固定",
    tog_snap_bottom: "🧲 画面下端へ自動吸着",
    tog_hourly_chime: "🕒 毎時00分の時報",
    btn_reset_pos: "🧭 キャラクター位置を定位置へリセット",
    os_hdr_macos_cur: "🍎 macOS 動作設定 (● 現在の OS)",
    os_hdr_macos_shared: "🍎 macOS 動作設定 (クロス OS 共有保存)",
    os_hdr_win_cur: "🪟 Windows 動作設定 (● 現在の OS)",
    os_hdr_win_shared: "🪟 Windows 動作設定 (クロス OS 共有保存)",
    os_hdr_linux_cur: "🐧 Linux 動作設定 (● 現在の OS)",
    os_hdr_linux_shared: "🐧 Linux 動作設定 (クロス OS 共有保存)",
    tog_mac_first_mouse: "非アクティブ時 1クリック反応 (起動時反映)",
    tog_mac_shadow: "ウィンドウに macOS シャドウを付与",
    tog_mac_dock: "Dock にアプリアイコンを表示 (要再起動)",
    tog_win_skip_taskbar: "タスクバーからキャラ枠を隠す",
    tog_win_shadow: "枠なしウィンドウシャドウを有効化",
    tog_linux_x11: "GNOME で X11 / XWayland 優先 (要再起動)",
    tog_linux_workarea: "パネル作業領域 (_NET_WORKAREA) を自動回避",
    hdr_voice_tts: "ヘッドレス音声・キャラクター声色 · Voice & TTS",
    tog_voice_tts: "🔊 音声読み上げ (TTS)",
    lbl_tts_engine: "音声エンジン",
    lbl_sakura_voice: "(\\0) さくら声色",
    lbl_kero_voice: "(\\1) 相棒声色",
    lbl_kero_arch: "(\\1) 声タイプ",
    lbl_kindle_tts_engine: "📚 Kindle 音声ライブラリ",
    lbl_kindle_voice: "📚 Kindle 音声モデル",
    lbl_note: "補足",
    voice_note_value: "AivisHub 追加は右クリック [🎤 音声・声色] から実行できます",
    hdr_ai_tools: "AI バックエンド＆会話ツール · AI & Tools",
    lbl_provider: "AI プロバイダー",
    lbl_model_api: "モデル / API",
    lbl_router_model: "🧭 ルーター (FunctionGemma)",
    lbl_coding_model: "💻 コーディング特化モデル",
    lbl_reasoning_model: "🧠 推論特化モデル",
    lbl_cloud_fallback: "☁️ クラウド昇格 / フォールバック",
    btn_setup_local_ai: "⚡ 無料ローカルAI (Ollama＋軽量モデル) 1クリック自動準備",
    btn_setup_local_ai_busy: "⏳ ローカルAI 自動セットアップ実行中...",
    lbl_best_free_ai: "推奨無料AI",
    hdr_autotalk_news: "雑談トーク & 関心ニュース · Auto-Talk & News",
    sld_autotalk_title: "雑談(自動トーク)間隔",
    sld_autotalk_sub: "左端OFF / 最短30秒～600秒",
    sld_weight_title: "関心ニュース比重",
    sld_weight_sub: "一般ニュース混合 (最大80%)",
    lbl_saved_interests: "記憶した関心事",
    interests_empty: "未登録 (会話で興味を話すと自動記憶)",
    btn_clear_interests: "🧹 記憶した関心事をリセット",
    tog_tools: "🧰 AI 自律ツール実行 (天気・Web・時刻・計算)",
    tog_clipboard: "📋 クリップボード内容参照を許可",
    hdr_storage: "JSON 設定保存先 · Storage (~/.nanikaai)",
    lbl_json_path: "JSON パス",
    btn_open_cfg_dir: "📂 設定フォルダを開く",
    btn_clear_hist: "🧹 会話履歴をクリア",
    footer_autosave: "💾 ~/.nanikaai/config.json · 即時自動保存",
    btn_reset_defaults: "↺ 既定値に戻す",
    comm_header: "何か藍に話しかける · Ask NanikaAi",
    comm_placeholder: "メッセージを入力…",
    menu_chat: "💬 話しかける (ダブルクリック)",
    tray_chat: "💬 話しかける",
    menu_settings: "⚙️ 設定パネル...",
    menu_default_skin: "🎨 既定スキンに戻す",
    menu_sec_skins_prefix: "👗 スキン",
    menu_install_nar: "  📁 スキン追加 (.nar / .zip)...",
    menu_sec_shells: "  [キャラクターシェル]",
    tray_sub_shells: "キャラクターシェル",
    menu_sec_balloons: "  [バルーン]",
    tray_sub_balloons: "バルーン",
    menu_sec_bind: "  [着せ替え]",
    tray_sub_bind: "着せ替え",
    menu_sec_voice_prefix: "🎤 音声・声色",
    menu_voice_tts: "🔊 音声読み上げ (TTS)",
    menu_voices_hdr_prefix: "  [声色 · ",
    menu_voice_sakura_auto_prefix: "(\\0) 声: 自動",
    menu_voice_kero_auto_prefix: "(\\1) 声: 自動",
    menu_voice_discovering: "    ⏳ 音声エンジン検出中… (AivisSpeech/VOICEVOX)",
    menu_voice_arch_hdr: "  [\\1 パートナー声タイプ]",
    tray_sub_arch: "\\1 パートナー声タイプ",
    menu_voice_engine_hdr: "  [音声エンジン]",
    tray_sub_engine: "音声エンジン",
    tray_sub_sak_voice: "\\0 声色",
    tray_sub_ker_voice: "\\1 相棒声色",
    menu_aivis_hdr: "  [📥 音声モデル追加 · AivisHub]",
    tray_sub_aivis: "📥 音声モデル追加 (AivisHub)",
    suffix_installed: " [導入済]",
    menu_sec_ai_prefix: "🤖 AI エンジン",
    menu_setup_local_ai: "  ⚡ 無料ローカルAIを1クリック自動準備 (Ollama)",
    menu_sec_tools: "🛠 ツール・言語",
    menu_hourly_chime: "🕒 時報 (毎時00分)",
    menu_l2d_studio: "🛠 Live2D スタジオを開く",
    menu_l2d_folder: "📂 Live2D サイドカーフォルダ",
    menu_check_updates: "🔄 更新確認",
    menu_clear_history: "🧹 履歴クリア",
    tray_sec_view_tools: "✨ 表示・言語・ツール",
    menu_kero: "パートナー (\\1)",
    menu_always_on_top: "常に手前",
    menu_live2d_motion: "✨ Live2D モーション",
    menu_click_through: "👻 すり抜けモード (クリック透過)",
    menu_reset_pos: "🧭 位置リセット",
    menu_open_cfg_folder: "📂 設定フォルダ (~/.nanikaai)",
    menu_quit: "❌ 終了",
    voice_auto_prefix: "自動",
    status_thinking: "AI 応答準備中…",
    status_local_ai_dry_run: "⚡ 無料ローカルAI 自動準備 (dry-run)",
    status_local_ai_busy: "⏳ 現在無料ローカルAIを準備中だよ…",
    status_local_ai_checking: "🔍 PCスペック・Ollama 導入状態を確認中…",
    status_clear_interests: "🧹 記憶した関心事をリセットしたよ",
    status_link_opened_prefix: "🔗 リンクを開きました: ",
    status_url_blocked_prefix: "⚠️ URLブロック: ",
    status_checking_updates: "GitHub リリース確認中…",
    status_tts_preparing: "音声エンジン準備中 (AivisSpeech / VOICEVOX / OS)…",
    hdr_focus_accessories: "集中・おやすみ・便利アクセサリ",
    tog_quiet_mode: "🌙 おやすみモード (音声・雑談・時報を一括静音)",
    tog_auto_hide_fs: "🖥 フルスクリーンアプリ起動時に自動退避",
    tog_autostart: "🚀 OS ログイン時に自動起動",
    tog_low_batt_alert: "🪫 低バッテリー注意お知らせ",
    lbl_pomodoro_preset: "🍅 集中タイマー",
    lbl_power_save: "🔋 省電力モード",
    btn_hide_chars: "🙈 一時非表示",
    btn_show_chars: "👁 キャラ再表示",
    btn_next_monitor: "🖥 モニタ/端移動",
    btn_pomo_stop: "⏹ 停止/リセット",
    btn_show_backlog: "📜 発話バックログ",
    btn_copy_last: "📋 最新回答コピー",
    menu_quiet_mode: "🌙 おやすみモード (静音)",
    menu_hide_chars: "🙈 キャラ一時非表示",
    menu_show_chars: "👁 キャラを表示",
    menu_pomo_stop: "⏹ ポモドーロ停止",
    menu_show_backlog: "📜 発話バックログを表示",
    menu_copy_last: "📋 最新の回答をコピー",
    menu_next_monitor: "🖥 次モニタ / 画面端へ移動",
    status_quiet_on: "🌙 おやすみモード ON (音声・雑談をミュートしたよ)",
    status_quiet_off: "☀️ おやすみモード解除 (通常運転に戻ったよ)",
    status_chars_hidden: "🙈 キャラを一時非表示にしました (トレイから復帰可)",
    status_chars_shown: "👁 キャラクターを表示しました",
    status_copied_reply: "📋 最新の回答をクリップボードへコピーしたよ",
    status_copy_empty: "ℹ️ コピーできる発話履歴がまだないよ",
    status_autostart_on: "🚀 OS ログイン時の自動起動を登録したよ",
    status_autostart_off: "🚀 OS ログイン時の自動起動を解除したよ",
    status_autostart_err: "⚠️ OS 自動起動の更新に失敗: ",
    status_monitor_moved_prefix: "🖥 モニタ ",
    status_dock_edge_left: "🖥 画面左下へ移動したよ",
    status_dock_edge_right: "🖥 画面右下へ移動したよ",
    status_power_save_prefix: "🔋 省電力モード: ",
    menu_ask_clipboard: "📋 クリップボードを質問・翻訳",
    menu_quick_memos: "📝 クイックメモを見る",
    menu_test_sakura_voice: "  ▶ (\\0) さくら声テスト発話",
    menu_test_kero_voice: "  ▶ (\\1) 相棒声テスト発話",
    menu_manage_voices: "  ⚙️ 音声モデルの追加・削除・詳細設定...",
    menu_pomo_skip: "  ⏭ 次のフェーズへスキップ",
    btn_pomo_skip: "⏭ スキップ",
    sld_voice_speed_title: "話す速さ (0.75x-1.35x)",
    sld_voice_speed_sub: "自然な会話速度の調整",
    sld_voice_inton_title: "抑揚・自然さ (0.70-1.50)",
    sld_voice_inton_sub: "AivisSpeech 抑揚＆テンポ動的変化",
    btn_preview_sakura: "▶ さくら試聴",
    btn_preview_kero: "▶ 相棒試聴",
    hdr_installed_models: "導入済み音声モデル (不要なモデルを削除可能)",
    hdr_recommended_voices: "おすすめの自然な声を追加 (AivisHub)",
    badge_builtin: "標準",
    btn_delete_model: "✕ 削除",
    btn_install_model: "⬇ 追加",
    lbl_no_aivm_models: "AivisSpeech 接続時に一覧表示されます",
    menu_calendar_agenda: "📅 今日の予定・出発目安を見る",
    sec_calendar_hdr: "カレンダー連携（Google / OS標準 / iCal / Yahoo!）",
    tgl_cal_enabled: "📅 予定リマインド＆出発前通知",
    tgl_cal_os_native: "🖥 OS標準カレンダーを自動読込",
    cyc_cal_briefing: "⏰ 定期リマインド間隔",
    cyc_cal_travel: "🚃 基本移動時間の目安",
    btn_cal_paste_ics: "📋 ICS / webcal URL貼付",
    btn_cal_sync_now: "🔄 今すぐ同期",
    btn_cal_show_agenda: "📅 予定を見る",
    btn_attach_image: "🖼 画像",
};

static STRINGS_KO: UiStrings = UiStrings {
    settings_title: "⚙️ 나니카아이 설정 패널 · Settings",
    tab_appearance: "🎨 외관·투명",
    tab_os: "🖥 OS·기능",
    tab_voice: "🎤 음성·목소리",
    tab_ai: "🤖 AI·저장소",
    hdr_opacity_scale: "투명도·표시 배율 · Opacity & Scale",
    sld_skin_opacity_title: "캐릭터 투명도 (15%-100%)",
    sld_skin_opacity_sub: "Live2D 24레이어·클래식 공통 / 내부 투과 방지",
    sld_balloon_opacity_title: "말풍선 투명도 (25%-100%)",
    sld_balloon_opacity_sub: "대화 말풍선 창 전체의 투명율",
    sld_scale_title: "표시 크기 배율 (50%-150%)",
    sld_scale_sub: "사쿠라·우뉴 공통 표시 배율",
    hdr_motion_skins: "모션·스킨 선택 · Motion & Skins",
    tog_opaque_hover: "마우스 호버 시 100% 불투명",
    tog_live2d: "✨ Live2D 모션·물리 흔들림",
    tog_eye_track: "👀 커서 시선 추적",
    tog_kero: "🐣 파트너 (\\1) 표시",
    lbl_shell: "셸 · Shell",
    lbl_balloon: "말풍선 · Balloon",
    hdr_desktop_behavior: "언어·데스크톱 공통 편의 기능",
    lbl_ui_language: "🌐 UI 언어",
    tog_always_on_top: "📌 항상 위에 표시",
    tog_click_through: "👻 클릭 통과 모드 (트레이/설정으로 해제)",
    tog_lock_pos: "🔒 드래그 위치 잠금",
    tog_snap_bottom: "🧲 화면 하단 자동 흡착",
    tog_hourly_chime: "🕒 매시 정각 시보 알림",
    btn_reset_pos: "🧭 캐릭터 위치 기본값으로 초기화",
    os_hdr_macos_cur: "🍎 macOS 동작 설정 (● 현재 OS)",
    os_hdr_macos_shared: "🍎 macOS 동작 설정 (OS 간 공유 저장)",
    os_hdr_win_cur: "🪟 Windows 동작 설정 (● 현재 OS)",
    os_hdr_win_shared: "🪟 Windows 동작 설정 (OS 간 공유 저장)",
    os_hdr_linux_cur: "🐧 Linux 동작 설정 (● 현재 OS)",
    os_hdr_linux_shared: "🐧 Linux 동작 설정 (OS 간 공유 저장)",
    tog_mac_first_mouse: "비활성 상태 1클릭 반응 (재시작 시 반영)",
    tog_mac_shadow: "창에 macOS 그림자 표시",
    tog_mac_dock: "Dock에 앱 아이콘 표시 (재시작 필요)",
    tog_win_skip_taskbar: "작업 표시줄에서 캐릭터 숨기기",
    tog_win_shadow: "테두리 없는 창 그림자 활성화",
    tog_linux_x11: "GNOME에서 X11 / XWayland 우선 (재시작 필요)",
    tog_linux_workarea: "패널 작업 영역 (_NET_WORKAREA) 자동 회피",
    hdr_voice_tts: "헤드리스 음성·캐릭터 음색 · Voice & TTS",
    tog_voice_tts: "🔊 음성 읽기 (TTS)",
    lbl_tts_engine: "음성 엔진",
    lbl_sakura_voice: "(\\0) 사쿠라 목소리",
    lbl_kero_voice: "(\\1) 파트너 목소리",
    lbl_kero_arch: "(\\1) 목소리 타입",
    lbl_kindle_tts_engine: "📚 Kindle 음성 라이브러리",
    lbl_kindle_voice: "📚 Kindle 음성 모델",
    lbl_note: "안내",
    voice_note_value: "AivisHub 모델 추가는 우클릭 [🎤 음성·목소리]에서 실행",
    hdr_ai_tools: "AI 백엔드 & 대화 도구 · AI & Tools",
    lbl_provider: "AI 프로바이더",
    lbl_model_api: "모델 / API",
    lbl_router_model: "🧭 라우터 (FunctionGemma)",
    lbl_coding_model: "💻 코딩 특화 모델",
    lbl_reasoning_model: "🧠 추론 특화 모델",
    lbl_cloud_fallback: "☁️ 클라우드 승격 / 폴백",
    btn_setup_local_ai: "⚡ 무료 로컬 AI (Ollama+경량 모델) 1클릭 자동 준비",
    btn_setup_local_ai_busy: "⏳ 로컬 AI 자동 설치 진행 중...",
    lbl_best_free_ai: "추천 무료 AI",
    hdr_autotalk_news: "잡담 토크 & 관심 뉴스 · Auto-Talk & News",
    sld_autotalk_title: "잡담(자동 토크) 간격",
    sld_autotalk_sub: "좌측 끝 OFF / 최저 30초~600초",
    sld_weight_title: "관심 뉴스 비중",
    sld_weight_sub: "일반 뉴스 혼합 (최대 80%)",
    lbl_saved_interests: "기억된 관심사",
    interests_empty: "미등록 (대화에서 관심사를 말하면 자동 기억)",
    btn_clear_interests: "🧹 기억된 관심사 초기화",
    tog_tools: "🧰 AI 자율 도구 실행 (날씨·웹·시각·계산)",
    tog_clipboard: "📋 클립보드 내용 참조 허용",
    hdr_storage: "JSON 설정 저장 위치 · Storage (~/.nanikaai)",
    lbl_json_path: "JSON 경로",
    btn_open_cfg_dir: "📂 설정 폴더 열기",
    btn_clear_hist: "🧹 대화 히스토리 비우기",
    footer_autosave: "💾 ~/.nanikaai/config.json · 즉시 자동 저장",
    btn_reset_defaults: "↺ 기본값 복원",
    comm_header: "나니카아이에게 말 걸기 · Ask NanikaAi",
    comm_placeholder: "메시지를 입력…",
    menu_chat: "💬 대화하기 (더블클릭)",
    tray_chat: "💬 대화하기",
    menu_settings: "⚙️ 설정 패널...",
    menu_default_skin: "🎨 기본 스킨으로 되돌리기",
    menu_sec_skins_prefix: "👗 스킨",
    menu_install_nar: "  📁 스킨 추가 (.nar / .zip)...",
    menu_sec_shells: "  [캐릭터 셸]",
    tray_sub_shells: "캐릭터 셸",
    menu_sec_balloons: "  [말풍선]",
    tray_sub_balloons: "말풍선",
    menu_sec_bind: "  [갈아입히기]",
    tray_sub_bind: "갈아입히기",
    menu_sec_voice_prefix: "🎤 음성·목소리",
    menu_voice_tts: "🔊 음성 읽기 (TTS)",
    menu_voices_hdr_prefix: "  [음색 · ",
    menu_voice_sakura_auto_prefix: "(\\0) 목소리: 자동",
    menu_voice_kero_auto_prefix: "(\\1) 목소리: 자동",
    menu_voice_discovering: "    ⏳ 음성 엔진 검색 중… (AivisSpeech/VOICEVOX)",
    menu_voice_arch_hdr: "  [\\1 파트너 목소리 타입]",
    tray_sub_arch: "\\1 파트너 목소리 타입",
    menu_voice_engine_hdr: "  [음성 엔진]",
    tray_sub_engine: "음성 엔진",
    tray_sub_sak_voice: "\\0 목소리",
    tray_sub_ker_voice: "\\1 파트너 목소리",
    menu_aivis_hdr: "  [📥 음성 모델 추가 · AivisHub]",
    tray_sub_aivis: "📥 음성 모델 추가 (AivisHub)",
    suffix_installed: " [설치됨]",
    menu_sec_ai_prefix: "🤖 AI 엔진",
    menu_setup_local_ai: "  ⚡ 무료 로컬 AI 1클릭 자동 준비 (Ollama)",
    menu_sec_tools: "🛠 도구·언어",
    menu_hourly_chime: "🕒 정각 시보 알림",
    menu_l2d_studio: "🛠 Live2D 스튜디오 열기",
    menu_l2d_folder: "📂 Live2D 사이드카 폴더",
    menu_check_updates: "🔄 업데이트 확인",
    menu_clear_history: "🧹 히스토리 삭제",
    tray_sec_view_tools: "✨ 화면·언어·도구",
    menu_kero: "파트너 (\\1)",
    menu_always_on_top: "항상 위에",
    menu_live2d_motion: "✨ Live2D 모션",
    menu_click_through: "👻 클릭 통과 모드",
    menu_reset_pos: "🧭 위치 초기화",
    menu_open_cfg_folder: "📂 설정 폴더 (~/.nanikaai)",
    menu_quit: "❌ 종료",
    voice_auto_prefix: "자동",
    status_thinking: "AI 응답 준비 중…",
    status_local_ai_dry_run: "⚡ 무료 로컬 AI 자동 준비 (dry-run)",
    status_local_ai_busy: "⏳ 현재 무료 로컬 AI를 준비하고 있어요…",
    status_local_ai_checking: "🔍 PC 사양 및 Ollama 설치 상태 확인 중…",
    status_clear_interests: "🧹 기억된 관심사를 초기화했어요",
    status_link_opened_prefix: "🔗 링크를 열었습니다: ",
    status_url_blocked_prefix: "⚠️ URL 차단됨: ",
    status_checking_updates: "GitHub 릴리스 확인 중…",
    status_tts_preparing: "음성 엔진 준비 중 (AivisSpeech / VOICEVOX / OS)…",
    hdr_focus_accessories: "집중·방해금지·편의 액세서리",
    tog_quiet_mode: "🌙 방해 금지 모드 (음성·잡담·시보 일괄 음소거)",
    tog_auto_hide_fs: "🖥 전체 화면 앱 실행 시 자동 숨김",
    tog_autostart: "🚀 OS 로그인 시 자동 시작",
    tog_low_batt_alert: "🪫 배터리 부족 알림",
    lbl_pomodoro_preset: "🍅 집중 타이머",
    lbl_power_save: "🔋 절전 모드",
    btn_hide_chars: "🙈 잠시 숨기기",
    btn_show_chars: "👁 다시 표시",
    btn_next_monitor: "🖥 모니터/좌우 이동",
    btn_pomo_stop: "⏹ 정지/초기화",
    btn_show_backlog: "📜 발화 백로그",
    btn_copy_last: "📋 최신 응답 복사",
    menu_quiet_mode: "🌙 방해 금지 (음소거)",
    menu_hide_chars: "🙈 캐릭터 숨기기",
    menu_show_chars: "👁 캐릭터 표시",
    menu_pomo_stop: "⏹ 뽀모도로 정지",
    menu_show_backlog: "📜 발화 백로그 보기",
    menu_copy_last: "📋 최신 응답 복사",
    menu_next_monitor: "🖥 다음 모니터 / 구석 이동",
    status_quiet_on: "🌙 방해 금지 모드 ON (음성과 잡담을 멈췄어요)",
    status_quiet_off: "☀️ 방해 금지 해제 (일반 모드로 돌아왔어요)",
    status_chars_hidden: "🙈 캐릭터를 잠시 숨겼어요 (트레이에서 복귀)",
    status_chars_shown: "👁 캐릭터를 다시 표시했어요",
    status_copied_reply: "📋 최신 응답을 클립보드에 복사했어요",
    status_copy_empty: "ℹ️ 복사할 발화 내역이 아직 없어요",
    status_autostart_on: "🚀 OS 로그인 자동 시작을 등록했어요",
    status_autostart_off: "🚀 OS 로그인 자동 시작을 해제했어요",
    status_autostart_err: "⚠️ 자동 시작 설정 실패: ",
    status_monitor_moved_prefix: "🖥 모니터 ",
    status_dock_edge_left: "🖥 화면 좌측 하단으로 이동했어요",
    status_dock_edge_right: "🖥 화면 우측 하단으로 이동했어요",
    status_power_save_prefix: "🔋 절전 모드: ",
    menu_ask_clipboard: "📋 클립보드 질문·번역",
    menu_quick_memos: "📝 퀵 메모 보기",
    menu_test_sakura_voice: "  ▶ (\\0) 사쿠라 목소리 테스트",
    menu_test_kero_voice: "  ▶ (\\1) 파트너 목소리 테스트",
    menu_manage_voices: "  ⚙️ 음성 모델 추가·삭제·상세 설정...",
    menu_pomo_skip: "  ⏭ 다음 단계로 건너뛰기",
    btn_pomo_skip: "⏭ 건너뛰기",
    sld_voice_speed_title: "말하기 속도 (0.75x-1.35x)",
    sld_voice_speed_sub: "자연스러운 대화 속도 조절",
    sld_voice_inton_title: "억양·자연스러움 (0.70-1.50)",
    sld_voice_inton_sub: "AivisSpeech 억양 및 리듬 강약",
    btn_preview_sakura: "▶ 사쿠라 미리듣기",
    btn_preview_kero: "▶ 파트너 미리듣기",
    hdr_installed_models: "설치된 음성 모델 (불필요한 모델 삭제)",
    hdr_recommended_voices: "추천 자연스러운 목소리 추가 (AivisHub)",
    badge_builtin: "기본",
    btn_delete_model: "✕ 삭제",
    btn_install_model: "⬇ 추가",
    lbl_no_aivm_models: "AivisSpeech 연결 시 목록이 표시됩니다",
    menu_calendar_agenda: "📅 오늘 일정·출발시간 보기",
    sec_calendar_hdr: "캘린더 연동 (Google / OS 내장 / iCal / Yahoo!)",
    tgl_cal_enabled: "📅 일정 리마인드 & 이동시간 출발 알림",
    tgl_cal_os_native: "🖥 OS 내장 캘린더 자동 읽기",
    cyc_cal_briefing: "⏰ 정기 리마인드 주기",
    cyc_cal_travel: "🚃 기본 이동시간 여유",
    btn_cal_paste_ics: "📋 ICS / webcal URL 붙여넣기",
    btn_cal_sync_now: "🔄 지금 동기화",
    btn_cal_show_agenda: "📅 일정 보기",
    btn_attach_image: "🖼 사진",
};

static STRINGS_EN: UiStrings = UiStrings {
    settings_title: "⚙️ NanikaAi Settings Panel",
    tab_appearance: "🎨 Look·Alpha",
    tab_os: "🖥 OS·Setup",
    tab_voice: "🎤 Voice·TTS",
    tab_ai: "🤖 AI·Storage",
    hdr_opacity_scale: "Opacity & Display Scale",
    sld_skin_opacity_title: "Character Opacity (15%-100%)",
    sld_skin_opacity_sub: "Applies to 24-layer Live2D & Classic skins",
    sld_balloon_opacity_title: "Balloon Opacity (25%-100%)",
    sld_balloon_opacity_sub: "Transparency of speech balloon windows",
    sld_scale_title: "Character Scale (50%-150%)",
    sld_scale_sub: "Shared scale factor for \\0 and \\1",
    hdr_motion_skins: "Motion & Skins",
    tog_opaque_hover: "100% Opaque on Mouse Hover",
    tog_live2d: "✨ Live2D Motion & Spring Physics",
    tog_eye_track: "👀 Cursor Eye Tracking",
    tog_kero: "🐣 Show Partner (\\1)",
    lbl_shell: "Shell Skin",
    lbl_balloon: "Balloon Skin",
    hdr_desktop_behavior: "UI Language & Desktop Behavior",
    lbl_ui_language: "🌐 UI Language",
    tog_always_on_top: "📌 Always on Top",
    tog_click_through: "👻 Click-Through Mode (Restore via Tray/Settings)",
    tog_lock_pos: "🔒 Lock Window Positions",
    tog_snap_bottom: "🧲 Snap Characters to Screen Bottom",
    tog_hourly_chime: "🕒 Hourly Chime (:00)",
    btn_reset_pos: "🧭 Reset Character Window Positions",
    os_hdr_macos_cur: "🍎 macOS Settings (● Active OS)",
    os_hdr_macos_shared: "🍎 macOS Settings (Cross-OS Synced)",
    os_hdr_win_cur: "🪟 Windows Settings (● Active OS)",
    os_hdr_win_shared: "🪟 Windows Settings (Cross-OS Synced)",
    os_hdr_linux_cur: "🐧 Linux Settings (● Active OS)",
    os_hdr_linux_shared: "🐧 Linux Settings (Cross-OS Synced)",
    tog_mac_first_mouse: "Respond on First Click When Inactive (Restart)",
    tog_mac_shadow: "Enable macOS Window Shadows",
    tog_mac_dock: "Show App Icon in Dock (Restart Required)",
    tog_win_skip_taskbar: "Hide Character Windows from Taskbar",
    tog_win_shadow: "Enable Borderless Window Shadows",
    tog_linux_x11: "Prefer X11 / XWayland on GNOME (Restart Required)",
    tog_linux_workarea: "Respect Panel Work Area (_NET_WORKAREA)",
    hdr_voice_tts: "Headless Voice & Character TTS",
    tog_voice_tts: "🔊 Enable Speech Output (TTS)",
    lbl_tts_engine: "TTS Engine",
    lbl_sakura_voice: "(\\0) Main Voice",
    lbl_kero_voice: "(\\1) Partner Voice",
    lbl_kero_arch: "(\\1) Archetype",
    lbl_kindle_tts_engine: "📚 Kindle Voice Library",
    lbl_kindle_voice: "📚 Kindle Voice Model",
    lbl_note: "Note",
    voice_note_value: "Install AivisHub voices from Right-Click [🎤 Voice]",
    hdr_ai_tools: "AI Backend & Conversation Tools",
    lbl_provider: "AI Provider",
    lbl_model_api: "Model / API",
    lbl_router_model: "🧭 Router (FunctionGemma)",
    lbl_coding_model: "💻 Coding Model",
    lbl_reasoning_model: "🧠 Reasoning Model",
    lbl_cloud_fallback: "☁️ Cloud Escalation / Fallback",
    btn_setup_local_ai: "⚡ 1-Click Free Local AI Setup (Ollama + Light Model)",
    btn_setup_local_ai_busy: "⏳ Setting up Local AI...",
    lbl_best_free_ai: "Best Free AI",
    hdr_autotalk_news: "Auto-Talk & Interest News",
    sld_autotalk_title: "Auto-Talk Interval",
    sld_autotalk_sub: "Left=OFF / Min 30s to 600s",
    sld_weight_title: "Interest News Ratio",
    sld_weight_sub: "Mixed with general news (Max 80%)",
    lbl_saved_interests: "Saved Topics",
    interests_empty: "None yet (chat about hobbies to save)",
    btn_clear_interests: "🧹 Clear Saved Interest Topics",
    tog_tools: "🧰 AI Autonomous Tools (Weather, Web, Time, Math)",
    tog_clipboard: "📋 Allow Clipboard Access Tool",
    hdr_storage: "JSON Config Storage (~/.nanikaai)",
    lbl_json_path: "Config JSON",
    btn_open_cfg_dir: "📂 Open Config Folder",
    btn_clear_hist: "🧹 Clear Chat History",
    footer_autosave: "💾 ~/.nanikaai/config.json · Auto-Saved",
    btn_reset_defaults: "↺ Reset Defaults",
    comm_header: "Ask NanikaAi",
    comm_placeholder: "Type a message…",
    menu_chat: "💬 Talk / Chat (Double-Click)",
    tray_chat: "💬 Talk / Chat",
    menu_settings: "⚙️ Settings Panel...",
    menu_default_skin: "🎨 Reset to Default Skin",
    menu_sec_skins_prefix: "👗 Skins",
    menu_install_nar: "  📁 Add Skin (.nar / .zip)...",
    menu_sec_shells: "  [Character Shells]",
    tray_sub_shells: "Character Shells",
    menu_sec_balloons: "  [Speech Balloons]",
    tray_sub_balloons: "Speech Balloons",
    menu_sec_bind: "  [Dress-up Parts]",
    tray_sub_bind: "Dress-up Parts",
    menu_sec_voice_prefix: "🎤 Voice & TTS",
    menu_voice_tts: "🔊 Enable Voice (TTS)",
    menu_voices_hdr_prefix: "  [Voices · ",
    menu_voice_sakura_auto_prefix: "(\\0) Voice: Auto",
    menu_voice_kero_auto_prefix: "(\\1) Voice: Auto",
    menu_voice_discovering: "    ⏳ Detecting TTS engines… (AivisSpeech/VOICEVOX)",
    menu_voice_arch_hdr: "  [\\1 Partner Voice Archetype]",
    tray_sub_arch: "\\1 Partner Archetype",
    menu_voice_engine_hdr: "  [TTS Engine]",
    tray_sub_engine: "TTS Engine",
    tray_sub_sak_voice: "\\0 Voice",
    tray_sub_ker_voice: "\\1 Partner Voice",
    menu_aivis_hdr: "  [📥 Add Voice Model · AivisHub]",
    tray_sub_aivis: "📥 Add Voice Model (AivisHub)",
    suffix_installed: " [Installed]",
    menu_sec_ai_prefix: "🤖 AI Engine",
    menu_setup_local_ai: "  ⚡ 1-Click Free Local AI Setup (Ollama)",
    menu_sec_tools: "🛠 Tools & Language",
    menu_hourly_chime: "🕒 Hourly Chime (:00)",
    menu_l2d_studio: "🛠 Open Live2D Rig Studio",
    menu_l2d_folder: "📂 Live2D Sidecar Folder",
    menu_check_updates: "🔄 Check Updates",
    menu_clear_history: "🧹 Clear History",
    tray_sec_view_tools: "✨ View, Language & Tools",
    menu_kero: "Partner (\\1)",
    menu_always_on_top: "Always on Top",
    menu_live2d_motion: "✨ Live2D Motion",
    menu_click_through: "👻 Click-Through Mode",
    menu_reset_pos: "🧭 Reset Positions",
    menu_open_cfg_folder: "📂 Config Folder (~/.nanikaai)",
    menu_quit: "❌ Quit",
    voice_auto_prefix: "Auto",
    status_thinking: "Preparing AI response…",
    status_local_ai_dry_run: "⚡ Free Local AI Auto-Setup (dry-run)",
    status_local_ai_busy: "⏳ Free Local AI setup already in progress…",
    status_local_ai_checking: "🔍 Checking PC specs & Ollama status…",
    status_clear_interests: "🧹 Cleared saved interest topics",
    status_link_opened_prefix: "🔗 Opened link: ",
    status_url_blocked_prefix: "⚠️ Blocked URL: ",
    status_checking_updates: "Checking GitHub Releases…",
    status_tts_preparing: "Preparing TTS engine (AivisSpeech / VOICEVOX / OS)…",
    hdr_focus_accessories: "Focus, Quiet & Desktop Utilities",
    tog_quiet_mode: "🌙 Quiet / Do Not Disturb (Mute TTS & Auto-Talk)",
    tog_auto_hide_fs: "🖥 Auto-Hide on Fullscreen App / Game",
    tog_autostart: "🚀 Launch Automatically at OS Login",
    tog_low_batt_alert: "🪫 Low Battery Care Alert",
    lbl_pomodoro_preset: "🍅 Pomodoro Timer",
    lbl_power_save: "🔋 Power Save",
    btn_hide_chars: "🙈 Hide Mascots",
    btn_show_chars: "👁 Show Mascots",
    btn_next_monitor: "🖥 Next Display/Side",
    btn_pomo_stop: "⏹ Stop / Reset",
    btn_show_backlog: "📜 View Backlog",
    btn_copy_last: "📋 Copy Last Reply",
    menu_quiet_mode: "🌙 Quiet Mode (Mute)",
    menu_hide_chars: "🙈 Hide Mascots",
    menu_show_chars: "👁 Show Mascots",
    menu_pomo_stop: "⏹ Stop Pomodoro",
    menu_show_backlog: "📜 Show Dialogue Backlog",
    menu_copy_last: "📋 Copy Latest Reply",
    menu_next_monitor: "🖥 Next Display / Corner",
    status_quiet_on: "🌙 Quiet Mode ON (TTS & Auto-Talk muted)",
    status_quiet_off: "☀️ Quiet Mode OFF (resumed normal activity)",
    status_chars_hidden: "🙈 Mascots hidden (restore via Tray or API)",
    status_chars_shown: "👁 Mascots restored on screen",
    status_copied_reply: "📋 Copied latest reply to clipboard",
    status_copy_empty: "ℹ️ No dialogue reply available to copy yet",
    status_autostart_on: "🚀 Enabled OS login auto-start",
    status_autostart_off: "🚀 Disabled OS login auto-start",
    status_autostart_err: "⚠️ Failed updating auto-start: ",
    status_monitor_moved_prefix: "🖥 Moved to display ",
    status_dock_edge_left: "🖥 Docked to bottom-left corner",
    status_dock_edge_right: "🖥 Docked to bottom-right corner",
    status_power_save_prefix: "🔋 Power-save mode: ",
    menu_ask_clipboard: "📋 Explain / Translate Clipboard",
    menu_quick_memos: "📝 View Quick Memos",
    menu_test_sakura_voice: "  ▶ (\\0) Test Sakura Voice",
    menu_test_kero_voice: "  ▶ (\\1) Test Partner Voice",
    menu_manage_voices: "  ⚙️ Add / Delete Voice Models...",
    menu_pomo_skip: "  ⏭ Skip to Next Phase",
    btn_pomo_skip: "⏭ Skip",
    sld_voice_speed_title: "Voice Speed (0.75x-1.35x)",
    sld_voice_speed_sub: "Adjust conversational speaking pace",
    sld_voice_inton_title: "Expressiveness (0.70-1.50)",
    sld_voice_inton_sub: "AivisSpeech intonation & tempo dynamics",
    btn_preview_sakura: "▶ Test Sakura",
    btn_preview_kero: "▶ Test Partner",
    hdr_installed_models: "Installed Voice Models (Delete Unused)",
    hdr_recommended_voices: "Recommended Natural Voices (AivisHub)",
    badge_builtin: "Built-in",
    btn_delete_model: "✕ Delete",
    btn_install_model: "⬇ Install",
    lbl_no_aivm_models: "Models appear once AivisSpeech is connected",
    menu_calendar_agenda: "📅 Upcoming Schedule & Leave Time",
    sec_calendar_hdr: "Calendar Sync (Google / OS / iCal / Yahoo!)",
    tgl_cal_enabled: "📅 Schedule & Travel Departure Alerts",
    tgl_cal_os_native: "🖥 Read OS Built-in Calendar",
    cyc_cal_briefing: "⏰ Periodic Reminder Interval",
    cyc_cal_travel: "🚃 Default Travel Buffer",
    btn_cal_paste_ics: "📋 Paste ICS / webcal URL",
    btn_cal_sync_now: "🔄 Sync Now",
    btn_cal_show_agenda: "📅 View Schedule",
    btn_attach_image: "🖼 Image",
};

pub fn ui_strings(lang: UiLang) -> &'static UiStrings {
    match lang {
        UiLang::Ja => &STRINGS_JA,
        UiLang::Ko => &STRINGS_KO,
        UiLang::En => &STRINGS_EN,
    }
}

pub fn power_save_label(lang: UiLang, mode: &str) -> &'static str {
    let norm = crate::accessory_utils::normalize_power_save_mode(mode);
    match (lang, norm.as_str()) {
        (UiLang::Ja, "auto") => "自動 (バッテリー省電力)",
        (UiLang::Ko, "auto") => "자동 (배터리 절전)",
        (UiLang::En, "auto") => "Auto (Battery Eco)",
        (UiLang::Ja, "on") => "常に省電力 (15 FPS)",
        (UiLang::Ko, "on") => "항상 절전 (15 FPS)",
        (UiLang::En, "on") => "Always Eco (15 FPS)",
        (UiLang::Ja, _) => "OFF (最大滑らか)",
        (UiLang::Ko, _) => "OFF (최대 부드러움)",
        (UiLang::En, _) => "OFF (Full 50 FPS)",
    }
}

pub fn provider_label(lang: UiLang, id: &str, fallback: &str) -> String {
    let lbl = match (lang, id) {
        (UiLang::Ja, "ollama") => "Ollama ローカルAI (無料)",
        (UiLang::Ja, "gemini-api") => "Gemini API (無料/APIキー)",
        (UiLang::Ja, "claude") => "Claude (API / CLI)",
        (UiLang::Ja, "chatgpt-free") => "ChatGPT / Codex CLI",
        (UiLang::Ja, "gemini-paid") => "Gemini CLI (agy連携)",
        (UiLang::Ja, "openai-compatible") => "OpenAI / 互換 API",
        (UiLang::Ja, "linux-ollama") => "Remote Ollama (遠隔GPU)",
        (UiLang::Ko, "ollama") => "Ollama 로컬 AI (무료)",
        (UiLang::Ko, "gemini-api") => "Gemini API (무료/API 키)",
        (UiLang::Ko, "claude") => "Claude (API / CLI)",
        (UiLang::Ko, "chatgpt-free") => "ChatGPT / Codex CLI",
        (UiLang::Ko, "gemini-paid") => "Gemini CLI (agy 연동)",
        (UiLang::Ko, "openai-compatible") => "OpenAI / 호환 API",
        (UiLang::Ko, "linux-ollama") => "Remote Ollama (원격 GPU)",
        (UiLang::En, "ollama") => "Ollama Local AI (Free)",
        (UiLang::En, "gemini-api") => "Gemini API (Key/Free)",
        (UiLang::En, "claude") => "Claude (API / CLI)",
        (UiLang::En, "chatgpt-free") => "ChatGPT / Codex CLI",
        (UiLang::En, "gemini-paid") => "Gemini CLI (agy)",
        (UiLang::En, "openai-compatible") => "OpenAI / Compatible API",
        (UiLang::En, "linux-ollama") => "Remote Ollama (GPU)",
        _ => return fallback.to_string(),
    };
    lbl.to_string()
}

pub fn tts_engine_label(lang: UiLang, engine_id: &str) -> String {
    match (lang, engine_id) {
        (UiLang::Ja, "inherit") => "標準エンジンに従う".to_string(),
        (UiLang::Ko, "inherit") => "기본 엔진 따름".to_string(),
        (UiLang::En, "inherit") => "Follow Default Engine".to_string(),
        (UiLang::Ja, "auto") => "自動 (Aivis→VV→OS)".to_string(),
        (UiLang::Ko, "auto") => "자동 (Aivis→VV→OS)".to_string(),
        (UiLang::En, "auto") => "Auto (Aivis→VV→OS)".to_string(),
        (_, "aivisspeech") => "AivisSpeech (port 10101)".to_string(),
        (_, "voicevox") => "VOICEVOX (port 50021)".to_string(),
        (_, "coeiroink") => "COEIROINK (port 50032)".to_string(),
        (UiLang::Ja, "os") => "OS 標準音声 (Native)".to_string(),
        (UiLang::Ko, "os") => "OS 기본 음성 (Native)".to_string(),
        (UiLang::En, "os") => "OS Native TTS".to_string(),
        _ => engine_id.to_string(),
    }
}

pub fn kero_archetype_label(lang: UiLang, code: &str) -> &'static str {
    match (lang, code) {
        (UiLang::Ja, "boy") => "少年/中性 (うにゅう標準)",
        (UiLang::Ja, "girl") => "女の子相棒 (女性パートナ)",
        (UiLang::Ja, "male") => "青年/男性 (渋めパートナ)",
        (UiLang::Ko, "boy") => "소년/중성 (우뉴 기본)",
        (UiLang::Ko, "girl") => "여자 파트너 (여성)",
        (UiLang::Ko, "male") => "청년/남성 (차분한 톤)",
        (UiLang::En, "boy") => "Boy / Neutral (Unyu)",
        (UiLang::En, "girl") => "Female Partner",
        (UiLang::En, "male") => "Male Partner",
        _ => "Auto",
    }
}

pub fn format_seconds(lang: UiLang, sec: u64) -> String {
    if sec == 0 {
        return "OFF".to_string();
    }
    match lang {
        UiLang::Ja => format!("{sec}秒"),
        UiLang::Ko => format!("{sec}초"),
        UiLang::En => format!("{sec}s"),
    }
}

pub fn format_menu_autotalk_row(lang: UiLang, sec: u64, is_tray: bool) -> String {
    let val = format_seconds(lang, sec);
    match (lang, is_tray) {
        (UiLang::Ja, false) => format!("  💬 雑談頻度: {val} (最短30秒)"),
        (UiLang::Ja, true) => format!("💬 雑談頻度: {val} (クリックで切替)"),
        (UiLang::Ko, false) => format!("  💬 잡담 빈도: {val} (최저 30초)"),
        (UiLang::Ko, true) => format!("💬 잡담 빈도: {val} (클릭 전환)"),
        (UiLang::En, false) => format!("  💬 Auto-Talk: {val} (Min 30s)"),
        (UiLang::En, true) => format!("💬 Auto-Talk: {val} (Click to cycle)"),
    }
}

pub fn format_autotalk_status(lang: UiLang, sec: u64) -> String {
    match (lang, sec == 0) {
        (UiLang::Ja, true) => "💬 雑談 (自動トーク) を OFF にしたよ".to_string(),
        (UiLang::Ja, false) => format!("💬 雑談頻度を {sec} 秒ごとにしたよ"),
        (UiLang::Ko, true) => "💬 잡담(자동 토크)을 OFF로 전환했어요".to_string(),
        (UiLang::Ko, false) => format!("💬 잡담 빈도를 {sec}초 간격으로 바꿨어요"),
        (UiLang::En, true) => "💬 Auto-Talk turned OFF".to_string(),
        (UiLang::En, false) => format!("💬 Auto-Talk set to every {sec}s"),
    }
}

pub fn format_menu_ui_lang_row(lang: UiLang, setting: &str, is_tray: bool) -> String {
    let label = format_ui_language_label(setting, lang);
    let prefix = if is_tray { "" } else { "  " };
    match lang {
        UiLang::Ja => format!("{prefix}🌐 UI言語: {label}"),
        UiLang::Ko => format!("{prefix}🌐 UI 언어: {label}"),
        UiLang::En => format!("{prefix}🌐 UI Language: {label}"),
    }
}

pub fn format_ui_lang_status(setting: &str, effective: UiLang) -> String {
    let label = format_ui_language_label(setting, effective);
    match effective {
        UiLang::Ja => format!("🌐 UI 言語を「{label}」に切り替えたよ"),
        UiLang::Ko => format!("🌐 UI 언어를 「{label}」로 전환했어요"),
        UiLang::En => format!("🌐 UI language switched to {label}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_mapping_and_english_fallback() {
        assert_eq!(parse_locale_to_ui_lang("ja-JP"), UiLang::Ja);
        assert_eq!(parse_locale_to_ui_lang("ja_JP.UTF-8"), UiLang::Ja);
        assert_eq!(parse_locale_to_ui_lang("ko-KR"), UiLang::Ko);
        assert_eq!(parse_locale_to_ui_lang("ko_KR.UTF-8"), UiLang::Ko);
        assert_eq!(parse_locale_to_ui_lang("en-US"), UiLang::En);
        // Unsupported / POSIX / empty locales must fall back cleanly to English
        for unsupported in [
            "fr-FR",
            "de_DE.UTF-8",
            "es-ES",
            "zh-CN",
            "ru_RU",
            "C.UTF-8",
            "POSIX",
            "",
        ] {
            assert_eq!(
                parse_locale_to_ui_lang(unsupported),
                UiLang::En,
                "Expected English fallback for unsupported locale `{unsupported}`"
            );
            assert_eq!(resolve_ui_lang_with_os("auto", unsupported), UiLang::En);
        }
    }

    #[test]
    fn explicit_ui_language_overrides_os_locale() {
        assert_eq!(resolve_ui_lang_with_os("ko", "ja-JP"), UiLang::Ko);
        assert_eq!(resolve_ui_lang_with_os("ja", "en-US"), UiLang::Ja);
        assert_eq!(resolve_ui_lang_with_os("en", "ko-KR"), UiLang::En);
        assert_eq!(resolve_ui_lang_with_os("invalid", "fr-FR"), UiLang::En);
        assert_eq!(normalize_ui_language("KO"), "ko");
        assert_eq!(normalize_ui_language("unknown"), "auto");
        assert!(is_valid_ui_language_setting("auto"));
        assert!(!is_valid_ui_language_setting("fr"));
    }

    #[test]
    fn ui_strings_populated_for_all_languages() {
        for lang in [UiLang::Ja, UiLang::Ko, UiLang::En] {
            let s = ui_strings(lang);
            assert!(!s.settings_title.is_empty());
            assert!(!s.tab_appearance.is_empty());
            assert!(!s.tab_os.is_empty());
            assert!(!s.tab_voice.is_empty());
            assert!(!s.tab_ai.is_empty());
            assert!(!s.btn_reset_defaults.is_empty());
            assert!(!s.status_thinking.is_empty());
        }
    }
}
