//! Quota reset planner (PRD 0003 RF1) and notice ledger (RF2).
//!
//! A pure function over the configuration, the last reports, an injected
//! `now` and the notification preferences: no clock, no network. It only
//! uses `resets_at` already reported by the provider; front ends schedule
//! and deliver the notices. [`plan`] adds what was already notified
//! ([`Ledger`]) and the window for a notice that came late, so a reset is
//! notified once even across restarts, suspension or clock changes.

mod lock;
mod shared;
mod timeline;
pub use lock::{lock_path, LedgerLock};
pub use shared::SharedLedger;
pub use timeline::{
    timeline, timeline_of_config, AccountView, Lane, ShareLevel, Timeline, TimelineRange,
};

use crate::alerts::AlertState;
use crate::metrics::{classify_metric, window_param, MetricState};
use crate::model::{Account, Config, Metric, MetricWindow, Report};
use crate::text::{params, provider_l10n, Localized};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Two `resets_at` of the same metric this close are the same reset
/// (rolling windows drift between queries).
pub const DRIFT_TOLERANCE: TimeDelta = TimeDelta::minutes(2);
/// Metrics of one account resetting this close become one grouped event.
const GROUP_SPAN: TimeDelta = TimeDelta::minutes(1);
/// A reset that passed longer ago than this is missed (the app was closed,
/// asleep or the clock jumped): never notified late.
pub const MISSED_GRACE: TimeDelta = TimeDelta::minutes(10);
/// How long [`Ledger`] remembers a notified reset.
pub const LEDGER_KEEP: TimeDelta = TimeDelta::days(8);

/// When a reset is worth a notice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotifyWhen {
    /// Some quota was consumed (`used > 0` or `remaining < limit`).
    #[default]
    OnlyIfUsed,
    OnlyIfExhausted,
    Always,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefs {
    /// Notice this long before `resets_at`; zero notifies at the reset.
    pub lead: TimeDelta,
    pub when: NotifyWhen,
    /// Let `MetricWindow::Other` quotas in.
    pub include_other: bool,
    /// [`crate::Account::key`]s whose notices are off.
    pub disabled: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetMetric {
    pub label: String,
    pub window: MetricWindow,
    pub resets_at: DateTime<Utc>,
}

/// One notice: the metrics of one account resetting within the same minute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetEvent {
    /// Position in `Config::accounts` (and the snapshot).
    pub account_index: usize,
    pub account_key: String,
    /// Sorted by `resets_at`; never empty.
    pub metrics: Vec<ResetMetric>,
    /// Earliest `resets_at` of the group.
    pub resets_at: DateTime<Utc>,
    /// `resets_at − lead`; may already be past when the lead window opened
    /// before this query.
    pub notify_at: DateTime<Utc>,
}

/// Identity of one metric reset: account key + metric label + `resets_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetKey {
    pub account_key: String,
    pub label: String,
    pub resets_at: DateTime<Utc>,
}

impl ResetKey {
    /// Same account and metric with `resets_at` within [`DRIFT_TOLERANCE`].
    /// A reset postponed beyond it is a new reset: the old notice is
    /// cancelled and the new one scheduled.
    pub fn same_reset(&self, other: &ResetKey) -> bool {
        self.account_key == other.account_key
            && self.label == other.label
            && (self.resets_at - other.resets_at).abs() <= DRIFT_TOLERANCE
    }

    /// SHA-256 of the account key and metric label: what [`Ledger`] keeps
    /// instead of names.
    fn digest(&self) -> String {
        key_digest(&self.account_key, &self.label)
    }
}

pub(crate) fn key_digest(account_key: &str, label: &str) -> String {
    crate::models::digest(format!("{account_key}\u{1f}{label}").as_bytes())
}

/// The text of a notice, resolved by the front end in its language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub title: Localized,
    pub body: Localized,
}

