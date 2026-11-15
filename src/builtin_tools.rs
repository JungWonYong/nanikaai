use arboard::Clipboard;
use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

use crate::config_store::{user_home_dir, ConfigStore};

const USER_AGENT_APP: &str = "AI-Ukagaka-Desktop/1.0";
const USER_AGENT_BROWSER: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

pub fn tools_schema() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get real-time current weather (temperature, humidity, wind, precipitation) and 48-hour min/max forecast for a city/region, or auto-detect current location if location is omitted.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "City or region name in Korean, Japanese, or English (e.g. 서울, 東京, Seoul, Tokyo). Leave empty for current location."
                        }
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_system_status",
                "description": "Get OS platform, CPU, memory, disk free space, hostname, and local time.",
                "parameters": {"type": "object", "properties": {}, "required": []}
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_clipboard",
                "description": "Read plain text from the desktop clipboard if enabled in settings.",
                "parameters": {"type": "object", "properties": {}, "required": []}
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List files and directories inside a local folder path.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "dir_path": {"type": "string", "description": "Directory path to inspect."}
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_text_file",
                "description": "Read first 6000 UTF-8 characters from a safe local text file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string", "description": "File path to read."}
                    },
                    "required": ["file_path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search live web pages, current news headlines, and factual snippets via DuckDuckGo and Google News RSS.",
                "parameters": {
                    "type": "object",
                    "properties": {"query": {"type": "string", "description": "Search query"}},
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "fetch_webpage",
                "description": "Fetch and read the main text content and title of a public http/https web page URL.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "Public http:// or https:// URL to read."}
                    },
                    "required": ["url"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "calculate",
                "description": "Evaluate a mathematical expression accurately (supports +, -, *, /, //, %, **, sqrt, sin, cos, tan, log, log10, pi, e, round, abs).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "expression": {"type": "string", "description": "Mathematical expression to evaluate"}
                    },
                    "required": ["expression"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "memory_save",
                "description": "Save an important user fact or preference to persistent Ghost memory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string", "description": "Short memory title"},
                        "content": {"type": "string", "description": "Detailed fact to remember"}
                    },
                    "required": ["title", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "memory_search",
                "description": "Search persistent Ghost memory by keyword.",
                "parameters": {
                    "type": "object",
                    "properties": {"keyword": {"type": "string", "description": "Keyword"}},
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "check_ollama_models",
                "description": "Check reachable Ollama models on the configured endpoint.",
                "parameters": {"type": "object", "properties": {}, "required": []}
            }
        }
    ])
}

fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return user_home_dir();
    }
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        return user_home_dir().join(rest);
    }
    PathBuf::from(trimmed)
}

pub fn is_sensitive_path(target: &Path) -> bool {
    let resolved = fs::canonicalize(target)
        .unwrap_or_else(|_| target.to_path_buf())
        .to_string_lossy()
        .to_string();
    static BLOCKED: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = BLOCKED.get_or_init(|| {
        vec![
            Regex::new(r"(?i)[/\\]\.ssh(?:[/\\]|$)").unwrap(),
            Regex::new(r"(?i)[/\\]\.gnupg(?:[/\\]|$)").unwrap(),
            Regex::new(r"(?i)[/\\]\.aws(?:[/\\]|$)").unwrap(),
            Regex::new(r"(?i)credentials[^/\\]*\.json$").unwrap(),
            Regex::new(r"(?i)\.env(?:\.[^/\\]+)?$").unwrap(),
            Regex::new(r"(?i)^/etc/shadow$").unwrap(),
        ]
    });
    patterns.iter().any(|re| re.is_match(&resolved))
}

fn is_non_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1])) // CGNAT 100.64.0.0/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (18..=19).contains(&o[1]))
        || o[0] >= 240
}

fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_non_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_non_public_v4(mapped);
            }
            let seg = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (seg[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (seg[0] == 0x2001 && seg[1] == 0x0db8)
        }
    }
}

