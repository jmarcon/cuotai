//! RF1.2: Claude Code session transcripts.
//!
//! Claude Code writes one JSON object per line to
//! `<config>/projects/<project>/<session>.jsonl` (subagents under the
//! session's directory), where `<config>` is each entry of the
//! comma-separated `CLAUDE_CONFIG_DIR` or, without it, both
//! `~/.config/claude` and `~/.claude` (the directories ccusage documents at
//! <https://ccusage.com/guide/claude/>). An assistant line carries
//! `timestamp`, `sessionId`, `requestId` and `message.{id, model, usage}`
//! with `input_tokens`, `output_tokens`, `cache_creation_input_tokens` and
//! `cache_read_input_tokens` (and `cache_creation.ephemeral_{5m,1h}_input_tokens`
//! when present); `message.content`, `cwd` and every other
//! field are never deserialized into a value.

use super::source::{anonymous, instant, is_object, listed_dirs, model_name, under_home, Entry};
use super::source::{FileState, Line, Source};
use super::{SourceId, Tokens};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) struct ClaudeCode;

/// The only fields read from a line.
#[derive(Deserialize)]
struct Record {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation: Option<CacheCreation>,
}

/// PRD 0030 RF5.1: the cache writes split by lifetime.
#[derive(Deserialize)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

/// Claude Code's placeholder model of locally generated messages.
const SYNTHETIC: &str = "<synthetic>";

impl Source for ClaudeCode {
    fn id(&self) -> SourceId {
        SourceId::ClaudeCode
    }

