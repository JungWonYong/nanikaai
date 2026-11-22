use crate::dialogue_parser::{ActionKind, DialogueAction, Scope};
use crate::i18n::UiLang;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const POWER_SAVE_CHOICES: &[&str] = &["auto", "on", "off"];
pub const POMODORO_PRESETS: &[(u32, u32)] = &[(25, 5), (50, 10), (15, 3), (45, 15), (52, 17)];
pub const POMODORO_LONG_BREAK_MINS: &[u32] = &[15, 20, 10, 30, 25];
pub const POMODORO_LONG_BREAK_EVERY: u32 = 4;
pub const MAX_BACKLOG_ENTRIES: usize = 30;

pub fn normalize_power_save_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "eco" | "always" => "on".to_string(),
        "off" | "full" | "disabled" => "off".to_string(),
        _ => "auto".to_string(),
    }
}

pub fn is_valid_power_save_mode(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "auto" | "on" | "off"
    )
}

pub fn effective_poll_interval_ms(
    live2d_on: bool,
    interactive_ui_open: bool,
    hidden: bool,
    power_save_mode: &str,
    on_battery: bool,
    idle_poll_ms: u64,
) -> u64 {
    if hidden {
        return 250;
    }
    if interactive_ui_open {
        return 20;
    }
    let norm = normalize_power_save_mode(power_save_mode);
    if norm == "on" || (norm == "auto" && on_battery) {
        return 66;
    }
    if live2d_on {
        20
    } else {
        idle_poll_ms
    }
}

/// Parse `defaults read com.apple.spaces` output and check if any active `"Current Space"`
/// block has `type = 4` (macOS native fullscreen Space). Inactive historical spaces under
/// `Spaces = ( ... )` are ignored.
pub fn parse_macos_spaces_fullscreen(raw: &str) -> bool {
    let mut in_current_space = false;
    let mut brace_depth: usize = 0;

    for raw_line in raw.lines() {
        let trimmed = raw_line.trim();
        if !in_current_space {
            if let Some(idx) = trimmed.find("\"Current Space\"") {
                let after = &trimmed[idx + "\"Current Space\"".len()..];
                if after.contains('=') && after.contains('{') {
                    in_current_space = true;
                    if let Some((_, rhs)) = after.split_once("type") {
                        let rhs_t = rhs.trim_start_matches([' ', '=', ';']).trim();
                        if rhs_t.starts_with('4') {
                            return true;
                        }
                    }
                    let opens = after.matches('{').count();
                    let closes = after.matches('}').count();
                    if closes >= opens {
                        in_current_space = false;
                        brace_depth = 0;
                    } else {
                        brace_depth = opens - closes;
                    }
                }
            }
            continue;
        }

        if brace_depth == 1 {
            if let Some((lhs, rhs)) = trimmed.split_once('=') {
                if lhs.trim() == "type" {
                    let val = rhs.trim().trim_end_matches(';').trim();
                    if val == "4" {
                        return true;
                    }
                }
            }
        }

        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        brace_depth = brace_depth.saturating_add(opens);
        if closes >= brace_depth {
            in_current_space = false;
            brace_depth = 0;
        } else {
            brace_depth -= closes;
        }
    }
    false
}

pub fn parse_xprop_active_window_id(raw: &str) -> Option<String> {
    for token in raw.split_whitespace() {
        let clean = token.trim_matches(&[',', ';', '"'][..]);
        if let Some(hex) = clean.strip_prefix("0x") {
            if !hex.is_empty()
                && hex.chars().all(|c| c.is_ascii_hexdigit())
                && hex.chars().any(|c| c != '0')
            {
                return Some(clean.to_string());
            }
        }
    }
    None
}

pub fn parse_xprop_wm_state_is_fullscreen(raw: &str) -> bool {
    raw.contains("_NET_WM_STATE_FULLSCREEN")
}

pub fn parse_windows_quns_is_fullscreen(raw: &str) -> bool {
    raw.trim()
        .parse::<u32>()
        .map(|v| matches!(v, 2 | 3 | 4))
        .unwrap_or(false)
}