pub fn validate_public_url(raw_url: &str) -> Result<String, String> {
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
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Err("Blocked local or internal hostname.".to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_non_public_ip(ip) {
            return Err(format!("Blocked non-public IP address: {ip}"));
        }
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| format!("Could not resolve host: {host}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("Could not resolve host: {host}"));
    }
    for addr in addrs {
        if is_non_public_ip(addr.ip()) {
            return Err(format!("Blocked non-public IP address: {}", addr.ip()));
        }
    }
    Ok(trimmed.to_string())
}

fn known_cities() -> &'static HashMap<&'static str, (f64, f64, &'static str)> {
    static MAP: OnceLock<HashMap<&'static str, (f64, f64, &'static str)>> = OnceLock::new();
    MAP.get_or_init(|| {
        HashMap::from([
            ("서울", (37.5665, 126.9780, "서울 (Seoul, KR)")),
            ("seoul", (37.5665, 126.9780, "서울 (Seoul, KR)")),
            ("부산", (35.1796, 129.0756, "부산 (Busan, KR)")),
            ("busan", (35.1796, 129.0756, "부산 (Busan, KR)")),
            ("인천", (37.4563, 126.7052, "인천 (Incheon, KR)")),
            ("incheon", (37.4563, 126.7052, "인천 (Incheon, KR)")),
            ("대구", (35.8714, 128.6014, "대구 (Daegu, KR)")),
            ("daegu", (35.8714, 128.6014, "대구 (Daegu, KR)")),
            ("대전", (36.3504, 127.3845, "대전 (Daejeon, KR)")),
            ("daejeon", (36.3504, 127.3845, "대전 (Daejeon, KR)")),
            ("광주", (35.1595, 126.8526, "광주 (Gwangju, KR)")),
            ("gwangju", (35.1595, 126.8526, "광주 (Gwangju, KR)")),
            ("울산", (35.5384, 129.3114, "울산 (Ulsan, KR)")),
            ("ulsan", (35.5384, 129.3114, "울산 (Ulsan, KR)")),
            ("수원", (37.2636, 127.0286, "수원 (Suwon, KR)")),
            ("suwon", (37.2636, 127.0286, "수원 (Suwon, KR)")),
            ("성남", (37.4200, 127.1267, "성남 (Seongnam, KR)")),
            ("분당", (37.4200, 127.1267, "성남 (Seongnam, KR)")),
            ("seongnam", (37.4200, 127.1267, "성남 (Seongnam, KR)")),
            ("제주", (33.4996, 126.5312, "제주 (Jeju, KR)")),
            ("jeju", (33.4996, 126.5312, "제주 (Jeju, KR)")),
            ("東京", (35.6762, 139.6503, "東京 (Tokyo, JP)")),
            ("도쿄", (35.6762, 139.6503, "東京 (Tokyo, JP)")),
            ("tokyo", (35.6762, 139.6503, "東京 (Tokyo, JP)")),
            ("大阪", (34.6937, 135.5023, "大阪 (Osaka, JP)")),
            ("오사카", (34.6937, 135.5023, "大阪 (Osaka, JP)")),
            ("osaka", (34.6937, 135.5023, "大阪 (Osaka, JP)")),
            ("京都", (35.0116, 135.7681, "京都 (Kyoto, JP)")),
            ("교토", (35.0116, 135.7681, "京都 (Kyoto, JP)")),
            ("kyoto", (35.0116, 135.7681, "京都 (Kyoto, JP)")),
            ("横浜", (35.4437, 139.6380, "横浜 (Yokohama, JP)")),
            ("요코하마", (35.4437, 139.6380, "横浜 (Yokohama, JP)")),
            ("yokohama", (35.4437, 139.6380, "横浜 (Yokohama, JP)")),
            ("名古屋", (35.1815, 136.9066, "名古屋 (Nagoya, JP)")),
            ("나고야", (35.1815, 136.9066, "名古屋 (Nagoya, JP)")),
            ("nagoya", (35.1815, 136.9066, "名古屋 (Nagoya, JP)")),
            ("札幌", (43.0618, 141.3545, "札幌 (Sapporo, JP)")),
            ("삿포로", (43.0618, 141.3545, "札幌 (Sapporo, JP)")),
            ("sapporo", (43.0618, 141.3545, "札幌 (Sapporo, JP)")),
            ("福岡", (33.5902, 130.4017, "福岡 (Fukuoka, JP)")),
            ("후쿠오카", (33.5902, 130.4017, "福岡 (Fukuoka, JP)")),
            ("fukuoka", (33.5902, 130.4017, "福岡 (Fukuoka, JP)")),
            ("神戸", (34.6901, 135.1955, "神戸 (Kobe, JP)")),
            ("고베", (34.6901, 135.1955, "神戸 (Kobe, JP)")),
            ("kobe", (34.6901, 135.1955, "神戸 (Kobe, JP)")),
            ("仙台", (38.2682, 140.8694, "仙台 (Sendai, JP)")),
            ("센다이", (38.2682, 140.8694, "仙台 (Sendai, JP)")),
            ("sendai", (38.2682, 140.8694, "仙台 (Sendai, JP)")),
            ("広島", (34.3853, 132.4553, "広島 (Hiroshima, JP)")),
            ("히로시마", (34.3853, 132.4553, "広島 (Hiroshima, JP)")),
            ("hiroshima", (34.3853, 132.4553, "広島 (Hiroshima, JP)")),
            ("沖縄", (26.2124, 127.6809, "沖縄・那覇 (Okinawa, JP)")),
            ("okinawa", (26.2124, 127.6809, "沖縄・那覇 (Okinawa, JP)")),
            ("new york", (40.7128, -74.0060, "New York, US")),
            ("뉴욕", (40.7128, -74.0060, "New York, US")),
            ("london", (51.5074, -0.1278, "London, UK")),
            ("런던", (51.5074, -0.1278, "London, UK")),
            ("paris", (48.8566, 2.3522, "Paris, FR")),
            ("파리", (48.8566, 2.3522, "Paris, FR")),
            ("san francisco", (37.7749, -122.4194, "San Francisco, US")),
            ("taipei", (25.0330, 121.5654, "Taipei, TW")),
            ("singapore", (1.3521, 103.8198, "Singapore, SG")),
        ])
    })
}

fn lookup_known_city(raw_loc: &str) -> Option<(f64, f64, String)> {
    let norm = raw_loc
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let table = known_cities();
    if let Some(&(lat, lon, lbl)) = table.get(norm.as_str()) {
        return Some((lat, lon, lbl.to_string()));
    }
    let compact = norm.replace(' ', "");
    if let Some(&(lat, lon, lbl)) = table.get(compact.as_str()) {
        return Some((lat, lon, lbl.to_string()));
    }
    for sfx in ["특별자치도", "특별자치시", "특별시", "광역시", "시", "도", "都", "府", "県", "市"] {
        if let Some(stem) = compact.strip_suffix(sfx) {
            if stem.chars().count() >= 2 {
                if let Some(&(lat, lon, lbl)) = table.get(stem) {
                    return Some((lat, lon, lbl.to_string()));
                }
            }
        }
    }
    None
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("<![CDATA[", "")
        .replace("]]>", "")
}

