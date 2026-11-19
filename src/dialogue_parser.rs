use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Sakura,
    Kero,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Sakura => "sakura",
            Scope::Kero => "kero",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Speaker,
    Surface,
    BalloonSize,
    Text,
    Choice,
    Link,
    QuickToggle,
    Clear,
    Pause,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogueAction {
    pub kind: ActionKind,
    pub scope: Scope,
    pub surface_id: i32,
    pub balloon_id: i32,
    pub text: String,
    pub label: String,
    pub choice_id: String,
    pub ms: u64,
}

impl DialogueAction {
    fn speaker(scope: Scope) -> Self {
        Self {
            kind: ActionKind::Speaker,
            scope,
            surface_id: 0,
            balloon_id: 0,
            text: String::new(),
            label: String::new(),
            choice_id: String::new(),
            ms: 0,
        }
    }

    fn surface(scope: Scope, surface_id: i32) -> Self {
        Self {
            kind: ActionKind::Surface,
            scope,
            surface_id,
            balloon_id: 0,
            text: String::new(),
            label: String::new(),
            choice_id: String::new(),
            ms: 0,
        }
    }

    fn balloon_size(scope: Scope, balloon_id: i32) -> Self {
        Self {
            kind: ActionKind::BalloonSize,
            scope,
            surface_id: 0,
            balloon_id,
            text: String::new(),
            label: String::new(),
            choice_id: String::new(),
            ms: 0,
        }
    }

    fn choice(label: String, choice_id: String) -> Self {
        Self {
            kind: ActionKind::Choice,
            scope: Scope::Sakura,
            surface_id: 0,
            balloon_id: 0,
            text: String::new(),
            label,
            choice_id,
            ms: 0,
        }
    }

    fn clear(scope: Scope) -> Self {
        Self {
            kind: ActionKind::Clear,
            scope,
            surface_id: 0,
            balloon_id: 0,
            text: String::new(),
            label: String::new(),
            choice_id: String::new(),
            ms: 0,
        }
    }

    fn pause(ms: u64) -> Self {
        Self {
            kind: ActionKind::Pause,
            scope: Scope::Sakura,
            surface_id: 0,
            balloon_id: 0,
            text: String::new(),
            label: String::new(),
            choice_id: String::new(),
            ms,
        }
    }

    fn link(label: String, url: String) -> Self {
        Self {
            kind: ActionKind::Link,
            scope: Scope::Sakura,
            surface_id: 0,
            balloon_id: 0,
            text: String::new(),
            label,
            choice_id: url,
            ms: 0,
        }
    }

    fn quick_toggle(scope: Scope) -> Self {
        Self {
            kind: ActionKind::QuickToggle,
            scope,
            surface_id: 0,
            balloon_id: 0,
            text: String::new(),
            label: String::new(),
            choice_id: String::new(),
            ms: 0,
        }
    }
}

fn think_complete_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)(?:<(?:think|thought|reasoning)>[\s\S]*?</(?:think|thought|reasoning)>|<\|(?:think|thought)\|>[\s\S]*?<\|/(?:think|thought)\|>)",
        )
        .unwrap()
    })
}

fn think_orphan_close_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)^[\s\S]*?(?:</(?:think|thought|reasoning)>|<\|/(?:think|thought)\|>)\s*")
            .unwrap()
    })
}

fn think_open_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)(?:<(?:think|thought|reasoning)>|<\|(?:think|thought)\|>)[\s\S]*$").unwrap()
    })
}

fn control_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)<\|[^|>\r\n]{1,64}\|>|<(?:start_of_turn|end_of_turn|bos|eos|pad|unk)>(?:model|user|assistant)?",
        )
        .unwrap()
    })
}

fn html_br_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap())
}

fn xml_wrapper_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)</?(?:sakura|kero|unyu|char0|char1|speaker|dialogue|response|reply|output|message|script|p|div|span)(?:\s+[^>\r\n]*)?>",
        )
        .unwrap()
    })
}

