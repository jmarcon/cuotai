//! PRD 0003 RF2/RF3: quota reset notices, and PRD 0008 RF2/RF3: usage
//! threshold alerts, both delivered by the backend because the hidden
//! webview may be suspended. The core plans the resets (`resets::plan`),
//! judges the readings (`alerts::evaluate`), writes the texts and keeps the
//! ledger of what was notified; here live the device's preferences, the
//! deliverer behind [`Notifier`] and one pass of the timer
//! ([`Notices::tick`]).
use crate::settings::{self, AlertSettings, NotifyPrefs};
use chrono::{DateTime, Utc};
use cuotai_core::{
    alerts::{self, AlertEvent, AlertPrefs, NOTICE_SPACING},
    resets::{self, Ledger, ResetEvent},
    Config, Metric, Report,
};
use std::path::PathBuf;

/// Language of the notices until the interface reports the one chosen (the
/// interface's own fallback).
const DEFAULT_LOCALE: &str = "en-US";

/// Delivers one notice to the operating system; tests use a fake.
pub trait Notifier {
    fn deliver(&self, title: &str, body: &str) -> Result<(), String>;
}

/// The system notification center, through `tauri-plugin-notification`.
pub struct SystemNotifier(pub tauri::AppHandle);

impl Notifier for SystemNotifier {
    fn deliver(&self, title: &str, body: &str) -> Result<(), String> {
        use tauri_plugin_notification::NotificationExt;
        self.0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|e| e.to_string())
    }
}

/// Ask the operating system to let the app notify: `granted`, `denied` or
/// `prompt` (still undecided).
pub fn request_permission(app: &tauri::AppHandle) -> &'static str {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    match app.notification().request_permission() {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => "prompt",
        Ok(PermissionState::Denied) | Err(_) => "denied",
    }
}

/// Where the preferences (`desktop.json`) and the ledger (`notified.json`)
/// persist.
#[derive(Debug, Clone)]
pub struct Store {
    pub settings: PathBuf,
    pub ledger: PathBuf,
}

/// The device's notice state. Without a [`Store`] (tests) nothing touches
/// the disk.
#[derive(Debug, Default)]
pub struct Notices {
    prefs: NotifyPrefs,
    alerts: AlertSettings,
    /// The core evaluator's preferences, rebuilt from `alerts` and the
    /// bells whenever either changes.
    evaluator: AlertPrefs,
    /// Notified resets and the usage alert state (`notified.json`).
    ledger: Ledger,
    locale: Option<String>,
    store: Option<Store>,
    /// PRD 0008 RF2.2: usage alerts judged but not delivered yet (at most
    /// one notification per [`NOTICE_SPACING`]), in arrival order.
    pending: Vec<AlertEvent>,
    /// When the last usage alert notification went out in this process.
    last_alert: Option<DateTime<Utc>>,
}

/// What one pass of the timer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub delivered: usize,
    /// When the next notice is due; `None` when nothing is ahead.
    pub next: Option<DateTime<Utc>>,
}

impl Notices {
    /// `desktop.json` and `<OS local data dir>/cuotai/notified.json`.
    pub fn load() -> Self {
        let store = settings::settings_path()
            .zip(Ledger::default_path())
            .map(|(settings, ledger)| Store { settings, ledger });
        Self::open(store)
    }

    /// The state persisted in `store`; a missing or unreadable file reads as
    /// the defaults (off, nothing notified).
    pub fn open(store: Option<Store>) -> Self {
        let Some(store) = store else {
            return Self::default();
        };
        let mut notices = Self {
            prefs: settings::read_notify(&store.settings),
            alerts: settings::read_alerts(&store.settings),
            ledger: Ledger::load(&store.ledger),
            locale: settings::read_locale(&store.settings),
            store: Some(store),
            ..Self::default()
        };
        notices.rebuild_evaluator();
        notices
    }

    pub fn prefs(&self) -> &NotifyPrefs {
        &self.prefs
    }