impl ResetEvent {
    pub fn keys(&self) -> impl Iterator<Item = ResetKey> + '_ {
        self.metrics.iter().map(|metric| ResetKey {
            account_key: self.account_key.clone(),
            label: metric.label.clone(),
            resets_at: metric.resets_at,
        })
    }

    /// "Claude Code: 5-hour quota reset" (or "… resets in 15 min" while
    /// `resets_at` is ahead of `now`), with the account name and the metric
    /// labels in the body. Only the provider, the account name and metric
    /// labels: never an e-mail, credential, URL or provider note. The body
    /// says the time is the provider's and the balance comes on the next
    /// refresh.
    pub fn notice(&self, account: &Account, now: DateTime<Utc>) -> Notice {
        self.notice_with(account, now, false)
    }

    /// [`Self::notice`], and with `private` on (PRD 0032 RF7) a body with
    /// neither the account name nor the metric labels.
    pub fn notice_with(&self, account: &Account, now: DateTime<Utc>, private: bool) -> Notice {
        let provider = provider_l10n(account.provider);
        let provider_pt = account.provider.label();
        // Distinct windows in reset order; an `other` window is its label.
        let mut windows: Vec<(crate::text::ParamValue, String)> = Vec::new();
        for metric in &self.metrics {
            let window = window_param(Some(metric.window), &metric.label);
            if !windows.iter().any(|(_, pt)| *pt == window.1) {
                windows.push(window);
            }
        }
        let ahead = self.resets_at - now;
        // Whole minutes, rounded up: "in 15 min" until the reset, never "in 0 min".
        let minutes = (ahead.num_seconds() + 59).div_euclid(60).max(1) as u64;
        let count = self.metrics.len();
        let single = (windows.len() == 1).then(|| windows.remove(0));
        let title = match (single, ahead > TimeDelta::zero()) {
            (Some((window, pt)), false) => Localized::with_params(
                "resets.title.reset",
                params([("provider", provider.into()), ("window", window)]),
                format!("{provider_pt}: cota {pt} renovada"),
            ),
            (Some((window, pt)), true) => Localized::with_params(
                "resets.title.soon",
                params([
                    ("provider", provider.into()),
                    ("window", window),
                    ("minutes", minutes.into()),
                ]),
                format!("{provider_pt}: cota {pt} renova em {minutes} min"),
            ),
            (_, false) => Localized::with_params(
                "resets.title.reset_many",
                params([("provider", provider.into()), ("count", count.into())]),
                format!("{provider_pt}: {count} cotas renovadas"),
            ),
            (_, true) => Localized::with_params(
                "resets.title.soon_many",
                params([
                    ("provider", provider.into()),
                    ("count", count.into()),
                    ("minutes", minutes.into()),
                ]),
                format!("{provider_pt}: {count} cotas renovam em {minutes} min"),
            ),
        };
        if private {
            return Notice {
                title,
                body: Localized::fixed(
                    "resets.body.private",
                    "Pelo horário informado pelo provedor; o novo saldo aparece na próxima atualização.",
                ),
            };
        }
        let labels = self
            .metrics
            .iter()
            .map(|metric| metric.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let body = Localized::with_params(
            "resets.body",
            params([
                ("account", account.name.as_str().into()),
                ("metrics", labels.as_str().into()),
            ]),
            format!(
                "{}: {labels}. Pelo horário informado pelo provedor; o novo saldo aparece na próxima atualização.",
                account.name
            ),
        );
        Notice { title, body }
    }
}

/// Upcoming reset notices, sorted by `notify_at` then account order.
/// `reports` follow `config.accounts`: a `Snapshot`'s reports or one
/// optional reading per account (`None` = nothing read yet); a report that
/// no longer matches its account (provider and name) is skipped. Eligible:
/// a `Quota` metric with a future `resets_at` in a `window_5h`/`daily`/
/// `weekly`/`monthly` window (`other` only with `include_other`) passing
/// `prefs.when`; a missing value is neither used nor exhausted.
pub fn upcoming<'a, I>(
    config: &Config,
    reports: I,
    now: DateTime<Utc>,
    prefs: &Prefs,
) -> Vec<ResetEvent>
where
    I: IntoIterator,
    I::Item: Into<Option<&'a Report>>,
{
    let mut events = Vec::new();
    for (account_index, (account, report)) in config.accounts.iter().zip(reports).enumerate() {
        let Some(report) = report.into() else {
            continue;
        };
        if report.provider != account.provider || report.account != account.name {
            continue;
        }
        let account_key = account.state_key();
        if prefs.disabled.contains(&account_key) {
            continue;
        }
        let mut due: Vec<ResetMetric> = report
            .metrics
            .iter()
            .filter_map(|metric| eligible(metric, now, prefs))
            .collect();
        due.sort_by_key(|metric| metric.resets_at);
        let mut rest = due.into_iter().peekable();
        while let Some(first) = rest.next() {
            let resets_at = first.resets_at;
            let mut metrics = vec![first];
            while let Some(next) = rest.next_if(|m| m.resets_at - resets_at < GROUP_SPAN) {
                metrics.push(next);
            }
            events.push(ResetEvent {
                account_index,
                account_key: account_key.clone(),
                metrics,
                resets_at,
                notify_at: resets_at - prefs.lead,
            });
        }
    }
    events.sort_by_key(|event| (event.notify_at, event.account_index));
    events
}

/// What to deliver now and what comes next.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// At most one event per account (in account order): everything of an
    /// account that is due at once, after a suspension or a clock jump
    /// included, becomes one grouped notice.
    pub due: Vec<ResetEvent>,
    /// The earliest notice still ahead; front ends sleep until its
    /// `notify_at` and show it as the next notice.
    pub next: Option<ResetEvent>,
}

