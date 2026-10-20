//! PRD 0003 RF2/RF3: quota reset notices, scheduled by the backend because
//! the hidden webview may be suspended. The core plans them
//! (`resets::plan`), writes their text and keeps the ledger of what was
//! notified; here live the device's preferences, the deliverer behind
//! [`Notifier`] and one pass of the timer ([`Notices::tick`]).
use crate::settings::{self, NotifyPrefs};
use chrono::{DateTime, Utc};
use cuotai_core::{
    resets::{self, Ledger, ResetEvent},
    Config, Report,
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
    ledger: Ledger,
    locale: Option<String>,
    store: Option<Store>,
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
        Self {
            prefs: settings::read_notify(&store.settings),
            ledger: Ledger::load(&store.ledger),
            locale: settings::read_locale(&store.settings),
            store: Some(store),
        }
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
        Ok(())
    }

    /// Turn the bell of `account_key` on or off.
    pub fn set_account(&mut self, account_key: &str, on: bool) -> Result<(), &'static str> {
        let mut prefs = self.prefs.clone();
        if on {
            prefs.off_accounts.remove(account_key);
        } else {
            prefs.off_accounts.insert(account_key.to_owned());
        }
        self.set_prefs(prefs)
    }

    /// Forget the bells of accounts no longer in `config` (after a save).
    pub fn keep_accounts(&mut self, config: &Config) -> Result<(), &'static str> {
        let keys: Vec<String> = config.accounts.iter().map(|a| a.key()).collect();
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

    /// One pass of the timer at `now`: deliver what is due (at most one
    /// grouped notice per account), remember it in the ledger (pruned after
    /// eight days) and say when the next one is due. `readings` follow
    /// `config.accounts`; nothing here queries a provider.
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
        if let (true, Some(store)) = (changed, &self.store) {
            // A failed write only risks repeating a notice after a restart.
            let _ = self.ledger.save(&store.ledger);
        }
        tick
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
            account: name.into(),
            provider: Provider::Claude,
            checked_at: t0() - mins(60),
            status: Status::Ready,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
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
}