fn strip_html(raw: &str) -> String {
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    let no_tags = TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap()).replace_all(raw, "");
    html_unescape(&no_tags)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn unwrap_ddg_url(raw_href: &str) -> String {
    let mut href = html_unescape(raw_href.trim());
    if href.starts_with("//") {
        href = format!("https:{href}");
    }
    if let Ok(u) = Url::parse(&href) {
        for (k, v) in u.query_pairs() {
            if k == "uddg" {
                return v.into_owned();
            }
        }
    }
    href
}

// --- Safe pure-Rust recursive-descent mathematical expression evaluator ---
struct MathParser<'a> {
    chars: &'a [u8],
    pos: usize,
}

impl<'a> MathParser<'a> {
    fn new(expr: &'a str) -> Self {
        Self {
            chars: expr.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.chars.get(self.pos).copied()
    }

    fn parse(&mut self) -> Result<f64, String> {
        let val = self.parse_add_sub()?;
        self.skip_ws();
        if self.pos < self.chars.len() {
            return Err(format!(
                "Unexpected trailing token at position {}",
                self.pos
            ));
        }
        if !val.is_finite() {
            return Err("Result is not finite.".to_string());
        }
        Ok(val)
    }

    fn parse_add_sub(&mut self) -> Result<f64, String> {
        let mut left = self.parse_mul_div()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    left += self.parse_mul_div()?;
                }
                Some(b'-') => {
                    self.pos += 1;
                    left -= self.parse_mul_div()?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_mul_div(&mut self) -> Result<f64, String> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_ws();
            if self.pos + 1 < self.chars.len()
                && self.chars[self.pos] == b'/'
                && self.chars[self.pos + 1] == b'/'
            {
                self.pos += 2;
                let right = self.parse_unary()?;
                if right == 0.0 {
                    return Err("Division by zero.".to_string());
                }
                left = (left / right).floor();
            } else if self.pos < self.chars.len()
                && self.chars[self.pos] == b'*'
                && (self.pos + 1 >= self.chars.len() || self.chars[self.pos + 1] != b'*')
            {
                self.pos += 1;
                left *= self.parse_unary()?;
            } else if self.pos < self.chars.len() && self.chars[self.pos] == b'/' {
                self.pos += 1;
                let right = self.parse_unary()?;
                if right == 0.0 {
                    return Err("Division by zero.".to_string());
                }
                left /= right;
            } else if self.pos < self.chars.len() && self.chars[self.pos] == b'%' {
                self.pos += 1;
                let right = self.parse_unary()?;
                if right == 0.0 {
                    return Err("Modulo by zero.".to_string());
                }
                left %= right;
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary()
            }
            Some(b'-') => {
                self.pos += 1;
                Ok(-self.parse_unary()?)
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<f64, String> {
        let base = self.parse_primary()?;
        self.skip_ws();
        if (self.pos + 1 < self.chars.len()
            && self.chars[self.pos] == b'*'
            && self.chars[self.pos + 1] == b'*')
            || (self.pos < self.chars.len() && self.chars[self.pos] == b'^')
        {
            self.pos += if self.chars[self.pos] == b'^' { 1 } else { 2 };
            let exp = self.parse_unary()?;
            if exp.abs() > 1000.0 || (base.abs() > 10000.0 && exp.abs() > 20.0) {
                return Err("Exponent exceeds safe bounds.".to_string());
            }
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        self.skip_ws();
        let Some(&ch) = self.chars.get(self.pos) else {
            return Err("Unexpected end of expression.".to_string());
        };
        if ch == b'(' {
            self.pos += 1;
            let val = self.parse_add_sub()?;
            if self.peek() != Some(b')') {
                return Err("Missing closing parenthesis ')'.".to_string());
            }
            self.pos += 1;
            return Ok(val);
        }
        if ch.is_ascii_digit() || ch == b'.' {
            let start = self.pos;
            while self.pos < self.chars.len()
                && (self.chars[self.pos].is_ascii_digit()
                    || self.chars[self.pos] == b'.'
                    || matches!(self.chars[self.pos], b'e' | b'E')
                        && self.pos + 1 < self.chars.len()
                        && (self.chars[self.pos + 1].is_ascii_digit()
                            || matches!(self.chars[self.pos + 1], b'+' | b'-')))
            {
                if matches!(self.chars[self.pos], b'e' | b'E') {
                    self.pos += 2;
                } else {
                    self.pos += 1;
                }
            }
            let num_str = std::str::from_utf8(&self.chars[start..self.pos])
                .map_err(|_| "Invalid number".to_string())?;
            return num_str
                .parse::<f64>()
                .map_err(|_| format!("Invalid number: {num_str}"));
        }
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let start = self.pos;
            while self.pos < self.chars.len()
                && (self.chars[self.pos].is_ascii_alphanumeric() || self.chars[self.pos] == b'_')
            {
                self.pos += 1;
            }
            let ident = std::str::from_utf8(&self.chars[start..self.pos])
                .map_err(|_| "Invalid identifier".to_string())?;
            if self.peek() == Some(b'(') {
                self.pos += 1;
                let mut args = Vec::new();
                if self.peek() != Some(b')') {
                    loop {
                        args.push(self.parse_add_sub()?);
                        if self.peek() == Some(b',') {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                }
                if self.peek() != Some(b')') {
                    return Err(format!("Missing ')' in call to {ident}"));
                }
                self.pos += 1;
                if args.len() != 1 {
                    return Err(format!("Function {ident} expects 1 argument"));
                }
                let a = args[0];
                return match ident {
                    "sqrt" => Ok(a.sqrt()),
                    "sin" => Ok(a.sin()),
                    "cos" => Ok(a.cos()),
                    "tan" => Ok(a.tan()),
                    "log" => Ok(a.ln()),
                    "log10" => Ok(a.log10()),
                    "ceil" => Ok(a.ceil()),
                    "floor" => Ok(a.floor()),
                    "round" => Ok(a.round()),
                    "abs" => Ok(a.abs()),
                    _ => Err(format!("Unsupported function: {ident}")),
                };
            }
            return match ident {
                "pi" => Ok(std::f64::consts::PI),
                "e" => Ok(std::f64::consts::E),
                _ => Err(format!("Unsupported identifier: {ident}")),
            };
        }
        Err(format!("Unexpected character '{}'", ch as char))
    }
}

pub fn eval_math_expression(expr: &str) -> Result<f64, String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err("expression is required.".to_string());
    }
    MathParser::new(trimmed).parse()
}

fn map_weather_symbol(sym: &str) -> String {
    static SUFFIX_RE: OnceLock<Regex> = OnceLock::new();
    let base = SUFFIX_RE
        .get_or_init(|| Regex::new(r"(?i)_(?:day|night|polartwilight)$").unwrap())
        .replace(sym.trim(), "")
        .to_ascii_lowercase();
    match base.as_str() {
        "" => "알 수 없음 / 不明 (Unknown)".to_string(),
        "clearsky" => "맑음 / 晴れ (Clear)".to_string(),
        "fair" => "대체로 맑음 / 晴れ時々曇り (Fair)".to_string(),
        "partlycloudy" => "구름 조금 / 薄曇り (Partly Cloudy)".to_string(),
        "cloudy" => "흐림 / 曇り (Cloudy)".to_string(),
        "fog" => "안개 / 霧 (Fog)".to_string(),
        "lightrain" => "약한 비 / 小雨 (Light Rain)".to_string(),
        "rain" => "비 / 雨 (Rain)".to_string(),
        "heavyrain" => "강한 비 / 大雨 (Heavy Rain)".to_string(),
        "lightrainshowers" => "약한 소나기 / 弱いにわか雨 (Light Rain Showers)".to_string(),
        "rainshowers" => "소나기 / にわか雨 (Rain Showers)".to_string(),
        "heavyrainshowers" => "강한 소나기 / 強いにわか雨 (Heavy Rain Showers)".to_string(),
        "lightsnow" => "약한 눈 / 小雪 (Light Snow)".to_string(),
        "snow" => "눈 / 雪 (Snow)".to_string(),
        "heavysnow" => "폭설 / 大雪 (Heavy Snow)".to_string(),
        s if s.contains("thunder") => "뇌우 / 雷雨 (Thunderstorm)".to_string(),
        other => other.to_string(),
    }
}

fn http_client(timeout_sec: u64, allow_redirects: bool) -> reqwest::blocking::Client {
    let builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_sec))
        .user_agent(USER_AGENT_APP);
    let builder = if allow_redirects {
        builder
    } else {
        builder.redirect(reqwest::redirect::Policy::none())
    };
    builder.build().unwrap_or_else(|_| reqwest::blocking::Client::new())
}

#[derive(Clone)]
pub struct BuiltinToolRunner {
    pub config_store: ConfigStore,
    pub workspace_dir: PathBuf,
}

impl BuiltinToolRunner {
    pub fn new(config_store: ConfigStore, workspace_dir: Option<PathBuf>) -> Self {
        Self {
            config_store,
            workspace_dir: workspace_dir
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        }
    }