/// [`upcoming`] minus what `ledger` already notified, split into what is
/// due at `now` and the next notice. A notice is due once its `notify_at`
/// passed while its reset is at most [`MISSED_GRACE`] old: a reset missed
/// longer ago (app closed or asleep, clock jumped) is never notified, and
/// a late lead-time notice counts the minutes from `now`. Deliver
/// `due`, then [`Ledger::record`] each event.
pub fn plan<'a, I>(
    config: &Config,
    reports: I,
    now: DateTime<Utc>,
    prefs: &Prefs,
    ledger: &Ledger,
) -> Plan
where
    I: IntoIterator,
    I::Item: Into<Option<&'a Report>>,
{
    let mut plan = Plan::default();
    for mut event in upcoming(config, reports, now - MISSED_GRACE, prefs) {
        let account_key = &event.account_key;
        event.metrics.retain(|metric| {
            !ledger.notified(&key_digest(account_key, &metric.label), metric.resets_at)
        });
        let Some(first) = event.metrics.first() else {
            continue;
        };
        event.resets_at = first.resets_at;
        event.notify_at = event.resets_at - prefs.lead;
        if event.notify_at > now {
            if plan
                .next
                .as_ref()
                .is_none_or(|next| event.notify_at < next.notify_at)
            {
                plan.next = Some(event);
            }
        } else if let Some(due) = plan
            .due
            .iter_mut()
            .find(|due| due.account_index == event.account_index)
        {
            due.metrics.extend(event.metrics);
            due.metrics.sort_by_key(|metric| metric.resets_at);
            due.resets_at = due.resets_at.min(event.resets_at);
            due.notify_at = due.notify_at.min(event.notify_at);
        } else {
            plan.due.push(event);
        }
    }
    plan.due.sort_by_key(|event| event.account_index);
    plan
}

/// The quota resets two consecutive readings of one account reveal (PRD
/// 0013 RF1): each metric of `next` eligible for a reset notice whatever
/// its usage (a `Quota` in a known window, `other` included) whose window
/// instance replaced the one `prev` reported under the same label: `prev`'s
/// `resets_at` had come by `next.checked_at` (within [`DRIFT_TOLERANCE`])
/// and `next`'s lies beyond it by more than the tolerance. A postponed
/// reset, an unknown `resets_at`, a metric missing from either reading or
/// readings of different accounts reveal nothing. In `next`'s metric order.
pub fn passed(prev: &Report, next: &Report) -> Vec<ResetMetric> {
    if prev.provider != next.provider || prev.account != next.account {
        return Vec::new();
    }
    let prefs = Prefs {
        when: NotifyWhen::Always,
        include_other: true,
        ..Prefs::default()
    };
    next.metrics
        .iter()
        .filter_map(|metric| {
            let before = prev
                .metrics
                .iter()
                .find(|before| before.label == metric.label)?
                .resets_at
                .filter(|before| *before <= next.checked_at + DRIFT_TOLERANCE)?;
            eligible(metric, before + DRIFT_TOLERANCE, &prefs)
        })
        .collect()
}

/// Notices already sent, persisted between runs (`notified.json`): the
/// notified resets and the usage alert state (PRD 0008 RF2.1), only digests
/// of account key and metric label plus thresholds and dates, forgotten
/// after [`LEDGER_KEEP`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    notified: Vec<Notified>,
    #[serde(default, skip_serializing_if = "AlertState::is_empty")]
    alerts: AlertState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Notified {
    digest: String,
    resets_at: DateTime<Utc>,
    at: DateTime<Utc>,
    /// PRD 0032 RF3.2: dropped by quiet hours, never sent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    skipped: bool,
}

