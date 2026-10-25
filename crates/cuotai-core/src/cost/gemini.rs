//! PRD 0030 RF2: Gemini CLI session recordings.
//!
//! Format reference (public, confirmed 2026-10-25): the Gemini CLI session
//! management guide (`docs/cli/session-management.md` in
//! google-gemini/gemini-cli) says sessions are stored in
//! `~/.gemini/tmp/<project_hash>/chats/`, and the recorder
//! (`packages/core/src/services/chatRecordingService.ts`) writes one JSON
//! Lines file per session, `session-<time>-<short id>.jsonl`. The first line
//! is a header with `sessionId`; message lines carry `id`, `timestamp`,
//! `type` and, for `"type": "gemini"`, `model` and `tokens` (`input`,
//! `output`, `cached`, `thoughts`, `tool`, `total`, taken from the API's
//! `promptTokenCount`, `candidatesTokenCount`, `cachedContentTokenCount`,
//! `thoughtsTokenCount`, `toolUsePromptTokenCount`, `totalTokenCount`);
//! `$set`, `$rewindTo` and `$patch` lines edit earlier ones. Only those
//! numbers, the time, the model and a digest of the session are read:
//! `content`, `thoughts`, `toolCalls`, `directories` and `summary` are never
//! deserialized into a value. Legacy single-object `.json` recordings and
//! the `*.tmp-*` / `*.unreadable-*` backups are not read.
//!
//! Google's prompt count includes the cached part, so `input` here is
//! `input - cached + tool`; thinking tokens are billed as output.

use super::source::{anonymous, instant, is_object, model_name, under_home, Entry};
use super::source::{FileState, Line, Source, UNKNOWN_MODEL};
use super::{SourceId, Tokens};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) struct GeminiCli;

#[derive(Deserialize)]
struct Record {
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tokens: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    cached: u64,
    #[serde(default)]
    thoughts: u64,
    #[serde(default)]
    tool: u64,
}

impl Usage {
    fn tokens(&self) -> Tokens {
        Tokens {
            input: self
                .input
                .saturating_sub(self.cached)
                .saturating_add(self.tool),
            output: self.output.saturating_add(self.thoughts),
            cache_read: self.cached,
            cache_write: 0,
            cache_write_1h: 0,
        }
    }
}

impl Source for GeminiCli {
    fn id(&self) -> SourceId {
        SourceId::GeminiCli
    }

    fn default_roots(
        &self,
        home: Option<&Path>,
        _var: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<PathBuf> {
        under_home(home, &[".gemini", "tmp"]).into_iter().collect()
    }

    fn pattern(&self) -> &'static str {
        "**/*.jsonl"
    }

    fn is_log(&self, name: &str) -> bool {
        name.ends_with(".jsonl")
    }