    pub fn execute(&self, name: &str, args: &Value) -> String {
        let str_arg = |k: &str| -> String {
            args.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        match name {
            "get_weather" => self.get_weather(&str_arg("location")),
            "get_system_status" => self.get_system_status(),
            "read_clipboard" => self.read_clipboard(),
            "list_files" => self.list_files(&str_arg("dir_path")),
            "read_text_file" => self.read_text_file(&str_arg("file_path")),
            "web_search" => self.web_search(&str_arg("query")),
            "fetch_webpage" => self.fetch_webpage(&str_arg("url")),
            "calculate" => self.calculate(&str_arg("expression")),
            "memory_save" => self.memory_save(&str_arg("title"), &str_arg("content")),
            "memory_search" => self.memory_search(&str_arg("keyword")),
            "check_ollama_models" => self.check_ollama_models(),
            _ => json!({"ok": false, "error": format!("Unknown tool: {name}")}).to_string(),
        }
    }

    fn get_system_status(&self) -> String {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let (ram_total_gb, ram_avail_gb) = query_ram_gb();
        let disk_free_gb = query_disk_free_gb(&self.workspace_dir);
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| {
                Command::new("hostname")
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "desktop".to_string())
            });
        json!({
            "ok": true,
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "hostname": hostname,
            "cpu_cores": cores,
            "ram_total_gb": ram_total_gb,
            "ram_available_gb": ram_avail_gb,
            "disk_free_gb": disk_free_gb,
            "local_time": Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        })
        .to_string()
    }

