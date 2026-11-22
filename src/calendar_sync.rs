//! Google Calendar / iCalendar (`.ics`) sync, travel-time estimation, and proactive schedule reminders.
//!
//! Security & privacy:
//! - Only fetches `https://` public URLs (SSRF-guarded via `validate_public_url`).
//! - Never logs or returns the secret `.ics` URL in API snapshots or chat text.
//! - Online meetings (Zoom, Google Meet, Teams, etc.) are automatically excluded from travel-time alerts.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::builtin_tools::validate_public_url;
use crate::i18n::UiLang;

/// A single parsed calendar event in the upcoming window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub all_day: bool,
    pub requires_travel: bool,
    pub estimated_travel_mins: u32,
    pub travel_source: String,
    #[serde(default)]
    pub calendar_source: String,
}

impl CalendarEvent {
    /// Recommended departure Unix timestamp (`start_ts - travel - prep`), or `start_ts` if no travel.
    pub fn recommended_departure_ts(&self, prep_mins: u32) -> i64 {
        if self.requires_travel {
            self.start_ts - ((self.estimated_travel_mins + prep_mins) as i64) * 60
        } else {
            self.start_ts
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalendarAlertKind {
    /// Time to start getting ready to leave (`start - travel - prep`).
    LeavePrep,
    /// Time to leave right now (`start - travel`).
    LeaveNow,
    /// Standard upcoming reminder for non-travel events (`start - remind_mins`).
    EventSoon,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarAlert {
    pub kind: CalendarAlertKind,
    pub event: CalendarEvent,
    pub mins_until_start: i64,
    pub prep_mins: u32,
}

/// Persistent cache of recently synced upcoming events (never stores secret ICS URL).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CalendarDiskCache {
    pub last_sync_ts: i64,
    pub events: Vec<CalendarEvent>,
}

/// In-memory runtime state for calendar sync, geocoding cache, and alert deduplication.
#[derive(Debug, Default)]
pub struct CalendarSyncState {
    pub events: Vec<CalendarEvent>,
    pub last_sync_ts: i64,
    pub last_error: Option<String>,
    pub geocode_cache: HashMap<String, Option<(f64, f64)>>,
    pub alerted: HashSet<(String, i64, CalendarAlertKind)>,
}

impl CalendarSyncState {
    pub fn cache_path(config_dir: &Path) -> PathBuf {
        config_dir.join("calendar_cache.json")
    }

    pub fn load_from_disk(&mut self, config_dir: &Path) {
        let path = Self::cache_path(config_dir);
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(cache) = serde_json::from_str::<CalendarDiskCache>(&raw) {
                self.last_sync_ts = cache.last_sync_ts;
                self.events = cache.events;
            }
        }
    }

    pub fn save_to_disk(&self, config_dir: &Path) {
        let _ = fs::create_dir_all(config_dir);
        let cache = CalendarDiskCache {
            last_sync_ts: self.last_sync_ts,
            events: self.events.clone(),
        };
        if let Ok(text) = serde_json::to_string_pretty(&cache) {
            let _ = fs::write(Self::cache_path(config_dir), text);
        }
    }

    /// Return upcoming non-all-day (or today's) events sorted by `start_ts`.
    pub fn upcoming_events(&self, now_ts: i64, horizon_hours: i64) -> Vec<CalendarEvent> {
        let max_ts = now_ts + horizon_hours.max(1) * 3600;
        let mut out: Vec<CalendarEvent> = self
            .events
            .iter()
            .filter(|e| e.end_ts >= now_ts && e.start_ts <= max_ts)
            .cloned()
            .collect();
        out.sort_by_key(|e| e.start_ts);
        out
    }

    /// Check if any upcoming event should fire a departure or pre-event alert right now.
    pub fn poll_due_alerts(
        &mut self,
        now_ts: i64,
        prep_mins: u32,
        remind_mins: u32,
    ) -> Vec<CalendarAlert> {
        let mut due = Vec::new();
        // Prune old dedup entries (> 24h past start)
        self.alerted.retain(|(_, start_ts, _)| *start_ts + 86_400 >= now_ts);

        for ev in &self.events {
            if ev.all_day || ev.start_ts <= now_ts {
                continue;
            }
            let mins_until = (ev.start_ts - now_ts + 59) / 60;
            if ev.requires_travel {
                let travel = ev.estimated_travel_mins as i64;
                let leave_prep_mins = travel + prep_mins as i64;
                let key_now = (ev.uid.clone(), ev.start_ts, CalendarAlertKind::LeaveNow);
                let key_prep = (ev.uid.clone(), ev.start_ts, CalendarAlertKind::LeavePrep);

                if mins_until <= travel && mins_until > (travel - 8).max(0) && !self.alerted.contains(&key_now) {
                    self.alerted.insert(key_now);
                    self.alerted.insert(key_prep);
                    due.push(CalendarAlert {
                        kind: CalendarAlertKind::LeaveNow,
                        event: ev.clone(),
                        mins_until_start: mins_until,
                        prep_mins,
                    });
                } else if prep_mins > 0
                    && mins_until <= leave_prep_mins
                    && mins_until > travel
                    && !self.alerted.contains(&key_prep)
                {
                    self.alerted.insert(key_prep);
                    due.push(CalendarAlert {
                        kind: CalendarAlertKind::LeavePrep,
                        event: ev.clone(),
                        mins_until_start: mins_until,
                        prep_mins,
                    });
                }
            } else {
                let rem = remind_mins.max(5) as i64;
                let key_soon = (ev.uid.clone(), ev.start_ts, CalendarAlertKind::EventSoon);
                if mins_until <= rem && mins_until > 0 && !self.alerted.contains(&key_soon) {
                    self.alerted.insert(key_soon);
                    due.push(CalendarAlert {
                        kind: CalendarAlertKind::EventSoon,
                        event: ev.clone(),
                        mins_until_start: mins_until,
                        prep_mins,
                    });
                }
            }
        }
        due
    }
}

/// Validate a Google / Apple iCloud / Yahoo! Japan / Yahoo / standard iCalendar URL (`https://` or `webcal://`).
pub fn validate_calendar_ics_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("URL が空です".to_string());
    }
    let normalized = if let Some(rest) = trimmed.strip_prefix("webcal://") {
        format!("https://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("webcals://") {
        format!("https://{rest}")
    } else {
        trimmed.to_string()
    };
    if !normalized.starts_with("https://") {
        return Err("セキュリティのため https:// または webcal:// の iCal (.ics) URL のみ対応しています".to_string());
    }
    validate_public_url(&normalized)?;
    Ok(normalized)
}

/// Detect a human-friendly calendar provider label from an ICS/webcal URL.
pub fn detect_calendar_source_label(url: &str) -> &'static str {
    let l = url.to_ascii_lowercase();
    if l.contains("calendar.google.com") || l.contains("google.com/calendar") {
        "Google"
    } else if l.contains("yahoo.co.jp") {
        "Yahoo! Japan"
    } else if l.contains("yahoo.com") {
        "Yahoo"
    } else if l.contains("icloud.com") || l.contains("apple.com") {
        "Apple iCloud"
    } else if l.contains("outlook.") || l.contains("office365.") || l.contains("live.com") {
        "Outlook"
    } else {
        "iCal"
    }
}

