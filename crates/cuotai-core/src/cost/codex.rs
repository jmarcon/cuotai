//! RF1.2: Codex CLI session rollouts.
//!
//! Codex CLI writes `rollout-<time>-<id>.jsonl` files under
//! `<home>/sessions/YYYY/MM/DD/` (and `<home>/archived_sessions/`), where
//! `<home>` is each entry of the comma-separated `CODEX_HOME` or `~/.codex`
//! (<https://ccusage.com/guide/codex/>). Every line is
//! `{"timestamp", "type", "payload"}` (`codex-rs/history/src/rollout_payload.rs`
//! in openai/codex): `session_meta` names the session (`payload.id`),
//! `turn_context` the model in effect (`payload.model`), and an `event_msg`
//! whose `payload.type` is `token_count` carries `payload.info` with
//! `total_token_usage` and `last_token_usage` (`input_tokens`,
//! `cached_input_tokens`, `cache_write_input_tokens` since 0.145.0,
//! `output_tokens`, `reasoning_output_tokens`, `total_tokens`;
//! `codex-rs/protocol/src/protocol.rs`). Messages, instructions, working
//! directories and tool output are never deserialized into a value.
//! Compressed `.jsonl.zst` rollouts are not read.

use super::source::{anonymous, instant, is_object, listed_dirs, model_name, under_home, Entry};
use super::source::{FileState, Line, Source, UNKNOWN_MODEL};
use super::{SourceId, Tokens};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) struct CodexCli;

#[derive(Deserialize)]
struct Head {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct With<T> {
    payload: T,
}

#[derive(Deserialize)]
struct SessionMeta {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct TurnContext {
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct EventKind {
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct TokenCount {
    #[serde(default)]
    info: Option<Info>,
}

#[derive(Deserialize)]
struct Info {
    total_token_usage: Usage,
    last_token_usage: Usage,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

impl Usage {
    /// `input_tokens` includes the cached and cache-written parts, and
    /// `output_tokens` the reasoning ones (OpenAI's usage semantics).
    fn tokens(&self) -> Tokens {
        Tokens {
            input: self
                .input_tokens
                .saturating_sub(self.cached_input_tokens)
                .saturating_sub(self.cache_write_input_tokens),
            output: self.output_tokens,
            cache_read: self.cached_input_tokens,
            cache_write: self.cache_write_input_tokens,
        }
    }

    fn total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens.saturating_add(self.output_tokens)
        }
    }
}

fn payload<'a, T: Deserialize<'a>>(line: &'a [u8]) -> Option<T> {
    serde_json::from_slice::<With<T>>(line)
        .ok()
        .map(|with| with.payload)
}

impl Source for CodexCli {
    fn id(&self) -> SourceId {
        SourceId::CodexCli
    }

    fn default_roots(
        &self,
        home: Option<&Path>,
        var: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<PathBuf> {
        let homes = listed_dirs(var, "CODEX_HOME", home)
            .unwrap_or_else(|| under_home(home, &[".codex"]).into_iter().collect());
        homes
            .into_iter()
            .flat_map(|dir| [dir.join("sessions"), dir.join("archived_sessions")])
            .collect()
    }

    fn pattern(&self) -> &'static str {
        "**/rollout-*.jsonl"
    }

    fn is_log(&self, name: &str) -> bool {
        name.starts_with("rollout-") && name.ends_with(".jsonl")
    }