    fn read_clipboard(&self) -> String {
        if !self.config_store.get().enable_clipboard_tool {
            return json!({"ok": false, "error": "Clipboard access disabled."}).to_string();
        }
        match Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(txt) => {
                let len = txt.chars().count();
                let preview: String = txt.chars().take(4000).collect();
                json!({"ok": true, "length": len, "text": preview}).to_string()
            }
            Err(e) => json!({"ok": false, "error": format!("Clipboard unavailable: {e}")}).to_string(),
        }
    }

    fn list_files(&self, raw_dir: &str) -> String {
        let target = if raw_dir.trim().is_empty() {
            self.workspace_dir.clone()
        } else {
            let p = expand_tilde(raw_dir);
            if p.is_absolute() {
                p
            } else {
                self.workspace_dir.join(p)
            }
        };
        if is_sensitive_path(&target) {
            return json!({"ok": false, "error": "Blocked sensitive path."}).to_string();
        }
        if !target.is_dir() {
            return json!({"ok": false, "error": format!("Directory not found: {}", target.display())}).to_string();
        }
        let mut entries = Vec::new();
        if let Ok(rd) = fs::read_dir(&target) {
            let mut items: Vec<_> = rd.filter_map(|e| e.ok()).collect();
            items.sort_by_key(|e| e.file_name());
            for e in items.into_iter().take(60) {
                entries.push(json!({
                    "name": e.file_name().to_string_lossy().to_string(),
                    "type": if e.path().is_dir() { "dir" } else { "file" }
                }));
            }
        }
        json!({"ok": true, "dir_path": target.display().to_string(), "entries": entries}).to_string()
    }

    fn read_text_file(&self, raw_path: &str) -> String {
        if raw_path.trim().is_empty() {
            return json!({"ok": false, "error": "file_path is required."}).to_string();
        }
        let p = expand_tilde(raw_path);
        let target = if p.is_absolute() {
            p
        } else {
            self.workspace_dir.join(p)
        };
        if is_sensitive_path(&target) {
            return json!({"ok": false, "error": "Blocked sensitive file."}).to_string();
        }
        if !target.is_file() {
            return json!({"ok": false, "error": format!("File not found: {}", target.display())}).to_string();
        }
        let mut buf = vec![0u8; 12288];
        let n = fs::File::open(&target)
            .and_then(|mut f| f.read(&mut buf))
            .unwrap_or(0);
        let data: String = String::from_utf8_lossy(&buf[..n]).chars().take(6000).collect();
        json!({"ok": true, "file_path": target.display().to_string(), "content": data}).to_string()
    }

    fn get_weather(&self, location: &str) -> String {
        let (lat, lon, resolved_label) = match resolve_coordinates(location) {
            Ok(v) => v,
            Err(e) => return json!({"ok": false, "error": e}).to_string(),
        };
        match fetch_met_no_weather(lat, lon, &resolved_label) {
            Ok(v) => v.to_string(),
            Err(met_err) => match fetch_wttr_weather(lat, lon, &resolved_label) {
                Ok(v) => v.to_string(),
                Err(wttr_err) => {
                    json!({"ok": false, "error": format!("Weather fetch failed ({met_err}; fallback: {wttr_err})")})
                        .to_string()
                }
            },
        }
    }

    fn web_search(&self, query: &str) -> String {
        let q = query.trim();
        if q.is_empty() {
            return json!({"ok": false, "error": "query is required."}).to_string();
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(USER_AGENT_BROWSER)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());

        let mut results: Vec<Value> = Vec::new();
        if let Ok(resp) = client
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", q)])
            .send()
        {
            if let Ok(html) = resp.text() {
                static LINK_RE: OnceLock<Regex> = OnceLock::new();
                static SNIP_RE: OnceLock<Regex> = OnceLock::new();
                static HREF_RE: OnceLock<Regex> = OnceLock::new();
                let link_re = LINK_RE.get_or_init(|| {
                    Regex::new(r#"(?is)<a\b([^>]*\bresult__a\b[^>]*)>(.*?)</a>"#).unwrap()
                });
                let snip_re = SNIP_RE.get_or_init(|| {
                    Regex::new(r#"(?is)<(?:a|div)\b[^>]*\bresult__snippet\b[^>]*>(.*?)</(?:a|div)>"#)
                        .unwrap()
                });
                let href_re = HREF_RE.get_or_init(|| Regex::new(r#"(?is)href\s*=\s*"([^"]*)""#).unwrap());

                let snippets: Vec<String> = snip_re
                    .captures_iter(&html)
                    .map(|c| strip_html(&c[1]))
                    .collect();

                for (idx, c) in link_re.captures_iter(&html).take(6).enumerate() {
                    let href_raw = href_re
                        .captures(&c[1])
                        .map(|hc| hc[1].to_string())
                        .unwrap_or_default();
                    let url = unwrap_ddg_url(&href_raw);
                    let title = strip_html(&c[2]);
                    let snippet = snippets.get(idx).cloned().unwrap_or_default();
                    if !title.is_empty() {
                        results.push(json!({"title": title, "url": url, "snippet": snippet}));
                    }
                }
            }
        }

        if !results.is_empty() {
            let low = q.to_ascii_lowercase();
            if ["뉴스", "속보", "최신", "ニュース", "news", "latest", "headline"]
                .iter()
                .any(|k| low.contains(k) || q.contains(k))
            {
                let mut rss = search_google_news_rss(&client, q);
                if !rss.is_empty() {
                    rss.truncate(3);
                    results.truncate(3);
                    rss.extend(results);
                    results = rss;
                }
            }
            return json!({"ok": true, "query": q, "results": results}).to_string();
        }

        let rss = search_google_news_rss(&client, q);
        if !rss.is_empty() {
            return json!({"ok": true, "source": "google_news_rss", "query": q, "results": rss})
                .to_string();
        }

        if let Ok(resp) = client
            .get("https://api.duckduckgo.com/")
            .query(&[("q", q), ("format", "json"), ("no_html", "1"), ("skip_disambig", "1")])
            .send()
        {
            if let Ok(data) = resp.json::<Value>() {
                let heading = data.get("Heading").and_then(|v| v.as_str()).unwrap_or("");
                let abs_text = data.get("AbstractText").and_then(|v| v.as_str()).unwrap_or("");
                let abs_url = data.get("AbstractURL").and_then(|v| v.as_str()).unwrap_or("");
                let mut fb_results = Vec::new();
                if !abs_text.is_empty() {
                    fb_results.push(json!({
                        "title": if heading.is_empty() { q } else { heading },
                        "url": abs_url,
                        "snippet": abs_text,
                    }));
                }
                if let Some(arr) = data.get("RelatedTopics").and_then(|v| v.as_array()) {
                    for item in arr.iter().take(5) {
                        if let Some(txt) = item.get("Text").and_then(|v| v.as_str()) {
                            let u = item.get("FirstURL").and_then(|v| v.as_str()).unwrap_or("");
                            fb_results.push(json!({
                                "title": txt.chars().take(80).collect::<String>(),
                                "url": u,
                                "snippet": txt,
                            }));
                        }
                    }
                }
                return json!({"ok": true, "query": q, "heading": heading, "abstract": abs_text, "results": fb_results})
                    .to_string();
            }
        }
        json!({"ok": false, "error": "Web search yielded no reachable results."}).to_string()
    }

    fn fetch_webpage(&self, raw_url: &str) -> String {
        let mut cur_url = match validate_public_url(raw_url) {
            Ok(u) => u,
            Err(e) => return json!({"ok": false, "error": e}).to_string(),
        };
        let client = http_client(8, false);
        for _hop in 0..4 {
            let Ok(resp) = client
                .get(&cur_url)
                .header(reqwest::header::USER_AGENT, USER_AGENT_BROWSER)
                .send()
            else {
                return json!({"ok": false, "error": "Request failed"}).to_string();
            };
            let status = resp.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if let Some(loc) = resp.headers().get(reqwest::header::LOCATION).and_then(|h| h.to_str().ok()) {
                    let Ok(base) = Url::parse(&cur_url) else {
                        break;
                    };
                    let Ok(joined) = base.join(loc) else {
                        break;
                    };
                    cur_url = match validate_public_url(joined.as_str()) {
                        Ok(u) => u,
                        Err(e) => return json!({"ok": false, "error": e}).to_string(),
                    };
                    continue;
                }
            }
            if !resp.status().is_success() {
                return json!({"ok": false, "error": format!("HTTP {status}")}).to_string();
            }
            let Ok(bytes) = resp.bytes() else {
                return json!({"ok": false, "error": "Failed to read body"}).to_string();
            };
            let slice = &bytes[..bytes.len().min(262144)];
            let page_html = String::from_utf8_lossy(slice).into_owned();
            static TITLE_RE: OnceLock<Regex> = OnceLock::new();
            static CLEAN_RE: OnceLock<Regex> = OnceLock::new();
            let title = TITLE_RE
                .get_or_init(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap())
                .captures(&page_html)
                .map(|c| strip_html(&c[1]))
                .unwrap_or_default();
            let cleaned = CLEAN_RE
                .get_or_init(|| {
                    Regex::new(r"(?is)<(?:script|style|noscript|svg|nav|footer)[^>]*>.*?</(?:script|style|noscript|svg|nav|footer)>").unwrap()
                })
                .replace_all(&page_html, " ");
            let text: String = strip_html(&cleaned).chars().take(3500).collect();
            return json!({
                "ok": true,
                "url": cur_url,
                "title": title.chars().take(200).collect::<String>(),
                "content": text,
            })
            .to_string();
        }
        json!({"ok": false, "error": "Too many HTTP redirects."}).to_string()
    }

    fn calculate(&self, expression: &str) -> String {
        match eval_math_expression(expression) {
            Ok(val) => json!({"ok": true, "expression": expression.trim(), "result": val}).to_string(),
            Err(e) => json!({"ok": false, "error": e}).to_string(),
        }
    }

    fn memory_save(&self, title: &str, content: &str) -> String {
        if title.trim().is_empty() || content.trim().is_empty() {
            return json!({"ok": false, "error": "title and content required."}).to_string();
        }
        let saved = self.config_store.upsert_memory(title, content);
        json!({"ok": true, "saved": saved}).to_string()
    }

    fn memory_search(&self, keyword: &str) -> String {
        let items = self.config_store.search_memories(keyword);
        json!({"ok": true, "count": items.len(), "items": items}).to_string()
    }

    fn check_ollama_models(&self) -> String {
        let cfg = self.config_store.get();
        let base = if matches!(cfg.provider.as_str(), "linux-ollama" | "remote-ollama") {
            cfg.remote_endpoint_url.trim_end_matches('/')
        } else {
            cfg.endpoint_url.trim_end_matches('/')
        };
        let client = http_client(4, true);
        match client.get(format!("{base}/api/tags")).send() {
            Ok(resp) => {
                let data: Value = resp.json().unwrap_or_else(|_| json!({}));
                let models: Vec<String> = data
                    .get("models")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m.get("name").and_then(|s| s.as_str()))
                            .filter(|n| !crate::agent_engine::is_forbidden_model(n))
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                json!({"ok": true, "endpoint": base, "models": models}).to_string()
            }
            Err(e) => json!({"ok": false, "error": e.to_string()}).to_string(),
        }
    }
}

fn search_google_news_rss(client: &reqwest::blocking::Client, query: &str) -> Vec<Value> {
    let has_ko = query.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c));
    let has_ja = query
        .chars()
        .any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    let (hl, gl, ceid) = if has_ko {
        ("ko", "KR", "KR:ko")
    } else if has_ja {
        ("ja", "JP", "JP:ja")
    } else {
        ("en-US", "US", "US:en")
    };
    let Ok(resp) = client
        .get("https://news.google.com/rss/search")
        .query(&[("q", query), ("hl", hl), ("gl", gl), ("ceid", ceid)])
        .send()
    else {
        return Vec::new();
    };
    let Ok(xml) = resp.text() else {
        return Vec::new();
    };

    static ITEM_RE: OnceLock<Regex> = OnceLock::new();
    let item_re = ITEM_RE.get_or_init(|| Regex::new(r"(?is)<item>(.*?)</item>").unwrap());
    let extract_tag = |blk: &str, tag: &str| -> String {
        let pat = format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>");
        Regex::new(&pat)
            .ok()
            .and_then(|re| re.captures(blk))
            .map(|c| strip_html(&c[1]))
            .unwrap_or_default()
    };

    item_re
        .captures_iter(&xml)
        .take(6)
        .filter_map(|c| {
            let blk = &c[1];
            let title = extract_tag(blk, "title");
            if title.is_empty() {
                return None;
            }
            let link = extract_tag(blk, "link");
            let pub_date = extract_tag(blk, "pubDate");
            let src = extract_tag(blk, "source");
            let snippet = [src, pub_date]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" | ");
            Some(json!({"title": title, "url": link, "snippet": snippet}))
        })
        .collect()
}