/// Detect explicit travel time tag like `[이동 45분]`, `[移動45分]`, `[travel 45m]`, `이동시간 30분`.
pub fn parse_explicit_travel_mins(text: &str) -> Option<u32> {
    let lower = text.to_ascii_lowercase();
    let prefixes = [
        "이동시간", "이동 ", "이동:", "이동", "移動時間", "移動 ", "移動:", "移動",
        "travel time", "travel ", "travel:", "commute ", "commute:",
    ];
    for p in prefixes {
        if let Some(pos) = lower.find(p) {
            let after = &text[pos + p.len()..];
            let digits: String = after
                .chars()
                .skip_while(|c| c.is_whitespace() || *c == ':' || *c == '：' || *c == '약' || *c == '約')
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(v) = digits.parse::<u32>() {
                if (5..=240).contains(&v) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Return true if the location/title looks like an online/remote meeting rather than a physical place.
pub fn is_online_meeting(location: &str, summary: &str, description: &str) -> bool {
    let loc_l = location.trim().to_ascii_lowercase();
    let online_markers = [
        "zoom.us",
        "meet.google.com",
        "teams.microsoft.com",
        "webex.com",
        "discord.gg",
        "discord.com",
        "whereby.com",
        "gather.town",
        "slack.com/huddle",
        "http://",
        "https://",
        "online",
        "remote",
        "zoom",
        "google meet",
        "ms teams",
        "オンライン",
        "リモート",
        "在宅",
        "온라인",
        "비대면",
        "화상회의",
        "재택",
    ];
    if !loc_l.is_empty() {
        return online_markers.iter().any(|m| loc_l.contains(m));
    }
    let combined = format!("{} {}", summary.to_ascii_lowercase(), description.to_ascii_lowercase());
    online_markers.iter().any(|m| combined.contains(m))
}

/// Decide whether an event requires physical travel.
pub fn detect_requires_travel(location: &str, summary: &str, description: &str) -> bool {
    if parse_explicit_travel_mins(&format!("{summary} {description}")).is_some() {
        return true;
    }
    let loc = location.trim();
    if !loc.is_empty() {
        return !is_online_meeting(loc, summary, description);
    }
    if is_online_meeting("", summary, description) {
        return false;
    }
    let combined = format!("{summary} {description}").to_ascii_lowercase();
    let travel_hints = [
        "병원", "치과", "미용실", "공항", "출장", "방문", "회식", "약속", "역",
        "病院", "歯医者", "美容院", "空港", "出張", "訪問", "飲み会", "駅",
        "airport", "hospital", "dentist", "clinic", "station", "office visit",
    ];
    travel_hints.iter().any(|h| combined.contains(h))
}

/// Convert Haversine straight-line distance (km) into realistic urban transit/driving travel minutes.
pub fn estimate_travel_mins_from_distance_km(km: f64) -> u32 {
    if !km.is_finite() || km < 0.0 {
        return 30;
    }
    let mins = if km < 0.8 {
        10.0
    } else if km < 2.0 {
        15.0 + km * 5.0
    } else if km < 10.0 {
        // Urban transit/traffic ~20 km/h + 12m walking/transfer buffer
        12.0 + (km / 20.0) * 60.0
    } else if km < 40.0 {
        // Suburban rail/car ~35 km/h + 15m buffer
        15.0 + (km / 35.0) * 60.0
    } else {
        // Long distance ~55 km/h + 20m buffer
        20.0 + (km / 55.0) * 60.0
    };
    (mins.round() as u32).clamp(10, 150)
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0_f64;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

/// Optional free OpenStreetMap Nominatim geocoding (1 query per uncached address, polite UA, short timeout).
fn geocode_nominatim_cached(
    query: &str,
    cache: &mut HashMap<String, Option<(f64, f64)>>,
) -> Option<(f64, f64)> {
    let key = query.trim().to_string();
    if key.len() < 2 {
        return None;
    }
    if let Some(cached) = cache.get(&key) {
        return *cached;
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .user_agent("NanikaAi-DesktopCompanion/0.1 (local-calendar-travel)")
        .build()
        .ok()?;
    let resp = client
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[("q", key.as_str()), ("format", "json"), ("limit", "1")])
        .send()
        .ok()?;
    if !resp.status().is_success() {
        cache.insert(key, None);
        return None;
    }
    let arr: Vec<serde_json::Value> = resp.json().ok()?;
    let coords = arr.first().and_then(|item| {
        let lat = item.get("lat")?.as_str()?.parse::<f64>().ok()?;
        let lon = item.get("lon")?.as_str()?.parse::<f64>().ok()?;
        Some((lat, lon))
    });
    cache.insert(key, coords);
    coords
}

/// Unfold RFC 5545 folded lines (`\r\n ` or `\n ` / `\t`).
fn unfold_ics_lines(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.replace("\r\n", "\n").lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&line[1..]);
            }
        } else {
            lines.push(line.to_string());
        }
    }
    lines
}

fn unescape_ics_text(s: &str) -> String {
    s.replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
        .trim()
        .to_string()
}

/// Parse an ICS date/datetime value into `(unix_ts, is_all_day)`.
fn parse_ics_datetime(val: &str, params: &str) -> Option<(i64, bool)> {
    let v = val.trim();
    if params.to_ascii_uppercase().contains("VALUE=DATE") || (v.len() == 8 && v.chars().all(|c| c.is_ascii_digit())) {
        let nd = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        let ndt = nd.and_hms_opt(0, 0, 0)?;
        let dt = Local
            .from_local_datetime(&ndt)
            .earliest()
            .unwrap_or_else(|| Local.from_utc_datetime(&ndt));
        return Some((dt.timestamp(), true));
    }
    if v.ends_with('Z') {
        let ndt = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&ndt).timestamp(), false));
    }
    // Floating or TZID local datetime -> interpret in system local timezone
    let ndt = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    let dt = Local
        .from_local_datetime(&ndt)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&ndt));
    Some((dt.timestamp(), false))
}

