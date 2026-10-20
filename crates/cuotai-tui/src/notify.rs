//! PRD 0003 RF4: quota reset notices while the TUI is open (`--notify`).
//!
//! The core plans the resets (`resets::plan`, the desktop's planner) and
//! writes the texts; here they become a header banner, the terminal bell
//! (`BEL`) and, only on terminals known to show it, an OSC 9 notification,
//! so an unknown sequence never reaches (and dirties) another screen.
//! Nothing persists: what was notified lives for the session, and a reset
//! that passed before the TUI opened is never announced. No provider is
//! queried for a notice.

use crate::ui::{local_time, safe};
use chrono::{DateTime, TimeDelta, Utc};
use cuotai_core::{
    resets::{self, Ledger, Prefs},
    Config, Report,
};

/// How long the header keeps the last notice.
const BANNER_KEEP: TimeDelta = TimeDelta::hours(1);
const BEL: u8 = 0x07;

pub(crate) struct Notices {
    /// The planner's defaults: at the reset, only if used, no `other`.
    prefs: Prefs,
    /// What this session already notified (or skipped); never saved.
    ledger: Ledger,
    started: DateTime<Utc>,
    osc9: bool,
    /// When the last notice went out and its header line.
    banner: Option<(DateTime<Utc>, String)>,
}

impl Notices {
    /// A session opened at `started`; `osc9` from [`osc9_supported`].
    pub(crate) fn new(started: DateTime<Utc>, osc9: bool) -> Self {
        Self {
            prefs: Prefs::default(),
            ledger: Ledger::default(),
            started,
            osc9,
            banner: None,
        }
    }

    /// One pass at `now` over `readings` (one per `config.accounts`): the
    /// resets due become the banner and the bytes to write to the terminal,
    /// one OSC 9 per notice when supported and then a single `BEL`; empty
    /// when nothing is due. Each reset is announced once per session.
    pub(crate) fn tick(
        &mut self,
        config: &Config,
        readings: &[Option<&Report>],
        now: DateTime<Utc>,
        locale: &str,
    ) -> Vec<u8> {
        self.ledger.prune(now);
        let plan = resets::plan(
            config,
            readings.iter().copied(),
            now,
            &self.prefs,
            &self.ledger,
        );
        let mut out = Vec::new();
        let mut lines = Vec::new();
        for mut event in plan.due {
            self.ledger.record(&event, now);
            // A reset whose notice time came before the session opened
            // (a reading restored from the history) is not this session's.
            let (lead, started) = (self.prefs.lead, self.started);
            event
                .metrics
                .retain(|metric| metric.resets_at - lead >= started);
            let Some(first) = event.metrics.first() else {
                continue;
            };
            event.resets_at = first.resets_at;
            event.notify_at = event.resets_at - lead;
            let account = &config.accounts[event.account_index];
            let notice = event.notice(account, now);
            let title = notice.title.resolve(locale);
            lines.push(format!("{} ({})", safe(&title), safe(&account.name)));
            if self.osc9 {
                let text = format!("{title} — {}", notice.body.resolve(locale));
                out.extend_from_slice(b"\x1b]9;");
                out.extend_from_slice(safe(&text).as_bytes());
                out.extend_from_slice(b"\x1b\\");
            }
        }
        if lines.is_empty() {
            return out;
        }
        out.push(BEL);
        self.banner = Some((now, format!("{} · {}", local_time(now), lines.join(" · "))));
        out
    }

    /// The header line of the last notice, for [`BANNER_KEEP`].
    pub(crate) fn banner(&self, now: DateTime<Utc>) -> Option<&str> {
        self.banner
            .as_ref()
            .filter(|(at, _)| now - *at < BANNER_KEEP)
            .map(|(_, line)| line.as_str())
    }
}

