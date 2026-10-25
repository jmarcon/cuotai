//! PRD 0003 RF4 and PRD 0008 RF4: quota reset notices and usage alerts
//! while the TUI is open (`--notify`).
//!
//! The core plans the resets (`resets::plan`, the desktop's planner),
//! judges each refresh's readings (`alerts::evaluate`, the desktop's
//! evaluator) and writes the texts; here they become a header banner, the
//! terminal bell (`BEL`) and, only on terminals known to show it, an OSC 9
//! notification, so an unknown sequence never reaches (and dirties)
//! another screen. Nothing persists: what was notified lives for the
//! session, and a reset that passed before the TUI opened is never
//! announced. No provider is queried for a notice.

use crate::ui::{local_time, safe};
use chrono::{DateTime, TimeDelta, Utc};
use cuotai_core::{
    alerts::{self, AlertEvent, AlertPrefs, AlertState, Lead, PaceInput, NOTICE_SPACING},
    pace::Point,
    refresh::{Generation, RefreshCycle},
    resets::{self, Ledger, Prefs},
    Account, Config, Report, Snapshot,
};
use std::time::{Duration, Instant};

/// PRD 0024 RF2.2: one account's pace input for a reading: its effective
/// refresh interval and the recorded recent points of each of its metrics.
pub(crate) type PaceOf<'a> = dyn Fn(usize, &Report) -> Option<(Duration, Vec<Vec<Point>>)> + 'a;

/// How long the header keeps the last notice.
const BANNER_KEEP: TimeDelta = TimeDelta::hours(1);
const BEL: u8 = 0x07;

pub(crate) struct Notices {
    /// The planner's defaults: at the reset, only if used, no `other`.
    prefs: Prefs,
    /// What this session already notified (or skipped); never saved.
    ledger: Ledger,
    /// PRD 0008 RF4: usage alerts on with the core defaults (75% and 90%,
    /// exhaustion and recovery, nothing muted).
    alert_prefs: AlertPrefs,
    /// What the usage alerts saw this session; never saved.
    alerts: AlertState,
    /// Alerts judged on a refresh, waiting for the next pass.
    pending: Vec<AlertEvent>,
    /// When the last usage alert went out ([`NOTICE_SPACING`]).
    last_alert: Option<DateTime<Utc>>,
    started: DateTime<Utc>,
    osc9: bool,
    /// When the last notice went out and its header line.
    banner: Option<(DateTime<Utc>, String)>,
}

impl Notices {
    /// A session opened at `started`; `osc9` from [`osc9_supported`].
    pub(crate) fn new(started: DateTime<Utc>, osc9: bool) -> Self {
        let mut alert_prefs = AlertPrefs::default();
        alert_prefs.enabled = true;
        Self {
            prefs: Prefs::default(),
            ledger: Ledger::default(),
            alert_prefs,
            alerts: AlertState::default(),
            pending: Vec::new(),
            last_alert: None,
            started,
            osc9,
            banner: None,
        }
    }

    /// PRD 0024 RF3.4: `--notify-projected`: warn `lead` before a projected
    /// exhaustion, for this session only. `--notify` alone never does.
    pub(crate) fn with_projected(mut self, lead: Option<Lead>) -> Self {
        self.alert_prefs.projected = lead;
        self
    }

    /// PRD 0008 RF4: the usage alerts `next` deserves after `prev`, the
    /// account's reading it replaces (PRD 0024 RF2: with `pace`, also a
    /// projected exhaustion); pass them to [`Self::record`] once the cycle
    /// applied `next`.
    fn assess(
        &self,
        prev: Option<&Report>,
        next: &Report,
        now: DateTime<Utc>,
        pace: Option<&(Duration, Vec<Vec<Point>>)>,
    ) -> Vec<AlertEvent> {
        let input = pace.map(|(interval, recent)| PaceInput {
            interval: *interval,
            recent,
        });
        alerts::evaluate_with(
            prev,
            next,
            &self.alert_prefs,
            &self.alerts,
            now,
            input.as_ref(),
        )
    }

    /// Remember `next`, the reading the cycle applied, and queue the
    /// `events` [`Self::assess`] gave for it for the next pass. A queued
    /// projection that `next` voids (PRD 0024 RF2.4) is dropped first.
    fn record(
        &mut self,
        next: &Report,
        events: Vec<AlertEvent>,
        now: DateTime<Utc>,
        pace: Option<&(Duration, Vec<Vec<Point>>)>,
    ) {
        self.alerts.update(next, &self.alert_prefs, &events, now);
        if let Some((interval, recent)) = pace {
            let input = PaceInput {
                interval: *interval,
                recent,
            };
            self.pending
                .retain(|event| !alerts::cancels(event, next, &input, now));
        }
        self.pending.extend(events);
    }