fn double_escaped_sakura_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\\\\([012hucen]|s\[|p\[|b\[|i\[|q\[|_w\[|_a|_q|_s|!\[|f\[|w[0-9\[])")
            .unwrap()
    })
}

fn fullwidth_sakura_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\u{FF3C}([012hucen]|s\[|p\[|b\[|i\[|q\[|_w\[|_a|_q|_s|!\[|f\[|w[0-9\[])")
            .unwrap()
    })
}

fn escaped_bracket_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\\([()（）\[\]【】])").unwrap())
}

fn speaker_tag_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)(?:^|\\n)\s*[^\n\\:\uff1a]{1,24}[(\uff08]\s*\\([012hu])\s*[)\uff09]\s*[:\uff1a]\s*").unwrap()
    })
}

fn bare_speaker_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)(?:^|\\n)\s*[^\n\\:\uff1a]{1,16}[:\uff1a]\s*\\([012hu]|p\[\d+\]|s\[)").unwrap()
    })
}

fn compact_speaker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\[(?:\\?([01])|(sakura|kero)):(-?\d+)\]").unwrap())
}

fn compact_surface_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\[s:(-?\d+)\]").unwrap())
}

fn compact_choice_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\[choice:([^\]]+)\]").unwrap())
}

fn p_scope_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\p\[(\d+)\]").unwrap())
}

fn surface_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\s\[([^\]]*)\]").unwrap())
}

fn balloon_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\b\[(-?\d+)\]").unwrap())
}

fn newline_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\n(?:\[half\])?").unwrap())
}

fn wait_ms_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\_?w\[(\d+)\]").unwrap())
}

fn wait_digit_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\w([0-9])").unwrap())
}

fn choice_q_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\q\[([^\]\r\n]+)\]").unwrap())
}

fn anchor_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\_a\[([^\]\r\n]*)\]([\s\S]*?)\\_a").unwrap())
}

fn bare_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(https?://[^\s<>\[\]「」『』（）()\x22'\\]+)").unwrap()
    })
}

fn benign_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\\(?:_s|_n|_a(?:\[[^\]]*\])?|__q(?:\[[^\]]*\])?|_l\[[^\]]*\]|_b\[[^\]]*\]|_v\[[^\]]*\]|i\[[^\]]*\]|c\[[^\]]*\]|!\[[^\]]*\]|f\[[^\]]*\]|\*|-|[txv45])",
        )
        .unwrap()
    })
}

fn more_dialogue_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\\(?:[012hu]|p\[\d+\]|s\[-?\d+\])|\[(?:\\?[01]|sakura|kero):-?\d+\]").unwrap()
    })
}

fn byte_fallback_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:<0x[0-9a-f]{2}>)+").unwrap())
}

/// Decodes contiguous SentencePiece/LLM `<0xHH>` byte-fallback tokens into UTF-8 text.
pub fn decode_byte_fallback_tokens(raw: &str) -> String {
    if !raw.contains("<0x") && !raw.contains("<0X") {
        return raw.to_string();
    }
    byte_fallback_re()
        .replace_all(raw, |caps: &regex::Captures| {
            let m = &caps[0];
            let mut bytes = Vec::with_capacity(m.len() / 6);
            for chunk in m.as_bytes().chunks_exact(6) {
                if let Ok(hex) = std::str::from_utf8(&chunk[3..5]) {
                    if let Ok(b) = u8::from_str_radix(hex, 16) {
                        bytes.push(b);
                    }
                }
            }
            String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|&c| c != '\u{FFFD}' && (!c.is_control() || matches!(c, '\n' | '\r' | '\t')))
                .collect::<String>()
        })
        .into_owned()
}

pub fn strip_reasoning_blocks(raw: &str) -> String {
    let decoded = decode_byte_fallback_tokens(raw);
    let pass1 = think_complete_re().replace_all(&decoded, "");
    let pass2 = think_orphan_close_re().replace_all(&pass1, "");
    let pass3 = think_open_re().replace_all(&pass2, "");
    let pass4 = control_token_re().replace_all(&pass3, "");
    let pass5 = html_br_re().replace_all(&pass4, "\\n");
    let pass6 = xml_wrapper_re().replace_all(&pass5, "");

    let mut s = pass6.trim().to_string();
    if s.starts_with("```") && s.ends_with("```") && s.len() >= 6 {
        let inner = &s[3..s.len() - 3];
        let stripped_head = if let Some(nl) = inner.find('\n') {
            let first_line = inner[..nl].trim();
            if first_line.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
                &inner[nl + 1..]
            } else {
                inner
            }
        } else {
            inner
        };
        s = stripped_head.trim().to_string();
    } else if s.starts_with('`') && s.ends_with('`') && s.len() >= 2 && s.contains('\\') {
        s = s[1..s.len() - 1].trim().to_string();
    }
    s
}