/// Parse RFC 5545 iCalendar text into upcoming events within `[now_ts - 1h, now_ts + window_hours]`.
pub fn parse_ics_events(
    raw_ics: &str,
    now_ts: i64,
    window_hours: i64,
    default_travel_mins: u32,
) -> Vec<CalendarEvent> {
    let lines = unfold_ics_lines(raw_ics);
    let min_ts = now_ts - 3600;
    let max_ts = now_ts + window_hours.max(12) * 3600;

    let mut events = Vec::new();
    let mut in_event = false;
    let mut uid = String::new();
    let mut summary = String::new();
    let mut location = String::new();
    let mut description = String::new();
    let mut start_opt: Option<(i64, bool)> = None;
    let mut end_opt: Option<(i64, bool)> = None;
    let mut rrule = String::new();
    let mut status = String::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("BEGIN:VEVENT") {
            in_event = true;
            uid.clear();
            summary.clear();
            location.clear();
            description.clear();
            start_opt = None;
            end_opt = None;
            rrule.clear();
            status.clear();
            continue;
        }
        if trimmed.eq_ignore_ascii_case("END:VEVENT") {
            if in_event && !status.eq_ignore_ascii_case("CANCELLED") {
                if let Some((start_ts, all_day)) = start_opt {
                    let dur_secs = end_opt
                        .map(|(e, _)| (e - start_ts).max(if all_day { 86_400 } else { 1800 }))
                        .unwrap_or(if all_day { 86_400 } else { 3600 });
                    let clean_summary = if summary.is_empty() {
                        "(제목 없음)".to_string()
                    } else {
                        summary.clone()
                    };
                    let req_travel = !all_day && detect_requires_travel(&location, &clean_summary, &description);
                    let (est_mins, src) = if !req_travel {
                        (0, "none".to_string())
                    } else if let Some(explicit) = parse_explicit_travel_mins(&format!("{clean_summary} {description}")) {
                        (explicit, "explicit".to_string())
                    } else {
                        (default_travel_mins.clamp(5, 180), "default".to_string())
                    };
                    let base_uid = if uid.is_empty() {
                        format!("ev_{start_ts}_{}", clean_summary.len())
                    } else {
                        uid.clone()
                    };

                    let mut occ_starts = Vec::new();
                    if rrule.is_empty() {
                        if start_ts + dur_secs >= min_ts && start_ts <= max_ts {
                            occ_starts.push(start_ts);
                        }
                    } else {
                        // Lightweight DAILY / WEEKLY recurrence expansion into the upcoming window
                        let upper = rrule.to_ascii_uppercase();
                        let step_days: Option<i64> = if upper.contains("FREQ=DAILY") {
                            Some(1)
                        } else if upper.contains("FREQ=WEEKLY") {
                            Some(7)
                        } else {
                            None
                        };
                        if let Some(step) = step_days {
                            let step_secs = step * 86_400;
                            let mut cur = start_ts;
                            if cur < min_ts {
                                let skip = (min_ts - cur) / step_secs;
                                cur += skip * step_secs;
                            }
                            for _ in 0..8 {
                                if cur > max_ts {
                                    break;
                                }
                                if cur + dur_secs >= min_ts {
                                    occ_starts.push(cur);
                                }
                                cur += step_secs;
                            }
                        } else if start_ts + dur_secs >= min_ts && start_ts <= max_ts {
                            occ_starts.push(start_ts);
                        }
                    }

                    for occ in occ_starts {
                        events.push(CalendarEvent {
                            uid: base_uid.clone(),
                            summary: clean_summary.clone(),
                            location: location.clone(),
                            description: description.clone(),
                            start_ts: occ,
                            end_ts: occ + dur_secs,
                            all_day,
                            requires_travel: req_travel,
                            estimated_travel_mins: est_mins,
                            travel_source: src.clone(),
                            calendar_source: "iCal".to_string(),
                        });
                    }
                }
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
        }
        let Some((lhs, val)) = line.split_once(':') else {
            continue;
        };
        let (prop, params) = lhs.split_once(';').unwrap_or((lhs, ""));
        match prop.to_ascii_uppercase().as_str() {
            "UID" => uid = val.trim().to_string(),
            "SUMMARY" => summary = unescape_ics_text(val),
            "LOCATION" => location = unescape_ics_text(val),
            "DESCRIPTION" => description = unescape_ics_text(val),
            "DTSTART" => start_opt = parse_ics_datetime(val, params),
            "DTEND" => end_opt = parse_ics_datetime(val, params),
            "RRULE" => rrule = val.trim().to_string(),
            "STATUS" => status = val.trim().to_string(),
            _ => {}
        }
    }

    events.sort_by_key(|e| e.start_ts);
    events
}