    fn default_roots(
        &self,
        home: Option<&Path>,
        var: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<PathBuf> {
        if let Some(dirs) = listed_dirs(var, "CLAUDE_CONFIG_DIR", home) {
            return dirs.into_iter().map(|dir| dir.join("projects")).collect();
        }
        [
            under_home(home, &[".config", "claude", "projects"]),
            under_home(home, &[".claude", "projects"]),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn is_log(&self, name: &str) -> bool {
        name.ends_with(".jsonl")
    }

    fn pattern(&self) -> &'static str {
        "**/*.jsonl"
    }

    fn parse(&self, line: &[u8], state: &mut FileState) -> Line {
        let Some(record) = is_object(line)
            .then(|| serde_json::from_slice::<Record>(line).ok())
            .flatten()
        else {
            return Line::Invalid;
        };
        let Some(message) = record.message else {
            return Line::Skip;
        };
        let Some(usage) = message.usage else {
            return Line::Skip;
        };
        if message.model.as_deref() == Some(SYNTHETIC) {
            return Line::Skip;
        }
        // A line with only the total counts it all as 5-minute writes.
        let (cache_write, cache_write_1h) = match &usage.cache_creation {
            Some(split) => (
                split.ephemeral_5m_input_tokens,
                split.ephemeral_1h_input_tokens,
            ),
            None => (usage.cache_creation_input_tokens, 0),
        };
        let tokens = Tokens {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_read: usage.cache_read_input_tokens,
            cache_write,
            cache_write_1h,
        };
        let (Some(at), Some(model)) = (
            record.timestamp.as_deref().and_then(instant),
            message.model.as_deref().and_then(model_name),
        ) else {
            return Line::Invalid;
        };
        if tokens.total() == 0 {
            return Line::Skip;
        }
        let source = self.id();
        if let Some(session) = record.session_id.as_deref() {
            state.session = Some(anonymous(source, session));
        }
        let dedup = match (message.id.as_deref(), record.request_id.as_deref()) {
            (Some(id), Some(request)) => Some(anonymous(source, &format!("{id}\0{request}"))),
            (Some(id), None) => Some(anonymous(source, id)),
            _ => None,
        };
        Line::Usage(Entry {
            at,
            model,
            tokens,
            session: state.session.clone().unwrap_or_default(),
            dedup,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn parse(line: &str) -> Line {
        ClaudeCode.parse(line.as_bytes(), &mut FileState::default())
    }

    const ASSISTANT: &str = r#"{"parentUuid":"p","isSidechain":false,"userType":"external","cwd":"/Users/ana/secret-project","sessionId":"s-1","version":"2.0.14","gitBranch":"main","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-sonnet-4-5-20250929","content":[{"type":"text","text":"SENSITIVE answer"}],"stop_reason":"end_turn","usage":{"input_tokens":12,"cache_creation_input_tokens":300,"cache_read_input_tokens":4000,"output_tokens":56,"service_tier":"standard","cache_creation":{"ephemeral_5m_input_tokens":300,"ephemeral_1h_input_tokens":0}}},"requestId":"req_1","type":"assistant","uuid":"u","timestamp":"2025-10-02T12:30:00.000Z"}"#;

    #[test]
    fn an_assistant_line_yields_its_numbers_only() {
        let Line::Usage(entry) = parse(ASSISTANT) else {
            panic!("usage expected");
        };
        assert_eq!(
            entry.at,
            Utc.with_ymd_and_hms(2025, 10, 2, 12, 30, 0).unwrap()
        );
        assert_eq!(entry.model, "claude-sonnet-4-5-20250929");
        assert_eq!(
            entry.tokens,
            Tokens {
                input: 12,
                output: 56,
                cache_read: 4000,
                cache_write: 300,
                cache_write_1h: 0
            }
        );
        assert_eq!(entry.session, anonymous(SourceId::ClaudeCode, "s-1"));
        assert_ne!(entry.session, "s-1");
        assert!(entry.dedup.is_some());
    }

    #[test]
    fn lines_without_usage_are_skipped_and_broken_ones_invalid() {
        for line in [
            r#"{"type":"user","message":{"role":"user","content":"SENSITIVE prompt"},"timestamp":"2025-10-02T12:00:00Z"}"#,
            r#"{"type":"summary","summary":"SENSITIVE title","leafUuid":"l"}"#,
            r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}},"timestamp":"2025-10-02T12:00:00Z"}"#,
            r#"{"message":{"model":"claude-opus-4-1","usage":{"input_tokens":0,"output_tokens":0}},"timestamp":"2025-10-02T12:00:00Z"}"#,
        ] {
            assert_eq!(parse(line), Line::Skip, "{line}");
        }
        for line in [
            "not json at all",
            r#"["an","array"]"#,
            r#"{"message":{"model":"claude-opus-4-1","usage":{"input_tokens":"SENSITIVE"}},"timestamp":"2025-10-02T12:00:00Z"}"#,
            r#"{"message":{"model":"claude-opus-4-1","usage":{"input_tokens":1}},"timestamp":"yesterday"}"#,
            r#"{"message":{"model":"SENSITIVE text with spaces","usage":{"input_tokens":1}},"timestamp":"2025-10-02T12:00:00Z"}"#,
            r#"{"message":{"usage":{"input_tokens":1}},"timestamp":"2025-10-02T12:00:00Z"}"#,
        ] {
            assert_eq!(parse(line), Line::Invalid, "{line}");
        }
    }

    #[test]
    fn a_copied_request_keeps_its_dedup_digest() {
        let (Line::Usage(a), Line::Usage(b)) = (
            parse(ASSISTANT),
            parse(&ASSISTANT.replace("\"s-1\"", "\"s-2\"")),
        ) else {
            panic!("usage expected");
        };
        assert_eq!(a.dedup, b.dedup);
        assert_ne!(a.session, b.session);
    }

    #[test]
    fn roots_follow_claude_config_dir_or_both_defaults() {
        let home = Path::new("/home/ana");
        let none = |_: &str| None;
        assert_eq!(
            ClaudeCode.default_roots(Some(home), &none),
            [
                home.join(".config/claude/projects"),
                home.join(".claude/projects")
            ]
        );
        let set = |name: &str| {
            (name == "CLAUDE_CONFIG_DIR").then(|| "~/work-claude, /backup/claude".to_owned())
        };
        assert_eq!(
            ClaudeCode.default_roots(Some(home), &set),
            [
                home.join("work-claude/projects"),
                PathBuf::from("/backup/claude/projects")
            ]
        );
        assert!(ClaudeCode.is_log("2f1c.jsonl"));
        assert!(!ClaudeCode.is_log("notes.json"));
    }
}
