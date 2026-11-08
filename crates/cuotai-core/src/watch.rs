//! `cuotai --watch` (PRD 0013 RF2) without its terminal: the interval it
//! accepts, the events it can select and the [`Watcher`] that turns each
//! new [`Export`] of the refresh cycle into the selected [`Event`]s.
//!
//! The watcher only reads exports (built by [`crate::headless::Driver`]
//! from the cycle the front end already runs), so watching never adds a
//! query. Its state lives in memory: a new watcher starts new baselines.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::account_edit::MAX_REFRESH_SECONDS;
use crate::alerts::PaceInput;
use crate::events::{self, Event, EventKind, EventState};
use crate::export::v1::{AccountExport, Export};
use crate::pace::Point;
use crate::text::{params, Localized, ParamValue};
use crate::{Config, Provider};

/// The shortest `--interval` accepted; a shorter one is an error, never
/// silently raised.
pub const MIN_INTERVAL_SECONDS: u64 = 60;

/// `config` refreshed every `seconds` instead of its global
/// `refresh_seconds`, for one run (nothing is saved). An account with its
/// own `refresh_seconds` keeps it, and a provider billed per query (AWS
/// Bedrock) is never polled more often than
/// [`Provider::METERED_MIN_REFRESH_SECONDS`]. `seconds` outside
/// [`MIN_INTERVAL_SECONDS`]..=[`MAX_REFRESH_SECONDS`] is rejected.
pub fn with_interval(config: &Config, seconds: u64) -> Result<Config, Localized> {
    if !(MIN_INTERVAL_SECONDS..=MAX_REFRESH_SECONDS).contains(&seconds) {
        return Err(Localized::with_params(
            "watch.error.interval_range",
            params([
                ("min", ParamValue::from(MIN_INTERVAL_SECONDS)),
                ("max", ParamValue::from(MAX_REFRESH_SECONDS)),
            ]),
            "The watch interval must be between {min} and {max} seconds; smaller values are rejected, not adjusted.",
        ));
    }
    let mut config = config.clone();
    for account in &mut config.accounts {
        if account.refresh_seconds.is_none() {
            let floor = if account.provider.is_metered() {
                Provider::METERED_MIN_REFRESH_SECONDS
            } else {
                0
            };
            account.refresh_seconds = Some(seconds.max(floor));
        }
    }
    Ok(config)
}

/// The events named in `names` ([`EventKind::name`]); an unknown or empty
/// name is an error that lists the valid ones.
pub fn parse_events<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<EventKind>, Localized> {
    names
        .into_iter()
        .map(|name| {
            let name = name.trim();
            EventKind::from_name(name).ok_or_else(|| {
                let known = EventKind::ALL.map(EventKind::name).join(", ");
                Localized::with_params(
                    "watch.error.unknown_event",
                    params([
                        ("name", ParamValue::from(name)),
                        ("known", ParamValue::from(known.as_str())),
                    ]),
                    "Unknown event: \"{name}\". Valid events: {known}.",
                )
            })
        })
        .collect()
}

/// The transitions between consecutive exports of one configuration.
#[derive(Debug)]
pub struct Watcher {
    state: EventState,
    selected: Option<BTreeSet<EventKind>>,
    previous: BTreeMap<String, AccountExport>,
}

impl Watcher {
    /// A watcher with `state`'s thresholds that reports the `selected`
    /// events (every event with `None`).
    pub fn new(state: EventState, selected: Option<BTreeSet<EventKind>>) -> Self {
        Self {
            state,
            selected,
            previous: BTreeMap::new(),
        }
    }

    /// PRD 0045 RF5.2: the account ids whose breakdown rows have events
    /// (`desktop.json` `alerts.breakdown_each`, read again by the caller
    /// before each observation).
    pub fn set_breakdown_alerts(&mut self, accounts: BTreeSet<String>) {
        self.state.set_breakdown_alerts(accounts);
    }

    /// The selected events of `export` after the export last observed
    /// ([`events::diff`] per account, in configuration order). The first
    /// reading of an account is its baseline; an account no longer in
    /// `export` is forgotten. Events not selected still move the state, so
    /// selecting never changes which transitions are real.
    pub fn observe(&mut self, export: &Export, now: DateTime<Utc>) -> Vec<Event> {
        self.observe_with(export, now, |_, _| None)
    }