/// Parse tab-separated lines emitted by OS native calendar bridges (`uid\tsummary\tlocation\tstart_ts\tend_ts\tall_day\tdescription`).
pub fn parse_os_native_calendar_tsv(
    tsv: &str,
    default_travel_mins: u32,
    source_label: &str,
) -> Vec<CalendarEvent> {
    let mut out = Vec::new();
    for line in tsv.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 6 {
            continue;
        }
        let uid = cols[0].trim();
        let summary = cols[1].trim();
        let location = cols[2].trim();
        let Ok(start_ts) = cols[3].trim().parse::<i64>() else {
            continue;
        };
        let end_ts = cols[4].trim().parse::<i64>().unwrap_or(start_ts + 3600).max(start_ts + 300);
        let all_day = matches!(cols[5].trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes");
        let description = cols.get(6).map(|s| s.trim()).unwrap_or("");
        let clean_summary = if summary.is_empty() { "(제목 없음)" } else { summary };
        let req_travel = !all_day && detect_requires_travel(location, clean_summary, description);
        let (est_mins, src) = if !req_travel {
            (0, "none".to_string())
        } else if let Some(explicit) = parse_explicit_travel_mins(&format!("{clean_summary} {description}")) {
            (explicit, "explicit".to_string())
        } else {
            (default_travel_mins.clamp(5, 180), "default".to_string())
        };
        out.push(CalendarEvent {
            uid: if uid.is_empty() {
                format!("os_{start_ts}_{}", clean_summary.len())
            } else {
                uid.to_string()
            },
            summary: clean_summary.to_string(),
            location: location.to_string(),
            description: description.to_string(),
            start_ts,
            end_ts,
            all_day,
            requires_travel: req_travel,
            estimated_travel_mins: est_mins,
            travel_source: src,
            calendar_source: source_label.to_string(),
        });
    }
    out
}

/// Read upcoming events from the OS built-in calendar app (macOS Calendar.app, Windows Outlook, Linux Evolution/KOrganizer).
pub fn fetch_os_native_calendar_events_blocking(
    now_ts: i64,
    window_hours: i64,
    default_travel_mins: u32,
) -> Vec<CalendarEvent> {
    #[cfg(target_os = "macos")]
    {
        // Read-only JXA bridge to Apple Calendar.app (skips gracefully if Calendar isn't permitted or has no events)
        let jxa = format!(
            r#"
            (function() {{
              try {{
                var calApp = Application("Calendar");
                var nowMs = Date.now();
                var startWin = new Date(nowMs - 3600 * 1000);
                var endWin = new Date(nowMs + {window_hours} * 3600 * 1000);
                var lines = [];
                var cals = calApp.calendars();
                for (var i = 0; i < cals.length; i++) {{
                  try {{
                    var evs = cals[i].events.whose({{
                      _and: [
                        {{ startDate: {{ _greaterThanEquals: startWin }} }},
                        {{ startDate: {{ _lessThanEquals: endWin }} }}
                      ]
                    }})();
                    for (var j = 0; j < evs.length; j++) {{
                      try {{
                        var e = evs[j];
                        var clean = function(v) {{ return String(v || "").replace(/[\t\r\n]+/g, " ").trim(); }};
                        var st = Math.floor(e.startDate().getTime() / 1000);
                        var en = Math.floor(e.endDate().getTime() / 1000);
                        var ad = e.alldayEvent() ? "1" : "0";
                        lines.push([clean(e.uid()), clean(e.summary()), clean(e.location()), st, en, ad, clean(e.description())].join("\t"));
                      }} catch (_) {{}}
                    }}
                  }} catch (_) {{}}
                }}
                return lines.join("\n");
              }} catch (_) {{
                return "";
              }}
            }})();
            "#
        );
        let _ = now_ts;
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-e", &jxa])
            .output()
        {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                return parse_os_native_calendar_tsv(&stdout, default_travel_mins, "macOS Calendar");
            }
        }
        return Vec::new();
    }

    #[cfg(target_os = "windows")]
    {
        let ps = format!(
            r#"
            try {{
              $ol = New-Object -ComObject Outlook.Application
              $ns = $ol.GetNamespace("MAPI")
              $folder = $ns.GetDefaultFolder(9)
              $items = $folder.Items
              $items.IncludeRecurrences = $true
              $items.Sort("[Start]")
              $now = Get-Date
              $end = $now.AddHours({window_hours})
              foreach ($it in $items) {{
                if ($it.Start -ge $now.AddHours(-1) -and $it.Start -le $end) {{
                  $u = ($it.EntryID -replace '[\t\r\n]+',' ')
                  $s = ($it.Subject -replace '[\t\r\n]+',' ')
                  $l = ($it.Location -replace '[\t\r\n]+',' ')
                  $st = [int64]([DateTimeOffset]$it.Start).ToUnixTimeSeconds()
                  $en = [int64]([DateTimeOffset]$it.End).ToUnixTimeSeconds()
                  $ad = if ($it.AllDayEvent) {{ "1" }} else {{ "0" }}
                  Write-Output "$u`t$s`t$l`t$st`t$en`t$ad`t"
                }}
              }}
            }} catch {{}}
            "#
        );
        let _ = now_ts;
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output()
        {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                return parse_os_native_calendar_tsv(&stdout, default_travel_mins, "Windows Calendar");
            }
        }
        return Vec::new();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut out = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let candidates = [
                PathBuf::from(&home).join(".local/share/evolution/calendar/system/calendar.ics"),
                PathBuf::from(&home).join(".local/share/korganizer/std.ics"),
            ];
            for p in candidates {
                if let Ok(raw) = fs::read_to_string(&p) {
                    let mut evs = parse_ics_events(&raw, now_ts, window_hours, default_travel_mins);
                    for e in &mut evs {
                        e.calendar_source = "Linux Calendar".to_string();
                    }
                    out.extend(evs);
                }
            }
        }
        out
    }
}

