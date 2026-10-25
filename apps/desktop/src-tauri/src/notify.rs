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
    account_id, aggregators,
    alerts::{
        self, AlertEvent, AlertKind, AlertPrefs, Lead, MissedCrossing, PaceInput, NOTICE_SPACING,
    },
    quiet::{self, Decision, QuietSchedule, QuietWindow, Subject},
    resets::{self, Ledger, ResetEvent, SharedLedger},
    Account, Config, Metric, Report,
};
use std::collections::BTreeMap;
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
    /// Notified resets and the usage alert state (`notified.json`), shared
    /// with the other processes of this profile (PRD 0032 RF4).
    ledger: SharedLedger,
    locale: Option<String>,
    store: Option<Store>,
    /// PRD 0008 RF2.2: usage alerts judged but not delivered yet (at most
    /// one notification per [`NOTICE_SPACING`]), in arrival order.
    pending: Vec<AlertEvent>,
    /// When the last usage alert notification went out in this process.
    last_alert: Option<DateTime<Utc>>,
    /// PRD 0032 RF2: the quiet hours of this device.
    quiet: QuietSchedule,
    /// PRD 0032 RF3.3: the digest's alerts waiting for the spacing.
    digest_pending: Vec<AlertEvent>,
    /// PRD 0032 RF8: per account, what its metrics crossed while the app
    /// was closed (the first reading of this process), until the window
    /// instance ends or the details were opened.
    missed: BTreeMap<String, Vec<MissedCrossing>>,
    /// The time zone of the quiet hours; `None` is the system's. Tests set
    /// a fixed offset.
    zone: Option<chrono::FixedOffset>,
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
            quiet: settings::read_quiet(&store.settings),
            ledger: SharedLedger::open(Some(store.ledger.clone())),
            locale: settings::read_locale(&store.settings),
            store: Some(store),
            ..Self::default()
        };
        notices.rebuild_evaluator();
        notices
    }

    #[cfg(test)]
    pub fn quiet(&self) -> &QuietSchedule {
        &self.quiet
    }

    /// PRD 0032 RF2: persist then apply the quiet hours; on a write failure
    /// nothing changes. Error: `unwritable`.
    pub fn set_quiet(&mut self, quiet: QuietSchedule) -> Result<(), &'static str> {
        if let Some(store) = &self.store {
            settings::write_quiet(&store.settings, &quiet)?;
        }
        self.quiet = quiet;
        Ok(())
    }

    /// The quiet window open at `now` in the system's time zone.
    fn window(&self, now: DateTime<Utc>) -> Option<QuietWindow> {
        match &self.zone {
            Some(zone) => quiet::window_at(&self.quiet, now, zone),
            None => quiet::window_at(&self.quiet, now, &chrono::Local),
        }
    }

    /// PRD 0032 RF8.2: what the metrics of `account_key` crossed while the
    /// app was closed.
    pub fn missed(&self, account_key: &str) -> &[MissedCrossing] {
        self.missed.get(account_key).map_or(&[], Vec::as_slice)
    }

    /// PRD 0032 RF8.2: the person opened the details of `account_key`: the
    /// marker goes away and its crossings count as seen for good.
    pub fn acknowledge_missed(&mut self, account_key: &str) {
        let Some(crossings) = self.missed.remove(account_key) else {
            return;
        };
        let key = account_key.to_owned();
        self.ledger
            .mutate(|ledger| ledger.alerts_mut().acknowledge(&key, &crossings));
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
        let evaluator = &self.evaluator;
        let alerts_on = self.alerts.enabled;
        self.pending
            .retain(|event| Self::judged(evaluator, alerts_on, event));
        Ok(())
    }

    /// Whether `event` reaches the user: the core's rule, and, while the
    /// usage alerts are off, only the projected warning, which has its own
    /// switch (PRD 0024 RF4.1).
    fn judged(evaluator: &AlertPrefs, alerts_on: bool, event: &AlertEvent) -> bool {
        evaluator.wants(event) && (alerts_on || event.kind == AlertKind::ProjectedExhaustion)
    }

    /// PRD 0024 RF4.1: turn the projected-exhaustion warning on with the
    /// system's answer to the permission request (only `granted` turns it
    /// on, `lead` minutes ahead; an unknown lead keeps it off) or off with
    /// `off`.
    pub fn switch_projected(
        &mut self,
        permission: &str,
        lead_minutes: u16,
    ) -> Result<(), &'static str> {
        let lead = Lead::from_minutes(u64::from(lead_minutes))
            .filter(|_| permission == "granted")
            .map(Lead::minutes);
        if permission == "granted" && lead.is_none() {
            return Err("invalid_lead");
        }
        self.set_alerts(AlertSettings {
            projected_lead_minutes: lead,
            ..self.alerts.clone()
        })
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

    /// PRD 0036 RF6.4: alert on each connection of the aggregator
    /// `account_key` (`each`) or only when a whole pool runs out.
    pub fn set_pool_alerts(&mut self, account_key: &str, each: bool) -> Result<(), &'static str> {
        let mode = if each {
            alerts::PoolAlerts::EachConnection
        } else {
            alerts::PoolAlerts::WholePool
        };
        self.set_alerts(self.alerts.with_pool_mode(account_key, mode))
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

    /// PRD 0029 RF2.3: rewrite the bells and muted metrics `desktop.json`
    /// keeps under a legacy key (`provider:name`) of an account of `config`
    /// to its id, once; entries of no configured account stay for
    /// [`Self::keep_accounts`] to prune. Idempotent.
    pub fn adopt_config(&mut self, config: &Config) -> Result<(), &'static str> {
        let legacy = account_id::legacy_keys(config);
        let mut alerts = self.alerts.clone();
        if account_id::migrate_map(&mut alerts.muted, &legacy) {
            self.set_alerts(alerts)?;
        }
        let mut prefs = self.prefs.clone();
        if account_id::migrate_set(&mut prefs.off_accounts, &legacy) {
            self.set_prefs(prefs)?;
        }
        Ok(())
    }

    /// PRD 0029 RF2.3: move what `notified.json` keeps under the legacy key
    /// of the accounts of `reports` to their ids (saved when it changed).
    /// Called with the readings held before judging new ones.
    pub fn adopt_reports<'a, I>(&mut self, reports: I)
    where
        I: IntoIterator<Item = &'a Report>,
        I::IntoIter: Clone,
    {
        let reports: Vec<&Report> = reports.into_iter().collect();
        self.ledger
            .mutate(|ledger| ledger.adopt_ids(reports.iter().copied()));
    }

    /// Forget the bells, muted metrics, alert state and pending alerts of
    /// accounts no longer in `config` (after a load or a save), once the
    /// legacy entries of its accounts moved to their ids
    /// ([`Self::adopt_config`]).
    pub fn keep_accounts(&mut self, config: &Config) -> Result<(), &'static str> {
        self.adopt_config(config)?;
        let keys: Vec<String> = config.accounts.iter().map(|a| a.state_key()).collect();
        // PRD 0036: a connection's choices live under its aggregator's id.
        let kept = |key: &String| {
            keys.contains(key)
                || keys.iter().any(|parent| {
                    key.strip_prefix(parent.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                })
        };
        self.pending.retain(|event| kept(&event.account_key));
        // Marks still under a legacy key of a configured account are kept
        // until a reading moves them (`adopt_reports`).
        self.ledger
            .mutate(|ledger| ledger.alerts_mut().retain_config(config));
        self.missed.retain(|key, _| keys.contains(key));
        self.digest_pending
            .retain(|event| keys.contains(&event.account_key));
        let mut alerts = self.alerts.clone();
        alerts.muted.retain(|key, _| kept(key));
        alerts.pool_each.retain(|key| keys.contains(key));
        if alerts != self.alerts {
            self.set_alerts(alerts)?;
        }
        let mut prefs = self.prefs.clone();
        prefs.off_accounts.retain(|key| kept(key));
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
        pace: Option<&PaceInput>,
    ) -> Vec<AlertEvent> {
        // PRD 0036 RF6.4: an aggregator alerts per pool unless the user
        // chose each connection for it.
        let mode = self.alerts.pool_mode(&next.state_key());
        let mut events = alerts::evaluate_units(
            prev,
            next,
            &self.evaluator,
            self.ledger.alerts(),
            now,
            mode,
            pace,
        );
        if !self.alerts.enabled {
            events.retain(|event| event.kind == AlertKind::ProjectedExhaustion);
        }
        events
    }

    /// Whether the projected warning is on, so a caller only reads the
    /// recent points (PRD 0024 RF3.2) when they can matter.
    pub fn projected_on(&self) -> bool {
        self.evaluator.projected.is_some()
    }

    /// Remember `next`, the reading the cycle applied (`prev` is the one it
    /// replaced), with the `events` [`Self::assess`] gave for it. Under the
    /// ledger lock (PRD 0032 RF4) the events another process already
    /// notified are dropped; the rest are queued for the next pass of the
    /// timer, except what quiet hours hold (PRD 0032 RF3): those are saved
    /// as deferred for the digest. The first reading of an account in this
    /// process also looks for crossings that happened while the app was
    /// closed (RF8). While alerts are off nothing is recorded: turning them
    /// on treats the next reading as a first reading.
    pub fn record(
        &mut self,
        prev: Option<&Report>,
        next: &Report,
        events: Vec<AlertEvent>,
        now: DateTime<Utc>,
        pace: Option<&PaceInput>,
    ) {
        if !self.evaluator.enabled {
            return;
        }
        // PRD 0024 RF2.4: a projection queued for this account is dropped
        // when this reading voids it (the delivered ones are never recalled).
        if let Some(pace) = pace {
            self.pending
                .retain(|event| !alerts::cancels(event, next, pace, now));
        } else {
            let key = next.state_key();
            self.pending.retain(|event| {
                !(event.kind == AlertKind::ProjectedExhaustion && event.account_key == key)
            });
        }
        self.ledger.sync();
        let window = self.window(now);
        let (mut deliver, mut deferred) = (Vec::new(), Vec::new());
        for event in events {
            match quiet::decide(Subject::Alert(&event), window) {
                Decision::Deliver => deliver.push(event),
                Decision::Defer(until) => deferred.push((event, until)),
                Decision::Drop => {}
            }
        }
        let first = self.ledger.alerts().is_first(next);
        let sent = self
            .ledger
            .claim_alerts(next, &self.evaluator, deliver, deferred, now);
        let key = next.state_key();
        if first {
            let found = self.ledger.alerts().missed(prev, next, &self.evaluator);
            if found.is_empty() {
                self.missed.remove(&key);
            } else {
                self.missed.insert(key, found);
            }
        } else if let Some(list) = self.missed.get_mut(&key) {
            list.retain(|crossing| crossing.is_current(next));
        }
        self.pending.extend(sent);
    }

    /// PRD 0008 RF3.4: the metrics of `report` with an active alert, while
    /// alerts are on and the account's bell and the metric are not muted.
    pub fn active<'r>(&'r self, report: &'r Report) -> impl Iterator<Item = &'r Metric> + 'r {
        let account_key = report.state_key();
        let on = self.alerts.enabled && self.account_on(&account_key);
        self.ledger
            .alerts()
            .active(report)
            .filter(move |metric| on && !self.metric_muted(&account_key, &metric.label))
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

    /// The next notice still ahead of `now` (tray and tooltip) and the
    /// account it names; `None` when notices are off. PRD 0036: for a
    /// connection of an aggregator, `account_index` is the aggregator's and
    /// the account is the connection's display form.
    pub fn next(
        &self,
        config: &Config,
        readings: &[Option<&Report>],
        now: DateTime<Utc>,
    ) -> Option<(ResetEvent, Account)> {
        if !self.prefs.enabled {
            return None;
        }
        let prefs = self.prefs.planner();
        let expanded = aggregators::expand(config, readings);
        let mut event = resets::plan(
            &expanded.config,
            expanded.readings(),
            now,
            &prefs,
            &self.ledger,
        )
        .next?;
        let account = expanded.display(event.account_index).clone();
        if let Some(unit) = expanded
            .origin
            .get(event.account_index)
            .and_then(Option::as_ref)
        {
            event.account_index = unit.parent_index;
        }
        Some((event, account))
    }

    /// One pass of the timer at `now`: deliver the reset notices that are
    /// due (at most one grouped notice per account), the digest that ends
    /// quiet hours and the pending usage alerts ([`Self::deliver_alerts`]),
    /// remember them in the ledger (pruned after eight days) and say when
    /// the next one is due. `readings` follow `config.accounts`; nothing
    /// here queries a provider. During quiet hours (PRD 0032) reset
    /// notices wait, or are dropped when the reset happens inside the
    /// window; the pass is due again when the window ends.
    pub fn tick(
        &mut self,
        config: &Config,
        readings: &[Option<&Report>],
        now: DateTime<Utc>,
        notifier: &dyn Notifier,
    ) -> Tick {
        self.ledger.sync();
        self.ledger.mutate(|ledger| {
            let adopted = ledger.adopt_ids(readings.iter().flatten().copied());
            ledger.prune(now) | adopted
        });
        let window = self.window(now);
        // PRD 0036 RF1.4: an aggregator's connections are planned and
        // notified as accounts after the configured ones.
        let expanded = aggregators::expand(config, readings);
        let mut tick = Tick {
            delivered: 0,
            next: window.map(|window| window.ends_at),
        };
        if self.prefs.enabled {
            let prefs = self.prefs.planner();
            let plan = resets::plan(
                &expanded.config,
                expanded.readings(),
                now,
                &prefs,
                &self.ledger,
            );
            let (mut deliver, mut skipped) = (Vec::new(), Vec::new());
            for event in plan.due {
                let subject = Subject::Reset {
                    resets_at: event.resets_at,
                };
                match quiet::decide(subject, window) {
                    Decision::Deliver => deliver.push(event),
                    Decision::Drop => skipped.push(event),
                    Decision::Defer(_) => {}
                }
            }
            // Recorded even when the delivery fails: a notice is sent at
            // most once, never retried into a burst.
            let sent = self.ledger.claim_resets(deliver, skipped, now);
            for event in &sent {
                let notice = event.notice_with(
                    expanded.display(event.account_index),
                    now,
                    self.prefs.private,
                );
                let locale = self.locale();
                let _ =
                    notifier.deliver(&notice.title.resolve(locale), &notice.body.resolve(locale));
            }
            tick.delivered = sent.len();
            tick.next = min_of(tick.next, plan.next.map(|event| event.notify_at));
        }
        if window.is_none() {
            let events = self.ledger.claim_digest(
                expanded.readings().into_iter().flatten(),
                &self.evaluator,
                self.quiet.digest,
                now,
            );
            self.digest_pending.extend(events);
        }
        let (delivered, next) = self.deliver_digest(&expanded, now, notifier);
        tick.delivered += delivered;
        tick.next = min_of(tick.next, next);
        let (delivered, next) = self.deliver_alerts(&expanded, now, window, notifier);
        tick.delivered += delivered;
        tick.next = min_of(tick.next, next);
        tick
    }

    /// PRD 0032 RF3.3/RF3.4: the digest that closes quiet hours, one
    /// notification subject to [`NOTICE_SPACING`] like any alert.
    fn deliver_digest(
        &mut self,
        expanded: &aggregators::Expanded,
        now: DateTime<Utc>,
        notifier: &dyn Notifier,
    ) -> (usize, Option<DateTime<Utc>>) {
        let config = &expanded.config;
        let keys: Vec<String> = config.accounts.iter().map(|a| a.state_key()).collect();
        let evaluator = &self.evaluator;
        let alerts_on = self.alerts.enabled;
        self.digest_pending.retain(|event| {
            Self::judged(evaluator, alerts_on, event) && keys.contains(&event.account_key)
        });
        if self.digest_pending.is_empty() {
            return (0, None);
        }
        if let Some(due) = self.last_alert.map(|last| last + NOTICE_SPACING) {
            if now < due {
                return (0, Some(due));
            }
        }
        let mut pending = std::mem::take(&mut self.digest_pending);
        let mut groups = Vec::new();
        for (index, key) in keys.iter().enumerate() {
            let account = expanded.display(index);
            let (mine, rest): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|event| event.account_key == *key);
            pending = rest;
            if !mine.is_empty() {
                groups.push((account, mine));
            }
        }
        let Some(notice) = quiet::digest(&groups, now, &chrono::Local, self.prefs.private) else {
            return (0, None);
        };
        let locale = self.locale();
        let _ = notifier.deliver(&notice.title.resolve(locale), &notice.body.resolve(locale));
        self.last_alert = Some(now);
        (1, None)
    }

    /// PRD 0008 RF2.2: the pending usage alerts still wanted (alerts on,
    /// bell and metric not muted, account still configured) as one
    /// notification: one account's alerts together, several accounts'
    /// together once [`NOTICE_SPACING`] holds them. Returns how many went
    /// out (0 or 1) and, while the spacing holds some back, when they may.
    /// Like reset notices, a delivery the system refused is not retried.
    /// Alerts that quiet hours would hold (all but exhaustion) wait in the
    /// queue for the end of the window `window`.
    fn deliver_alerts(
        &mut self,
        expanded: &aggregators::Expanded,
        now: DateTime<Utc>,
        window: Option<QuietWindow>,
        notifier: &dyn Notifier,
    ) -> (usize, Option<DateTime<Utc>>) {
        let config = &expanded.config;
        let keys: Vec<String> = config.accounts.iter().map(|a| a.state_key()).collect();
        let evaluator = &self.evaluator;
        let alerts_on = self.alerts.enabled;
        self.pending.retain(|event| {
            Self::judged(evaluator, alerts_on, event) && keys.contains(&event.account_key)
        });
        let (mut pending, held): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|event| quiet::decide(Subject::Alert(event), window) == Decision::Deliver);
        self.pending = held;
        if pending.is_empty() {
            return (0, None);
        }
        if let Some(due) = self.last_alert.map(|last| last + NOTICE_SPACING) {
            if now < due {
                self.pending.append(&mut pending);
                return (0, Some(due));
            }
        }
        let mut groups = Vec::new();
        for (index, key) in keys.iter().enumerate() {
            let account = expanded.display(index);
            let (mine, rest): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|event| event.account_key == *key);
            pending = rest;
            if !mine.is_empty() {
                groups.push((account, mine));
            }
        }
        let Some(notice) = alerts::notice_with(&groups, now, &chrono::Local, self.prefs.private)
        else {
            return (0, None);
        };
        let locale = self.locale();
        let _ = notifier.deliver(&notice.title.resolve(locale), &notice.body.resolve(locale));
        self.last_alert = Some(now);
        (1, None)
    }
}