    /// Forget the usage alerts of accounts no longer in `config` (after a
    /// save), so one added back later starts afresh.
    pub(crate) fn keep_accounts(&mut self, config: &Config) {
        let keys: Vec<String> = config.accounts.iter().map(Account::state_key).collect();
        self.pending
            .retain(|event| keys.contains(&event.account_key));
        self.alerts.retain_accounts(keys.iter().map(String::as_str));
    }

    /// One pass at `now` over `readings` (one per `config.accounts`): the
    /// resets due and the pending usage alerts become the banner and the
    /// bytes to write to the terminal, one OSC 9 per notice when supported
    /// and then a single `BEL`; empty when nothing is due. Each reset is
    /// announced once per session; usage alerts go out at most once per
    /// [`NOTICE_SPACING`], the ones arriving sooner wait for the next.
    pub(crate) fn tick(
        &mut self,
        config: &Config,
        readings: &[Option<&Report>],
        now: DateTime<Utc>,
        locale: &str,
    ) -> Vec<u8> {
        self.ledger.prune(now);
        self.alerts.prune(now);
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
                osc9(
                    &mut out,
                    &format!("{title} — {}", notice.body.resolve(locale)),
                );
            }
        }
        self.deliver_alerts(config, now, locale, &mut out, &mut lines);
        if lines.is_empty() {
            return out;
        }
        out.push(BEL);
        self.banner = Some((now, format!("{} · {}", local_time(now), lines.join(" · "))));
        out
    }

    /// The pending usage alerts, unless [`NOTICE_SPACING`] still holds
    /// them: one line (and OSC 9) per account, in configuration order,
    /// with the core text ("Claude: 90% of the 5-hour window used —
    /// Work: Session (5h) · resets 14:30"). Alerts of accounts no longer
    /// configured are dropped.
    fn deliver_alerts(
        &mut self,
        config: &Config,
        now: DateTime<Utc>,
        locale: &str,
        out: &mut Vec<u8>,
        lines: &mut Vec<String>,
    ) {
        if self.pending.is_empty()
            || self
                .last_alert
                .is_some_and(|last| now < last + NOTICE_SPACING)
        {
            return;
        }
        let mut pending = std::mem::take(&mut self.pending);
        for account in &config.accounts {
            let key = account.state_key();
            let (mine, rest): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|event| event.account_key == key);
            pending = rest;
            let Some(notice) = alerts::notice(&[(account, mine)], now, &chrono::Local) else {
                continue;
            };
            let text = format!(
                "{} — {}",
                notice.title.resolve(locale),
                notice.body.resolve(locale)
            );
            lines.push(safe(&text).into_owned());
            if self.osc9 {
                osc9(out, &text);
            }
            self.last_alert = Some(now);
        }
    }

    /// Whether projected exhaustion is on (`--notify-projected`), so the
    /// recent points are read only then.
    fn projecting(&self) -> bool {
        self.alert_prefs.projected.is_some()
    }

    /// The header line of the last notice, for [`BANNER_KEEP`].
    pub(crate) fn banner(&self, now: DateTime<Utc>) -> Option<&str> {
        self.banner
            .as_ref()
            .filter(|(at, _)| now - *at < BANNER_KEEP)
            .map(|(_, line)| line.as_str())
    }
}

/// One OSC 9 notification with `text` (control characters removed).
fn osc9(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(b"\x1b]9;");
    out.extend_from_slice(safe(text).as_bytes());
    out.extend_from_slice(b"\x1b\\");
}

/// What notices read, like the desktop: the account's current report or,
/// while it failed or nothing answered yet, its last valid reading (also
/// the `prev` of a usage alert's first reading, PRD 0008 RF1.7).
fn reading(cycle: &RefreshCycle, index: usize) -> Option<&Report> {
    cycle
        .last_good_report(index)
        .or_else(|| cycle.account_report(index))
}

/// [`reading`] for every account of the cycle.
pub(crate) fn readings(cycle: &RefreshCycle) -> Vec<Option<&Report>> {
    (0..cycle.len())
        .map(|index| reading(cycle, index))
        .collect()
}