impl Ledger {
    /// `<OS local data dir>/cuotai/notified.json`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|dir| dir.join("cuotai").join("notified.json"))
    }

    /// A missing file is an empty ledger. A file that cannot be parsed is
    /// set aside as `notified.json.corrupt` and read as empty, so the next
    /// save recreates it (PRD 0032 §6).
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = std::fs::read(path) else {
            return Self::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            let mut aside = path.as_os_str().to_owned();
            aside.push(".corrupt");
            let _ = std::fs::rename(path, aside);
            Self::default()
        })
    }

    /// Atomic write (temporary file + rename), readable by its owner only
    /// on Unix.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
        std::fs::create_dir_all(dir)?;
        let body = serde_json::to_vec(self)?;
        // tempfile creates 0600 files on Unix.
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&body)?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    /// The same reset (within [`DRIFT_TOLERANCE`]) of the metric behind
    /// `digest` was notified.
    fn notified(&self, digest: &str, resets_at: DateTime<Utc>) -> bool {
        self.notified.iter().any(|entry| {
            entry.digest == digest && (entry.resets_at - resets_at).abs() <= DRIFT_TOLERANCE
        })
    }

    /// Remember every metric of `event` as notified at `now`.
    pub fn record(&mut self, event: &ResetEvent, now: DateTime<Utc>) {
        for key in event.keys() {
            self.notified.push(Notified {
                digest: key.digest(),
                resets_at: key.resets_at,
                at: now,
                skipped: false,
            });
        }
    }

    /// PRD 0032 RF3.2: remember every metric of `event` as dropped by
    /// quiet hours, so it is never sent at the end of the window.
    pub fn record_skipped(&mut self, event: &ResetEvent, now: DateTime<Utc>) {
        for key in event.keys() {
            self.notified.push(Notified {
                digest: key.digest(),
                resets_at: key.resets_at,
                at: now,
                skipped: true,
            });
        }
    }

    /// PRD 0032 RF4.2: run `change` on the ledger **as it is on disk** while
    /// holding `notified.lock`, then write it back: the process re-reads
    /// what another one recorded, records its own and releases the lock
    /// before anything is delivered. The in-memory first-reading set is
    /// kept. When the lock cannot be taken the change still applies, to the
    /// freshest copy (a rare duplicate is the worst case).
    pub fn claim<R>(&mut self, path: &Path, change: impl FnOnce(&mut Ledger) -> R) -> R {
        let _lock = LedgerLock::acquire(path);
        let mut fresh = Self::load(path);
        fresh.alerts.keep_session_of(&self.alerts);
        *self = fresh;
        let result = change(self);
        // A failed write only risks repeating a notice after a restart.
        let _ = self.save(path);
        result
    }

    /// PRD 0032 RF4: re-read the ledger from `path` (no lock: writes are
    /// atomic renames), keeping the in-memory first-reading set. Call it
    /// when the file's modification time changed.
    pub fn reload(&mut self, path: &Path) {
        let mut fresh = Self::load(path);
        fresh.alerts.keep_session_of(&self.alerts);
        *self = fresh;
    }

    /// PRD 0032 RF4: remember `next` and claim the usage alerts `events` it
    /// deserves (judged on a possibly stale copy): the ones another process
    /// already notified are dropped, the rest recorded as notified (the
    /// `deferred` ones, held by quiet hours until their instant, as
    /// deferred) and returned for delivery. Nothing is locked or written
    /// when the reading changes no mark and carries no event. With no
    /// `path` the in-memory state is the truth.
    pub fn claim_alerts(
        &mut self,
        path: Option<&Path>,
        next: &Report,
        prefs: &crate::alerts::AlertPrefs,
        events: Vec<crate::alerts::AlertEvent>,
        deferred: Vec<(crate::alerts::AlertEvent, DateTime<Utc>)>,
        now: DateTime<Utc>,
    ) -> Vec<crate::alerts::AlertEvent> {
        let mut probe = self.alerts.clone();
        let changed = probe.update_units_with(next, prefs, &events, &deferred, now);
        let Some(path) = path.filter(|_| changed || !events.is_empty() || !deferred.is_empty())
        else {
            self.alerts = probe;
            return events;
        };
        self.claim(path, |ledger| {
            let events: Vec<_> = events
                .into_iter()
                .filter(|event| !ledger.alerts.notified(event))
                .collect();
            let deferred: Vec<_> = deferred
                .into_iter()
                .filter(|(event, _)| {
                    !ledger.alerts.notified(event) && !ledger.alerts.deferred(event)
                })
                .collect();
            ledger
                .alerts
                .update_units_with(next, prefs, &events, &deferred, now);
            events
        })
    }

    /// PRD 0032 RF3.3 and RF4.3: settle the crossings deferred until `now`
    /// or before ([`AlertState::take_deferred`]) under the lock, so only
    /// one process sends the digest. Nothing is locked while no crossing
    /// waits.
    pub fn claim_digest<'r>(
        &mut self,
        path: Option<&Path>,
        reports: impl IntoIterator<Item = &'r Report>,
        prefs: &crate::alerts::AlertPrefs,
        digest: bool,
        now: DateTime<Utc>,
    ) -> Vec<crate::alerts::AlertEvent> {
        if self.alerts.next_deferred().is_none_or(|due| due > now) {
            return Vec::new();
        }
        let reports: Vec<&Report> = reports.into_iter().collect();
        match path {
            Some(path) => self.claim(path, |ledger| {
                ledger
                    .alerts
                    .take_deferred(reports.iter().copied(), prefs, digest, now)
            }),
            None => self
                .alerts
                .take_deferred(reports.iter().copied(), prefs, digest, now),
        }
    }

    /// PRD 0032 RF4: claim the reset notices in `due` (from a plan made on
    /// a possibly stale copy): the metrics another process already recorded
    /// are dropped, the rest recorded (`skipped` ones as dropped by quiet
    /// hours) and returned for delivery. With no `path` (tests, no data
    /// dir) the in-memory ledger is the truth.
    pub fn claim_resets(
        &mut self,
        path: Option<&Path>,
        due: Vec<ResetEvent>,
        skipped: Vec<ResetEvent>,
        now: DateTime<Utc>,
    ) -> Vec<ResetEvent> {
        let apply = |ledger: &mut Ledger| {
            let mut kept = Vec::new();
            for mut event in due {
                let account = event.account_key.clone();
                event.metrics.retain(|metric| {
                    !ledger.notified(&key_digest(&account, &metric.label), metric.resets_at)
                });
                if let Some(first) = event.metrics.first() {
                    event.resets_at = first.resets_at;
                    ledger.record(&event, now);
                    kept.push(event);
                }
            }
            for mut event in skipped {
                let account = event.account_key.clone();
                event.metrics.retain(|metric| {
                    !ledger.notified(&key_digest(&account, &metric.label), metric.resets_at)
                });
                if !event.metrics.is_empty() {
                    ledger.record_skipped(&event, now);
                }
            }
            kept
        };
        match path {
            Some(path) => self.claim(path, apply),
            None => apply(self),
        }
    }

    /// Forget what was notified and reset more than [`LEDGER_KEEP`] before
    /// `now`, and the alert marks as old ([`AlertState::prune`]); `true`
    /// when something was dropped.
    pub fn prune(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.notified.len();
        self.notified
            .retain(|entry| entry.resets_at.max(entry.at) >= now - LEDGER_KEEP);
        let alerts = self.alerts.prune(now);
        self.notified.len() != before || alerts
    }

    /// PRD 0029 RF2.3: rewrite the reset notices and alert marks an older
    /// release keyed by the legacy key of each report's account to its state
    /// key (see [`AlertState::adopt_ids`]). Front ends call it with the
    /// readings they hold before judging new ones. Idempotent; `true` when
    /// something changed and the ledger should be saved.
    pub fn adopt_ids<'a, I>(&mut self, reports: I) -> bool
    where
        I: IntoIterator<Item = &'a Report>,
        I::IntoIter: Clone,
    {
        let reports = reports.into_iter();
        let mut changed = false;
        for report in reports.clone() {
            let (legacy, id) = (report.account_key(), report.state_key());
            if legacy == id {
                continue;
            }
            for metric in &report.metrics {
                let (old, new) = (
                    key_digest(&legacy, &metric.label),
                    key_digest(&id, &metric.label),
                );
                for entry in &mut self.notified {
                    if entry.digest == old {
                        entry.digest = new.clone();
                        changed = true;
                    }
                }
            }
        }
        changed | self.alerts.adopt_ids(reports)
    }

    /// What usage alerts remember (PRD 0008).
    pub fn alerts(&self) -> &AlertState {
        &self.alerts
    }

    pub fn alerts_mut(&mut self) -> &mut AlertState {
        &mut self.alerts
    }
}