/// Load any local `.ics` files dropped into `<config_dir>/calendars/` (for offline iCal / Yahoo / Outlook exports).
pub fn load_local_ics_dir_events(
    config_dir: &Path,
    now_ts: i64,
    window_hours: i64,
    default_travel_mins: u32,
) -> Vec<CalendarEvent> {
    let dir = config_dir.join("calendars");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ics"))
        {
            if let Ok(raw) = fs::read_to_string(&path) {
                let mut evs = parse_ics_events(&raw, now_ts, window_hours, default_travel_mins);
                let fname = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Local .ics");
                for e in &mut evs {
                    e.calendar_source = format!("Local ({fname})");
                }
                out.extend(evs);
            }
        }
    }
    out
}

/// Sync all configured sources (OS built-in calendar + multiple ICS/webcal URLs + local `.ics` folder),
/// deduplicate events, and optionally refine travel time via OSM Nominatim.
pub fn sync_all_calendars_blocking(
    ics_urls: &[String],
    use_os_native: bool,
    config_dir: &Path,
    home_location: &str,
    default_travel_mins: u32,
    geocode_cache: &mut HashMap<String, Option<(f64, f64)>>,
) -> Result<Vec<CalendarEvent>, String> {
    let now_ts = Utc::now().timestamp();
    let mut merged: Vec<CalendarEvent> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    if use_os_native {
        merged.extend(fetch_os_native_calendar_events_blocking(
            now_ts,
            48,
            default_travel_mins,
        ));
    }

    merged.extend(load_local_ics_dir_events(
        config_dir,
        now_ts,
        48,
        default_travel_mins,
    ));

    for url in ics_urls {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            continue;
        }
        match fetch_and_sync_calendar_blocking(trimmed, "", default_travel_mins, geocode_cache) {
            Ok(mut evs) => {
                let label = detect_calendar_source_label(trimmed).to_string();
                for e in &mut evs {
                    e.calendar_source = label.clone();
                }
                merged.extend(evs);
            }
            Err(e) => errors.push(e),
        }
    }

    // Deduplicate across OS calendar & subscribed ICS feeds by (start_ts, lowercased summary)
    let mut seen: HashSet<(i64, String)> = HashSet::new();
    let mut deduped: Vec<CalendarEvent> = Vec::new();
    for ev in merged {
        let key = (ev.start_ts, ev.summary.trim().to_ascii_lowercase());
        if seen.insert(key) {
            deduped.push(ev);
        }
    }
    deduped.sort_by_key(|e| e.start_ts);

    // Optional free distance-based travel estimation when home_location is set
    let home_trimmed = home_location.trim();
    if !home_trimmed.is_empty()
        && deduped
            .iter()
            .any(|e| e.requires_travel && e.travel_source == "default" && !e.location.is_empty())
    {
        if let Some((hlat, hlon)) = geocode_nominatim_cached(home_trimmed, geocode_cache) {
            let mut look_budget = 6usize;
            for ev in &mut deduped {
                if ev.requires_travel
                    && ev.travel_source == "default"
                    && !ev.location.is_empty()
                    && look_budget > 0
                {
                    look_budget -= 1;
                    if let Some((elat, elon)) = geocode_nominatim_cached(&ev.location, geocode_cache) {
                        let km = haversine_km(hlat, hlon, elat, elon);
                        ev.estimated_travel_mins = estimate_travel_mins_from_distance_km(km);
                        ev.travel_source = format!("map_est({km:.1}km)");
                    }
                }
            }
        }
    }

    if deduped.is_empty() && !errors.is_empty() && !use_os_native {
        return Err(errors.join(" / "));
    }
    Ok(deduped)
}

/// Escape Sakura Script control characters (`\`, newlines) in external calendar fields so `.ics` cannot inject tags.
pub fn sanitize_calendar_sakura_text(raw: &str) -> String {
    raw.replace('\\', "＼")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

/// Fetch ICS feed over HTTPS with redirect SSRF protection, parse upcoming 48h events, and optionally refine travel time via OSM Nominatim.
pub fn fetch_and_sync_calendar_blocking(
    ics_url: &str,
    home_location: &str,
    default_travel_mins: u32,
    geocode_cache: &mut HashMap<String, Option<(f64, f64)>>,
) -> Result<Vec<CalendarEvent>, String> {
    let mut current_url = validate_calendar_ics_url(ics_url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("NanikaAi-DesktopCompanion/0.1")
        .build()
        .map_err(|e| format!("HTTP クライアント生成失敗: {e}"))?;

    let mut resp_opt = None;
    for _hop in 0..4 {
        let resp = client
            .get(&current_url)
            .send()
            .map_err(|e| format!("カレンダー取得失敗: {e}"))?;
        if resp.status().is_redirection() {
            let loc = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "リダイレクト先が不正です".to_string())?;
            let next = reqwest::Url::parse(&current_url)
                .and_then(|base| base.join(loc))
                .map_err(|e| format!("リダイレクトURL解析失敗: {e}"))?;
            current_url = validate_calendar_ics_url(next.as_str())?;
            continue;
        }
        resp_opt = Some(resp);
        break;
    }
    let resp = resp_opt.ok_or_else(|| "リダイレクト回数が上限を超えました".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("カレンダー取得 HTTP {}", resp.status().as_u16()));
    }
    let body = resp
        .text()
        .map_err(|e| format!("カレンダー本文読込失敗: {e}"))?;
    if !body.contains("BEGIN:VCALENDAR") {
        return Err("有効な iCalendar (.ics) データではありません".to_string());
    }

    let now_ts = Utc::now().timestamp();
    let mut events = parse_ics_events(&body, now_ts, 48, default_travel_mins);

    // Optional free distance-based travel estimation when home_location is set
    let home_trimmed = home_location.trim();
    if !home_trimmed.is_empty() && events.iter().any(|e| e.requires_travel && e.travel_source == "default" && !e.location.is_empty()) {
        if let Some((hlat, hlon)) = geocode_nominatim_cached(home_trimmed, geocode_cache) {
            let mut look_budget = 4usize;
            for ev in &mut events {
                if ev.requires_travel && ev.travel_source == "default" && !ev.location.is_empty() && look_budget > 0 {
                    look_budget -= 1;
                    if let Some((elat, elon)) = geocode_nominatim_cached(&ev.location, geocode_cache) {
                        let km = haversine_km(hlat, hlon, elat, elon);
                        ev.estimated_travel_mins = estimate_travel_mins_from_distance_km(km);
                        ev.travel_source = format!("map_est({km:.1}km)");
                    }
                }
            }
        }
    }

    Ok(events)
}

