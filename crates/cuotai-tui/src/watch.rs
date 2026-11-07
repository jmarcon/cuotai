//! `cuotai --watch` (PRD 0013 RF2): the events of the headless refresh
//! cycle ([`crate::headless`]) on stdout, as NDJSON with `--jsonl` (one
//! `events` payload per line, flushed at once) or as one readable line per
//! transition; messages and diagnostics go to stderr. The engine is the
//! core's ([`cuotai_core::watch::Watcher`]); its baselines live in memory,
//! so a restart starts new ones.
//!
//! With `--hooks` (PRD 0013 RF3) the same events also run the confirmed
//! rules of `hooks.toml` through the core's [`Runner`] (without `--jsonl`
//! stdout then stays empty); [`confirmed_hooks`] shows the exact commands
//! and asks for the confirmation the core requires before any of them runs.

use std::{
    collections::BTreeSet,
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use chrono::{DateTime, Utc};
use cuotai_core::{
    alerts::Lead,
    events::{Event, EventKind, EventState},
    export::v1::Export,
    hooks::{self, Confirmed, Consent, Hooks, Run, RunLog, Runner},
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
    /// `--hooks`: the rules the user confirmed.
    pub hooks: Option<Confirmed>,
    /// PRD 0024 RF5.1: `--projected-lead` of `quota_exhaustion_projected`
    /// (the core's 60 minutes without it).
    pub projected_lead: Option<Lead>,
    /// `--reset-lead`: minutes before a window resets that
    /// `quota_reset_soon` fires (0 turns it off).
    pub reset_lead: u32,
    /// PRD 0034 RF5.2: the language of the `fallback` texts of `--jsonl`.
    pub fallback_locale: Option<String>,
}

pub(crate) struct Watch {
    watcher: Watcher,
    jsonl: bool,
    fallback_locale: Option<String>,
    hooks: Option<Runner>,
    /// PRD 0032 RF5.1: the desktop's quiet hours, which a hook rule with
    /// `quiet = "skip"` honours (the stream itself is never held).
    quiet: cuotai_core::notify_prefs::Source,
    /// Started with `--hooks`: readable lines stay off stdout even while
    /// the rules are suspended by a reload (PRD 0031 RF12).
    with_hooks: bool,
}

impl Watch {
    /// Must run inside the Tokio runtime (the hooks' runner starts here).
    pub(crate) fn new(args: WatchArgs) -> Self {
        Self {
            watcher: Watcher::new(
                match args.projected_lead {
                    Some(lead) => EventState::default().with_projected_lead(lead),
                    None => EventState::default(),
                }
                .with_reset_lead(chrono::TimeDelta::minutes(i64::from(args.reset_lead))),
                args.events,
            ),
            jsonl: args.jsonl,
            fallback_locale: args.fallback_locale,
            with_hooks: args.hooks.is_some(),
            hooks: args.hooks.map(start_hooks),
            quiet: cuotai_core::notify_prefs::Source::open(),
        }
    }

    /// PRD 0031 RF12: replace the running hooks (`None` suspends them).
    /// Only meaningful when the run started with `--hooks`.
    pub(crate) fn set_hooks(&mut self, hooks: Option<Confirmed>) {
        self.hooks = hooks.map(start_hooks);
    }

    /// The stderr line that opens the run.
    pub(crate) fn started(&self, accounts: usize) -> String {
        let mut line = crate::l10n::trf(
            "watch.started",
            "cuotai: watching {count} account(s); the first reading of each one is the baseline \
             and emits no events. Ctrl+C stops.",
            &[("count", &accounts)],
        );
        if let Some(runner) = &self.hooks {
            line.push_str(&crate::l10n::trf(
                "watch.hooks_active",
                " {count} hook(s) active.",
                &[("count", &runner.rules().len())],
            ));
        }
        line
    }

    /// Write the events of `export` after the last one (and hand them to
    /// the hooks, which never wait); `false` when stdout was closed (the
    /// consumer went away), which ends the run.
    pub(crate) fn emit(
        &mut self,
        export: &Export,
        now: DateTime<Utc>,
        pace: &crate::notify::PaceOf,
    ) -> io::Result<bool> {
        // PRD 0045 RF5.2: rows have events only for the accounts whose
        // "Alert on breakdown rows" is on in the desktop's settings.
        self.quiet.refresh();
        let rows = self.quiet.loaded().alerts.breakdown_each.clone();
        self.watcher.set_breakdown_alerts(rows);
        let observed = self.watcher.observe_transitions(export, now, pace);
        if observed.is_empty() {
            return Ok(true);
        }
        let locale = crate::ui::tui_locale();
        if let Some(runner) = &self.hooks {
            let quiet = &mut self.quiet;
            let (mut dropped, mut unclaimed) = (0, 0);
            for event in &observed {
                // PRD 0054 RF2: claimed across processes before it is queued.
                let submitted = runner.submit_in(event, quiet.quiet_at(event.event.occurred_at));
                dropped += submitted.dropped;
                unclaimed += submitted.unclaimed;
            }
            if dropped > 0 {
                crate::cli::message_with(
                    crate::cli::Level::Warn,
                    "hooks.dropped",
                    &[("count", crate::cli::Field::Count(dropped as u64))],
                    format_args!("cuotai: {}", hooks::dropped(dropped).resolve(locale)),
                );
            }
            if unclaimed > 0 {
                eprintln!("cuotai: {}", hooks::claims_unavailable().resolve(locale));
            }
        }
        let events: Vec<Event> = observed
            .into_iter()
            .map(|observed| observed.event)
            .collect();
        if self.with_hooks && !self.jsonl {
            return Ok(true);
        }
        let jsonl = self.jsonl.then_some(self.fallback_locale.as_deref());
        match write_events(&mut io::stdout().lock(), &events, jsonl, locale) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// The runner of `confirmed`, logging to the local data directory; a run
/// that did not succeed (and a log that cannot be written, once) is one
/// stderr line.
fn start_hooks(confirmed: Confirmed) -> Runner {
    let log = RunLog::in_data_dir();
    let log_path = log.as_ref().map(|log| log.path().to_path_buf());
    let warned = AtomicBool::new(false);
    let on_run = move |run: &Run, log_error: Option<&io::Error>| {
        let locale = crate::ui::tui_locale();
        if !run.succeeded() {
            crate::cli::message(
                crate::cli::Level::Warn,
                "hooks.run_failed",
                format_args!("cuotai: hook {}", safe(&run.summary().resolve(locale))),
            );
        }
        if let (Some(_), Some(path)) = (log_error, &log_path) {
            if !warned.swap(true, Ordering::Relaxed) {
                crate::cli::message(
                    crate::cli::Level::Warn,
                    "hooks.log_unwritable",
                    format_args!("cuotai: {}", hooks::log_unwritable(path).resolve(locale)),
                );
            }
        }
    };
    Runner::start(confirmed, log, Arc::new(on_run))
}

/// PRD 0013 RF3.2: the rules of the `hooks.toml` beside the accounts' TOML
/// at `config`, once they are confirmed exactly as they are. Unconfirmed
/// (never, or since a change) they are listed on stderr and confirmed by
/// typing the answer in a terminal; without one it is a local error.
pub(crate) fn confirmed_hooks(config: &Path, locale: &str) -> Result<Confirmed, String> {
    let hooks = Hooks::load(&Hooks::path_beside(config)).map_err(|error| error.resolve(locale))?;
    let consent = Consent::in_data_dir().map_err(|error| error.resolve(locale))?;
    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    confirm(
        &consent,
        hooks,
        interactive,
        &mut io::stdin().lock(),
        &mut io::stderr(),
        locale,
    )
}

fn confirm(
    consent: &Consent,
    hooks: Hooks,
    interactive: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
    locale: &str,
) -> Result<Confirmed, String> {
    let hooks = match consent.check(hooks) {
        Ok(confirmed) => return Ok(confirmed),
        Err(hooks) => hooks,
    };
    let io_error = |error: io::Error| error.to_string();
    // PRD 0050 RF2.4: the listing and the digest confirmed come from one
    // resolution of the programs; one that cannot be pinned is listed as
    // such and refused.
    let (lines, shown) = match hooks.approval() {
        Ok((lines, digest)) => (lines, Ok(digest)),
        Err(error) => (hooks.listing(), Err(error)),
    };
    for line in lines {
        let indent = match &*line.id {
            "hooks.confirm.header" => "",
            "hooks.confirm.rule" => "  ",
            _ => "     ",
        };
        writeln!(out, "{indent}{}", line.resolve(locale)).map_err(io_error)?;
    }
    let shown = shown.map_err(|error| error.resolve(locale))?;
    if !interactive {
        return Err(hooks::not_confirmed(&hooks).resolve(locale));
    }
    let answer = hooks::confirm_answer().resolve(locale);
    write!(out, "{} ", hooks::confirm_question(&answer).resolve(locale)).map_err(io_error)?;
    out.flush().map_err(io_error)?;
    let mut typed = String::new();
    input.read_line(&mut typed).map_err(io_error)?;
    if !cuotai_core::text::answer_matches(&typed, &answer) {
        return Err(hooks::declined().resolve(locale));
    }
    consent
        .confirm_shown(hooks, &shown)
        .map_err(|error| error.resolve(locale))
}

/// One line per event, flushed after each.
/// `jsonl`: NDJSON, with the `fallback` texts in that language (English
/// without one), instead of readable lines.
fn write_events(
    out: &mut impl Write,
    events: &[Event],
    jsonl: Option<Option<&str>>,
    locale: &str,
) -> io::Result<()> {
    for event in events {
        if let Some(fallback_locale) = jsonl {
            let mut value = serde_json::to_value(event)?;
            if let Some(fallback_locale) = fallback_locale {
                cuotai_core::text::localize_json_fallbacks(&mut value, fallback_locale);
            }
            serde_json::to_writer(&mut *out, &value)?;
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
            account_id: "acc_test00000000".into(),
            account_name: name.into(),
            accounts: Vec::new(),
            provider: Provider::Claude,
            parent: None,
            metric: Some(MetricRef::new("Semanal", Some(MetricWindow::Weekly))),
            current: Values {
                threshold: Some(90),
                ..Values::default()
            },
            previous: None,
            sample: false,
            row: None,
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
        write_events(&mut out, &events, Some(None), "en-US").unwrap();
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
            None,
            "en-US",
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.ends_with("  Work (Claude): Semanal: 90% used\n"),
            "{text:?}"
        );
    }

    fn rules(dir: &Path, command: &str) -> Hooks {
        Hooks::from_toml(
            dir.join("hooks.toml"),
            &format!(
                "[[hooks]]\nname = \"say\"\nevents = [\"quota_exhausted\"]\ncommand = {command}\n"
            ),
        )
        .unwrap()
        // PRD 0050 RF2.4: `say` is the fixture program in `dir`.
        .with_search_path(Some(dir.as_os_str().to_owned()))
    }

    fn fixture_program(dir: &Path) {
        let say = dir.join("say");
        std::fs::write(&say, "fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&say, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// PRD 0050 RF2.4: a program not found on the captured `PATH` is
    /// listed as such and refused before any question.
    #[test]
    fn an_unresolved_program_is_refused_before_asking() {
        let dir = tempfile::tempdir().unwrap();
        let consent = Consent::at(dir.path().join("consent.json"));
        let mut out = Vec::new();
        let error = confirm(
            &consent,
            rules(dir.path(), r#"["say"]"#),
            true,
            &mut "yes\n".as_bytes(),
            &mut out,
            "en-US",
        )
        .unwrap_err();
        assert!(
            error.contains("not an executable file found on PATH"),
            "{error}"
        );
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("program: not found on PATH"), "{out}");
        assert!(!out.contains("type"), "nothing asked: {out}");
    }

    /// PRD 0013 RF3.2: unconfirmed rules are listed with their exact
    /// commands; only the typed answer confirms them (and nothing is asked
    /// again until they change), any other answer or no terminal refuses.
    #[test]
    fn hooks_run_only_after_the_typed_confirmation() {
        let dir = std::env::temp_dir().join(format!("cuotai-tui-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        fixture_program(&dir);
        let consent = Consent::at(dir.join("consent.json"));
        let original = r#"["say", "a; b"]"#;
        let ask = |interactive, typed: &str, command| {
            let mut out = Vec::new();
            let result = confirm(
                &consent,
                rules(&dir, command),
                interactive,
                &mut typed.as_bytes(),
                &mut out,
                "en-US",
            );
            (result.map(|_| ()), String::from_utf8(out).unwrap())
        };

        let (result, out) = ask(false, "yes\n", original);
        let error = result.unwrap_err();
        assert!(error.contains("not confirmed yet"), "{error}");
        assert!(out.contains(r#"     command: ["say","a; b"]"#), "{out}");
        assert!(
            !out.contains("type"),
            "no question without a terminal: {out}"
        );

        for typed in ["", "no\n", "y\n", "yes please\n"] {
            let (result, out) = ask(true, typed, original);
            assert_eq!(result.unwrap_err(), "Hooks not confirmed; nothing was run.");
            assert!(out.ends_with("(any other answer declines): "), "{out}");
        }
        assert!(consent.check(rules(&dir, original)).is_err());

        let (result, _) = ask(true, " YES \n", original);
        assert!(result.is_ok());
        let (result, out) = ask(false, "", original);
        assert!(result.is_ok());
        assert_eq!(out, "", "confirmed rules are not listed again");

        let changed = r#"["say", "a; c"]"#;
        let (result, out) = ask(false, "", changed);
        assert!(
            result.is_err(),
            "a changed command needs a new confirmation"
        );
        assert!(out.contains("a; c"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