pub(crate) fn eligible(metric: &Metric, now: DateTime<Utc>, prefs: &Prefs) -> Option<ResetMetric> {
    let resets_at = metric.resets_at.filter(|at| *at > now)?;
    let window = metric.window.filter(|window| match window {
        MetricWindow::Other => prefs.include_other,
        _ => true,
    })?;
    if !crate::metrics::is_plan_limit(metric) {
        return None;
    }
    let exhausted = matches!(classify_metric(metric), MetricState::Exhausted { .. });
    let finite = |value: Option<f64>| value.filter(|v| v.is_finite());
    let used = finite(metric.used).is_some_and(|used| used > 0.0)
        || matches!(
            (finite(metric.remaining), finite(metric.limit)),
            (Some(remaining), Some(limit)) if remaining < limit
        );
    let wanted = match prefs.when {
        NotifyWhen::OnlyIfUsed => used,
        NotifyWhen::OnlyIfExhausted => exhausted,
        NotifyWhen::Always => true,
    };
    wanted.then(|| ResetMetric {
        label: metric.label.clone(),
        window,
        resets_at,
    })
}

/// PRD 0024 RF1.2: ↺ "Returns {at}" for the time the quota comes back
/// (`blocked_until` or the binding reset), already formatted by the
/// caller; `None` (an unknown return) reads "Return unknown", never a time.
pub fn returns_chip(at: Option<&str>) -> crate::pace::Chip {
    let text = match at {
        Some(at) => Localized::with_param("resets.chip.returns", "at", at, format!("Volta {at}")),
        None => Localized::fixed("resets.chip.return_unknown", "Retorno incerto"),
    };
    crate::pace::Chip {
        glyph: crate::pace::GLYPH_RETURNS,
        text,
        compact: at.map(str::to_owned),
        hint: None,
    }
}