fn resolve_coordinates(location: &str) -> Result<(f64, f64, String), String> {
    let cleaned = location.trim();
    let low = cleaned.to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "" | "current" | "here" | "local" | "auto" | "현재" | "현재 위치" | "현재위치" | "여기" | "지금 있는 곳" | "現在地" | "ここ" | "現在"
    ) {
        return Ok(detect_ip_location());
    }
    if let Some(known) = lookup_known_city(cleaned) {
        return Ok(known);
    }
    let client = http_client(6, true);
    let resp = client
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[
            ("q", cleaned),
            ("format", "jsonv2"),
            ("limit", "1"),
            ("accept-language", "ko,ja,en"),
        ])
        .send()
        .map_err(|e| format!("Geocoding failed: {e}"))?;
    let items: Value = resp.json().map_err(|e| format!("Geocoding JSON failed: {e}"))?;
    let top = items
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| format!("Location not found: {location}"))?;
    let lat = top
        .get("lat")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| "Missing latitude".to_string())?;
    let lon = top
        .get("lon")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| "Missing longitude".to_string())?;
    let label = top
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or(cleaned)
        .chars()
        .take(80)
        .collect();
    Ok((lat, lon, label))
}

fn detect_ip_location() -> (f64, f64, String) {
    let client = http_client(4, true);
    if let Ok(r) = client.get("https://ipwho.is/").send() {
        if let Ok(d) = r.json::<Value>() {
            if d.get("success").and_then(|v| v.as_bool()) == Some(true) {
                if let (Some(lat), Some(lon)) = (
                    d.get("latitude").and_then(|v| v.as_f64()),
                    d.get("longitude").and_then(|v| v.as_f64()),
                ) {
                    let city = d.get("city").and_then(|v| v.as_str()).unwrap_or("");
                    let country = d.get("country").and_then(|v| v.as_str()).unwrap_or("");
                    let lbl = [city, country]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return (lat, lon, if lbl.is_empty() { "Current Location".to_string() } else { lbl });
                }
            }
        }
    }
    (35.6762, 139.6503, "Tokyo, Japan (default)".to_string())
}