    /// [`Self::observe`] with each account's pace input (PRD 0024 RF5):
    /// `pace(index, report)` gives the account's effective refresh interval
    /// and the recent points of each metric of `report`
    /// ([`crate::Monitor::recent_points`]), so `quota_exhaustion_projected`
    /// can fire; `None` never projects for that reading.
    pub fn observe_with(
        &mut self,
        export: &Export,
        now: DateTime<Utc>,
        pace: impl Fn(usize, &crate::Report) -> Option<(std::time::Duration, Vec<Vec<Point>>)>,
    ) -> Vec<Event> {
        self.observe_transitions(export, now, pace)
            .into_iter()
            .map(|observed| observed.event)
            .collect()
    }

    /// PRD 0054 RF2.2: [`Self::observe_with`] with each event's
    /// [`Transition`], which hooks claim across processes before running.
    pub fn observe_transitions(
        &mut self,
        export: &Export,
        now: DateTime<Utc>,
        pace: impl Fn(usize, &crate::Report) -> Option<(std::time::Duration, Vec<Vec<Point>>)>,
    ) -> Vec<ObservedEvent> {
        let mut out = Vec::new();
        let units = export.units();
        for (next, parent) in &units {
            let blank;
            let prev = match self.previous.get(&next.id) {
                Some(prev) => prev,
                None => {
                    blank = AccountExport {
                        report: None,
                        ..next.clone()
                    };
                    &blank
                }
            };
            out.extend(
                {
                    let input = next
                        .report
                        .as_ref()
                        // A connection of an aggregator has no recorded pace of its own.
                        .filter(|_| parent.is_none())
                        .and_then(|report| pace(next.index, report));
                    let input = input.as_ref().map(|(interval, recent)| PaceInput {
                        interval: *interval,
                        recent,
                    });
                    events::diff_with(prev, next, &mut self.state, now, input.as_ref())
                }
                .into_iter()
                .map(|mut event| {
                    event.parent.clone_from(parent);
                    if event.event == EventKind::ProviderIncident {
                        event.accounts = incident_accounts(export, next);
                    }
                    event
                })
                .filter(|event| {
                    self.selected
                        .as_ref()
                        .is_none_or(|selected| selected.contains(&event.event))
                })
                .map(|event| ObservedEvent {
                    transition: Transition::of(&event, next),
                    event,
                }),
            );
        }
        self.state
            .retain_accounts(units.iter().map(|(account, _)| account.id.as_str()));
        self.previous = units
            .into_iter()
            .map(|(account, _)| (account.id.clone(), account))
            .collect();
        out
    }
}

/// PRD 0054 RF2.2: an event with the identity of the transition behind it.
/// The identity is internal: it never enters the schema-1 payload.
#[derive(Debug, Clone)]
pub struct ObservedEvent {
    pub event: Event,
    pub transition: Transition,
}

impl ObservedEvent {
    /// PRD 0044 RF6.2: the `notification_sent` of one channel delivery,
    /// identified by the channel, the kind, the account and the time it
    /// was rendered, so the hook claims run it once.
    pub fn notification_sent(
        sent: &crate::channels::delivery::Sent,
        account: Option<&crate::Account>,
    ) -> Self {
        let event = Event::notification_sent(sent, account);
        let key = [
            event.event.name(),
            sent.channel.as_str(),
            sent.kind,
            event.account_id.as_str(),
        ]
        .join("\u{1e}");
        Self {
            transition: Transition::observed(key, sent.at),
            event,
        }
    }
}

/// What makes two observations the same transition, in this process or in
/// another one: the event kind, the account (and breakdown row), the metric
/// and threshold, and the instance: the window's reset time for an event
/// about a window (matched within the reset drift tolerance), otherwise the
/// time of the reading it came from (`Report.checked_at`, or the provider status'
/// reading time for an incident). Never `occurred_at`, so two processes
/// observing the same reading seconds apart agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    key: String,
    at: DateTime<Utc>,
    windowed: bool,
}