/// Apply one account's answer to the cycle. With `--notify` (PRD 0008
/// RF4), a report is judged against the reading it replaces and, only
/// once the cycle accepted it, its usage alerts are queued for the next
/// [`Notices::tick`]. No query happens here.
pub(crate) fn finish_account(
    cycle: &mut RefreshCycle,
    notices: Option<&mut Notices>,
    index: usize,
    report: Option<Report>,
    token: Generation,
    now: DateTime<Utc>,
    pace: &PaceOf,
) {
    let (Some(notices), Some(next)) = (notices, report.as_ref()) else {
        cycle.finish_account(index, report, token, Instant::now());
        return;
    };
    let input = notices.projecting().then(|| pace(index, next)).flatten();
    let events = notices.assess(reading(cycle, index), next, now, input.as_ref());
    if cycle.finish_account(index, report, token, Instant::now()) {
        if let Some(next) = cycle.account_report(index) {
            notices.record(next, events, now, input.as_ref());
        }
    }
}

/// [`finish_account`] for a full sweep: every report judged against the
/// reading it replaces, the alerts queued once the cycle accepted it.
pub(crate) fn finish_full(
    cycle: &mut RefreshCycle,
    notices: Option<&mut Notices>,
    snapshot: Option<Snapshot>,
    token: Generation,
    now: DateTime<Utc>,
    pace: &PaceOf,
) {
    let (Some(notices), Some(next)) = (notices, snapshot.as_ref()) else {
        cycle.finish_full(snapshot, token, Instant::now());
        return;
    };
    let judged: Vec<_> = next
        .reports
        .iter()
        .enumerate()
        .map(|(index, report)| {
            let input = notices.projecting().then(|| pace(index, report)).flatten();
            let events = notices.assess(reading(cycle, index), report, now, input.as_ref());
            (events, input)
        })
        .collect();
    if cycle.finish_full(snapshot, token, Instant::now()) {
        for (index, (events, input)) in judged.into_iter().enumerate() {
            if let Some(next) = cycle.account_report(index) {
                notices.record(next, events, now, input.as_ref());
            }
        }
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

    #[test]
    fn notify_alone_never_projects() {
        let notices = Notices::new(start(), false);
        assert!(!notices.projecting());
        assert!(notices.with_projected(Some(Lead::Min15)).projecting());
    }
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
            id: None,
            name: name.into(),
            provider: Provider::Claude,
            ..Account::default()
        }
    }

    fn report(account: &Account, used: f64, resets_at: DateTime<Utc>) -> Report {
        Report {
            account_id: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: start(),
            status: Status::Ready,
            metrics: vec![Metric {
                billing: Default::default(),
                scope: None,
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
            session_expires_at: None,
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

    /// A 5h reading of `used`% taken at `checked_at`, its window ending at
    /// `resets_at` (kept in the future so no reset notice joins in).
    fn measured(
        account: &Account,
        used: f64,
        checked_at: DateTime<Utc>,
        resets_at: DateTime<Utc>,
    ) -> Report {
        Report {
            checked_at,
            ..report(account, used, resets_at)
        }
    }

    /// One account's query answering `report` at its `checked_at`.
    fn answer(cycle: &mut RefreshCycle, notices: &mut Notices, index: usize, report: Report) {
        let token = cycle.begin(index).expect("slot free");
        let now = report.checked_at;
        finish_account(
            cycle,
            Some(notices),
            index,
            Some(report),
            token,
            now,
            &|_, _| None,
        );
    }

    fn pass(
        notices: &mut Notices,
        config: &Config,
        cycle: &RefreshCycle,
        now: DateTime<Utc>,
    ) -> Vec<u8> {
        notices.tick(config, &readings(cycle), now, "en-US")
    }

    #[test]
    fn a_threshold_crossed_on_a_refresh_rings_once_per_window() {
        let config = config(vec![account("Pessoal")]);
        let pessoal = &config.accounts[0];
        let window = start() + TimeDelta::hours(3);
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut notices = Notices::new(start(), false);

        // The first reading of the session is no news, even above 75%.
        answer(
            &mut cycle,
            &mut notices,
            0,
            measured(pessoal, 80.0, start() + mins(1), window),
        );
        assert!(pass(&mut notices, &config, &cycle, start() + mins(1)).is_empty());

        answer(
            &mut cycle,
            &mut notices,
            0,
            measured(pessoal, 92.0, start() + mins(2), window),
        );
        let at = start() + mins(2);
        assert_eq!(pass(&mut notices, &config, &cycle, at), vec![BEL]);
        let banner = notices.banner(at).expect("banner after the alert");
        assert!(
            banner.contains("Claude: 90% of the 5-hour window used — Pessoal: 5h · resets "),
            "{banner}"
        );

        // Oscillating around 90% in the same window stays silent.
        for (minute, used) in [(4, 88.0), (6, 93.0), (8, 89.0), (10, 91.0)] {
            let at = start() + mins(minute);
            answer(
                &mut cycle,
                &mut notices,
                0,
                measured(pessoal, used, at, window),
            );
            assert!(
                pass(&mut notices, &config, &cycle, at).is_empty(),
                "{minute}"
            );
        }

        // A new window instance re-arms it.
        let next_window = window + TimeDelta::hours(5);
        let at = start() + mins(12);
        answer(
            &mut cycle,
            &mut notices,
            0,
            measured(pessoal, 91.0, at, next_window),
        );
        assert_eq!(pass(&mut notices, &config, &cycle, at), vec![BEL]);
    }

    #[test]
    fn alerts_of_a_sweep_share_one_bell_and_later_ones_wait_a_minute() {
        let config = config(vec![account("a"), account("b")]);
        let window = start() + TimeDelta::hours(3);
        let sweep = |used: [f64; 2], at| Snapshot {
            reports: config
                .accounts
                .iter()
                .zip(used)
                .map(|(account, used)| measured(account, used, at, window))
                .collect(),
            started_at: at,
            finished_at: at,
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut notices = Notices::new(start(), true);
        for (used, minute) in [([50.0, 50.0], 1), ([95.0, 80.0], 2)] {
            let at = start() + mins(minute);
            let token = cycle.begin_full().expect("no sweep in flight");
            finish_full(
                &mut cycle,
                Some(&mut notices),
                Some(sweep(used, at)),
                token,
                at,
                &|_, _| None,
            );
        }
        let at = start() + mins(2);
        let out = pass(&mut notices, &config, &cycle, at);
        let text = String::from_utf8(out.clone()).unwrap();
        assert_eq!(bells(&out), 1);
        assert_eq!(text.matches("\x1b]9;").count(), 2);
        let banner = notices.banner(at).unwrap();
        assert!(
            banner.contains("90% of the 5-hour window used — a: 5h"),
            "{banner}"
        );
        assert!(
            banner.contains(" · Claude: 75% of the 5-hour window used — b: 5h"),
            "{banner}"
        );

        // Twenty seconds later "a" runs out: held, then delivered alone.
        let soon = at + TimeDelta::seconds(20);
        answer(
            &mut cycle,
            &mut notices,
            0,
            measured(&config.accounts[0], 100.0, soon, window),
        );
        assert!(pass(&mut notices, &config, &cycle, soon).is_empty());
        assert!(pass(&mut notices, &config, &cycle, at + TimeDelta::seconds(59)).is_empty());
        let out = pass(&mut notices, &config, &cycle, at + NOTICE_SPACING);
        assert_eq!(bells(&out), 1);
        let banner = notices.banner(at + NOTICE_SPACING).unwrap();
        assert!(
            banner.contains("Claude: 5-hour quota exhausted — a: 5h"),
            "{banner}"
        );
        assert!(!banner.contains("— b:"), "{banner}");
    }

    #[test]
    fn a_stale_restored_reading_or_a_removed_account_never_rings() {
        let remaining = config(vec![account("a")]);
        let config = config(vec![account("a"), account("b")]);
        let window = start() + TimeDelta::hours(3);
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        // The history's last readings, 30 minutes before the TUI opened.
        cycle.restore_last_good_at(
            config
                .accounts
                .iter()
                .map(|account| Some(measured(account, 50.0, start() - mins(30), window)))
                .collect(),
            start(),
        );
        let mut notices = Notices::new(start(), false);
        answer(
            &mut cycle,
            &mut notices,
            0,
            measured(&config.accounts[0], 95.0, start(), window),
        );
        assert!(pass(&mut notices, &config, &cycle, start()).is_empty());

        // "b" crosses 90%, then is removed before the next pass.
        let at = start() + mins(1);
        answer(
            &mut cycle,
            &mut notices,
            1,
            measured(&config.accounts[1], 60.0, at, window),
        );
        let later = at + mins(1);
        answer(
            &mut cycle,
            &mut notices,
            1,
            measured(&config.accounts[1], 95.0, later, window),
        );
        assert_eq!(notices.pending.len(), 1, "b's 90% alert is queued");
        notices.keep_accounts(&remaining);
        assert!(notices
            .tick(&remaining, &[cycle.account_report(0)], later, "en-US")
            .is_empty());
        assert_eq!(notices.banner(later), None);
    }
}