fn format_local_hhmm(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_string())
}

fn is_same_local_day(ts_a: i64, ts_b: i64) -> bool {
    match (Local.timestamp_opt(ts_a, 0).single(), Local.timestamp_opt(ts_b, 0).single()) {
        (Some(a), Some(b)) => a.year() == b.year() && a.ordinal() == b.ordinal(),
        _ => false,
    }
}

/// Format today's & upcoming schedule as a Sakura Script balloon with `[🔄 Sync]` choice.
pub fn format_agenda_balloon_script(
    events: &[CalendarEvent],
    connected: bool,
    prep_mins: u32,
    ui_lang: UiLang,
) -> String {
    if !connected {
        return match ui_lang {
            UiLang::Ko => "\\0\\s[0]연결된 캘린더가 아직 없어요.\\n설정 패널(OS·기능 탭)에서 [📅 OS 내장 캘린더]를 켜거나, Google / Apple iCloud / Yahoo! Japan / Yahoo 캘린더의 .ics(webcal://) 주소를 복사해 [📋 ICS URL 붙여넣기]를 눌러 주세요!\\e".to_string(),
            UiLang::En => "\\0\\s[0]No calendars connected yet.\\nEnable [📅 OS Built-in Calendar] in Settings (OS & Tools tab), or copy a Google / Apple iCloud / Yahoo! Japan / Yahoo .ics (webcal://) URL and click [📋 Paste ICS URL]!\\e".to_string(),
            UiLang::Ja => "\\0\\s[0]まだカレンダーが連携されていないよ。\\n設定パネル（OS・機能タブ）で［📅 OS標準カレンダー］をONにするか、Google / Apple iCloud / Yahoo!カレンダー(.ics / webcal://)をコピーして［📋 ICS URL貼付］を押してね！\\e".to_string(),
        };
    }

    let now_ts = Utc::now().timestamp();
    let upcoming: Vec<&CalendarEvent> = events
        .iter()
        .filter(|e| e.end_ts >= now_ts)
        .take(6)
        .collect();

    if upcoming.is_empty() {
        return match ui_lang {
            UiLang::Ko => "\\0\\s[5]오늘과 내일 예정된 일정이 없어요! 여유롭게 보내세요♪\\n\\q[🔄 지금 동기화,__cal_sync__]\\e".to_string(),
            UiLang::En => "\\0\\s[5]No upcoming events in the next 48 hours! Enjoy your free time♪\\n\\q[🔄 Sync Now,__cal_sync__]\\e".to_string(),
            UiLang::Ja => "\\0\\s[5]これから48時間の予定はないよ！ゆっくり過ごしてね♪\\n\\q[🔄 今すぐ同期,__cal_sync__]\\e".to_string(),
        };
    }

    let mut lines = Vec::new();
    let header = match ui_lang {
        UiLang::Ko => "\\0\\s[0]\\_q📅 다가오는 일정 (이동시간 반영)\\n",
        UiLang::En => "\\0\\s[0]\\_q📅 Upcoming Schedule (with Travel Time)\\n",
        UiLang::Ja => "\\0\\s[0]\\_q📅 これからの予定（移動時間込み）\\n",
    };
    lines.push(header.to_string());

    for ev in upcoming {
        let day_tag = if is_same_local_day(ev.start_ts, now_ts) {
            match ui_lang {
                UiLang::Ko => "오늘",
                UiLang::En => "Today",
                UiLang::Ja => "今日",
            }
        } else {
            match ui_lang {
                UiLang::Ko => "내일",
                UiLang::En => "Tmrw",
                UiLang::Ja => "明日",
            }
        };
        let time_str = if ev.all_day {
            match ui_lang {
                UiLang::Ko => "종일".to_string(),
                UiLang::En => "All-day".to_string(),
                UiLang::Ja => "終日".to_string(),
            }
        } else {
            format_local_hhmm(ev.start_ts)
        };
        let safe_summary = sanitize_calendar_sakura_text(&ev.summary);
        let safe_loc = sanitize_calendar_sakura_text(&ev.location);
        if ev.requires_travel {
            let dep = format_local_hhmm(ev.recommended_departure_ts(prep_mins));
            let loc_part = if safe_loc.is_empty() {
                String::new()
            } else {
                format!(" @{}", safe_loc.chars().take(20).collect::<String>())
            };
            let row = match ui_lang {
                UiLang::Ko => format!("• [{day_tag} {time_str}] {safe_summary}{loc_part}\\n   🚃 이동 약{}분 → {dep} 출발 추천\\n", ev.estimated_travel_mins),
                UiLang::En => format!("• [{day_tag} {time_str}] {safe_summary}{loc_part}\\n   🚃 ~{}m travel → leave by {dep}\\n", ev.estimated_travel_mins),
                UiLang::Ja => format!("• [{day_tag} {time_str}] {safe_summary}{loc_part}\\n   🚃 移動約{}分 → {dep} 出発目安\\n", ev.estimated_travel_mins),
            };
            lines.push(row);
        } else {
            lines.push(format!("• [{day_tag} {time_str}] {safe_summary}\\n"));
        }
    }
    let sync_btn = match ui_lang {
        UiLang::Ko => "\\_q\\q[🔄 지금 동기화,__cal_sync__]\\e",
        UiLang::En => "\\_q\\q[🔄 Sync Now,__cal_sync__]\\e",
        UiLang::Ja => "\\_q\\q[🔄 今すぐ同期,__cal_sync__]\\e",
    };
    lines.push(sync_btn.to_string());
    lines.concat()
}