impl Transition {
    /// The transition of `event`, observed in `account`'s export.
    pub fn of(event: &Event, account: &AccountExport) -> Self {
        let key = [
            event.event.name().to_owned(),
            event.account_id.clone(),
            event.parent.clone().unwrap_or_default(),
            event
                .metric
                .as_ref()
                .map(|metric| format!("{}\u{1f}{:?}", metric.label, metric.window))
                .unwrap_or_default(),
            event
                .current
                .threshold
                .map(|threshold| threshold.to_string())
                .unwrap_or_default(),
        ]
        .join("\u{1e}");
        // PRD 0045 RF5.3: each breakdown row is a transition of its own.
        // An account's own events keep the key they had before rows.
        let key = match &event.row {
            Some(row) => format!(
                "{key}\u{1e}{}\u{1f}{}",
                crate::breakdown::scope_slug(row.scope),
                row.scope_id
            ),
            None => key,
        };
        let window = event
            .metric
            .as_ref()
            .and(event.current.resets_at.or_else(|| {
                event
                    .previous
                    .as_ref()
                    .and_then(|previous| previous.resets_at)
            }));
        if let Some(at) = window {
            return Self::windowed(key, at);
        }
        let reading = if event.event == EventKind::ProviderIncident {
            account
                .incident
                .as_ref()
                .and_then(|incident| incident.read_at)
        } else {
            account.report.as_ref().map(|report| report.checked_at)
        };
        Self::observed(key, reading.unwrap_or(event.occurred_at))
    }

    /// A transition identified by the reading taken at `at`.
    pub fn observed(key: impl Into<String>, at: DateTime<Utc>) -> Self {
        Self {
            key: key.into(),
            at,
            windowed: false,
        }
    }

    /// A transition of the window instance that resets at `at`.
    pub fn windowed(key: impl Into<String>, at: DateTime<Utc>) -> Self {
        Self {
            key: key.into(),
            at,
            windowed: true,
        }
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn at(&self) -> DateTime<Utc> {
        self.at
    }

    pub(crate) fn is_windowed(&self) -> bool {
        self.windowed
    }
}

/// PRD 0026 RF3.2: a front end's way to fill the event journal from the
/// exports of the cycle it already runs. Keeps its own baselines in memory
/// (the first export is the baseline and writes nothing); `usage_updated`
/// never reaches the journal and the history's duplicate rule makes two
/// processes on one profile safe.
#[derive(Debug)]
pub struct JournalTap {
    watcher: Watcher,
}

impl Default for JournalTap {
    fn default() -> Self {
        Self::new()
    }
}

impl JournalTap {
    pub fn new() -> Self {
        Self {
            watcher: Watcher::new(EventState::default(), None),
        }
    }

    /// Observe `export` and append its transitions to `store`'s journal.
    /// Call only while the history records. Returns the rows written.
    pub fn observe(
        &mut self,
        store: &crate::history::HistoryStore,
        export: &Export,
        now: DateTime<Utc>,
    ) -> Result<usize, crate::history::HistoryError> {
        self.observe_with(store, export, now, |_, _| None)
    }

    /// [`Self::observe`] with each account's pace input (PRD 0024 RF5), so
    /// `quota_exhaustion_projected` is journaled like the hooks see it.
    pub fn observe_with(
        &mut self,
        store: &crate::history::HistoryStore,
        export: &Export,
        now: DateTime<Utc>,
        pace: impl Fn(usize, &crate::Report) -> Option<(std::time::Duration, Vec<Vec<Point>>)>,
    ) -> Result<usize, crate::history::HistoryError> {
        store.journal_record(&self.watcher.observe_with(export, now, pace))
    }
}

/// PRD 0031 RF8.1: the keys, in configuration order, of the accounts that
/// read the same provider status as `of` (same provider and source, a
/// current reading), the first being the one the event names.
fn incident_accounts(export: &Export, of: &AccountExport) -> Vec<String> {
    let source = |account: &AccountExport| {
        account
            .incident
            .as_ref()
            .filter(|incident| !incident.stale)
            .map(|incident| (account.provider.slug(), incident.page_url.clone()))
    };
    let wanted = source(of);
    export
        .accounts
        .iter()
        .filter(|account| wanted.is_some() && source(account) == wanted)
        .map(|account| account.key.clone())
        .collect()
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