    fn parse(&self, line: &[u8], state: &mut FileState) -> Line {
        if !is_object(line) {
            return Line::Invalid;
        }
        let Ok(record) = serde_json::from_slice::<Record>(line) else {
            return Line::Invalid;
        };
        if let Some(id) = record.session_id.filter(|_| record.kind.is_none()) {
            state.session = Some(anonymous(self.id(), &id));
            return Line::Skip;
        }
        if record.kind.as_deref() != Some("gemini") {
            return Line::Skip;
        }
        let Some(usage) = record.tokens else {
            return Line::Skip;
        };
        let Some(at) = record.timestamp.as_deref().and_then(instant) else {
            return Line::Invalid;
        };
        let model = match record.model.as_deref().map(model_name) {
            Some(Some(model)) => model,
            Some(None) => return Line::Invalid,
            None => state
                .model
                .clone()
                .unwrap_or_else(|| UNKNOWN_MODEL.to_owned()),
        };
        state.model = Some(model.clone());
        let tokens = usage.tokens();
        if tokens.total() == 0 {
            return Line::Skip;
        }
        let session = state.session.clone().unwrap_or_default();
        // The recorder rewrites a file in place (rewinds, patches): a
        // message already counted by its id is not counted again.
        let dedup = record
            .id
            .map(|id| anonymous(self.id(), &format!("{session}\0{id}")));
        Line::Usage(Entry {
            at,
            model,
            tokens,
            session,
            dedup,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#"{"sessionId":"abc-session","projectHash":"SENSITIVE","startTime":"2026-10-01T09:00:00.000Z","lastUpdated":"2026-10-01T09:00:00.000Z","kind":"main","directories":["/Users/ana/SENSITIVE-repo"]}"#;
    const USER: &str = r#"{"id":"m1","timestamp":"2026-10-01T09:00:01.000Z","type":"user","content":[{"text":"SENSITIVE prompt"}]}"#;
    const GEMINI: &str = r#"{"id":"m2","timestamp":"2026-10-01T09:00:05.000Z","type":"gemini","content":"SENSITIVE answer","thoughts":[{"subject":"SENSITIVE","timestamp":"2026-10-01T09:00:03.000Z"}],"tokens":{"input":1000,"output":200,"cached":400,"thoughts":50,"tool":30,"total":1280},"model":"gemini-2.5-pro","toolCalls":[{"id":"t","result":"SENSITIVE /Users/ana/secret"}]}"#;

    fn parse(lines: &[&str]) -> Vec<Line> {
        let mut state = FileState::default();
        lines
            .iter()
            .map(|line| GeminiCli.parse(line.as_bytes(), &mut state))
            .collect()
    }

    #[test]
    fn a_message_yields_only_numbers_the_model_and_digests() {
        let lines = parse(&[HEADER, USER, GEMINI]);
        assert_eq!(lines[0], Line::Skip);
        assert_eq!(lines[1], Line::Skip);
        let Line::Usage(entry) = &lines[2] else {
            panic!("{:?}", lines[2]);
        };
        assert_eq!(entry.model, "gemini-2.5-pro");
        assert_eq!(
            entry.tokens,
            Tokens {
                input: 630,
                output: 250,
                cache_read: 400,
                cache_write: 0,
                cache_write_1h: 0
            }
        );
        let shown = format!("{entry:?}");
        for secret in ["SENSITIVE", "abc-session", "Users", "m2"] {
            assert!(!shown.contains(secret), "{secret} leaked: {shown}");
        }
        assert!(entry.dedup.is_some());
        assert_eq!(entry.at.to_rfc3339(), "2026-10-01T09:00:05+00:00");
    }

    #[test]
    fn edits_and_foreign_lines_are_skipped_or_invalid() {
        let lines = parse(&[
            r#"{"$set":{"lastUpdated":"2026-10-01T09:10:00.000Z"}}"#,
            r#"{"$rewindTo":"m1"}"#,
            r#"{"id":"m3","timestamp":"2026-10-01T09:00:06.000Z","type":"gemini","content":"x","tokens":null}"#,
            "[1,2]",
            "not json",
            r#"{"id":"m4","timestamp":"yesterday","type":"gemini","tokens":{"input":1}}"#,
        ]);
        assert_eq!(&lines[..3], &[Line::Skip, Line::Skip, Line::Skip]);
        assert_eq!(&lines[3..], &[Line::Invalid, Line::Invalid, Line::Invalid]);
    }

    #[test]
    fn a_message_without_a_model_keeps_the_last_one() {
        let lines = parse(&[
            HEADER,
            GEMINI,
            r#"{"id":"m9","timestamp":"2026-10-01T09:01:00.000Z","type":"gemini","tokens":{"input":10,"output":1,"cached":0,"thoughts":0,"tool":0,"total":11}}"#,
        ]);
        let Line::Usage(entry) = &lines[2] else {
            panic!("{:?}", lines[2]);
        };
        assert_eq!(entry.model, "gemini-2.5-pro");
    }

    #[test]
    fn the_same_message_in_a_rewritten_file_has_the_same_digest() {
        let first = parse(&[HEADER, GEMINI]);
        let again = parse(&[HEADER, USER, GEMINI]);
        let (Line::Usage(a), Line::Usage(b)) = (&first[1], &again[2]) else {
            panic!("no usage");
        };
        assert_eq!(a.dedup, b.dedup);
    }

    #[test]
    fn the_roots_and_files() {
        let roots = GeminiCli.default_roots(Some(Path::new("/home/ana")), &|_| None);
        assert_eq!(roots, [PathBuf::from("/home/ana/.gemini/tmp")]);
        assert!(GeminiCli.is_log("session-2026-10-01T09-00-abc12345.jsonl"));
        assert!(!GeminiCli.is_log("session-x.jsonl.unreadable-1"));
        assert!(!GeminiCli.is_log("session-x.json"));
    }
}