pub fn normalize_sakura_script(text: &str) -> String {
    let unescaped = double_escaped_sakura_re().replace_all(text, |caps: &regex::Captures| {
        format!("\\{}", &caps[1])
    });
    let halfwidth = fullwidth_sakura_re().replace_all(&unescaped, |caps: &regex::Captures| {
        format!("\\{}", &caps[1])
    });
    let unescaped_brackets = escaped_bracket_re().replace_all(&halfwidth, "$1");

    // Strip speaker prefixes such as "さくら(\0): " or "\nうにゅう(\1): " safely without lookbehind.
    let mut out = String::with_capacity(unescaped_brackets.len());
    for (idx, line) in unescaped_brackets.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let step1 = speaker_tag_prefix_re().replace_all(line, |caps: &regex::Captures| {
            let prefix = if caps[0].starts_with("\\n") { "\\n" } else { "" };
            let scope_ch = if &caps[1] == "2" { "1" } else { &caps[1] };
            format!("{prefix}\\{scope_ch}")
        });
        let step2 = bare_speaker_prefix_re().replace_all(&step1, |caps: &regex::Captures| {
            let prefix = if caps[0].starts_with("\\n") { "\\n" } else { "" };
            format!("{prefix}\\{}", &caps[1])
        });
        out.push_str(&step2);
    }
    strip_stage_emotion_tags(out.trim())
}

fn resolve_emotion_keyword_surface(raw: &str, scope: Scope) -> Option<i32> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let sakura_id = if s.contains("苦笑")
        || s.contains("困")
        || s.contains("悩")
        || s.contains("焦")
        || s.contains("汗")
        || s.contains("trouble")
        || s.contains("worri")
        || s.contains("worry")
        || s.contains("sweat")
        || s.contains("awkward")
        || s.contains("fluster")
        || s.contains("당황")
        || s.contains("곤란")
        || s.contains("난처")
        || s.contains("땀")
    {
        Some(1)
    } else if s.contains("照")
        || s.contains("赤面")
        || s.contains("恥")
        || s.contains("shy")
        || s.contains("blush")
        || s.contains("embarrass")
        || s.contains("부끄")
        || s.contains("수줍")
        || s.contains("홍조")
    {
        Some(1)
    } else if s.contains("驚")
        || s.contains("びっくり")
        || s.contains("ショック")
        || s.contains("surpris")
        || s.contains("shock")
        || s.contains("gasp")
        || s.contains("놀람")
        || s.contains("놀라")
        || s.contains("깜짝")
    {
        Some(2)
    } else if s.contains("悲")
        || s.contains("泣")
        || s.contains("切な")
        || s.contains("sad")
        || s.contains("teary")
        || s.contains("cry")
        || s.contains("슬픔")
        || s.contains("슬퍼")
        || s.contains("울먹")
    {
        Some(3)
    } else if s.contains("呆")
        || s.contains("ジト")
        || s.contains("ため息")
        || s.contains("クール")
        || s.contains("気怠")
        || s.contains("毒舌")
        || s.contains("annoy")
        || s.contains("sigh")
        || s.contains("deadpan")
        || s.contains("cool")
        || s.contains("어이없")
        || s.contains("한숨")
    {
        Some(4)
    } else if s.contains("目閉")
        || s.contains("目を閉")
        || s.contains("ウインク")
        || s.contains("ウィンク")
        || s.contains("小声")
        || s.contains("囁")
        || s.contains("wink")
        || s.contains("closed")
        || s.contains("whisper")
        || s.contains("눈감")
        || s.contains("눈 감")
        || s.contains("윙크")
        || s.contains("속삭")
    {
        Some(6)
    } else if s.contains("怒")
        || s.contains("ムッ")
        || s.contains("真剣")
        || s.contains("真面目")
        || s.contains("考")
        || s.contains("angr")
        || s.contains("mad")
        || s.contains("pout")
        || s.contains("serious")
        || s.contains("think")
        || s.contains("화남")
        || s.contains("화내")
        || s.contains("삐짐")
        || s.contains("진지")
        || s.contains("고민")
    {
        Some(7)
    } else if s.contains("得意")
        || s.contains("ニヤリ")
        || s.contains("ドヤ")
        || s.contains("smug")
        || s.contains("proud")
        || s.contains("우쭐")
        || s.contains("능청")
    {
        Some(8)
    } else if s.contains("微笑")
        || s.contains("笑顔")
        || s.contains("笑み")
        || s.contains("満面")
        || s.contains("元気")
        || s.contains("明る")
        || s.contains("嬉")
        || s.contains("喜")
        || s.contains("楽")
        || s.contains("smile")
        || s.contains("happy")
        || s.contains("joy")
        || s.contains("grin")
        || s.contains("laugh")
        || s.contains("미소")
        || s.contains("웃음")
        || s.contains("활짝")
        || s.contains("기쁨")
    {
        Some(5)
    } else if s.contains("素")
        || s.contains("通常")
        || s.contains("普通")
        || s.contains("無表情")
        || s.contains("すまし")
        || s.contains("冷静")
        || s.contains("落ち着")
        || s.contains("normal")
        || s.contains("neutral")
        || s.contains("calm")
        || s.contains("무표정")
        || s.contains("보통")
        || s.contains("차분")
    {
        Some(0)
    } else {
        None
    };

    sakura_id.map(|id| {
        if scope == Scope::Sakura {
            id
        } else {
            match id {
                2 | 8 => 11,
                _ => 10,
            }
        }
    })
}