fn min_of(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk(key: &str) -> String {
        cuotai_core::AccountId::derive(key, None).to_string()
    }
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
            diagnostic: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    fn on(lead_minutes: u32) -> Notices {
        let mut notices = Notices::default();
        notices
            .set_prefs(NotifyPrefs {
                enabled: true,
                lead_minutes,
                private: false,
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
        notices.set_account(&sk("claude:Work"), false).unwrap();
        assert_eq!(
            tick(&mut notices, &cfg, &reports, t0() - mins(4), &fake).next,
            None
        );
        notices.set_account(&sk("claude:Work"), true).unwrap();
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
        assert!(!reopened.account_on(&sk("claude:Work")));
        assert!(reopened.account_on("claude:Old"));
        assert_eq!(reopened.prefs().off_accounts.len(), 1);
    }

    /// PRD 0029 RF2.3: bells and muted metrics `desktop.json` keeps under
    /// a legacy key move to the account id on load, survive a rename and
    /// a second migration changes nothing.
    #[test]
    fn legacy_bells_and_mutes_move_to_the_id_and_survive_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let mut notices = Notices::open(Some(store(dir.path())));
        notices.set_account("claude:Work", false).unwrap();
        notices
            .set_metric_alert("claude:Work", "5h", false)
            .unwrap();
        let mut cfg = config(&["Work"]);
        cfg.accounts[0].id = cuotai_core::AccountId::parse("acc_0123456789ab");
        let id = cfg.accounts[0].state_key();

        let mut reopened = Notices::open(Some(store(dir.path())));
        reopened.adopt_config(&cfg).unwrap();
        reopened.adopt_config(&cfg).unwrap();
        cfg.accounts[0].name = "Renamed".into();
        reopened.keep_accounts(&cfg).unwrap();
        let after = Notices::open(Some(store(dir.path())));
        assert!(!after.account_on(&id));
        assert!(after.metric_muted(&id, "5h"));
        assert_eq!(after.prefs().off_accounts.len(), 1);
        assert_eq!(after.alerts().muted.len(), 1);
    }

    /// A 9router instance `home` whose two Claude connections used
    /// `used_a`% and `used_b`% of the 5h window at `at`.
    fn router(at: DateTime<Utc>, used_a: f64, used_b: f64) -> Report {
        let sub = |id: &str, used| {
            let mut metric = quota("5h", MetricWindow::Window5h, w());
            metric.used = Some(used);
            cuotai_core::SubReport {
                id: id.into(),
                upstream: "claude".into(),
                provider: Some(Provider::Claude),
                label: id.into(),
                status: Status::Ready,
                metrics: vec![metric],
                notes_l10n: Vec::new(),
                checked_at: at,
                as_of: None,
                enabled_upstream: Some(true),
                duplicate_of: None,
                plan: None,
                via: None,
            }
        };
        Report {
            provider: Provider::NineRouter,
            checked_at: at,
            sub_reports: vec![sub("a", used_a), sub("b", used_b)],
            ..report("home", Vec::new())
        }
    }

    fn router_config() -> Config {
        Config {
            accounts: vec![Account {
                name: "home".into(),
                provider: Provider::NineRouter,
                ..Account::default()
            }],
            ..Config::default()
        }
    }

    /// PRD 0036: the next reset of a connection names the connection and
    /// points at its aggregator's position, never past the configuration.
    #[test]
    fn the_next_reset_of_a_connection_points_at_its_aggregator() {
        let notices = on(0);
        let cfg = router_config();
        let reading = router(t0() - mins(60), 50.0, 0.0);
        let (event, account) = notices
            .next(&cfg, &[Some(&reading)], t0() - mins(5))
            .expect("a used connection has a reset ahead");
        assert_eq!(event.account_index, 0);
        assert_eq!(account.name, "home · a");
        assert_eq!(account.provider, Provider::Claude);
    }

    /// PRD 0036 RF6.4: an aggregator notifies once when a whole pool runs
    /// out (the default), or per connection when the user chose it; a
    /// save keeps the choices of its connections.
    #[test]
    fn an_aggregator_alerts_per_pool_by_default_or_per_connection_by_choice() {
        let cfg = router_config();
        // PRD 0029: the aggregator and its connections are keyed by its id.
        let id = cfg.accounts[0].state_key();
        let conn = format!("{id}/a");
        let fake = FakeNotifier::default();
        let mut notices = alerting(None);
        let a = router(t0(), 50.0, 50.0);
        let b = router(t0() + mins(5), 100.0, 50.0);
        assert_eq!(
            observe(&mut notices, &cfg, Some(&a), &b, &fake).delivered,
            0
        );
        let c = router(t0() + mins(10), 100.0, 100.0);
        let readings = [Some(&c)];
        let events = notices.assess(Some(&b), &c, c.checked_at, None);
        notices.record(None, &c, events, c.checked_at, None);
        assert_eq!(
            notices
                .tick(&cfg, &readings, c.checked_at + NOTICE_SPACING, &fake)
                .delivered,
            1
        );
        assert_eq!(fake.titles(), ["9router · Claude: all accounts exhausted"]);

        let fake = FakeNotifier::default();
        let mut notices = alerting(None);
        notices.set_pool_alerts(&id, true).unwrap();
        let events = notices.assess(Some(&a), &b, b.checked_at, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].account_key, conn);
        notices.record(None, &b, events, b.checked_at, None);
        let readings = [Some(&b)];
        assert_eq!(
            notices.tick(&cfg, &readings, b.checked_at, &fake).delivered,
            1
        );
        // The notice names the connection's provider and label.
        assert!(
            fake.titles()[0].starts_with("Claude: "),
            "{:?}",
            fake.titles()
        );
        assert!(fake.0.borrow()[0].1.starts_with("home · a"));

        // A save keeps the connection's muted metric and the mode.
        notices.set_metric_alert(&conn, "5h", false).unwrap();
        notices.keep_accounts(&cfg).unwrap();
        assert!(notices.metric_muted(&conn, "5h"));
        assert!(notices.alerts().pool_each.contains(&id));
        notices.keep_accounts(&config(&["Work"])).unwrap();
        assert!(!notices.metric_muted(&conn, "5h"));
        assert!(notices.alerts().pool_each.is_empty());
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
        let public = NotifyPrefs {
            private: false,
            ..notices.prefs().clone()
        };
        notices.set_prefs(public).unwrap();
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
        let events = notices.assess(prev, next, now, None);
        notices.record(prev, next, events, now, None);
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
            let events = notices.assess(Some(&a), &b, b.checked_at, None);
            assert_eq!(events.len(), 1);
            notices.record(Some(&a), &b, events, b.checked_at, None);
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
        notices.set_account(&sk("claude:Work"), false).unwrap();
        assert_eq!(notices.tick(&cfg, &[], b.checked_at, &fake).delivered, 0);
        assert_eq!(notices.active(&b).count(), 0);
        assert!(fake.titles().is_empty());
        // A muted metric stays silent; unmuted, only what comes next fires.
        let mut notices = alerting(None);
        notices
            .set_metric_alert(&sk("claude:Work"), "5h", false)
            .unwrap();
        assert_eq!(
            observe(&mut notices, &cfg, Some(&a), &b, &fake).delivered,
            0
        );
        notices
            .set_metric_alert(&sk("claude:Work"), "5h", true)
            .unwrap();
        assert!(!notices.metric_muted(&sk("claude:Work"), "5h"));
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
        let events = notices.assess(Some(&a), &b, b.checked_at, None);
        notices.record(Some(&a), &b, events, b.checked_at, None);
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
        assert!(reopened.metric_muted(&sk("claude:Work"), "5h"));
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

    /// PRD 0024 RF3.2: a steep series, one reading every 5 minutes from
    /// 40% (1 point a minute), judged with its recent points like the
    /// backend does. Returns how many notices went out per reading.
    fn steep_run(notices: &mut Notices, cfg: &Config, readings: usize) -> Vec<usize> {
        let fake = FakeNotifier::default();
        let mut points = Vec::new();
        let mut prev: Option<Report> = None;
        let mut out = Vec::new();
        for i in 0..readings {
            let at = t0() - mins(30) + mins(5 * i as i64);
            let used = 40.0 + 5.0 * i as f64;
            let next = usage("Work", at, used, w());
            points.push(cuotai_core::pace::Point { taken_at: at, used });
            let lists = vec![points.clone()];
            let input = PaceInput {
                interval: std::time::Duration::from_secs(300),
                recent: &lists,
            };
            let events = notices.assess(prev.as_ref(), &next, at, Some(&input));
            notices.record(prev.as_ref(), &next, events, at, Some(&input));
            out.push(notices.tick(cfg, &[], at, &fake).delivered);
            prev = Some(next);
        }
        let titles = fake.titles();
        // PRD 0032 RF7: private texts (the default) name the provider only.
        assert!(titles.iter().all(|t| t.contains("Claude")), "{titles:?}");
        out
    }

    #[test]
    fn the_projected_warning_has_its_own_switch_and_notifies_once_per_window() {
        let cfg = config(&["Work"]);
        // Off by default: nothing, even for a steep series.
        let mut off = Notices::open(None);
        assert!(!off.projected_on());
        assert!(steep_run(&mut off, &cfg, 12).iter().all(|n| *n == 0));
        // A refused permission keeps it off; an unknown lead is refused.
        off.switch_projected("denied", 30).unwrap();
        assert!(!off.projected_on());
        assert_eq!(off.switch_projected("granted", 20), Err("invalid_lead"));
        // On with the usage alerts still off: only the projection notifies.
        let mut on = Notices::open(None);
        on.switch_projected("granted", 60).unwrap();
        assert!(on.projected_on() && !on.alerts().enabled);
        let sent = steep_run(&mut on, &cfg, 12);
        assert_eq!(sent.iter().sum::<usize>(), 1, "{sent:?}");
        // Back off: nothing queued survives.
        on.switch_projected("off", 60).unwrap();
        assert!(!on.projected_on());
    }

    #[test]
    fn a_reading_that_voids_a_queued_projection_drops_it_before_delivery() {
        let cfg = config(&["Work"]);
        let mut notices = Notices::open(None);
        notices.switch_projected("granted", 60).unwrap();
        let mut points = Vec::new();
        let mut prev: Option<Report> = None;
        let mut queued = false;
        // Held back by the spacing: record the projection without ticking.
        for i in 0..4 {
            let at = t0() - mins(30) + mins(5 * i);
            let used = 40.0 + 5.0 * i as f64;
            let next = usage("Work", at, used, w());
            points.push(cuotai_core::pace::Point { taken_at: at, used });
            let lists = vec![points.clone()];
            let input = PaceInput {
                interval: std::time::Duration::from_secs(300),
                recent: &lists,
            };
            let events = notices.assess(prev.as_ref(), &next, at, Some(&input));
            queued |= !events.is_empty();
            notices.record(prev.as_ref(), &next, events, at, Some(&input));
            prev = Some(next);
        }
        assert!(queued, "the steep series queued a projection");
        // The next reading is a new window with little use: the projection
        // is void, so the timer has nothing to deliver.
        let at = t0() - mins(5);
        let fresh = usage("Work", at, 2.0, w() + TimeDelta::hours(5));
        let lists = vec![Vec::new()];
        let input = PaceInput {
            interval: std::time::Duration::from_secs(300),
            recent: &lists,
        };
        notices.record(None, &fresh, Vec::new(), at, Some(&input));
        let fake = FakeNotifier::default();
        assert_eq!(notices.tick(&cfg, &[], at, &fake).delivered, 0);
    }
}

#[cfg(test)]
#[path = "notify_quiet_tests.rs"]
mod quiet_tests;