    fn parse(&self, line: &[u8], state: &mut FileState) -> Line {
        let Some(head) = is_object(line)
            .then(|| serde_json::from_slice::<Head>(line).ok())
            .flatten()
        else {
            return Line::Invalid;
        };
        let source = self.id();
        match head.kind.as_deref() {
            Some("session_meta") => match payload::<SessionMeta>(line) {
                Some(meta) => {
                    if let Some(id) = meta.id {
                        state.session = Some(anonymous(source, &id));
                    }
                    Line::Skip
                }
                None => Line::Invalid,
            },
            Some("turn_context") => {
                match payload::<TurnContext>(line).map(|turn| turn.model.as_deref().map(model_name))
                {
                    Some(Some(Some(model))) => {
                        state.model = Some(model);
                        Line::Skip
                    }
                    Some(None) => Line::Skip,
                    _ => Line::Invalid,
                }
            }
            Some("event_msg") => {
                let Some(event) = payload::<EventKind>(line) else {
                    return Line::Invalid;
                };
                if event.kind.as_deref() != Some("token_count") {
                    return Line::Skip;
                }
                let Some(count) = payload::<TokenCount>(line) else {
                    return Line::Invalid;
                };
                let Some(info) = count.info else {
                    return Line::Skip;
                };
                let Some(at) = head.timestamp.as_deref().and_then(instant) else {
                    return Line::Invalid;
                };
                // Codex repeats the last snapshot when nothing new was
                // used; the cumulative total tells the repetition apart.
                let total = info.total_token_usage.total();
                if state.total == Some(total) {
                    return Line::Skip;
                }
                state.total = Some(total);
                let tokens = info.last_token_usage.tokens();
                if tokens.total() == 0 {
                    return Line::Skip;
                }
                let session = state.session.clone().unwrap_or_default();
                Line::Usage(Entry {
                    at,
                    model: state
                        .model
                        .clone()
                        .unwrap_or_else(|| UNKNOWN_MODEL.to_owned()),
                    tokens,
                    // The same rollout under `archived_sessions/` repeats
                    // the session's cumulative totals: counted once.
                    dedup: Some(anonymous(source, &format!("{session}\0{total}"))),
                    session,
                })
            }
            _ => Line::Skip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"{"timestamp":"2025-10-02T09:00:00.000Z","type":"session_meta","payload":{"id":"0199a-session","timestamp":"2025-10-02T09:00:00.000Z","cwd":"/Users/ana/SENSITIVE-repo","originator":"codex_cli_rs","cli_version":"0.46.0","instructions":"SENSITIVE instructions","git":{"commit_hash":"abc","branch":"main","repository_url":"git@example.com:ana/secret.git"}}}"#;
    const TURN: &str = r#"{"timestamp":"2025-10-02T09:00:01.000Z","type":"turn_context","payload":{"cwd":"/Users/ana/SENSITIVE-repo","approval_policy":"on-request","sandbox_policy":{"mode":"workspace-write"},"model":"gpt-5-codex","effort":"medium","summary":"auto"}}"#;
    const MESSAGE: &str = r#"{"timestamp":"2025-10-02T09:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"SENSITIVE prompt"}]}}"#;

    fn count(total: u64, input: u64, cached: u64, output: u64) -> String {
        format!(
            r#"{{"timestamp":"2025-10-02T09:01:00.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{total},"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"reasoning_output_tokens":7,"total_tokens":{}}},"model_context_window":272000}},"rate_limits":{{"primary":{{"used_percent":12.0,"window_minutes":300}}}}}}}}"#,
            input + output
        )
    }

    #[test]
    fn a_rollout_yields_token_counts_under_the_turn_model() {
        let mut state = FileState::default();
        for line in [META, TURN, MESSAGE] {
            assert_eq!(CodexCli.parse(line.as_bytes(), &mut state), Line::Skip);
        }
        let Line::Usage(entry) =
            CodexCli.parse(count(1500, 1200, 1000, 300).as_bytes(), &mut state)
        else {
            panic!("usage expected");
        };
        assert_eq!(entry.model, "gpt-5-codex");
        assert_eq!(
            entry.tokens,
            Tokens {
                input: 200,
                output: 300,
                cache_read: 1000,
                cache_write: 0
            }
        );
        assert_eq!(
            entry.session,
            anonymous(SourceId::CodexCli, "0199a-session")
        );
        // A repeated snapshot (same cumulative total) is not new usage.
        assert_eq!(
            CodexCli.parse(count(1500, 1200, 1000, 300).as_bytes(), &mut state),
            Line::Skip
        );
        assert!(matches!(
            CodexCli.parse(count(1600, 100, 0, 0).as_bytes(), &mut state),
            Line::Usage(_)
        ));
    }

    #[test]
    fn usage_without_a_turn_context_has_an_unknown_model() {
        let mut state = FileState::default();
        let Line::Usage(entry) = CodexCli.parse(count(10, 10, 0, 0).as_bytes(), &mut state) else {
            panic!("usage expected");
        };
        assert_eq!(entry.model, UNKNOWN_MODEL);
    }

    #[test]
    fn broken_lines_are_invalid_and_other_events_skipped() {
        let mut state = FileState::default();
        let null_info = r#"{"timestamp":"2025-10-02T09:01:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#;
        let other = r#"{"timestamp":"2025-10-02T09:01:00Z","type":"event_msg","payload":{"type":"agent_message","message":"SENSITIVE reply"}}"#;
        for line in [null_info, other, r#"{"no":"type"}"#] {
            assert_eq!(
                CodexCli.parse(line.as_bytes(), &mut state),
                Line::Skip,
                "{line}"
            );
        }
        let bad_model = TURN.replace("gpt-5-codex", "SENSITIVE free text");
        let bad_time = count(5, 5, 0, 0).replace("2025-10-02T09:01:00.000Z", "SENSITIVE");
        let bad_count = r#"{"timestamp":"2025-10-02T09:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":"SENSITIVE"}}}}"#;
        for line in [
            "{truncated",
            bad_model.as_str(),
            bad_time.as_str(),
            bad_count,
        ] {
            assert_eq!(
                CodexCli.parse(line.as_bytes(), &mut state),
                Line::Invalid,
                "{line}"
            );
        }
        assert_eq!(state.model, None);
    }

    #[test]
    fn roots_are_sessions_and_archives_of_each_codex_home() {
        let home = Path::new("/home/ana");
        assert_eq!(
            CodexCli.default_roots(Some(home), &|_| None),
            [
                home.join(".codex/sessions"),
                home.join(".codex/archived_sessions")
            ]
        );
        let set = |name: &str| (name == "CODEX_HOME").then(|| "/srv/codex".to_owned());
        assert_eq!(
            CodexCli.default_roots(Some(home), &set),
            [
                PathBuf::from("/srv/codex/sessions"),
                PathBuf::from("/srv/codex/archived_sessions")
            ]
        );
        assert!(CodexCli.is_log("rollout-2025-10-02T09-00-00-0199a.jsonl"));
        assert!(!CodexCli.is_log("rollout-2025-10-02.jsonl.zst"));
        assert!(!CodexCli.is_log("history.jsonl"));
    }
}
