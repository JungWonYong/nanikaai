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
}

fn think_complete_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<(?:think|thought)>[\s\S]*?</(?:think|thought)>").unwrap())
}

fn think_open_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<(?:think|thought)>[\s\S]*$").unwrap())
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
    RE.get_or_init(|| Regex::new(r"^\\s\[(-?\d+)\]").unwrap())
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
    RE.get_or_init(|| Regex::new(r"^\\_w\[(\d+)\]").unwrap())
}

fn wait_digit_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\w([1-9])").unwrap())
}

fn choice_q_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\q\[([^\]\r\n]+)\]").unwrap())
}

fn benign_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\\(?:_q|_s|!\[[^\]]*\]|f\[[^\]]*\]|[txv])").unwrap())
}

fn more_dialogue_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\\(?:[012hu]|p\[\d+\]|s\[-?\d+\])|\[(?:\\?[01]|sakura|kero):-?\d+\]").unwrap()
    })
}

pub fn strip_reasoning_blocks(raw: &str) -> String {
    let pass1 = think_complete_re().replace_all(raw, "");
    let pass2 = think_open_re().replace_all(&pass1, "");
    pass2.trim().to_string()
}

pub fn normalize_sakura_script(text: &str) -> String {
    // Strip speaker prefixes such as "さくら(\0): " or "\nうにゅう(\1): " safely without lookbehind.
    let mut out = String::with_capacity(text.len());
    for (idx, line) in text.split('\n').enumerate() {
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
    out.trim().to_string()
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
            if nxt == 'c' {
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
                let sid: i32 = m[1].parse().unwrap_or(0);
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
}
