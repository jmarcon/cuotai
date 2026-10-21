//! `cuotai --watch` (PRD 0013 RF2): the events of the headless refresh
//! cycle ([`crate::headless`]) on stdout, as NDJSON with `--jsonl` (one
//! `events` payload per line, flushed at once) or as one readable line per
//! transition; messages and diagnostics go to stderr. The engine is the
//! core's ([`cuotai_core::watch::Watcher`]); its baselines live in memory,
//! so a restart starts new ones.

use std::{
    collections::BTreeSet,
    io::{self, Write},
};

use chrono::{DateTime, Utc};
use cuotai_core::{
    events::{Event, EventKind, EventState},
    export::v1::Export,
    text::provider_l10n,
    watch::Watcher,
};

use crate::ui::safe;

/// What `--watch` was given on the command line.
pub(crate) struct WatchArgs {
    /// `--events`: only these (every event without it).
    pub events: Option<BTreeSet<EventKind>>,
    /// `--jsonl`: NDJSON instead of readable lines.
    pub jsonl: bool,
}

pub(crate) struct Watch {
    watcher: Watcher,
    jsonl: bool,
}

impl Watch {
    pub(crate) fn new(args: WatchArgs) -> Self {
        Self {
            watcher: Watcher::new(EventState::default(), args.events),
            jsonl: args.jsonl,
        }
    }

    /// The stderr line that opens the run.
    pub(crate) fn started(&self, accounts: usize) -> String {
        format!(
            "cuotai: acompanhando {accounts} conta(s); a primeira leitura de cada uma é a base \
             e não gera eventos. Ctrl+C encerra."
        )
    }

    /// Write the events of `export` after the last one; `false` when
    /// stdout was closed (the consumer went away), which ends the run.
    pub(crate) fn emit(&mut self, export: &Export, now: DateTime<Utc>) -> io::Result<bool> {
        let events = self.watcher.observe(export, now);
        if events.is_empty() {
            return Ok(true);
        }
        let locale = crate::ui::tui_locale();
        match write_events(&mut io::stdout().lock(), &events, self.jsonl, locale) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// One line per event, flushed after each.
fn write_events(
    out: &mut impl Write,
    events: &[Event],
    jsonl: bool,
    locale: &str,
) -> io::Result<()> {
    for event in events {
        if jsonl {
            serde_json::to_writer(&mut *out, event)?;
            writeln!(out)?;
        } else {
            writeln!(out, "{}", summary_line(event, locale))?;
        }
        out.flush()?;
    }
    Ok(())
}

/// `2026-10-20 14:02:11  Work (Claude): Sessão (5h): 90% usado`, in local
/// time and `locale`, without terminal controls.
fn summary_line(event: &Event, locale: &str) -> String {
    let at = event
        .occurred_at
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S");
    format!(
        "{at}  {} ({}): {}",
        safe(&event.account_name),
        provider_l10n(event.provider).resolve(locale),
        safe(&event.summary().resolve(locale))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{
        events::{MetricRef, Values, SCHEMA},
        MetricWindow, Provider,
    };

    fn event(kind: EventKind, name: &str) -> Event {
        Event {
            schema: SCHEMA,
            event: kind,
            occurred_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap(),
            account_key: format!("claude:{name}"),
            account_name: name.into(),
            provider: Provider::Claude,
            metric: Some(MetricRef {
                label: "Semanal".into(),
                window: Some(MetricWindow::Weekly),
            }),
            current: Values {
                threshold: Some(90),
                ..Values::default()
            },
            previous: None,
        }
    }

    /// RF2.2: one JSON object per line, each with its schema, ready for
    /// `jq` and line-by-line readers.
    #[test]
    fn jsonl_writes_one_payload_per_line() {
        let events = [
            event(EventKind::QuotaThresholdCrossed, "Work"),
            event(EventKind::QuotaExhausted, "Line\nbreak"),
        ];
        let mut out = Vec::new();
        write_events(&mut out, &events, true, "en-US").unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        let parsed: Vec<serde_json::Value> = lines
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed[0]["schema"], 1);
        assert_eq!(parsed[0]["event"], "quota_threshold_crossed");
        assert_eq!(parsed[1]["event"], "quota_exhausted");
        assert_eq!(parsed[1]["account_name"], "Line\nbreak");
    }

    /// RF2.3: a readable line per transition, in the chosen language and
    /// without terminal controls from the account name.
    #[test]
    fn summary_names_the_account_and_the_transition() {
        let mut out = Vec::new();
        write_events(
            &mut out,
            &[event(EventKind::QuotaThresholdCrossed, "Wo\x1brk")],
            false,
            "en-US",
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.ends_with("  Work (Claude): Semanal: 90% used\n"),
            "{text:?}"
        );
    }
}