/// OSC 9 shows a desktop notification on iTerm2, WezTerm and Ghostty
/// (`TERM_PROGRAM`); elsewhere, and inside tmux or screen, it is never sent.
pub(crate) fn osc9_supported(var: impl Fn(&str) -> Option<String>) -> bool {
    if var("TMUX").is_some() || var("STY").is_some() {
        return false;
    }
    matches!(
        var("TERM_PROGRAM").as_deref(),
        Some("iTerm.app" | "WezTerm" | "ghostty")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{Account, Metric, MetricKind, MetricWindow, Provider, Status};

    fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 15, 12, 0, 0).unwrap()
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    fn account(name: &str) -> Account {
        Account {
            name: name.into(),
            provider: Provider::Claude,
            ..Account::default()
        }
    }

    fn report(account: &Account, used: f64, resets_at: DateTime<Utc>) -> Report {
        Report {
            account: account.name.clone(),
            provider: account.provider,
            checked_at: start(),
            status: Status::Ready,
            metrics: vec![Metric {
                label: "5h".into(),
                used: Some(used),
                limit: Some(100.0),
                remaining: None,
                unit: "%".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Window5h),
                period: None,
                resets_at: Some(resets_at),
            }],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    fn config(accounts: Vec<Account>) -> Config {
        Config {
            accounts,
            ..Config::default()
        }
    }

    fn bells(bytes: &[u8]) -> usize {
        bytes.iter().filter(|b| **b == BEL).count()
    }

    #[test]
    fn rings_once_at_the_reset_and_keeps_the_banner_for_an_hour() {
        let config = config(vec![account("Pessoal")]);
        let reading = report(&config.accounts[0], 40.0, start() + mins(1));
        let mut notices = Notices::new(start(), false);

        let before = notices.tick(
            &config,
            &[Some(&reading)],
            start() + mins(1) - TimeDelta::seconds(1),
            "en-US",
        );
        assert!(before.is_empty());
        assert_eq!(notices.banner(start()), None);

        let at = start() + mins(1);
        assert_eq!(
            notices.tick(&config, &[Some(&reading)], at, "en-US"),
            vec![BEL]
        );
        let banner = notices.banner(at).expect("banner after the notice");
        assert!(banner.contains("5-hour quota reset (Pessoal)"), "{banner}");

        // Same reading, later passes and drift within the tolerance: silent.
        let drifted = report(&config.accounts[0], 40.0, at + TimeDelta::seconds(90));
        for later in [at + TimeDelta::seconds(1), at + mins(5)] {
            assert!(notices
                .tick(&config, &[Some(&drifted)], later, "en-US")
                .is_empty());
        }
        assert!(notices.banner(at + mins(59)).is_some());
        assert_eq!(notices.banner(at + mins(60)), None);
    }

    #[test]
    fn an_unused_quota_or_a_reset_before_the_session_is_not_announced() {
        let config = config(vec![account("a"), account("b")]);
        let idle = report(&config.accounts[0], 0.0, start() + mins(1));
        // Restored from the history: it reset while the TUI was closed.
        let missed = report(&config.accounts[1], 50.0, start() - mins(3));
        let mut notices = Notices::new(start(), true);
        for at in [start(), start() + mins(2)] {
            assert!(notices
                .tick(&config, &[Some(&idle), Some(&missed)], at, "en-US")
                .is_empty());
        }
        assert_eq!(notices.banner(start() + mins(2)), None);
    }

    #[test]
    fn accounts_due_together_share_one_bell_and_one_osc9_each() {
        let config = config(vec![account("a\u{1b}[2J"), account("b")]);
        let at = start() + mins(10);
        let readings = [
            report(&config.accounts[0], 10.0, at),
            report(&config.accounts[1], 20.0, at),
        ];
        // A long suspension: the next pass comes 5 minutes late.
        let mut notices = Notices::new(start(), true);
        let out = notices.tick(
            &config,
            &[Some(&readings[0]), Some(&readings[1])],
            at + mins(5),
            "en-US",
        );
        let text = String::from_utf8(out.clone()).unwrap();
        assert_eq!(bells(&out), 1);
        assert_eq!(text.matches("\x1b]9;").count(), 2);
        assert_eq!(text.matches("\x1b\\").count(), 2);
        assert!(text.ends_with('\x07'));
        // Control bytes from an account name never reach the terminal.
        assert!(!text.contains("\x1b[2J"));
        assert!(text.contains("a[2J"));
        let banner = notices.banner(at + mins(5)).unwrap();
        assert!(banner.contains("(a[2J) · "), "{banner}");
        assert!(banner.ends_with("(b)"), "{banner}");
    }

    #[test]
    fn osc9_only_on_terminals_that_show_it() {
        let supported = |pairs: &[(&str, &str)]| {
            osc9_supported(|key| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_string())
            })
        };
        for program in ["iTerm.app", "WezTerm", "ghostty"] {
            assert!(supported(&[("TERM_PROGRAM", program)]), "{program}");
        }
        assert!(!supported(&[("TERM_PROGRAM", "Apple_Terminal")]));
        assert!(!supported(&[("TERM_PROGRAM", "vscode")]));
        assert!(!supported(&[]));
        assert!(!supported(&[
            ("TERM_PROGRAM", "iTerm.app"),
            ("TMUX", "/tmp/tmux-501/default,1,0")
        ]));
        assert!(!supported(&[
            ("TERM_PROGRAM", "WezTerm"),
            ("STY", "1.pts-0")
        ]));
    }
}