/// PRD 0024 RF1.4: "Resets {at} · {in}" for the details, with `at` the
/// clock time formatted by the caller (DS-MSG-8) and `in` the core
/// [`crate::text::countdown`] of the time left.
pub fn pair_label(at: &str, left: TimeDelta) -> Localized {
    let countdown = crate::text::countdown(left);
    let fallback = format!("Renova {at} · {}", countdown.fallback);
    Localized::with_params(
        "resets.pair",
        params([("at", at.into()), ("in", countdown.into())]),
        fallback,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk(key: &str) -> String {
        crate::AccountId::derive(key, None).to_string()
    }
    use crate::model::{Account, MetricKind, Provider, Status};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 15, 12, 0, 0).unwrap()
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    fn quota(label: &str, used: Option<f64>, resets_at: Option<DateTime<Utc>>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used,
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at,
        }
    }

    fn setup(metrics: Vec<Vec<Metric>>) -> (Config, Vec<Report>) {
        let accounts: Vec<Account> = (0..metrics.len())
            .map(|i| Account {
                id: None,
                name: format!("acc{i}"),
                provider: Provider::Claude,
                ..Account::default()
            })
            .collect();
        let reports = accounts
            .iter()
            .zip(metrics)
            .map(|(account, metrics)| Report {
                account_id: None,
                diagnostic: None,
                account: account.name.clone(),
                provider: account.provider,
                checked_at: now(),
                status: Status::Ready,
                metrics,
                notes: Vec::new(),
                notes_l10n: Vec::new(),
                source: String::new(),
                session_expires_at: None,
                sub_reports: Vec::new(),
                as_of: None,
                overage_hint: None,
            })
            .collect();
        let config = Config {
            accounts,
            ..Config::default()
        };
        (config, reports)
    }

    /// PRD 0029 RF2.3: a `notified.json` written before account ids (marks
    /// and notices keyed by `provider:name`) moves to the account's id once
    /// a reading of it is in hand, so a rename afterwards keeps the pending
    /// reset notice and the active 90 % alert; running it again changes
    /// nothing, and the file keeps the shape an older release reads.
    #[test]
    fn a_legacy_ledger_moves_to_the_id_and_survives_a_rename() {
        let resets_at = now() + mins(30);
        let legacy = "claude:acc0";
        let digest = |key: &str| crate::models::digest(key.as_bytes());
        let file = serde_json::json!({
            "notified": [{
                "digest": key_digest(legacy, "5h"),
                "resets_at": resets_at,
                "at": now(),
            }],
            "alerts": { "marks": [{
                "account": digest(legacy),
                "metric": key_digest(legacy, "5h"),
                "kind": "threshold",
                "threshold": 90,
                "resets_at": resets_at,
                "notified": true,
                "at": now(),
            }]},
        });
        let mut ledger: Ledger = serde_json::from_value(file).unwrap();
        let (mut config, mut reports) = setup(vec![vec![quota("5h", Some(95.0), Some(resets_at))]]);
        let id = crate::AccountId::parse("acc_0123456789ab").unwrap();
        config.accounts[0].id = Some(id.clone());
        reports[0].account_id = Some(id.clone());

        assert!(ledger.adopt_ids(&reports));
        assert!(!ledger.adopt_ids(&reports), "idempotent");
        // Pruning by the configuration keeps the adopted marks.
        assert!(!ledger.alerts_mut().retain_config(&config));

        // Rename: same id, another legacy key.
        config.accounts[0].name = "renamed".into();
        reports[0].account = "renamed".into();
        assert!(ledger.notified(&key_digest(id.as_str(), "5h"), resets_at));
        assert_eq!(
            ledger
                .alerts()
                .active(&reports[0])
                .map(|metric| metric.label.as_str())
                .collect::<Vec<_>>(),
            ["5h"]
        );
        let prefs = Prefs::default();
        let plan = plan(&config, &reports, now() + mins(29), &prefs, &ledger);
        assert!(plan.due.is_empty() && plan.next.is_none(), "{plan:?}");
        // Still the file shape an older release reads.
        let saved = serde_json::to_value(&ledger).unwrap();
        assert!(saved["notified"][0]["digest"].is_string());
        assert!(saved["alerts"]["marks"][0]["metric"].is_string());
    }

    fn labels(metrics: Vec<Metric>, prefs: &Prefs) -> Vec<String> {
        let (config, reports) = setup(vec![metrics]);
        upcoming(&config, &reports, now(), prefs)
            .into_iter()
            .flat_map(|event| event.metrics.into_iter().map(|m| m.label))
            .collect()
    }

    #[test]
    fn eligibility_table() {
        let soon = Some(now() + mins(30));
        let with = |mut metric: Metric, edit: fn(&mut Metric)| {
            edit(&mut metric);
            metric
        };
        let used = quota("used", Some(10.0), soon);
        let idle = quota("idle", Some(0.0), soon);
        let spent = quota("spent", Some(100.0), soon);
        let below = with(quota("below", None, soon), |m| m.remaining = Some(60.0));
        let full = with(quota("full", None, soon), |m| m.remaining = Some(100.0));
        let exhausted_without_limit = with(quota("exhausted-no-limit", None, soon), |m| {
            m.limit = None;
            m.remaining = Some(0.0);
        });
        let unknown = quota("unknown", None, soon);
        let base = vec![
            used.clone(),
            idle.clone(),
            spent.clone(),
            below.clone(),
            full.clone(),
            unknown.clone(),
            exhausted_without_limit.clone(),
        ];
        let prefs = |when| Prefs {
            when,
            ..Prefs::default()
        };
        let other = with(quota("other", Some(10.0), soon), |m| {
            m.window = Some(MetricWindow::Other)
        });
        let cases: Vec<(&str, Vec<Metric>, Prefs, Vec<&str>)> = vec![
            (
                "only if used: used > 0 or remaining < limit; missing limit is not used",
                base.clone(),
                Prefs::default(),
                vec!["used", "spent", "below"],
            ),
            (
                "only if exhausted, including remaining zero without a reported limit",
                base.clone(),
                prefs(NotifyWhen::OnlyIfExhausted),
                vec!["spent", "exhausted-no-limit"],
            ),
            (
                "always",
                base,
                prefs(NotifyWhen::Always),
                vec![
                    "used",
                    "idle",
                    "spent",
                    "below",
                    "full",
                    "unknown",
                    "exhausted-no-limit",
                ],
            ),
            (
                "no resets_at",
                vec![quota("none", Some(10.0), None)],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "past or current resets_at",
                vec![
                    quota("past", Some(10.0), Some(now() - mins(1))),
                    quota("now", Some(10.0), Some(now())),
                ],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "rate limit and balance never enter",
                vec![
                    with(used.clone(), |m| m.kind = MetricKind::RateLimit),
                    with(used.clone(), |m| m.kind = MetricKind::Balance),
                ],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "overage never enters",
                vec![with(used.clone(), |m| {
                    m.billing = crate::model::Billing::Overage
                })],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "no window",
                vec![with(used.clone(), |m| m.window = None)],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "other window off",
                vec![other.clone()],
                Prefs::default(),
                vec![],
            ),
            (
                "other window on",
                vec![other],
                Prefs {
                    include_other: true,
                    ..Prefs::default()
                },
                vec!["other"],
            ),
        ];
        for (case, metrics, prefs, expected) in cases {
            let mut got = labels(metrics, &prefs);
            got.sort();
            let mut expected: Vec<String> = expected.into_iter().map(String::from).collect();
            expected.sort();
            assert_eq!(got, expected, "{case}");
        }
    }

    #[test]
    fn groups_same_minute_per_account_and_applies_lead() {
        let at = now() + mins(60);
        let (config, reports) = setup(vec![
            vec![
                quota("weekly", Some(5.0), Some(at + TimeDelta::seconds(40))),
                quota("5h", Some(5.0), Some(at)),
                quota("later", Some(5.0), Some(at + mins(5))),
            ],
            vec![quota("5h", Some(5.0), Some(at))],
        ]);
        let prefs = Prefs {
            lead: mins(15),
            ..Prefs::default()
        };
        let events = upcoming(&config, &reports, now(), &prefs);
        let shape: Vec<(usize, Vec<&str>, DateTime<Utc>)> = events
            .iter()
            .map(|e| {
                let labels = e.metrics.iter().map(|m| m.label.as_str()).collect();
                (e.account_index, labels, e.notify_at)
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, vec!["5h", "weekly"], at - mins(15)),
                (1, vec!["5h"], at - mins(15)),
                (0, vec!["later"], at + mins(5) - mins(15)),
            ]
        );
        assert_eq!(events[0].account_key, sk("claude:acc0"));
        assert_eq!(events[0].resets_at, at);
    }

    #[test]
    fn disabled_or_mismatched_account_is_skipped() {
        let soon = Some(now() + mins(30));
        let (mut config, reports) = setup(vec![
            vec![quota("a", Some(5.0), soon)],
            vec![quota("b", Some(5.0), soon)],
        ]);
        let prefs = Prefs {
            disabled: BTreeSet::from([sk("claude:acc0")]),
            ..Prefs::default()
        };
        let events = upcoming(&config, &reports, now(), &prefs);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].account_index, 1);
        // A renamed account no longer matches its stale report.
        config.accounts[1].name = "renamed".into();
        assert!(upcoming(&config, &reports, now(), &prefs).is_empty());
    }

    #[test]
    fn drift_within_tolerance_is_the_same_reset() {
        let at = now() + mins(60);
        let key = |resets_at| {
            let (config, reports) = setup(vec![vec![quota("5h", Some(5.0), Some(resets_at))]]);
            let events = upcoming(&config, &reports, now(), &Prefs::default());
            let key = events[0].keys().next().unwrap();
            key
        };
        let first = key(at);
        for (case, later, same) in [
            ("drift below tolerance", at + TimeDelta::seconds(90), true),
            ("drift at tolerance", at - DRIFT_TOLERANCE, true),
            ("postponed beyond tolerance", at + mins(3), false),
        ] {
            assert_eq!(first.same_reset(&key(later)), same, "{case}");
        }
        let mut other = first.clone();
        other.label = "weekly".into();
        assert!(!first.same_reset(&other), "different metric");
    }

    fn event(metrics: &[(&str, MetricWindow, DateTime<Utc>)]) -> ResetEvent {
        let metrics: Vec<ResetMetric> = metrics
            .iter()
            .map(|(label, window, resets_at)| ResetMetric {
                label: (*label).into(),
                window: *window,
                resets_at: *resets_at,
            })
            .collect();
        ResetEvent {
            account_index: 0,
            account_key: "claude:Work".into(),
            resets_at: metrics[0].resets_at,
            notify_at: metrics[0].resets_at,
            metrics,
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "Work".into(),
            provider: Provider::Claude,
            ..Account::default()
        }
    }

    #[test]
    fn notice_text_in_three_languages_names_only_provider_window_account_and_labels() {
        let reset = event(&[("Session (5h)", MetricWindow::Window5h, now())]);
        let notice = reset.notice(&account(), now());
        let cases = [
            ("pt-BR", "Claude: cota de 5 horas renovada"),
            ("en-US", "Claude: 5-hour quota reset"),
            ("es", "Claude: cuota de 5 horas renovada"),
        ];
        for (locale, title) in cases {
            assert_eq!(notice.title.resolve(locale), title, "{locale}");
            let body = notice.body.resolve(locale);
            assert!(body.starts_with("Work: Session (5h). "), "{locale}: {body}");
        }
        // Ahead of the reset: whole minutes rounded up, never "0 min".
        let soon = reset.notice(&account(), now() - TimeDelta::seconds(14 * 60 + 10));
        assert_eq!(
            soon.title.resolve("en-US"),
            "Claude: 5-hour quota resets in 15 min"
        );
        let last_second = reset.notice(&account(), now() - TimeDelta::seconds(1));
        assert_eq!(
            last_second.title.resolve("es"),
            "Claude: la cuota de 5 horas se renueva en 1 min"
        );
    }

    #[test]
    fn notice_of_several_windows_counts_them_and_an_other_window_is_its_label() {
        let many = event(&[
            ("Session (5h)", MetricWindow::Window5h, now()),
            ("Week", MetricWindow::Weekly, now()),
        ]);
        let notice = many.notice(&account(), now());
        assert_eq!(notice.title.resolve("en-US"), "Claude: 2 quotas reset");
        assert!(notice
            .body
            .resolve("en-US")
            .starts_with("Work: Session (5h), Week. "));
        let same_window = event(&[
            ("Opus (5h)", MetricWindow::Window5h, now()),
            ("Sonnet (5h)", MetricWindow::Window5h, now()),
        ]);
        assert_eq!(
            same_window.notice(&account(), now()).title.resolve("en-US"),
            "Claude: 5-hour quota reset"
        );
        let other = event(&[("Credits", MetricWindow::Other, now())]);
        assert_eq!(
            other.notice(&account(), now()).title.resolve("pt-BR"),
            "Claude: cota Credits renovada"
        );
    }

    #[test]
    fn ledger_file_keeps_only_digests_and_dates_and_forgets_after_eight_days() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai").join("notified.json");
        assert_eq!(Ledger::load(&path), Ledger::default(), "missing file");
        let reset = event(&[("Session (5h)", MetricWindow::Window5h, now())]);
        let mut ledger = Ledger::default();
        ledger.record(&reset, now());
        ledger.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("Work") && !text.contains("Session") && !text.contains("claude"));
        let loaded = Ledger::load(&path);
        assert_eq!(loaded, ledger);
        let mut kept = loaded.clone();
        assert!(!kept.prune(now() + LEDGER_KEEP), "eight days are kept");
        assert!(kept.prune(now() + LEDGER_KEEP + TimeDelta::seconds(1)));
        assert_eq!(kept, Ledger::default());
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Ledger::load(&path), Ledger::default(), "unreadable file");
    }

    /// PRD 0013 RF1: a reset is a new window instance after the old one
    /// came; drift, postponement, unknown times and other kinds are not.
    #[test]
    fn passed_resets_need_the_old_instance_to_come_and_a_new_one_beyond_drift() {
        let reading = |at: DateTime<Utc>, metrics: Vec<Metric>| {
            let (_, mut reports) = setup(vec![metrics]);
            let mut report = reports.remove(0);
            report.checked_at = at;
            report
        };
        let old = now() + mins(1);
        let labels = |prev: &Report, next: &Report| -> Vec<String> {
            passed(prev, next).into_iter().map(|m| m.label).collect()
        };
        let prev = reading(now(), vec![quota("q", Some(90.0), Some(old))]);
        let cases = [
            (
                "new instance",
                now() + mins(5),
                Some(old + TimeDelta::hours(5)),
                true,
            ),
            (
                "idle quota resets too",
                now() + mins(5),
                Some(old + TimeDelta::hours(5)),
                true,
            ),
            ("drift only", now() + mins(5), Some(old + mins(2)), false),
            (
                "postponed before it came",
                now() - mins(2),
                Some(old + TimeDelta::hours(5)),
                false,
            ),
            ("unknown new time", now() + mins(5), None, false),
            (
                "came within the drift tolerance",
                old - mins(2),
                Some(old + TimeDelta::hours(5)),
                true,
            ),
        ];
        for (case, at, resets_at, expected) in cases {
            let used = if case.starts_with("idle") { 0.0 } else { 5.0 };
            let next = reading(at, vec![quota("q", Some(used), resets_at)]);
            assert_eq!(labels(&prev, &next) == ["q"], expected, "{case}");
        }
        let mut other_kind = quota("q", Some(5.0), Some(old + TimeDelta::hours(5)));
        other_kind.kind = MetricKind::Credits;
        let next = reading(now() + mins(5), vec![other_kind]);
        assert!(labels(&prev, &next).is_empty(), "not a quota");
        let mut other_account = reading(
            now() + mins(5),
            vec![quota("q", Some(5.0), Some(old + TimeDelta::hours(5)))],
        );
        other_account.account = "acc9".into();
        assert!(labels(&prev, &other_account).is_empty(), "another account");
        let unknown_before = reading(now(), vec![quota("q", Some(90.0), None)]);
        let next = reading(
            now() + mins(5),
            vec![quota("q", Some(5.0), Some(old + TimeDelta::hours(5)))],
        );
        assert!(
            labels(&unknown_before, &next).is_empty(),
            "unknown old time"
        );
    }
}

#[cfg(test)]
#[path = "resets_claim_tests.rs"]
mod claim_tests;