fn resolve_stage_emotion_token(token: &str, scope: Scope) -> Option<i32> {
    let t = token.trim();
    if t == "笑" || t == "ｗ" || t.eq_ignore_ascii_case("w") {
        return Some(if scope == Scope::Sakura { 5 } else { 10 });
    }
    resolve_emotion_keyword_surface(t, scope)
}

fn try_match_stage_emotion_tag(rest: &str, scope: Scope) -> Option<(usize, i32)> {
    let open = rest.chars().next()?;
    if !matches!(open, '(' | '（' | '[' | '【') {
        return None;
    }
    if open == '['
        && (compact_speaker_re().is_match(rest)
            || compact_surface_re().is_match(rest)
            || compact_choice_re().is_match(rest))
    {
        return None;
    }
    let after_open = &rest[open.len_utf8()..];
    let mut inner_chars = 0usize;
    let mut close_byte = None;
    for (idx, c) in after_open.char_indices() {
        if matches!(c, '\n' | '\r' | '(' | '（' | '[' | '【') {
            return None;
        }
        let is_close = match open {
            '(' => c == ')' || c == '）',
            '（' => c == '）' || c == ')',
            '[' => c == ']' || c == '］',
            '【' => c == '】',
            _ => false,
        };
        if is_close {
            close_byte = Some(open.len_utf8() + idx + c.len_utf8());
            break;
        }
        inner_chars += 1;
        if inner_chars > 28 {
            return None;
        }
    }
    let end_byte = close_byte?;
    let close_ch_len = rest[..end_byte].chars().last()?.len_utf8();
    let inner = rest[open.len_utf8()..end_byte - close_ch_len].trim();
    let char_cnt = inner.chars().count();
    if char_cnt == 0
        || char_cnt > 20
        || inner
            .chars()
            .any(|c| c.is_ascii_digit() || matches!(c, ':' | '=' | '<' | '>' | '\\' | '.'))
    {
        return None;
    }
    let tokens: Vec<&str> = inner
        .split(|c: char| matches!(c, '/' | '／' | '・' | ',' | '、' | '|') || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() || tokens.len() > 4 {
        return None;
    }
    let mut first_sid = None;
    for tok in tokens {
        let sid = resolve_stage_emotion_token(tok, scope)?;
        if first_sid.is_none() {
            first_sid = Some(sid);
        }
    }
    first_sid.map(|sid| (end_byte, sid))
}

fn strip_stage_emotion_tags(input: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(input.len());
    let mut scope = Scope::Sakura;
    let mut at_head = true;
    let mut saw_surface = false;
    let mut i = 0usize;
    let n = input.len();

    while i < n {
        if at_head {
            let mut la = i;
            while la < n {
                if let Some(c) = input[la..].chars().next() {
                    if matches!(c, ' ' | '\t' | '\u{3000}') {
                        la += c.len_utf8();
                        continue;
                    }
                }
                break;
            }
            if let Some((bracket_len, sid)) = try_match_stage_emotion_tag(&input[la..], scope) {
                if !saw_surface {
                    let _ = write!(out, "\\s[{sid}]");
                    saw_surface = true;
                }
                i = la + bracket_len;
                while i < n {
                    if let Some(c) = input[i..].chars().next() {
                        if matches!(c, ' ' | '\t' | '\u{3000}') {
                            i += c.len_utf8();
                            continue;
                        }
                    }
                    break;
                }
                continue;
            }
        }

        let rest = &input[i..];
        let ch = rest.chars().next().unwrap();

        if ch == '[' {
            if let Some(m) = compact_speaker_re().captures(rest) {
                let end = m.get(0).unwrap().end();
                let is_kero = m.get(1).is_some_and(|x| x.as_str() == "1")
                    || m.get(2)
                        .is_some_and(|x| x.as_str().eq_ignore_ascii_case("kero"));
                scope = if is_kero { Scope::Kero } else { Scope::Sakura };
                at_head = true;
                saw_surface = true;
                out.push_str(&rest[..end]);
                i += end;
                continue;
            }
            if let Some(m) = compact_surface_re().find(rest) {
                saw_surface = true;
                out.push_str(m.as_str());
                i += m.end();
                continue;
            }
            if let Some(m) = compact_choice_re().find(rest) {
                at_head = false;
                out.push_str(m.as_str());
                i += m.end();
                continue;
            }
        }

        if ch == '\\' {
            let after = &rest[1..];
            if let Some(nxt) = after.chars().next() {
                match nxt {
                    '\\' => {
                        out.push_str("\\\\");
                        at_head = false;
                        i += 2;
                        continue;
                    }
                    '0' | 'h' => {
                        out.push_str(&rest[..2]);
                        scope = Scope::Sakura;
                        at_head = true;
                        saw_surface = false;
                        i += 2;
                        continue;
                    }
                    '1' | '2' | 'u' => {
                        out.push_str(&rest[..2]);
                        scope = Scope::Kero;
                        at_head = true;
                        saw_surface = false;
                        i += 2;
                        continue;
                    }
                    'c' if !after.starts_with("c[") => {
                        out.push_str("\\c");
                        at_head = true;
                        saw_surface = false;
                        i += 2;
                        continue;
                    }
                    'e' => {
                        out.push_str("\\e");
                        at_head = true;
                        saw_surface = false;
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(m) = p_scope_re().captures(rest) {
                let end = m.get(0).unwrap().end();
                scope = if &m[1] == "1" { Scope::Kero } else { Scope::Sakura };
                at_head = true;
                saw_surface = false;
                out.push_str(&rest[..end]);
                i += end;
                continue;
            }
            if let Some(m) = surface_re().find(rest) {
                saw_surface = true;
                out.push_str(m.as_str());
                i += m.end();
                continue;
            }
            if let Some(m) = newline_re().find(rest) {
                at_head = true;
                saw_surface = false;
                out.push_str(m.as_str());
                i += m.end();
                continue;
            }
            if let Some(m) = balloon_re()
                .find(rest)
                .or_else(|| wait_ms_re().find(rest))
                .or_else(|| wait_digit_re().find(rest))
                .or_else(|| benign_tag_re().find(rest))
            {
                out.push_str(m.as_str());
                i += m.end();
                continue;
            }
            if rest.starts_with("\\_q") {
                out.push_str("\\_q");
                i += 3;
                continue;
            }
            if let Some(m) = anchor_link_re().find(rest).or_else(|| choice_q_re().find(rest)) {
                at_head = false;
                out.push_str(m.as_str());
                i += m.end();
                continue;
            }
        }

        if matches!(ch, '\n' | '\r') {
            at_head = true;
            saw_surface = false;
        } else if !matches!(ch, ' ' | '\t' | '\u{3000}') {
            at_head = false;
        }
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn append_text(actions: &mut Vec<DialogueAction>, scope: Scope, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    if let Some(last) = actions.last_mut() {
        if last.kind == ActionKind::Text && last.scope == scope {
            last.text.push_str(chunk);
            return;
        }
    }
    actions.push(DialogueAction {
        kind: ActionKind::Text,
        scope,
        surface_id: 0,
        balloon_id: 0,
        text: chunk.to_string(),
        label: String::new(),
        choice_id: String::new(),
        ms: 0,
    });
}

pub fn parse_dialogue(raw_input: &str) -> Vec<DialogueAction> {
    let stripped = strip_reasoning_blocks(raw_input);
    let text = normalize_sakura_script(&stripped);
    let mut actions: Vec<DialogueAction> = Vec::new();
    let mut scope = Scope::Sakura;
    let mut i = 0;
    let n = text.len();

    while i < n {
        let rest = &text[i..];
        let ch = rest.chars().next().unwrap();

        if ch == '[' {
            if let Some(m) = compact_speaker_re().captures(rest) {
                let is_kero = m.get(1).is_some_and(|x| x.as_str() == "1")
                    || m.get(2)
                        .is_some_and(|x| x.as_str().eq_ignore_ascii_case("kero"));
                scope = if is_kero { Scope::Kero } else { Scope::Sakura };
                let sid: i32 = m[3].parse().unwrap_or(0);
                actions.push(DialogueAction::speaker(scope));
                actions.push(DialogueAction::surface(scope, sid));
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = compact_surface_re().captures(rest) {
                let sid: i32 = m[1].parse().unwrap_or(0);
                actions.push(DialogueAction::surface(scope, sid));
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = compact_choice_re().captures(rest) {
                for item in m[1].split('|') {
                    let trimmed = item.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some((lbl, cid)) = trimmed.split_once('=') {
                        actions.push(DialogueAction::choice(
                            lbl.trim().to_string(),
                            cid.trim().to_string(),
                        ));
                    } else {
                        actions.push(DialogueAction::choice(
                            trimmed.to_string(),
                            trimmed.to_string(),
                        ));
                    }
                }
                i += m.get(0).unwrap().end();
                continue;
            }
        }

        if ch == '\\' {
            let after_slash = &rest[1..];
            if after_slash.is_empty() {
                append_text(&mut actions, scope, "\\");
                break;
            }
            let nxt = after_slash.chars().next().unwrap();
            if nxt == '\\' {
                append_text(&mut actions, scope, "\\");
                i += 2;
                continue;
            }
            if nxt == 'e' {
                let tail = &rest[2..];
                if more_dialogue_re().is_match(tail) {
                    i += 2;
                    while i < n && matches!(text[i..].chars().next(), Some(' ' | '\t' | '\r' | '\n')) {
                        i += 1;
                    }
                    continue;
                }
                if (tail.starts_with('\n') || tail.starts_with("\r\n")) && tail.trim().chars().count() >= 12 {
                    scope = Scope::Sakura;
                    actions.push(DialogueAction::speaker(scope));
                    append_text(&mut actions, scope, "\n");
                    i += 2;
                    while i < n && matches!(text[i..].chars().next(), Some(' ' | '\t' | '\r' | '\n')) {
                        i += 1;
                    }
                    continue;
                }
                break;
            }
            if nxt == '0' || nxt == 'h' {
                scope = Scope::Sakura;
                actions.push(DialogueAction::speaker(scope));
                i += 2;
                continue;
            }
            if nxt == '1' || nxt == '2' || nxt == 'u' {
                scope = Scope::Kero;
                actions.push(DialogueAction::speaker(scope));
                i += 2;
                continue;
            }
            if nxt == 'c' && !after_slash.starts_with("c[") {
                actions.push(DialogueAction::clear(scope));
                i += 2;
                continue;
            }

            if let Some(m) = p_scope_re().captures(rest) {
                scope = if &m[1] == "1" { Scope::Kero } else { Scope::Sakura };
                actions.push(DialogueAction::speaker(scope));
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = surface_re().captures(rest) {
                let raw_sid = m[1].trim();
                let sid: i32 = if let Ok(n) = raw_sid.parse::<i32>() {
                    n
                } else if let Some(n) = resolve_emotion_keyword_surface(raw_sid, scope) {
                    n
                } else if scope == Scope::Sakura {
                    0
                } else {
                    10
                };
                actions.push(DialogueAction::surface(scope, sid));
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = balloon_re().captures(rest) {
                let bid: i32 = m[1].parse().unwrap_or(0);
                actions.push(DialogueAction::balloon_size(scope, bid));
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = newline_re().find(rest) {
                append_text(&mut actions, scope, "\n");
                i += m.end();
                continue;
            }
            if let Some(m) = wait_ms_re().captures(rest) {
                let ms: u64 = m[1].parse().unwrap_or(0);
                actions.push(DialogueAction::pause(ms));
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = wait_digit_re().captures(rest) {
                let d: u64 = m[1].parse().unwrap_or(1);
                actions.push(DialogueAction::pause(d * 50));
                i += m.get(0).unwrap().end();
                continue;
            }
            if rest.starts_with(r"\_q") {
                actions.push(DialogueAction::quick_toggle(scope));
                i += 3;
                continue;
            }
            if let Some(m) = anchor_link_re().captures(rest) {
                let target = m[1].trim().to_string();
                let raw_label = m[2].trim().to_string();
                let label = if raw_label.is_empty() {
                    target.clone()
                } else {
                    raw_label
                };
                if !label.is_empty() {
                    append_text(&mut actions, scope, &label);
                }
                let lower = target.to_ascii_lowercase();
                if lower.starts_with("http://") || lower.starts_with("https://") {
                    actions.push(DialogueAction::link(label, target));
                } else if !target.is_empty() {
                    actions.push(DialogueAction::choice(label, target));
                }
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = choice_q_re().captures(rest) {
                let inner = &m[1];
                if let Some((lbl, cid)) = inner.rsplit_once(',') {
                    actions.push(DialogueAction::choice(
                        lbl.trim().to_string(),
                        cid.trim().to_string(),
                    ));
                } else {
                    actions.push(DialogueAction::choice(
                        inner.trim().to_string(),
                        inner.trim().to_string(),
                    ));
                }
                i += m.get(0).unwrap().end();
                continue;
            }
            if let Some(m) = benign_tag_re().find(rest) {
                i += m.end();
                continue;
            }
        }
        if rest.starts_with("http://") || rest.starts_with("https://") {
            if let Some(m) = bare_url_re().find(rest) {
                let raw = m.as_str();
                let cleaned = raw
                    .trim_end_matches(&['.', ',', ';', ':', '。', '、', '！', '？', '!', '?'][..]);
                if cleaned.len() > "http://a".len() {
                    append_text(&mut actions, scope, cleaned);
                    actions.push(DialogueAction::link(
                        cleaned.to_string(),
                        cleaned.to_string(),
                    ));
                    i += cleaned.len();
                    continue;
                }
            }
        }

        let mut buf = [0u8; 4];
        append_text(&mut actions, scope, ch.encode_utf8(&mut buf));
        i += ch.len_utf8();
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dual_speaker_choices_and_recovers_after_early_end() {
        let script = "<think>hidden</think>さくら(\\0): \\0\\s[5]안녕!\\n\\e\\1\\s[10]반가워!\\q[날씨,weather]\\e";
        let acts = parse_dialogue(script);
        assert!(acts.iter().any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 5));
        assert!(acts.iter().any(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura && a.text.contains("안녕!")));
        assert!(acts.iter().any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Kero && a.surface_id == 10));
        assert!(acts.iter().any(|a| a.kind == ActionKind::Choice && a.label == "날씨" && a.choice_id == "weather"));
    }

    #[test]
    fn decodes_gemma_byte_fallback_tokens_cleanly() {
        let raw = "\\1\\s[10]よし、仕事せんとな！<0xE3><0x80><0x80>何か手伝うことある？<0xF0><0x9F><0x98><0x8A>\\e";
        let acts = parse_dialogue(raw);
        let kero_text: String = acts
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Kero)
            .map(|a| a.text.as_str())
            .collect();
        assert!(!kero_text.contains("<0x"), "raw byte token leaked: {kero_text}");
        assert!(kero_text.contains('\u{3000}'), "expected fullwidth space in {kero_text}");
        assert!(kero_text.contains('😊'), "expected UTF-8 emoji in {kero_text}");
    }

    #[test]
    fn strips_all_llm_control_html_escaped_and_benign_tags() {
        let raw = "```sakura\npreface</think><|thought|>secret plan<|/thought|><sakura>\\\\0\\\\s[smile]\\w0こんにちは<br/>\\_a[info]案内\\_a\\i[10]</sakura>＼1＼s[10]おう！<0xE3><0x80><0x80><0xFF><|eot_id|><end_of_turn>\\e\n```";
        let acts = parse_dialogue(raw);
        let sak_text: String = acts
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        let kero_text: String = acts
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Kero)
            .map(|a| a.text.as_str())
            .collect();

        assert!(acts.iter().any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 5));
        assert!(acts.iter().any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Kero && a.surface_id == 10));
        assert_eq!(sak_text, "こんにちは\n案内");
        assert_eq!(kero_text, "おう！\u{3000}");
        let combined = format!("{sak_text}{kero_text}");
        for bad in ["preface", "secret", "<|", "|>", "<0x", "<end_of_turn>", "<sakura>", "<br", "\\", "\u{FFFD}", "_a", "i[10]"] {
            assert!(!combined.contains(bad), "leaked `{bad}` in `{combined}`");
        }
    }

    #[test]
    fn strips_leaked_stage_emotion_tags_and_preserves_real_parentheses() {
        let a1 = parse_dialogue(r"\0\s[1](困り/照れ)えっ、な、何するの…。\e");
        assert!(a1
            .iter()
            .any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 1));
        let t1: String = a1
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t1, "えっ、な、何するの…。");

        let a2 = parse_dialogue("(困り/照れ)えっ、な、何するの…。");
        assert!(a2
            .iter()
            .any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 1));
        let t2: String = a2
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t2, "えっ、な、何するの…。");

        let a3 = parse_dialogue(r"\1（ニヤリ/得意げ） おお、ええやん！\e");
        assert!(a3
            .iter()
            .any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Kero && a.surface_id == 11));
        let t3: String = a3
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Kero)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t3, "おお、ええやん！");

        let a4 = parse_dialogue(r"(당황/부끄) 어, 뭐 하는 거야…!\e");
        assert!(a4
            .iter()
            .any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 1));
        let t4: String = a4
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t4, "어, 뭐 하는 거야…!");

        let a5 = parse_dialogue(r"\0\s[0](macOS) 100% Safe Rust だよ（笑）\e");
        let t5: String = a5
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t5, "(macOS) 100% Safe Rust だよ（笑）");

        let a6 = parse_dialogue(r"\0\(照れ\) あ、ちょっと…！\e");
        assert!(a6
            .iter()
            .any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 1));
        let t6: String = a6
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t6, "あ、ちょっと…！");

        let a7 = parse_dialogue(r"\0\（困り/照れ\） \[驚き\]ほんと！？ \(macOS\)\e");
        assert!(a7
            .iter()
            .any(|a| a.kind == ActionKind::Surface && a.scope == Scope::Sakura && a.surface_id == 1));
        let t7: String = a7
            .iter()
            .filter(|a| a.kind == ActionKind::Text && a.scope == Scope::Sakura)
            .map(|a| a.text.as_str())
            .collect();
        assert_eq!(t7, "ほんと！？ (macOS)");
    }
}