fn fetch_met_no_weather(lat: f64, lon: f64, label: &str) -> Result<Value, String> {
    let client = http_client(7, true);
    let resp = client
        .get("https://api.met.no/weatherapi/locationforecast/2.0/compact")
        .query(&[("lat", format!("{lat:.4}")), ("lon", format!("{lon:.4}"))])
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let data: Value = resp.json().map_err(|e| e.to_string())?;
    let series = data
        .pointer("/properties/timeseries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Empty timeseries".to_string())?;
    let cur = series.first().ok_or_else(|| "Empty timeseries".to_string())?;
    let details = cur.pointer("/data/instant/details").cloned().unwrap_or_else(|| json!({}));
    let next_blk = cur
        .pointer("/data/next_1_hours")
        .or_else(|| cur.pointer("/data/next_6_hours"))
        .or_else(|| cur.pointer("/data/next_12_hours"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let sym = next_blk
        .pointer("/summary/symbol_code")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let precip = next_blk
        .pointer("/details/precipitation_amount")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let calc_range = |slice: &[Value]| -> Value {
        let temps: Vec<f64> = slice
            .iter()
            .filter_map(|e| {
                e.pointer("/data/instant/details/air_temperature")
                    .and_then(|v| v.as_f64())
            })
            .collect();
        if let (Some(mn), Some(mx)) = (
            temps.iter().copied().reduce(f64::min),
            temps.iter().copied().reduce(f64::max),
        ) {
            json!({"min_c": mn, "max_c": mx})
        } else {
            json!({})
        }
    };
    let end24 = series.len().min(24);
    let end48 = series.len().min(48);

    Ok(json!({
        "ok": true,
        "source": "met.no",
        "location": label,
        "latitude": (lat * 10000.0).round() / 10000.0,
        "longitude": (lon * 10000.0).round() / 10000.0,
        "observed_at_utc": cur.get("time"),
        "condition": map_weather_symbol(sym),
        "temperature_c": details.get("air_temperature"),
        "humidity_pct": details.get("relative_humidity"),
        "wind_speed_m_s": details.get("wind_speed"),
        "pressure_hpa": details.get("air_pressure_at_sea_level"),
        "precipitation_next_h_mm": precip,
        "next_24h": calc_range(&series[..end24]),
        "tomorrow_24_48h": if series.len() > 24 { calc_range(&series[24..end48]) } else { json!({}) },
    }))
}

fn fetch_wttr_weather(lat: f64, lon: f64, label: &str) -> Result<Value, String> {
    let client = http_client(5, true);
    let resp = client
        .get(format!("https://wttr.in/{lat:.4},{lon:.4}"))
        .query(&[("format", "j1")])
        .send()
        .map_err(|e| e.to_string())?;
    let data: Value = resp.json().map_err(|e| e.to_string())?;
    let cur = data
        .get("current_condition")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let desc = cur
        .pointer("/weatherDesc/0/value")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let f64_field = |k: &str| -> Option<f64> {
        cur.get(k).and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
    };
    Ok(json!({
        "ok": true,
        "source": "wttr.in",
        "location": label,
        "condition": desc,
        "temperature_c": f64_field("temp_C"),
        "humidity_pct": f64_field("humidity"),
        "wind_speed_m_s": f64_field("windspeedKmph").map(|v| (v / 3.6 * 10.0).round() / 10.0),
        "pressure_hpa": f64_field("pressure"),
        "precipitation_next_h_mm": f64_field("precipMM").unwrap_or(0.0),
    }))
}

fn query_ram_gb() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        let total = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
            .unwrap_or(0);
        let vm = Command::new("vm_stat")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let page_size = Regex::new(r"page size of (\d+) bytes")
            .ok()
            .and_then(|re| re.captures(&vm))
            .and_then(|c| c[1].parse::<u64>().ok())
            .unwrap_or(16384);
        let mut free_pages = 0u64;
        for key in ["Pages free", "Pages speculative", "Pages inactive"] {
            if let Some(val) = Regex::new(&format!(r"{key}:\s+(\d+)"))
                .ok()
                .and_then(|re| re.captures(&vm))
                .and_then(|c| c[1].parse::<u64>().ok())
            {
                free_pages += val;
            }
        }
        let round2 = |v: f64| (v * 100.0).round() / 100.0;
        return (
            round2(total as f64 / 1_073_741_824.0),
            round2((free_pages * page_size) as f64 / 1_073_741_824.0),
        );
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(txt) = fs::read_to_string("/proc/meminfo") {
            let find_kb = |k: &str| -> u64 {
                Regex::new(&format!(r"{k}:\s+(\d+)\s+kB"))
                    .ok()
                    .and_then(|re| re.captures(&txt))
                    .and_then(|c| c[1].parse::<u64>().ok())
                    .unwrap_or(0)
            };
            let round2 = |v: f64| (v * 100.0).round() / 100.0;
            return (
                round2(find_kb("MemTotal") as f64 / 1_048_576.0),
                round2(find_kb("MemAvailable") as f64 / 1_048_576.0),
            );
        }
        (0.0, 0.0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        (0.0, 0.0)
    }
}

fn query_disk_free_gb(path: &Path) -> f64 {
    #[cfg(unix)]
    {
        if let Ok(out) = Command::new("df").args(["-k", &path.display().to_string()]).output() {
            let txt = String::from_utf8_lossy(&out.stdout);
            if let Some(last) = txt.lines().last() {
                let cols: Vec<&str> = last.split_whitespace().collect();
                if cols.len() >= 4 {
                    if let Ok(avail_kb) = cols[3].parse::<u64>() {
                        return ((avail_kb as f64 / 1_048_576.0) * 100.0).round() / 100.0;
                    }
                }
            }
        }
    }
    let _ = path;
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_eval_and_security_guards_work() {
        assert!((eval_math_expression("sqrt(144) + 2 ** 5 / 4").unwrap() - 20.0).abs() < 1e-6);
        assert!(eval_math_expression("10 ** 2000").is_err());
        assert!(validate_public_url("http://127.0.0.1:9821/api/health").is_err());
        assert!(validate_public_url("http://localhost:11434").is_err());
        assert!(validate_public_url("file:///etc/passwd").is_err());
        assert!(is_sensitive_path(Path::new("/tmp/foo/.ssh/id_rsa")));
        assert!(is_sensitive_path(Path::new("/tmp/credentials.json")));
    }
}