    /// Persist then apply `prefs`; on a write failure nothing changes.
    /// Error: `unwritable`.
    pub fn set_prefs(&mut self, prefs: NotifyPrefs) -> Result<(), &'static str> {
        if let Some(store) = &self.store {
            settings::write_notify(&store.settings, &prefs)?;
        }
        self.prefs = prefs;
        self.rebuild_evaluator();
        Ok(())
    }

    /// Turn reset notices on with the system's answer to the permission
    /// request (only `granted` turns them on) or off with `off`.
    pub fn switch_resets(&mut self, permission: &str) -> Result<(), &'static str> {
        self.set_prefs(NotifyPrefs {
            enabled: permission == "granted",
            ..self.prefs.clone()
        })
    }

    pub fn alerts(&self) -> &AlertSettings {
        &self.alerts
    }

    /// Persist then apply the usage alert preferences; on a write failure
    /// nothing changes. Turning alerts off cancels the pending ones.
    /// Error: `unwritable`.
    pub fn set_alerts(&mut self, alerts: AlertSettings) -> Result<(), &'static str> {
        if let Some(store) = &self.store {
            settings::write_alerts(&store.settings, &alerts)?;
        }
        self.alerts = alerts;
        self.rebuild_evaluator();
        if !self.alerts.enabled {
            self.pending.clear();
        }
        Ok(())
    }

    /// Turn usage alerts on with the system's answer to the permission
    /// request (only `granted` turns them on) or off with `off`.
    pub fn switch_alerts(&mut self, permission: &str) -> Result<(), &'static str> {
        self.set_alerts(AlertSettings {
            enabled: permission == "granted",
            ..self.alerts.clone()
        })
    }

    /// PRD 0008 RF3.2: mute (`on = false`) or unmute one metric's alerts.
    pub fn set_metric_alert(
        &mut self,
        account_key: &str,
        label: &str,
        on: bool,
    ) -> Result<(), &'static str> {
        self.set_alerts(self.alerts.with_metric(account_key, label, on))
    }

    /// Whether the metric `label` of `account_key` is muted in the details.
    pub fn metric_muted(&self, account_key: &str, label: &str) -> bool {
        self.alerts
            .muted
            .get(account_key)
            .is_some_and(|labels| labels.contains(label))
    }

    fn rebuild_evaluator(&mut self) {
        self.evaluator = self.alerts.evaluator(&self.prefs.off_accounts);
    }

    /// Turn the bell of `account_key` on or off: its reset notices and
    /// usage alerts (PRD 0008 RF3.2).
    pub fn set_account(&mut self, account_key: &str, on: bool) -> Result<(), &'static str> {
        let mut prefs = self.prefs.clone();
        if on {
            prefs.off_accounts.remove(account_key);
        } else {
            prefs.off_accounts.insert(account_key.to_owned());
        }
        self.set_prefs(prefs)
    }

    /// Forget the bells, muted metrics, alert state and pending alerts of
    /// accounts no longer in `config` (after a save).
    pub fn keep_accounts(&mut self, config: &Config) -> Result<(), &'static str> {
        let keys: Vec<String> = config.accounts.iter().map(|a| a.key()).collect();
        self.pending
            .retain(|event| keys.contains(&event.account_key));
        if self
            .ledger
            .alerts_mut()
            .retain_accounts(keys.iter().map(String::as_str))
        {
            self.save_ledger();
        }
        let mut alerts = self.alerts.clone();
        alerts.muted.retain(|key, _| keys.contains(key));
        if alerts != self.alerts {
            self.set_alerts(alerts)?;
        }
        let mut prefs = self.prefs.clone();
        prefs.off_accounts.retain(|key| keys.contains(key));
        if prefs == self.prefs {
            return Ok(());
        }
        self.set_prefs(prefs)
    }

    /// The bell of `account_key` (on unless turned off).
    pub fn account_on(&self, account_key: &str) -> bool {
        !self.prefs.off_accounts.contains(account_key)
    }

    /// PRD 0008 RF2.2: the alerts `next` deserves after `prev` (the
    /// account's reading it replaces), judged before the cycle applies it;
    /// pass them to [`Self::record`] once it did. Nothing while alerts are
    /// off.
    pub fn assess(
        &self,
        prev: Option<&Report>,
        next: &Report,
        now: DateTime<Utc>,
    ) -> Vec<AlertEvent> {
        alerts::evaluate(prev, next, &self.evaluator, self.ledger.alerts(), now)
    }

    /// Remember `next`, the reading the cycle applied, with the `events`
    /// [`Self::assess`] gave for it: the core state (saved to
    /// `notified.json` when it changed) and the events queued for the next
    /// pass of the timer. While alerts are off nothing is recorded: turning
    /// them on treats the next reading as a first reading.
    pub fn record(&mut self, next: &Report, events: Vec<AlertEvent>, now: DateTime<Utc>) {
        if !self.evaluator.enabled {
            return;
        }
        if self
            .ledger
            .alerts_mut()
            .update(next, &self.evaluator, &events, now)
        {
            self.save_ledger();
        }
        self.pending.extend(events);
    }

    /// PRD 0008 RF3.4: the metrics of `report` with an active alert, while
    /// alerts are on and the account's bell and the metric are not muted.
    pub fn active<'r>(&'r self, report: &'r Report) -> impl Iterator<Item = &'r Metric> + 'r {
        let account_key = report.account_key();
        let on = self.evaluator.enabled && self.account_on(&account_key);
        self.ledger
            .alerts()
            .active(report)
            .filter(move |metric| on && !self.metric_muted(&account_key, &metric.label))
    }

    fn save_ledger(&self) {
        if let Some(store) = &self.store {
            // A failed write only risks repeating a notice after a restart.
            let _ = self.ledger.save(&store.ledger);
        }
    }

    /// Remember the language chosen in the app for the next notices: a
    /// BCP 47-like code (`pt-BR`, `es`, a user pack's code). Errors:
    /// `invalid_locale`, `unwritable`.
    pub fn set_locale(&mut self, locale: &str) -> Result<(), &'static str> {
        let valid = !locale.is_empty()
            && locale.len() <= 35
            && locale
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Err("invalid_locale");
        }
        if self.locale.as_deref() == Some(locale) {
            return Ok(());
        }
        if let Some(store) = &self.store {
            settings::write_locale(&store.settings, locale)?;
        }
        self.locale = Some(locale.to_owned());
        Ok(())
    }

    fn locale(&self) -> &str {
        self.locale.as_deref().unwrap_or(DEFAULT_LOCALE)
    }

    /// The next notice still ahead of `now` (tray and tooltip); `None` when
    /// notices are off.
    pub fn next(
        &self,
        config: &Config,
        readings: &[Option<&Report>],
        now: DateTime<Utc>,
    ) -> Option<ResetEvent> {
        if !self.prefs.enabled {
            return None;
        }
        let prefs = self.prefs.planner();
        resets::plan(config, readings.iter().copied(), now, &prefs, &self.ledger).next
    }

    /// One pass of the timer at `now`: deliver the reset notices that are
    /// due (at most one grouped notice per account) and the pending usage
    /// alerts ([`Self::deliver_alerts`]), remember them in the ledger
    /// (pruned after eight days) and say when the next one is due.
    /// `readings` follow `config.accounts`; nothing here queries a provider.
    pub fn tick(
        &mut self,
        config: &Config,
        readings: &[Option<&Report>],
        now: DateTime<Utc>,
        notifier: &dyn Notifier,
    ) -> Tick {
        let mut changed = self.ledger.prune(now);
        let mut tick = Tick {
            delivered: 0,
            next: None,
        };
        if self.prefs.enabled {
            let prefs = self.prefs.planner();
            let plan = resets::plan(config, readings.iter().copied(), now, &prefs, &self.ledger);
            for event in &plan.due {
                let notice = event.notice(&config.accounts[event.account_index], now);
                let locale = self.locale();
                // Recorded even when the delivery failed: a notice is sent
                // at most once, never retried into a burst.
                let _ =
                    notifier.deliver(&notice.title.resolve(locale), &notice.body.resolve(locale));
                self.ledger.record(event, now);
            }
            changed |= !plan.due.is_empty();
            tick.delivered = plan.due.len();
            tick.next = plan.next.map(|event| event.notify_at);
        }
        let (delivered, next) = self.deliver_alerts(config, now, notifier);
        tick.delivered += delivered;
        tick.next = match (tick.next, next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if changed {
            self.save_ledger();
        }
        tick
    }

    /// PRD 0008 RF2.2: the pending usage alerts still wanted (alerts on,
    /// bell and metric not muted, account still configured) as one
    /// notification: one account's alerts together, several accounts'
    /// together once [`NOTICE_SPACING`] holds them. Returns how many went
    /// out (0 or 1) and, while the spacing holds some back, when they may.
    /// Like reset notices, a delivery the system refused is not retried.
    fn deliver_alerts(
        &mut self,
        config: &Config,
        now: DateTime<Utc>,
        notifier: &dyn Notifier,
    ) -> (usize, Option<DateTime<Utc>>) {
        let keys: Vec<String> = config.accounts.iter().map(|a| a.key()).collect();
        let evaluator = &self.evaluator;
        self.pending
            .retain(|event| evaluator.wants(event) && keys.contains(&event.account_key));
        if self.pending.is_empty() {
            return (0, None);
        }
        if let Some(due) = self.last_alert.map(|last| last + NOTICE_SPACING) {
            if now < due {
                return (0, Some(due));
            }
        }
        let mut pending = std::mem::take(&mut self.pending);
        let mut groups = Vec::new();
        for (account, key) in config.accounts.iter().zip(&keys) {
            let (mine, rest): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|event| event.account_key == *key);
            pending = rest;
            if !mine.is_empty() {
                groups.push((account, mine));
            }
        }
        let Some(notice) = alerts::notice(&groups, now, &chrono::Local) else {
            return (0, None);
        };
        let locale = self.locale();
        let _ = notifier.deliver(&notice.title.resolve(locale), &notice.body.resolve(locale));
        self.last_alert = Some(now);
        (1, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, TimeZone};
    use cuotai_core::{Account, Metric, MetricKind, MetricWindow, Provider, Status};
    use std::cell::RefCell;

    /// Records what would reach the notification center.
    #[derive(Default)]
    struct FakeNotifier(RefCell<Vec<(String, String)>>);

    impl Notifier for FakeNotifier {
        fn deliver(&self, title: &str, body: &str) -> Result<(), String> {
            self.0.borrow_mut().push((title.into(), body.into()));
            Ok(())
        }
    }

    impl FakeNotifier {
        fn titles(&self) -> Vec<String> {
            self.0.borrow().iter().map(|(t, _)| t.clone()).collect()
        }
    }

    /// The fake clock's origin; every reset is placed relative to it.
    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 2, 14, 0, 0).unwrap()
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    fn quota(label: &str, window: MetricWindow, resets_at: DateTime<Utc>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used: Some(40.0),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(window),
            period: None,
            resets_at: Some(resets_at),
        }
    }

    fn config(names: &[&str]) -> Config {
        Config {
            accounts: names
                .iter()
                .map(|name| Account {
                    id: None,
                    name: (*name).into(),
                    provider: Provider::Claude,
                    ..Account::default()
                })
                .collect(),
            ..Config::default()
        }
    }

    fn report(name: &str, metrics: Vec<Metric>) -> Report {
        Report {
            account_id: None,
            account: name.into(),
            provider: Provider::Claude,
            checked_at: t0() - mins(60),
            status: Status::Ready,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
        }
    }

    fn on(lead_minutes: u32) -> Notices {
        let mut notices = Notices::default();
        notices
            .set_prefs(NotifyPrefs {
                enabled: true,
                lead_minutes,
                ..NotifyPrefs::default()
            })
            .unwrap();
        notices
    }

    fn tick(
        notices: &mut Notices,
        cfg: &Config,
        reports: &[Report],
        now: DateTime<Utc>,
        fake: &FakeNotifier,
    ) -> Tick {
        let readings: Vec<Option<&Report>> = reports.iter().map(Some).collect();
        notices.tick(cfg, &readings, now, fake)
    }

    fn store(dir: &std::path::Path) -> Store {
        Store {
            settings: dir.join("desktop.json"),
            ledger: dir.join("notified.json"),
        }
    }

    #[test]
    fn notifies_once_at_the_reset_and_sleeps_until_it_before() {
        let cfg = config(&["Work"]);
        let reports = [report(
            "Work",
            vec![quota("Session (5h)", MetricWindow::Window5h, t0())],
        )];
        let fake = FakeNotifier::default();
        let mut notices = on(0);
        let before = tick(&mut notices, &cfg, &reports, t0() - mins(30), &fake);
        assert_eq!(
            before,
            Tick {
                delivered: 0,
                next: Some(t0())
            }
        );
        assert_eq!(tick(&mut notices, &cfg, &reports, t0(), &fake).delivered, 1);
        assert_eq!(fake.titles(), ["Claude: 5-hour quota reset"]);
        let (_, body) = fake.0.borrow()[0].clone();
        assert!(body.starts_with("Work: Session (5h). "), "{body}");
        // The next pass, a minute later, does not repeat it.
        let after = tick(&mut notices, &cfg, &reports, t0() + mins(1), &fake);
        assert_eq!(
            after,
            Tick {
                delivered: 0,
                next: None
            }
        );
        assert_eq!(fake.titles().len(), 1);
    }

    #[test]
    fn lead_time_notifies_ahead_in_the_chosen_language() {
        let cfg = config(&["Work"]);
        let reports = [report(
            "Work",
            vec![quota("Session (5h)", MetricWindow::Window5h, t0())],
        )];
        let fake = FakeNotifier::default();
        let mut notices = on(15);
        notices.set_locale("es").unwrap();
        let before = tick(&mut notices, &cfg, &reports, t0() - mins(30), &fake);
        assert_eq!(before.next, Some(t0() - mins(15)));
        assert_eq!(
            tick(&mut notices, &cfg, &reports, t0() - mins(15), &fake).delivered,
            1
        );
        assert_eq!(
            fake.titles(),
            ["Claude: la cuota de 5 horas se renueva en 15 min"]
        );
        // The reset itself is the same reset: no second notice.
        assert_eq!(tick(&mut notices, &cfg, &reports, t0(), &fake).delivered, 0);
        assert_eq!(notices.set_locale("pt BR; rm"), Err("invalid_locale"));
    }

    #[test]
    fn a_postponed_reset_cancels_the_pending_notice_and_drift_does_not_repeat() {
        let cfg = config(&["Work"]);
        let fake = FakeNotifier::default();
        let mut notices = on(0);
        let planned = [report(
            "Work",
            vec![quota("5h", MetricWindow::Window5h, t0())],
        )];
        assert_eq!(
            tick(&mut notices, &cfg, &planned, t0() - mins(10), &fake).next,
            Some(t0())
        );
        // A later query moved the reset 30 minutes away (beyond the drift).
        let postponed = [report(
            "Work",
            vec![quota("5h", MetricWindow::Window5h, t0() + mins(30))],
        )];
        let at_old_time = tick(&mut notices, &cfg, &postponed, t0(), &fake);
        assert_eq!(
            at_old_time,
            Tick {
                delivered: 0,
                next: Some(t0() + mins(30))
            }
        );
        assert_eq!(
            tick(&mut notices, &cfg, &postponed, t0() + mins(30), &fake).delivered,
            1
        );
        // The next query reports the same reset 90 s later: still notified.
        let drifted = [report(
            "Work",
            vec![quota(
                "5h",
                MetricWindow::Window5h,
                t0() + mins(30) + TimeDelta::seconds(90),
            )],
        )];
        assert_eq!(
            tick(&mut notices, &cfg, &drifted, t0() + mins(31), &fake).delivered,
            0
        );
        assert_eq!(fake.titles().len(), 1);
    }

    #[test]
    fn restarting_before_or_after_the_reset_notifies_once_and_a_reset_missed_while_closed_never() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(&["Work"]);
        let reports = [report(
            "Work",
            vec![quota("5h", MetricWindow::Window5h, t0())],
        )];
        let fake = FakeNotifier::default();
        let mut notices = Notices::open(Some(store(dir.path())));
        notices
            .set_prefs(NotifyPrefs {
                enabled: true,
                ..NotifyPrefs::default()
            })
            .unwrap();
        assert_eq!(
            tick(&mut notices, &cfg, &reports, t0() - mins(5), &fake).delivered,
            0
        );
        // Restart before the time: preferences survive, the notice comes once.
        let mut notices = Notices::open(Some(store(dir.path())));
        assert!(notices.prefs().enabled);
        assert_eq!(tick(&mut notices, &cfg, &reports, t0(), &fake).delivered, 1);
        // Restart after the time (within the window): not repeated.
        let mut notices = Notices::open(Some(store(dir.path())));
        assert_eq!(
            tick(&mut notices, &cfg, &reports, t0() + mins(3), &fake).delivered,
            0
        );
        assert_eq!(fake.titles().len(), 1);

        // Closed at the reset: opening within 10 min notifies it once (from
        // the restored reading); later than that it was missed.
        for (opened, expected) in [(mins(5), 1), (mins(11), 0)] {
            let dir = tempfile::tempdir().unwrap();
            let fake = FakeNotifier::default();
            let mut notices = Notices::open(Some(store(dir.path())));
            notices
                .set_prefs(NotifyPrefs {
                    enabled: true,
                    ..NotifyPrefs::default()
                })
                .unwrap();
            assert_eq!(
                tick(&mut notices, &cfg, &reports, t0() + opened, &fake).delivered,
                expected,
                "{opened}"
            );
            let mut notices = Notices::open(Some(store(dir.path())));
            assert_eq!(
                tick(&mut notices, &cfg, &reports, t0() + opened + mins(1), &fake).delivered,
                0,
                "{opened}"
            );
        }
    }

    #[test]
    fn after_a_long_suspension_each_account_gets_at_most_one_grouped_notice() {
        let cfg = config(&["Work", "Home"]);
        let reports = [
            report(
                "Work",
                vec![
                    quota("Session (5h)", MetricWindow::Window5h, t0() - mins(4)),
                    quota("Week", MetricWindow::Weekly, t0() - mins(2)),
                ],
            ),
            // Reset three hours before waking up: missed, never notified.
            report(
                "Home",
                vec![quota("5h", MetricWindow::Window5h, t0() - mins(180))],
            ),
        ];
        let fake = FakeNotifier::default();
        let mut notices = on(0);
        let woke = tick(&mut notices, &cfg, &reports, t0(), &fake);
        assert_eq!(
            woke,
            Tick {
                delivered: 1,
                next: None
            }
        );
        assert_eq!(fake.titles(), ["Claude: 2 quotas reset"]);
        assert!(fake.0.borrow()[0]
            .1
            .starts_with("Work: Session (5h), Week. "));
    }

    #[test]
    fn removed_account_turned_off_bell_or_global_switch_cancel_pending_notices() {
        let reports = [report(
            "Work",
            vec![quota("5h", MetricWindow::Window5h, t0())],
        )];
        let fake = FakeNotifier::default();
        // Removed from the configuration: its stale reading no longer counts.
        let mut notices = on(0);
        let removed = config(&[]);
        assert_eq!(tick(&mut notices, &removed, &[], t0(), &fake).delivered, 0);
        let cfg = config(&["Work"]);
        // The account's bell off, then the global switch off midway.
        let mut notices = on(0);
        assert!(tick(&mut notices, &cfg, &reports, t0() - mins(5), &fake)
            .next
            .is_some());
        notices.set_account("claude:Work", false).unwrap();
        assert_eq!(
            tick(&mut notices, &cfg, &reports, t0() - mins(4), &fake).next,
            None
        );
        notices.set_account("claude:Work", true).unwrap();
        notices
            .set_prefs(NotifyPrefs {
                enabled: false,
                ..notices.prefs().clone()
            })
            .unwrap();
        assert_eq!(
            tick(&mut notices, &cfg, &reports, t0(), &fake),
            Tick {
                delivered: 0,
                next: None
            }
        );
        assert!(notices
            .next(&cfg, &[Some(&reports[0])], t0() - mins(5))
            .is_none());
        assert!(fake.titles().is_empty());
    }

    #[test]
    fn saving_the_configuration_prunes_bells_of_removed_or_renamed_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let mut notices = Notices::open(Some(store(dir.path())));
        notices.set_account("claude:Work", false).unwrap();
        notices.set_account("claude:Old", false).unwrap();
        notices.keep_accounts(&config(&["Work", "New"])).unwrap();
        let reopened = Notices::open(Some(store(dir.path())));
        assert!(!reopened.account_on("claude:Work"));
        assert!(reopened.account_on("claude:Old"));
        assert_eq!(reopened.prefs().off_accounts.len(), 1);
    }

    /// A system that refuses every notice (permission denied).
    #[derive(Default)]
    struct RefusingNotifier(RefCell<usize>);

    impl Notifier for RefusingNotifier {
        fn deliver(&self, _: &str, _: &str) -> Result<(), String> {
            *self.0.borrow_mut() += 1;
            Err("denied".into())
        }
    }

    /// The window most readings belong to.
    fn w() -> DateTime<Utc> {
        t0() + TimeDelta::hours(3)
    }

    /// A reading of `name` at `at` with `used`% of the "5h" window.
    fn usage(name: &str, at: DateTime<Utc>, used: f64, resets_at: DateTime<Utc>) -> Report {
        let mut metric = quota("5h", MetricWindow::Window5h, resets_at);
        metric.used = Some(used);
        Report {
            checked_at: at,
            ..report(name, vec![metric])
        }
    }

    fn alerting(store: Option<Store>) -> Notices {
        let mut notices = Notices::open(store);
        notices.switch_alerts("granted").unwrap();
        notices
    }

    /// What the backend does once the cycle applied `next` at its time,
    /// then the timer's pass that follows.
    fn observe(
        notices: &mut Notices,
        cfg: &Config,
        prev: Option<&Report>,
        next: &Report,
        notifier: &dyn Notifier,
    ) -> Tick {
        let now = next.checked_at;
        let events = notices.assess(prev, next, now);
        notices.record(next, events, now);
        notices.tick(cfg, &[], now, notifier)
    }

    #[test]
    fn one_alert_per_threshold_and_window_and_oscillation_stays_silent() {
        let cfg = config(&["Work"]);
        let fake = FakeNotifier::default();
        let mut notices = alerting(None);
        let next = w() + TimeDelta::hours(5);
        let steps = [
            (50.0, w()),
            (80.0, w()),
            (92.0, w()),
            (89.0, w()),
            (91.0, w()),
            (10.0, next),
            (95.0, next),
        ];
        let readings: Vec<Report> = steps
            .iter()
            .enumerate()
            .map(|(i, (used, resets_at))| {
                usage("Work", t0() + mins(5 * i as i64), *used, *resets_at)
            })
            .collect();
        let delivered: Vec<usize> = (0..readings.len())
            .map(|i| {
                let prev = i.checked_sub(1).map(|p| &readings[p]);
                observe(&mut notices, &cfg, prev, &readings[i], &fake).delivered
            })
            .collect();
        assert_eq!(delivered, [0, 1, 1, 0, 0, 0, 1]);
        assert_eq!(
            fake.titles(),
            [
                "Claude: 75% of the 5-hour window used",
                "Claude: 90% of the 5-hour window used",
                "Claude: 90% of the 5-hour window used",
            ]
        );
        let (_, body) = fake.0.borrow()[1].clone();
        assert!(body.starts_with("Work: 5h · resets "), "{body}");
        // The tray and the card highlight the account while 90% holds.
        let labels: Vec<&str> = notices
            .active(&readings[6])
            .map(|metric| metric.label.as_str())
            .collect();
        assert_eq!(labels, ["5h"]);
    }

    #[test]
    fn one_notice_per_account_per_cycle_and_at_most_one_per_minute() {
        let cfg = config(&["Work", "Home", "Lab"]);
        let fake = FakeNotifier::default();
        let mut notices = alerting(None);
        let week = t0() + TimeDelta::days(3);
        let two = |at, five_h: f64, weekly: f64| {
            let mut session = quota("5h", MetricWindow::Window5h, w());
            session.used = Some(five_h);
            let mut long = quota("Week", MetricWindow::Weekly, week);
            long.used = Some(weekly);
            Report {
                checked_at: at,
                ..report("Work", vec![session, long])
            }
        };
        let (before, after) = (two(t0(), 50.0, 50.0), two(t0() + mins(5), 92.0, 80.0));
        let tick = observe(&mut notices, &cfg, Some(&before), &after, &fake);
        assert_eq!(tick.delivered, 1);
        assert_eq!(fake.titles(), ["Claude: 2 usage alerts"]);
        assert_eq!(fake.0.borrow()[0].1, "Work: 5h 90%; Week 75%");

        // Two other accounts cross within the same minute: held, then one
        // notice for both.
        let at = t0() + mins(5);
        let home = usage("Home", at + TimeDelta::seconds(30), 91.0, w());
        let held = observe(
            &mut notices,
            &cfg,
            Some(&usage("Home", t0(), 50.0, w())),
            &home,
            &fake,
        );
        assert_eq!(
            held,
            Tick {
                delivered: 0,
                next: Some(at + NOTICE_SPACING)
            }
        );
        let lab = usage("Lab", at + TimeDelta::seconds(40), 76.0, w());
        let lab_prev = usage("Lab", t0(), 50.0, w());
        assert_eq!(
            observe(&mut notices, &cfg, Some(&lab_prev), &lab, &fake).delivered,
            0
        );
        let later = notices.tick(&cfg, &[], at + NOTICE_SPACING, &fake);
        assert_eq!(
            later,
            Tick {
                delivered: 1,
                next: None
            }
        );
        assert_eq!(fake.titles()[1], "Cuotai: 2 usage alerts");
        assert_eq!(
            fake.0.borrow()[1].1,
            "Claude Home: 5h 90%; Claude Lab: 5h 75%"
        );
    }

    #[test]
    fn restarting_never_resends_and_custom_thresholds_persist() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(&["Work"]);
        let fake = FakeNotifier::default();
        let mut notices = alerting(Some(store(dir.path())));
        let custom = notices
            .alerts()
            .with_choices(&[85, 60, 85], true, false)
            .unwrap();
        notices.set_alerts(custom).unwrap();
        let readings = [
            usage("Work", t0(), 50.0, w()),
            usage("Work", t0() + mins(5), 87.0, w()),
            // Below 85 but not re-armed (that takes 80).
            usage("Work", t0() + mins(10), 82.0, w()),
        ];
        for i in 1..readings.len() {
            observe(
                &mut notices,
                &cfg,
                Some(&readings[i - 1]),
                &readings[i],
                &fake,
            );
        }
        assert_eq!(fake.titles(), ["Claude: 85% of the 5-hour window used"]);
        let ledger = std::fs::read_to_string(dir.path().join("notified.json")).unwrap();
        for needle in ["Work", "5h", "claude", "87"] {
            assert!(
                !ledger.contains(&format!("\"{needle}")),
                "{needle}: {ledger}"
            );
        }

        // Reopened 5 minutes later: preferences survive, and the recorded
        // 82% (the restored reading) then 88% does not resend 85.
        let mut reopened = Notices::open(Some(store(dir.path())));
        assert!(reopened.alerts().enabled);
        assert_eq!(reopened.alerts().thresholds, [60, 85]);
        assert!(!reopened.alerts().recovery);
        let again = usage("Work", t0() + mins(15), 88.0, w());
        let tick = observe(&mut reopened, &cfg, Some(&readings[2]), &again, &fake);
        assert_eq!(tick.delivered, 0);
        // Without the persisted state the same first reading would repeat it.
        let mut forgetful = alerting(None);
        forgetful.set_alerts(reopened.alerts().clone()).unwrap();
        assert_eq!(
            observe(&mut forgetful, &cfg, Some(&readings[2]), &again, &fake).delivered,
            1
        );
    }

    #[test]
    fn alerts_off_midway_or_a_muted_bell_or_metric_cancel_pending_alerts() {
        let cfg = config(&["Work"]);
        let fake = FakeNotifier::default();
        let (a, b) = (
            usage("Work", t0(), 50.0, w()),
            usage("Work", t0() + mins(5), 92.0, w()),
        );
        let judged = |notices: &mut Notices| {
            let events = notices.assess(Some(&a), &b, b.checked_at);
            assert_eq!(events.len(), 1);
            notices.record(&b, events, b.checked_at);
        };
        // Turned off between the reading and the timer: nothing goes out,
        // and once back on the same level does not come back.
        let mut notices = alerting(None);
        judged(&mut notices);
        notices.switch_alerts("off").unwrap();
        assert_eq!(notices.tick(&cfg, &[], b.checked_at, &fake).delivered, 0);
        notices.switch_alerts("granted").unwrap();
        let c = usage("Work", t0() + mins(10), 93.0, w());
        assert_eq!(
            observe(&mut notices, &cfg, Some(&b), &c, &fake).delivered,
            0
        );
        // The account's bell turned off midway.
        let mut notices = alerting(None);
        judged(&mut notices);
        notices.set_account("claude:Work", false).unwrap();
        assert_eq!(notices.tick(&cfg, &[], b.checked_at, &fake).delivered, 0);
        assert_eq!(notices.active(&b).count(), 0);
        assert!(fake.titles().is_empty());
        // A muted metric stays silent; unmuted, only what comes next fires.
        let mut notices = alerting(None);
        notices
            .set_metric_alert("claude:Work", "5h", false)
            .unwrap();
        assert_eq!(
            observe(&mut notices, &cfg, Some(&a), &b, &fake).delivered,
            0
        );
        notices.set_metric_alert("claude:Work", "5h", true).unwrap();
        assert!(!notices.metric_muted("claude:Work", "5h"));
        let full = usage("Work", t0() + mins(10), 100.0, w());
        assert_eq!(
            observe(&mut notices, &cfg, Some(&b), &full, &fake).delivered,
            1
        );
        assert_eq!(fake.titles(), ["Claude: 5-hour quota exhausted"]);
    }

    #[test]
    fn permission_denied_keeps_alerts_off_and_a_refused_notice_is_not_retried() {
        let cfg = config(&["Work"]);
        let fake = FakeNotifier::default();
        let (a, b) = (
            usage("Work", t0(), 50.0, w()),
            usage("Work", t0() + mins(5), 92.0, w()),
        );
        let mut notices = Notices::default();
        for answer in ["denied", "prompt"] {
            notices.switch_alerts(answer).unwrap();
            assert!(!notices.alerts().enabled, "{answer}");
        }
        assert_eq!(
            observe(&mut notices, &cfg, Some(&a), &b, &fake).delivered,
            0
        );
        assert!(fake.titles().is_empty());
        // The system drops the notice: tried once, never into a burst.
        let refusing = RefusingNotifier::default();
        let mut notices = alerting(None);
        assert_eq!(
            observe(&mut notices, &cfg, Some(&a), &b, &refusing).delivered,
            1
        );
        let later = notices.tick(&cfg, &[], t0() + mins(20), &refusing);
        assert_eq!(later.delivered, 0);
        assert_eq!(*refusing.0.borrow(), 1);
    }

    #[test]
    fn removing_an_account_forgets_its_alert_state_pending_alerts_and_muted_metrics() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeNotifier::default();
        let both = config(&["Work", "Old"]);
        let mut notices = alerting(Some(store(dir.path())));
        let (a, b) = (
            usage("Old", t0(), 50.0, w()),
            usage("Old", t0() + mins(5), 92.0, w()),
        );
        let events = notices.assess(Some(&a), &b, b.checked_at);
        notices.record(&b, events, b.checked_at);
        notices
            .set_metric_alert("claude:Old", "Week", false)
            .unwrap();
        notices
            .set_metric_alert("claude:Work", "5h", false)
            .unwrap();
        let kept = config(&["Work"]);
        notices.keep_accounts(&kept).unwrap();
        assert_eq!(notices.tick(&kept, &[], b.checked_at, &fake).delivered, 0);
        let mut reopened = Notices::open(Some(store(dir.path())));
        assert!(!reopened.metric_muted("claude:Old", "Week"));
        assert!(reopened.metric_muted("claude:Work", "5h"));
        // Added back under the same name: nothing of the old one is known,
        // so a crossing within 10 minutes of the recorded reading fires.
        let (c, d) = (
            usage("Old", t0() + mins(10), 87.0, w()),
            usage("Old", t0() + mins(15), 92.0, w()),
        );
        assert_eq!(
            observe(&mut reopened, &both, Some(&c), &d, &fake).delivered,
            1
        );
        assert_eq!(fake.titles(), ["Claude: 90% of the 5-hour window used"]);
    }
}