pub fn probe_fullscreen_app_active() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("defaults")
            .args(["read", "com.apple.spaces"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if out.status.success() {
                let txt = String::from_utf8_lossy(&out.stdout);
                return parse_macos_spaces_fullscreen(&txt);
            }
        }
        false
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const PS_QUNS: &str = "$sig='[DllImport(\"shell32.dll\")] public static extern int SHQueryUserNotificationState(out int s);';\
            $t=Add-Type -MemberDefinition $sig -Name WinQuns -Namespace NanikaAi -PassThru;\
            $s=0; if($t::SHQueryUserNotificationState([ref]$s) -eq 0){ $s } else { 0 }";
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", PS_QUNS])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
        {
            if out.status.success() {
                let txt = String::from_utf8_lossy(&out.stdout);
                return parse_windows_quns_is_fullscreen(&txt);
            }
        }
        false
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let Ok(out) = std::process::Command::new("xprop")
            .args(["-root", "_NET_ACTIVE_WINDOW"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        else {
            return false;
        };
        if !out.status.success() {
            return false;
        }
        let Some(wid) = parse_xprop_active_window_id(&String::from_utf8_lossy(&out.stdout)) else {
            return false;
        };
        if let Ok(st) = std::process::Command::new("xprop")
            .args(["-id", &wid, "_NET_WM_STATE"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if st.status.success() {
                return parse_xprop_wm_state_is_fullscreen(&String::from_utf8_lossy(&st.stdout));
            }
        }
        false
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatteryStatus {
    pub on_battery: bool,
    pub percent: Option<u8>,
}

fn extract_first_percent(line: &str) -> Option<u8> {
    for seg in line.split('%').take(1) {
        let digits: String = seg
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !digits.is_empty() && seg.len() != line.len() {
            if let Ok(val) = digits.parse::<u16>() {
                return Some(val.min(100) as u8);
            }
        }
    }
    None
}

pub fn parse_macos_pmset_battery(raw: &str) -> BatteryStatus {
    let on_battery = raw.contains("'Battery Power'");
    let percent = raw
        .lines()
        .find(|l| l.contains("InternalBattery"))
        .and_then(extract_first_percent);
    BatteryStatus {
        on_battery,
        percent,
    }
}

pub fn parse_linux_sys_battery(status_txt: &str, capacity_txt: &str) -> BatteryStatus {
    let on_battery = status_txt.trim().eq_ignore_ascii_case("Discharging");
    let percent = capacity_txt
        .trim()
        .parse::<u16>()
        .ok()
        .map(|v| v.min(100) as u8);
    BatteryStatus {
        on_battery,
        percent,
    }
}

pub fn parse_windows_battery_line(raw: &str) -> BatteryStatus {
    let trimmed = raw.trim();
    let Some((on_str, pct_str)) = trimmed.split_once('|') else {
        return BatteryStatus::default();
    };
    let on_battery = matches!(on_str.trim(), "1" | "true" | "True");
    let percent = pct_str
        .trim()
        .parse::<u16>()
        .ok()
        .map(|v| v.min(100) as u8);
    BatteryStatus {
        on_battery,
        percent,
    }
}

pub fn probe_battery_status() -> BatteryStatus {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("pmset")
            .args(["-g", "batt"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if out.status.success() {
                return parse_macos_pmset_battery(&String::from_utf8_lossy(&out.stdout));
            }
        }
        BatteryStatus::default()
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const PS_BATT: &str = "Add-Type -AssemblyName System.Windows.Forms;\
            $ps=[System.Windows.Forms.SystemInformation]::PowerStatus;\
            $on=if($ps.PowerLineStatus -eq [System.Windows.Forms.PowerLineStatus]::Offline){1}else{0};\
            $pct=if(($ps.BatteryChargeStatus -band 128) -eq 0 -and $ps.BatteryLifePercent -ge 0 -and $ps.BatteryLifePercent -le 1){[math]::Round($ps.BatteryLifePercent*100)}else{''};\
            \"$on|$pct\"";
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", PS_BATT])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
        {
            if out.status.success() {
                return parse_windows_battery_line(&String::from_utf8_lossy(&out.stdout));
            }
        }
        BatteryStatus::default()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let base = Path::new("/sys/class/power_supply");
        if let Ok(rd) = fs::read_dir(base) {
            let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !name.starts_with("BAT") {
                    continue;
                }
                let st = fs::read_to_string(p.join("status")).unwrap_or_default();
                let cap = fs::read_to_string(p.join("capacity")).unwrap_or_default();
                return parse_linux_sys_battery(&st, &cap);
            }
        }
        BatteryStatus::default()
    }
}

pub fn build_low_battery_script(percent: u8, lang: UiLang) -> String {
    match lang {
        UiLang::Ja => format!(
            "\\0\\s[1]ねぇ、ノートPCのバッテリーが残り {percent}% になってるよ！省電力で動きを抑えつつ待ってるね。\\1\\s[11]電源ケーブルつなぐん忘れんようにな！\\e"
        ),
        UiLang::Ko => format!(
            "\\0\\s[1]저기, 배터리 잔량이 {percent}%밖에 안 남았어요! 절전 모드로 줄여서 대기하고 있을게요.\\1\\s[11]충전기 꽂는 거 잊지 마라!\\e"
        ),
        UiLang::En => format!(
            "\\0\\s[1]Heads up — battery is down to {percent}%! I'll keep animation light to save power.\\1\\s[11]Better plug in your charger soon!\\e"
        ),
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PomodoroPhase {
    Idle,
    Focus,
    Break,
}

impl PomodoroPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            PomodoroPhase::Idle => "idle",
            PomodoroPhase::Focus => "focus",
            PomodoroPhase::Break => "break",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PomodoroTransition {
    FocusCompleted { cycles: u32, next_break_mins: u32 },
    BreakCompleted { cycles: u32, next_work_mins: u32 },
}

#[derive(Clone, Debug)]
pub struct PomodoroTimer {
    pub phase: PomodoroPhase,
    pub paused: bool,
    pub preset_idx: usize,
    pub work_mins: u32,
    pub break_mins: u32,
    pub remaining_secs: u64,
    pub completed_cycles: u32,
    pub total_focus_mins: u32,
    pub long_break_mins: u32,
    pub last_tick_at: Option<Instant>,
}

impl Default for PomodoroTimer {
    fn default() -> Self {
        Self::new(0)
    }
}

impl PomodoroTimer {
    pub fn new(preset_idx: usize) -> Self {
        let idx = preset_idx.min(POMODORO_PRESETS.len().saturating_sub(1));
        let (work_mins, break_mins) = POMODORO_PRESETS[idx];
        let long_break_mins = POMODORO_LONG_BREAK_MINS.get(idx).copied().unwrap_or(15);
        Self {
            phase: PomodoroPhase::Idle,
            paused: false,
            preset_idx: idx,
            work_mins,
            break_mins,
            remaining_secs: (work_mins as u64) * 60,
            completed_cycles: 0,
            total_focus_mins: 0,
            long_break_mins,
            last_tick_at: None,
        }
    }

    pub fn set_preset(&mut self, idx: usize) {
        let clamped = idx.min(POMODORO_PRESETS.len().saturating_sub(1));
        let (w, b) = POMODORO_PRESETS[clamped];
        self.preset_idx = clamped;
        self.work_mins = w;
        self.break_mins = b;
        self.long_break_mins = POMODORO_LONG_BREAK_MINS.get(clamped).copied().unwrap_or(15);
        if self.phase == PomodoroPhase::Idle {
            self.remaining_secs = (w as u64) * 60;
        }
    }

    pub fn start_focus(&mut self) {
        self.phase = PomodoroPhase::Focus;
        self.paused = false;
        self.remaining_secs = (self.work_mins as u64) * 60;
        self.last_tick_at = Some(Instant::now());
    }

    pub fn toggle_start_pause(&mut self) {
        match self.phase {
            PomodoroPhase::Idle => self.start_focus(),
            PomodoroPhase::Focus | PomodoroPhase::Break => {
                self.paused = !self.paused;
                self.last_tick_at = if self.paused {
                    None
                } else {
                    Some(Instant::now())
                };
            }
        }
    }

    pub fn stop(&mut self) {
        self.phase = PomodoroPhase::Idle;
        self.paused = false;
        self.remaining_secs = (self.work_mins as u64) * 60;
        self.last_tick_at = None;
    }

    pub fn skip_phase(&mut self) -> Option<PomodoroTransition> {
        match self.phase {
            PomodoroPhase::Idle => None,
            PomodoroPhase::Focus => {
                let rem = self.remaining_secs.max(1);
                self.advance_secs(rem)
            }
            PomodoroPhase::Break => {
                let rem = self.remaining_secs.max(1);
                self.advance_secs(rem)
            }
        }
    }

    pub fn is_focusing(&self) -> bool {
        self.phase == PomodoroPhase::Focus && !self.paused
    }

    pub fn is_active(&self) -> bool {
        self.phase != PomodoroPhase::Idle
    }

    pub fn advance_secs(&mut self, elapsed: u64) -> Option<PomodoroTransition> {
        if self.phase == PomodoroPhase::Idle || self.paused || elapsed == 0 {
            return None;
        }
        if elapsed < self.remaining_secs {
            self.remaining_secs -= elapsed;
            return None;
        }
        match self.phase {
            PomodoroPhase::Idle => None,
            PomodoroPhase::Focus => {
                self.completed_cycles = self.completed_cycles.saturating_add(1);
                self.total_focus_mins = self.total_focus_mins.saturating_add(self.work_mins);
                let next_break = if self.completed_cycles % POMODORO_LONG_BREAK_EVERY == 0 {
                    self.long_break_mins
                } else {
                    self.break_mins
                };
                self.phase = PomodoroPhase::Break;
                self.paused = false;
                self.remaining_secs = (next_break as u64) * 60;
                Some(PomodoroTransition::FocusCompleted {
                    cycles: self.completed_cycles,
                    next_break_mins: next_break,
                })
            }
            PomodoroPhase::Break => {
                self.phase = PomodoroPhase::Idle;
                self.paused = false;
                self.remaining_secs = (self.work_mins as u64) * 60;
                self.last_tick_at = None;
                Some(PomodoroTransition::BreakCompleted {
                    cycles: self.completed_cycles,
                    next_work_mins: self.work_mins,
                })
            }
        }
    }

    pub fn tick(&mut self) -> Option<PomodoroTransition> {
        if self.phase == PomodoroPhase::Idle || self.paused {
            self.last_tick_at = None;
            return None;
        }
        let now = Instant::now();
        let prev = match self.last_tick_at {
            Some(t) => t,
            None => {
                self.last_tick_at = Some(now);
                return None;
            }
        };
        let elapsed = now.duration_since(prev).as_secs();
        if elapsed == 0 {
            return None;
        }
        self.last_tick_at = Some(now);
        self.advance_secs(elapsed)
    }

    pub fn format_mm_ss(&self) -> String {
        let mins = self.remaining_secs / 60;
        let secs = self.remaining_secs % 60;
        format!("{mins:02}:{secs:02}")
    }

    pub fn format_preset_label(&self, lang: UiLang) -> String {
        let (w, b, lb) = (self.work_mins, self.break_mins, self.long_break_mins);
        match lang {
            UiLang::Ja => format!("{w}分集中 / {b}分休憩 (長{lb}分)"),
            UiLang::Ko => format!("{w}분 집중 / {b}분 휴식 (긴 {lb}분)"),
            UiLang::En => format!("{w}m Focus / {b}m Break (Long {lb}m)"),
        }
    }

    pub fn format_button_label(&self, lang: UiLang) -> String {
        let mm_ss = self.format_mm_ss();
        match (self.phase, self.paused, lang) {
            (PomodoroPhase::Idle, _, UiLang::Ja) => {
                format!("🍅 集中開始 ({}分)", self.work_mins)
            }
            (PomodoroPhase::Idle, _, UiLang::Ko) => {
                format!("🍅 집중 시작 ({}분)", self.work_mins)
            }
            (PomodoroPhase::Idle, _, UiLang::En) => {
                format!("🍅 Focus ({}m)", self.work_mins)
            }
            (PomodoroPhase::Focus, false, UiLang::Ja) => format!("⏸ 集中 {mm_ss}"),
            (PomodoroPhase::Focus, false, UiLang::Ko) => format!("⏸ 집중 {mm_ss}"),
            (PomodoroPhase::Focus, false, UiLang::En) => format!("⏸ Focus {mm_ss}"),
            (PomodoroPhase::Focus, true, UiLang::Ja) => format!("▶ 再開 ({mm_ss})"),
            (PomodoroPhase::Focus, true, UiLang::Ko) => format!("▶ 재개 ({mm_ss})"),
            (PomodoroPhase::Focus, true, UiLang::En) => format!("▶ Resume ({mm_ss})"),
            (PomodoroPhase::Break, false, UiLang::Ja) => format!("⏸ 休憩 {mm_ss}"),
            (PomodoroPhase::Break, false, UiLang::Ko) => format!("⏸ 휴식 {mm_ss}"),
            (PomodoroPhase::Break, false, UiLang::En) => format!("⏸ Break {mm_ss}"),
            (PomodoroPhase::Break, true, UiLang::Ja) => format!("▶ 休憩再開 ({mm_ss})"),
            (PomodoroPhase::Break, true, UiLang::Ko) => format!("▶ 휴식 재개 ({mm_ss})"),
            (PomodoroPhase::Break, true, UiLang::En) => format!("▶ Resume ({mm_ss})"),
        }
    }

    pub fn format_menu_label(&self, lang: UiLang, is_tray: bool) -> String {
        let prefix = if is_tray { "" } else { "  " };
        let mm_ss = self.format_mm_ss();
        let c = self.completed_cycles;
        match (self.phase, self.paused, lang) {
            (PomodoroPhase::Idle, _, UiLang::Ja) => {
                format!("{prefix}🍅 ポモドーロ集中 ({}分 · 完了{c})", self.work_mins)
            }
            (PomodoroPhase::Idle, _, UiLang::Ko) => {
                format!("{prefix}🍅 뽀모도로 집중 ({}분 · 완료 {c})", self.work_mins)
            }
            (PomodoroPhase::Idle, _, UiLang::En) => {
                format!("{prefix}🍅 Start Pomodoro ({}m · #{c})", self.work_mins)
            }
            (PomodoroPhase::Focus, false, UiLang::Ja) => {
                format!("{prefix}🍅 集中中 {mm_ss} (一時停止)")
            }
            (PomodoroPhase::Focus, false, UiLang::Ko) => {
                format!("{prefix}🍅 집중 중 {mm_ss} (일시정지)")
            }
            (PomodoroPhase::Focus, false, UiLang::En) => {
                format!("{prefix}🍅 Focusing {mm_ss} (Pause)")
            }
            (PomodoroPhase::Focus, true, UiLang::Ja) => {
                format!("{prefix}🍅 集中停止中 {mm_ss} (再開)")
            }
            (PomodoroPhase::Focus, true, UiLang::Ko) => {
                format!("{prefix}🍅 집중 정지됨 {mm_ss} (재개)")
            }
            (PomodoroPhase::Focus, true, UiLang::En) => {
                format!("{prefix}🍅 Paused {mm_ss} (Resume)")
            }
            (PomodoroPhase::Break, false, UiLang::Ja) => {
                format!("{prefix}☕ 休憩中 {mm_ss} (一時停止)")
            }
            (PomodoroPhase::Break, false, UiLang::Ko) => {
                format!("{prefix}☕ 휴식 중 {mm_ss} (일시정지)")
            }
            (PomodoroPhase::Break, false, UiLang::En) => {
                format!("{prefix}☕ Break {mm_ss} (Pause)")
            }
            (PomodoroPhase::Break, true, UiLang::Ja) => {
                format!("{prefix}☕ 休憩停止中 {mm_ss} (再開)")
            }
            (PomodoroPhase::Break, true, UiLang::Ko) => {
                format!("{prefix}☕ 휴식 정지됨 {mm_ss} (재개)")
            }
            (PomodoroPhase::Break, true, UiLang::En) => {
                format!("{prefix}☕ Break Paused {mm_ss} (Resume)")
            }
        }
    }

    pub fn snapshot_json(&self) -> Value {
        json!({
            "phase": self.phase.as_str(),
            "paused": self.paused,
            "preset_idx": self.preset_idx,
            "work_mins": self.work_mins,
            "break_mins": self.break_mins,
            "long_break_mins": self.long_break_mins,
            "remaining_secs": self.remaining_secs,
            "remaining_mm_ss": self.format_mm_ss(),
            "completed_cycles": self.completed_cycles,
            "total_focus_mins": self.total_focus_mins,
        })
    }
}

pub fn build_pomodoro_transition_script(trans: PomodoroTransition, lang: UiLang) -> String {
    match (trans, lang) {
        (
            PomodoroTransition::FocusCompleted {
                cycles,
                next_break_mins,
            },
            UiLang::Ja,
        ) => format!(
            "\\0\\s[5]お疲れさま！{cycles}セット目の集中が完了したよ！\\n{next_break_mins}分だけ目を閉じてストレッチしよっか。\\1\\s[12]よう頑張ったな！水も飲んどきや！\\e"
        ),
        (
            PomodoroTransition::FocusCompleted {
                cycles,
                next_break_mins,
            },
            UiLang::Ko,
        ) => format!(
            "\\0\\s[5]수고했어요! {cycles}번째 집중 타임이 끝났어요!\\n{next_break_mins}분 동안 가볍게 스트레칭하면서 쉬자. \\1\\s[12]고생 많았데이! 물 한 잔 마셔라!\\e"
        ),
        (
            PomodoroTransition::FocusCompleted {
                cycles,
                next_break_mins,
            },
            UiLang::En,
        ) => format!(
            "\\0\\s[5]Great job! Focus cycle #{cycles} is complete!\\nLet's take a {next_break_mins}-minute break and stretch. \\1\\s[12]Nice work! Grab some water too!\\e"
        ),
        (
            PomodoroTransition::BreakCompleted {
                cycles,
                next_work_mins,
            },
            UiLang::Ja,
        ) => format!(
            "\\0\\s[5]休憩おしまい！（これまで {cycles}セット完了）\\n次の {next_work_mins}分集中を始める？\\n\\q[▶ 次の集中を開始,__pomo_start__]\\1\\s[10]準備できたら押してな！\\e"
        ),
        (
            PomodoroTransition::BreakCompleted {
                cycles,
                next_work_mins,
            },
            UiLang::Ko,
        ) => format!(
            "\\0\\s[5]휴식 시간이 끝났어요! (지금까지 {cycles}세트 완료)\\n다음 {next_work_mins}분 집중을 바로 시작할까요?\\n\\q[▶ 다음 집중 시작,__pomo_start__]\\1\\s[10]준비되면 눌러라!\\e"
        ),
        (
            PomodoroTransition::BreakCompleted {
                cycles,
                next_work_mins,
            },
            UiLang::En,
        ) => format!(
            "\\0\\s[5]Break time's over! ({cycles} cycle(s) done so far.)\\nReady for the next {next_work_mins}-minute focus block?\\n\\q[▶ Start Next Focus,__pomo_start__]\\1\\s[10]Hit start whenever you're ready!\\e"
        ),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BacklogEntry {
    pub timestamp: String,
    pub sakura_text: String,
    pub kero_text: String,
    pub urls: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct DialogueBacklog {
    entries: VecDeque<BacklogEntry>,
}

fn sanitize_script_fragment(raw: &str) -> String {
    raw.replace('\\', "/").trim().to_string()
}

impl DialogueBacklog {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push_from_actions(&mut self, actions: &[DialogueAction], timestamp: String) {
        let mut active = Scope::Sakura;
        let mut sakura_text = String::new();
        let mut kero_text = String::new();
        let mut urls: Vec<(String, String)> = Vec::new();

        for act in actions {
            match act.kind {
                ActionKind::Speaker => active = act.scope,
                ActionKind::Clear => {
                    if active == Scope::Kero {
                        kero_text.clear();
                    } else {
                        sakura_text.clear();
                    }
                }
                ActionKind::Text => {
                    let t = act.text.trim();
                    if t.is_empty() || t == "…" {
                        continue;
                    }
                    let dst = if active == Scope::Kero {
                        &mut kero_text
                    } else {
                        &mut sakura_text
                    };
                    if !dst.is_empty() && !dst.ends_with('\n') && !t.starts_with('\n') {
                        dst.push('\n');
                    }
                    dst.push_str(t);
                }
                ActionKind::Link => {
                    if !act.choice_id.is_empty()
                        && !urls.iter().any(|(u, _)| u == &act.choice_id)
                    {
                        urls.push((act.choice_id.clone(), act.label.clone()));
                    }
                }
                ActionKind::Choice
                | ActionKind::Surface
                | ActionKind::Pause
                | ActionKind::QuickToggle
                | ActionKind::BalloonSize => {}
            }
        }

        let sak_clean = sakura_text.trim().to_string();
        let ker_clean = kero_text.trim().to_string();
        if sak_clean.is_empty() && ker_clean.is_empty() {
            return;
        }
        if let Some(last) = self.entries.back() {
            if last.sakura_text == sak_clean && last.kero_text == ker_clean {
                return;
            }
        }
        if self.entries.len() >= MAX_BACKLOG_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(BacklogEntry {
            timestamp,
            sakura_text: sak_clean,
            kero_text: ker_clean,
            urls,
        });
    }

    pub fn latest_plain_text(&self) -> Option<String> {
        let last = self.entries.back()?;
        let mut parts = Vec::new();
        if !last.sakura_text.is_empty() {
            parts.push(last.sakura_text.clone());
        }
        if !last.kero_text.is_empty() {
            parts.push(format!("(Partner) {}", last.kero_text));
        }
        for (url, lbl) in &last.urls {
            if lbl.is_empty() || lbl == url {
                parts.push(url.clone());
            } else {
                parts.push(format!("{lbl}: {url}"));
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    pub fn entries_newest_first(&self) -> Vec<BacklogEntry> {
        self.entries.iter().rev().cloned().collect()
    }

    pub fn format_replay_script(&self, limit: usize, lang: UiLang) -> String {
        if self.entries.is_empty() || limit == 0 {
            return match lang {
                UiLang::Ja => "\\_q\\0\\s[0]まだ表示できる発話バックログがないよ。\\n話しかけたりニュースが出るとここに貯まるよ！\\_q\\e".to_string(),
                UiLang::Ko => "\\_q\\0\\s[0]아직 보여줄 발화 백로그가 없어요.\\n대화를 나누거나 뉴스가 나오면 여기에 기록돼요!\\_q\\e".to_string(),
                UiLang::En => "\\_q\\0\\s[0]No dialogue backlog entries yet.\\nTalk with me or receive updates and they'll appear here!\\_q\\e".to_string(),
            };
        }

        let header = match lang {
            UiLang::Ja => "📜 発話バックログ (最新順)",
            UiLang::Ko => "📜 발화 백로그 (최신순)",
            UiLang::En => "📜 Dialogue Backlog (Latest First)",
        };
        let copy_lbl = match lang {
            UiLang::Ja => "📋 最新回答をコピー",
            UiLang::Ko => "📋 최신 응답 복사",
            UiLang::En => "📋 Copy Latest Reply",
        };
        let partner_prefix = match lang {
            UiLang::Ja => "相棒",
            UiLang::Ko => "파트너",
            UiLang::En => "Partner",
        };

        let mut body = format!("\\_q\\0\\s[0]{header}\\n");
        for entry in self.entries.iter().rev().take(limit) {
            let short_ts: String = entry.timestamp.chars().take(5).collect();
            body.push_str(&format!("\\n[{short_ts}] "));
            if !entry.sakura_text.is_empty() {
                body.push_str(&sanitize_script_fragment(&entry.sakura_text));
            }
            if !entry.kero_text.is_empty() {
                let k = sanitize_script_fragment(&entry.kero_text);
                if !entry.sakura_text.is_empty() {
                    body.push_str(&format!("\\n  ({partner_prefix}) {k}"));
                } else {
                    body.push_str(&format!("({partner_prefix}) {k}"));
                }
            }
            for (url, lbl) in &entry.urls {
                let safe_lbl = sanitize_script_fragment(if lbl.is_empty() { url } else { lbl });
                body.push_str(&format!("\\n  \\_a[{url}]{safe_lbl}\\_a"));
            }
        }
        body.push_str(&format!("\\n\\q[{copy_lbl},__copy_last_reply__]\\_q\\e"));
        body
    }
}

fn xml_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn format_macos_launch_agent_plist(exe_path: &Path) -> String {
    let exe_esc = xml_escape(&exe_path.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>Label</key>\n\
\t<string>io.github.nanikaai.desktop</string>\n\
\t<key>ProgramArguments</key>\n\
\t<array>\n\
\t\t<string>{exe_esc}</string>\n\
\t</array>\n\
\t<key>RunAtLoad</key>\n\
\t<true/>\n\
\t<key>KeepAlive</key>\n\
\t<false/>\n\
</dict>\n\
</plist>\n"
    )
}

pub fn format_linux_autostart_desktop(exe_path: &Path) -> String {
    let exe_str = exe_path.display().to_string().replace('"', "\\\"");
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Version=1.0\n\
Name=NanikaAi\n\
Comment=AI Desktop Mascot Baseware\n\
Exec=\"{exe_str}\"\n\
Terminal=false\n\
X-GNOME-Autostart-enabled=true\n"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoItem {
    pub id: u64,
    pub text: String,
    pub done: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct MemoStore {
    file_path: PathBuf,
}

impl MemoStore {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            file_path: config_dir.join("memos.json"),
        }
    }

    pub fn list(&self) -> Vec<MemoItem> {
        let Ok(raw) = std::fs::read_to_string(&self.file_path) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<MemoItem>>(&raw).unwrap_or_default()
    }

    fn save(&self, items: &[MemoItem]) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("memo dir error: {e}"))?;
        }
        let body = serde_json::to_string_pretty(items)
            .map_err(|e| format!("memo serialize error: {e}"))?;
        let tmp = self.file_path.with_extension("json.tmp");
        std::fs::write(&tmp, body).map_err(|e| format!("memo write error: {e}"))?;
        std::fs::rename(&tmp, &self.file_path).map_err(|e| format!("memo rename error: {e}"))
    }

    pub fn add(&self, raw_text: &str) -> Result<MemoItem, String> {
        let clean: String = raw_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(240)
            .collect();
        if clean.is_empty() {
            return Err("empty memo".to_string());
        }
        let mut items = self.list();
        let next_id = items.iter().map(|m| m.id).max().unwrap_or(0) + 1;
        let item = MemoItem {
            id: next_id,
            text: clean,
            done: false,
            created_at: chrono::Local::now().format("%m/%d %H:%M").to_string(),
        };
        items.push(item.clone());
        if items.len() > 60 {
            let excess = items.len() - 60;
            items.drain(0..excess);
        }
        self.save(&items)?;
        Ok(item)
    }

    pub fn toggle_done(&self, id: u64) -> Option<MemoItem> {
        let mut items = self.list();
        let found = items.iter_mut().find(|m| m.id == id)?;
        found.done = !found.done;
        let out = found.clone();
        let _ = self.save(&items);
        Some(out)
    }

    pub fn set_done(&self, id: u64, done: bool) -> Option<MemoItem> {
        let mut items = self.list();
        let found = items.iter_mut().find(|m| m.id == id)?;
        found.done = done;
        let out = found.clone();
        let _ = self.save(&items);
        Some(out)
    }

    pub fn delete(&self, id: u64) -> bool {
        let mut items = self.list();
        let before = items.len();
        items.retain(|m| m.id != id);
        if items.len() < before {
            let _ = self.save(&items);
            true
        } else {
            false
        }
    }

    pub fn clear_done(&self) -> usize {
        let mut items = self.list();
        let before = items.len();
        items.retain(|m| !m.done);
        let removed = before.saturating_sub(items.len());
        if removed > 0 {
            let _ = self.save(&items);
        }
        removed
    }

    pub fn format_balloon_script(&self, lang: UiLang) -> String {
        let items = self.list();
        let pending: Vec<&MemoItem> = items.iter().filter(|m| !m.done).collect();
        let done_count = items.len().saturating_sub(pending.len());
        if items.is_empty() {
            return match lang {
                UiLang::Ja => "\\_q\\0\\s[0]📝 クイックメモはまだ空だよ。\\nチャットで「牛乳を買うってメモして」のように話しかけると記録できるよ！\\_q\\e".to_string(),
                UiLang::Ko => "\\_q\\0\\s[0]📝 퀵 메모가 아직 비어 있어요.\\n채팅으로 \"우유 사기 메모해 줘\"처럼 말하면 바로 저장할 수 있어요!\\_q\\e".to_string(),
                UiLang::En => "\\_q\\0\\s[0]📝 No quick memos yet.\\nTell me in chat (e.g. \"Add a memo to buy milk\") and I'll keep track of it!\\_q\\e".to_string(),
            };
        }
        let header = match lang {
            UiLang::Ja => format!("📝 クイックメモ (未完了 {} / 完了 {})", pending.len(), done_count),
            UiLang::Ko => format!("📝 퀵 메모 (미완료 {} / 완료 {})", pending.len(), done_count),
            UiLang::En => format!("📝 Quick Memos ({} open / {} done)", pending.len(), done_count),
        };
        let mut body = format!("\\_q\\0\\s[0]{header}\\n");
        for m in pending.iter().rev().take(6) {
            let safe_txt = sanitize_script_fragment(&m.text);
            let short_lbl: String = safe_txt.chars().take(28).collect();
            body.push_str(&format!(
                "\\n• [#{id}] {safe_txt}\\n  \\q[✓ 完了 #{id}: {short_lbl},__memo_done_{id}__]",
                id = m.id
            ));
        }
        if done_count > 0 {
            let clr_lbl = match lang {
                UiLang::Ja => "🧹 完了済みメモを整理",
                UiLang::Ko => "🧹 완료된 메모 정리",
                UiLang::En => "🧹 Clear Completed Memos",
            };
            body.push_str(&format!("\\n\\q[{clr_lbl},__memo_clear_done__]"));
        }
        body.push_str("\\_q\\e");
        body
    }
}

/// Guard and wrap clipboard text for explicit user-triggered AI help (blocks secrets/keys).
pub fn sanitize_clipboard_for_ai(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    let secret_markers = [
        "-----begin ",
        "private key-----",
        "ghp_",
        "gho_",
        "github_pat_",
        "aiza",
        "akia",
        "xoxb-",
        "xoxp-",
        "sk-proj-",
        "sk-ant-",
        "sk-live-",
        "ya29.",
        "hf_",
        "glpat-",
        "discord.com/api/webhooks/",
        "hooks.slack.com/",
        "eyjhbgcioi",
    ];
    let has_generic_sk = lower
        .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '=' | ':' | ',' | ';'))
        .any(|tok| tok.starts_with("sk-") && tok.len() >= 20);
    if secret_markers.iter().any(|m| lower.contains(m))
        || has_generic_sk
        || (lower.contains("api_key") && (lower.contains('=') || lower.contains(':')))
        || (lower.contains("password") && (lower.contains('=') || lower.contains(':')))
    {
        return Err("secret_detected".to_string());
    }
    let clipped: String = trimmed.chars().take(1600).collect();
    let safe_re = regex::Regex::new(r"(?i)</?\s*untrusted_data[^>]*>").expect("valid regex");
    let safe = safe_re.replace_all(&clipped, "[untrusted_data_tag]");
    Ok(format!(
        "<untrusted_data type=\"clipboard\">\n{safe}\n</untrusted_data>"
    ))
}

pub fn format_windows_run_command(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.display())
}

pub fn apply_os_autostart_in_dir(
    enabled: bool,
    exe_path: &Path,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    let (file_path, content) = (
        target_dir.join("io.github.nanikaai.desktop.plist"),
        format_macos_launch_agent_plist(exe_path),
    );
    #[cfg(not(target_os = "macos"))]
    let (file_path, content) = (
        target_dir.join("nanikaai.desktop"),
        format_linux_autostart_desktop(exe_path),
    );

    if enabled {
        fs::create_dir_all(target_dir).map_err(|e| format!("create autostart dir: {e}"))?;
        fs::write(&file_path, content).map_err(|e| format!("write autostart file: {e}"))?;
    } else if file_path.exists() {
        fs::remove_file(&file_path).map_err(|e| format!("remove autostart file: {e}"))?;
    }
    Ok(file_path)
}

pub fn apply_os_autostart(enabled: bool, exe_path: &Path) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let dir = crate::config_store::user_home_dir()
            .join("Library")
            .join("LaunchAgents");
        let path = apply_os_autostart_in_dir(enabled, exe_path, &dir)?;
        Ok(path.display().to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let mut cmd = std::process::Command::new("reg");
        if enabled {
            let val = format_windows_run_command(exe_path);
            cmd.args(["add", REG_KEY, "/v", "NanikaAi", "/t", "REG_SZ", "/d", &val, "/f"]);
        } else {
            cmd.args(["delete", REG_KEY, "/v", "NanikaAi", "/f"]);
        }
        let out = cmd
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("reg command failed: {e}"))?;
        if enabled && !out.status.success() {
            return Err(format!(
                "reg add failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(format!("{REG_KEY}\\NanikaAi"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::config_store::user_home_dir().join(".config"));
        let dir = xdg.join("autostart");
        let path = apply_os_autostart_in_dir(enabled, exe_path, &dir)?;
        Ok(path.display().to_string())
    }
}

pub fn select_next_monitor_index(
    monitors: &[(i32, i32, u32, u32)],
    current_pos: (i32, i32),
) -> Option<usize> {
    if monitors.len() < 2 {
        return None;
    }
    let idx = monitors
        .iter()
        .position(|&(mx, my, mw, mh)| {
            current_pos.0 >= mx
                && current_pos.0 < mx.saturating_add(mw as i32)
                && current_pos.1 >= my
                && current_pos.1 < my.saturating_add(mh as i32)
        })
        .unwrap_or_else(|| {
            monitors
                .iter()
                .enumerate()
                .min_by_key(|(_, &(mx, my, mw, mh))| {
                    let cx = mx as i64 + (mw as i64) / 2;
                    let cy = my as i64 + (mh as i64) / 2;
                    let dx = cx - current_pos.0 as i64;
                    let dy = cy - current_pos.1 as i64;
                    dx * dx + dy * dy
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        });
    Some((idx + 1) % monitors.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_parser::parse_dialogue;

    #[test]
    fn macos_spaces_fullscreen_parser_ignores_inactive_spaces() {
        let normal = r#"{
            Monitors = (
                {
                    "Current Space" = {
                        ManagedSpaceID = 1;
                        type = 0;
                        uuid = "";
                    };
                    Spaces = (
                        { ManagedSpaceID = 1; type = 0; },
                        { ManagedSpaceID = 42; type = 4; }
                    );
                }
            );
        }"#;
        assert!(!parse_macos_spaces_fullscreen(normal));

        let fullscreen = r#"{
            Monitors = (
                {
                    "Current Space" = {
                        ManagedSpaceID = 42;
                        WindowManagerInfo = {
                            type = 0;
                        };
                        type = 4;
                        uuid = "ABCD";
                    };
                }
            );
        }"#;
        assert!(parse_macos_spaces_fullscreen(fullscreen));
    }

    #[test]
    fn xprop_and_windows_fullscreen_parsers() {
        assert_eq!(
            parse_xprop_active_window_id("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x3a00004"),
            Some("0x3a00004".to_string())
        );
        assert_eq!(
            parse_xprop_active_window_id("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0"),
            None
        );
        assert!(parse_xprop_wm_state_is_fullscreen(
            "_NET_WM_STATE(ATOM) = _NET_WM_STATE_MAXIMIZED_VERT, _NET_WM_STATE_FULLSCREEN"
        ));
        assert!(!parse_xprop_wm_state_is_fullscreen(
            "_NET_WM_STATE(ATOM) = _NET_WM_STATE_MAXIMIZED_HORZ"
        ));
        assert!(parse_windows_quns_is_fullscreen("3"));
        assert!(parse_windows_quns_is_fullscreen("4"));
        assert!(!parse_windows_quns_is_fullscreen("5"));
    }

    #[test]
    fn battery_and_power_save_parsers() {
        let pmset_batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t18%; discharging; 0:42 remaining\n";
        assert_eq!(
            parse_macos_pmset_battery(pmset_batt),
            BatteryStatus {
                on_battery: true,
                percent: Some(18),
            }
        );
        let pmset_ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t80%; AC attached; not charging\n -APC RS 1200\t100%; AC attached\n";
        assert_eq!(
            parse_macos_pmset_battery(pmset_ac),
            BatteryStatus {
                on_battery: false,
                percent: Some(80),
            }
        );
        assert_eq!(
            parse_linux_sys_battery("Discharging\n", "14\n"),
            BatteryStatus {
                on_battery: true,
                percent: Some(14),
            }
        );
        assert_eq!(
            parse_windows_battery_line("1|19"),
            BatteryStatus {
                on_battery: true,
                percent: Some(19),
            }
        );
        assert_eq!(effective_poll_interval_ms(true, false, true, "off", false, 500), 250);
        assert_eq!(effective_poll_interval_ms(true, false, false, "auto", true, 500), 66);
        assert_eq!(effective_poll_interval_ms(true, true, false, "on", true, 500), 20);
        assert_eq!(effective_poll_interval_ms(true, false, false, "off", true, 500), 20);
    }

    #[test]
    fn pomodoro_timer_cycle_transitions_and_pause() {
        let mut p = PomodoroTimer::new(0);
        assert_eq!(p.phase, PomodoroPhase::Idle);
        p.start_focus();
        assert!(p.is_focusing());
        assert_eq!(p.remaining_secs, 25 * 60);

        p.toggle_start_pause();
        assert!(p.paused);
        assert!(!p.is_focusing());
        assert_eq!(p.advance_secs(600), None);

        p.toggle_start_pause();
        assert!(!p.paused);
        assert!(p.is_focusing());
        assert_eq!(p.advance_secs(100), None);
        assert_eq!(p.remaining_secs, 25 * 60 - 100);

        let trans1 = p.advance_secs(25 * 60);
        assert_eq!(
            trans1,
            Some(PomodoroTransition::FocusCompleted {
                cycles: 1,
                next_break_mins: 5
            })
        );
        assert_eq!(p.phase, PomodoroPhase::Break);
        assert!(!p.is_focusing());
        assert_eq!(p.remaining_secs, 5 * 60);

        let trans2 = p.advance_secs(5 * 60);
        assert_eq!(
            trans2,
            Some(PomodoroTransition::BreakCompleted {
                cycles: 1,
                next_work_mins: 25
            })
        );
        assert_eq!(p.phase, PomodoroPhase::Idle);
        assert!(!p.is_active());
    }

    #[test]
    fn backlog_ring_dedup_links_and_replay() {
        let mut bl = DialogueBacklog::default();
        assert!(bl.format_replay_script(5, UiLang::En).contains("No dialogue backlog"));

        for i in 0..35 {
            let script = if i == 34 {
                format!("\\0\\s[5]Line {i} \\_a[https://example.com/n/{i}]Read\\_a\\1\\s[10]Reply {i}\\e")
            } else {
                format!("\\0\\s[0]Line {i}\\e")
            };
            bl.push_from_actions(&parse_dialogue(&script), format!("08:{:02}:00", i % 60));
        }
        assert_eq!(bl.len(), MAX_BACKLOG_ENTRIES);

        let entries = bl.entries_newest_first();
        assert_eq!(entries[0].sakura_text, "Line 34 Read");
        assert_eq!(entries[0].kero_text, "Reply 34");
        assert_eq!(
            entries[0].urls,
            vec![("https://example.com/n/34".to_string(), "Read".to_string())]
        );
        let plain = bl.latest_plain_text().expect("plain text");
        assert!(plain.contains("Line 34"));
        assert!(plain.contains("https://example.com/n/34"));

        let replay = bl.format_replay_script(3, UiLang::Ja);
        assert!(replay.starts_with("\\_q\\0"));
        assert!(replay.contains("__copy_last_reply__"));
        assert!(replay.contains("\\_a[https://example.com/n/34]Read\\_a"));
    }

    #[test]
    fn autostart_manifests_and_monitor_cycling() {
        let plist = format_macos_launch_agent_plist(Path::new("/Applications/Nanika&Ai.app/Contents/MacOS/nanikaai"));
        assert!(plist.contains("io.github.nanikaai.desktop"));
        assert!(plist.contains("Nanika&amp;Ai.app"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));

        let desktop = format_linux_autostart_desktop(Path::new("/usr/local/bin/nanikaai"));
        assert!(desktop.contains("[Desktop Entry]"));
        assert!(desktop.contains("Exec=\"/usr/local/bin/nanikaai\""));
        assert!(desktop.contains("X-GNOME-Autostart-enabled=true"));

        let win_cmd = format_windows_run_command(Path::new("C:\\Program Files\\NanikaAi\\nanikaai.exe"));
        assert_eq!(win_cmd, "\"C:\\Program Files\\NanikaAi\\nanikaai.exe\"");

        assert_eq!(select_next_monitor_index(&[(0, 0, 1920, 1080)], (100, 100)), None);
        let mons = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert_eq!(select_next_monitor_index(&mons, (500, 500)), Some(1));
        assert_eq!(select_next_monitor_index(&mons, (2100, 200)), Some(0));
    }

    #[test]
    fn memo_store_crud_and_clipboard_secret_guard() {
        let tmp = std::env::temp_dir().join(format!(
            "nanikaai_memo_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = MemoStore::new(&tmp);
        assert!(store.list().is_empty());

        let m1 = store.add("Buy milk and coffee").expect("add m1");
        let m2 = store.add("Review PR #42").expect("add m2");
        assert_eq!(store.list().len(), 2);
        assert!(store.toggle_done(m1.id).unwrap().done);
        let script = store.format_balloon_script(UiLang::Ja);
        assert!(script.contains("Review PR #42"));
        assert!(script.contains("__memo_clear_done__"));
        assert_eq!(store.clear_done(), 1);
        assert_eq!(store.list().len(), 1);
        assert!(store.delete(m2.id));
        assert!(store.list().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);

        let wrapped = sanitize_clipboard_for_ai("Rust ownership prevents data races.")
            .expect("safe clipboard text");
        assert!(wrapped.contains("<untrusted_data type=\"clipboard\">"));
        let fake_pem = format!("{}{}\nabc", "-----BEGIN ", "PRIVATE KEY-----");
        assert_eq!(
            sanitize_clipboard_for_ai(&fake_pem).unwrap_err(),
            "secret_detected"
        );
        let fake_pat = format!("{}{}", "ghp_", "1234567890abcdef");
        assert_eq!(
            sanitize_clipboard_for_ai(&fake_pat).unwrap_err(),
            "secret_detected"
        );
    }
}