/// Format a proactive departure / pre-event alert as dual-character Sakura Script.
pub fn format_calendar_alert_script(alert: &CalendarAlert, ui_lang: UiLang) -> String {
    let start_hm = format_local_hhmm(alert.event.start_ts);
    let title = sanitize_calendar_sakura_text(&alert.event.summary);
    let safe_loc = sanitize_calendar_sakura_text(&alert.event.location);
    let loc = if safe_loc.is_empty() {
        String::new()
    } else {
        format!(" ({})", safe_loc.chars().take(24).collect::<String>())
    };
    let travel = alert.event.estimated_travel_mins;
    let mins = alert.mins_until_start;

    match (alert.kind, ui_lang) {
        (CalendarAlertKind::LeavePrep, UiLang::Ko) => format!(
            "\\0\\s[5]곧 출발 준비할 시간이에요!\\n{start_hm} 「{title}」{loc}까지 이동 약 {travel}분 예상돼요. {prep}분 뒤에 출발하면 딱 맞아요!\\1\\s[10]지갑이랑 핸드폰 챙겼나 미리 확인해 둬라!\\e",
            prep = alert.prep_mins
        ),
        (CalendarAlertKind::LeavePrep, UiLang::En) => format!(
            "\\0\\s[5]Time to get ready to head out!\\n\"{title}\"{loc} starts at {start_hm} (~{travel}m travel). Aim to leave in {prep} min!\\1\\s[10]Don't forget your keys and phone!\\e",
            prep = alert.prep_mins
        ),
        (CalendarAlertKind::LeavePrep, UiLang::Ja) => format!(
            "\\0\\s[5]そろそろ出発の準備時間だよ！\\n{start_hm}「{title}」{loc}まで移動約{travel}分の見込みだから、あと{prep}分で出発するとちょうどいいよ！\\1\\s[10]忘れ物ないか今のうちにチェックしときや！\\e",
            prep = alert.prep_mins
        ),
        (CalendarAlertKind::LeaveNow, UiLang::Ko) => format!(
            "\\0\\s[3]지금 출발할 시간이에요!\\n{start_hm} 「{title}」{loc}까지 {mins}분 남았어요 (예상 이동 {travel}분). 늦지 않게 출발하세요!\\1\\s[10]어서 가자, 파이팅이야!\\e"
        ),
        (CalendarAlertKind::LeaveNow, UiLang::En) => format!(
            "\\0\\s[3]Time to leave right now!\\n\"{title}\"{loc} starts at {start_hm} ({mins}m left, ~{travel}m travel). Have a safe trip!\\1\\s[10]Off you go, don't be late!\\e"
        ),
        (CalendarAlertKind::LeaveNow, UiLang::Ja) => format!(
            "\\0\\s[3]今すぐ出発する時間だよ！\\n{start_hm}「{title}」{loc}まであと{mins}分（移動目安{travel}分）。気をつけていってらっしゃい！\\1\\s[10]遅刻せんようにな、いってらっしゃい！\\e"
        ),
        (CalendarAlertKind::EventSoon, UiLang::Ko) => format!(
            "\\0\\s[5]일정 {mins}분 전 알림이에요!\\n{start_hm} 「{title}」{loc} 곧 시작해요.\\1\\s[10]미리 창 열어 두고 준비해 둬!\\e"
        ),
        (CalendarAlertKind::EventSoon, UiLang::En) => format!(
            "\\0\\s[5]Reminder: {mins} minutes until your event!\\n\"{title}\"{loc} starts at {start_hm}.\\1\\s[10]Get your workspace ready!\\e"
        ),
        (CalendarAlertKind::EventSoon, UiLang::Ja) => format!(
            "\\0\\s[5]予定の{mins}分前だよ！\\n{start_hm}から「{title}」{loc}が始まるよ。\\1\\s[10]そろそろ準備しときや！\\e"
        ),
    }
}

/// Format a gentle periodic schedule reminder for the next upcoming event today.
pub fn format_periodic_briefing_script(
    events: &[CalendarEvent],
    now_ts: i64,
    prep_mins: u32,
    ui_lang: UiLang,
) -> Option<String> {
    let next = events
        .iter()
        .filter(|e| !e.all_day && e.start_ts > now_ts && is_same_local_day(e.start_ts, now_ts))
        .min_by_key(|e| e.start_ts)?;
    let start_hm = format_local_hhmm(next.start_ts);
    let mins_left = (next.start_ts - now_ts) / 60;
    let time_hint = if mins_left >= 60 {
        let h = mins_left / 60;
        let m = mins_left % 60;
        match ui_lang {
            UiLang::Ko => format!("{h}시간 {m}분 후"),
            UiLang::En => format!("in {h}h {m}m"),
            UiLang::Ja => format!("あと{h}時間{m}分"),
        }
    } else {
        match ui_lang {
            UiLang::Ko => format!("{mins_left}분 후"),
            UiLang::En => format!("in {mins_left}m"),
            UiLang::Ja => format!("あと{mins_left}分"),
        }
    };

    let safe_summary = sanitize_calendar_sakura_text(&next.summary);
    let msg = if next.requires_travel {
        let dep_hm = format_local_hhmm(next.recommended_departure_ts(prep_mins));
        match ui_lang {
            UiLang::Ko => format!(
                "\\0\\s[0]오늘 다음 일정 리마인드예요♪\\n{start_hm} 「{safe_summary}」({time_hint}) — 이동 약 {}분이라 {dep_hm}쯤 출발하면 좋아요!\\1\\s[10]시간 맞춰서 내가 또 알려줄게!\\e",
                next.estimated_travel_mins
            ),
            UiLang::En => format!(
                "\\0\\s[0]Quick schedule reminder♪\\nNext up: \"{safe_summary}\" at {start_hm} ({time_hint}) — ~{}m travel, plan to leave by {dep_hm}!\\1\\s[10]I'll nudge you when it's time to go!\\e",
                next.estimated_travel_mins
            ),
            UiLang::Ja => format!(
                "\\0\\s[0]今日の次の予定リマインドだよ♪\\n{start_hm}「{safe_summary}」（{time_hint}）— 移動約{}分だから{dep_hm}ごろ出発がおすすめ！\\1\\s[10]時間になったらまた声かけるで！\\e",
                next.estimated_travel_mins
            ),
        }
    } else {
        match ui_lang {
            UiLang::Ko => format!(
                "\\0\\s[0]오늘 다음 일정 리마인드예요♪\\n{start_hm} 「{safe_summary}」({time_hint}) 예정되어 있어요.\\1\\s[10]잊지 말고 챙기자!\\e"
            ),
            UiLang::En => format!(
                "\\0\\s[0]Quick schedule reminder♪\\nNext up: \"{safe_summary}\" at {start_hm} ({time_hint}).\\1\\s[10]Keep it on your radar!\\e"
            ),
            UiLang::Ja => format!(
                "\\0\\s[0]今日の次の予定リマインドだよ♪\\n{start_hm}から「{safe_summary}」（{time_hint}）があるよ。\\1\\s[10]忘れんとこな！\\e"
            ),
        }
    };
    Some(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn ics_parser_handles_travel_vs_online_and_explicit_duration() {
        let now = Utc::now();
        let ev1_start = (now + ChronoDuration::minutes(50)).format("%Y%m%dT%H%M%SZ");
        let ev1_end = (now + ChronoDuration::minutes(110)).format("%Y%m%dT%H%M%SZ");
        let ev2_start = (now + ChronoDuration::minutes(120)).format("%Y%m%dT%H%M%SZ");
        let ev2_end = (now + ChronoDuration::minutes(180)).format("%Y%m%dT%H%M%SZ");

        let sample = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
             BEGIN:VEVENT\r\nUID:u1\r\nSUMMARY:치과 예약 [이동 40분]\r\nLOCATION:강남역 10번 출구\r\nDTSTART:{ev1_start}\r\nDTEND:{ev1_end}\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:u2\r\nSUMMARY:주간 회의\r\nLOCATION:https://meet.google.com/abc-defg-hij\r\nDTSTART:{ev2_start}\r\nDTEND:{ev2_end}\r\nEND:VEVENT\r\n\
             END:VCALENDAR\r\n"
        );
        let events = parse_ics_events(&sample, now.timestamp(), 24, 30);
        assert_eq!(events.len(), 2);
        assert!(events[0].requires_travel);
        assert_eq!(events[0].estimated_travel_mins, 40);
        assert_eq!(events[0].travel_source, "explicit");
        assert!(!events[1].requires_travel);
        assert_eq!(events[1].estimated_travel_mins, 0);
    }

    #[test]
    fn departure_alert_fires_once_with_travel_and_prep_buffer() {
        let now_ts = Utc::now().timestamp();
        let mut st = CalendarSyncState::default();
        st.events.push(CalendarEvent {
            uid: "trip-1".to_string(),
            summary: "병원 방문".to_string(),
            location: "서울역".to_string(),
            description: String::new(),
            start_ts: now_ts + 38 * 60, // 38m from now; travel=30m + prep=10m => 40m threshold!
            end_ts: now_ts + 98 * 60,
            all_day: false,
            requires_travel: true,
            estimated_travel_mins: 30,
            travel_source: "default".to_string(),
            calendar_source: "Google".to_string(),
        });
        let alerts = st.poll_due_alerts(now_ts, 10, 15);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, CalendarAlertKind::LeavePrep);
        // Deduplicated on second poll
        assert!(st.poll_due_alerts(now_ts, 10, 15).is_empty());
        // LeaveNow fires when within 30m
        let alerts_now = st.poll_due_alerts(now_ts + 10 * 60, 10, 15);
        assert_eq!(alerts_now.len(), 1);
        assert_eq!(alerts_now[0].kind, CalendarAlertKind::LeaveNow);
    }

    #[test]
    fn haversine_and_url_security_guard_work() {
        // Tokyo Station -> Shibuya Station (~6.5 km) => ~32m urban transit
        let km = haversine_km(35.6812, 139.7671, 35.6580, 139.7016);
        assert!((km - 6.4).abs() < 1.0);
        let mins = estimate_travel_mins_from_distance_km(km);
        assert!((20..=45).contains(&mins));

        assert!(validate_calendar_ics_url("http://calendar.google.com/test.ics").is_err());
        assert!(validate_calendar_ics_url("https://127.0.0.1/basic.ics").is_err());
        assert!(validate_calendar_ics_url("https://calendar.google.com/calendar/ical/test/basic.ics").is_ok());
        assert_eq!(
            validate_calendar_ics_url("webcal://p12-caldav.icloud.com/published/2/demo").unwrap(),
            "https://p12-caldav.icloud.com/published/2/demo"
        );
        assert_eq!(detect_calendar_source_label("https://calendar.google.com/calendar/ical/x/basic.ics"), "Google");
        assert_eq!(detect_calendar_source_label("https://calendar.yahoo.co.jp/ical/sample.ics"), "Yahoo! Japan");
        assert_eq!(detect_calendar_source_label("https://calendar.yahoo.com/ical/sample.ics"), "Yahoo");
        assert_eq!(detect_calendar_source_label("webcal://p12-caldav.icloud.com/published/2/x"), "Apple iCloud");

        let tsv = "os-1\t東京駅で打ち合わせ\t東京駅\t2000000000\t2000003600\t0\t[移動45分]\n";
        let os_evs = parse_os_native_calendar_tsv(tsv, 30, "macOS Calendar");
        assert_eq!(os_evs.len(), 1);
        assert!(os_evs[0].requires_travel);
        assert_eq!(os_evs[0].estimated_travel_mins, 45);
        assert_eq!(os_evs[0].calendar_source, "macOS Calendar");
    }
}
