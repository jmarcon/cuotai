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
use crate::events::{self, Event, EventKind, EventState};
use crate::export::v1::{AccountExport, Export};
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
            format!(
                "O intervalo do watch deve ficar entre {MIN_INTERVAL_SECONDS} e \
                 {MAX_REFRESH_SECONDS} segundos; valores menores são recusados, não ajustados."
            ),
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
                    format!("Evento desconhecido: \"{name}\". Eventos válidos: {known}."),
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

    /// The selected events of `export` after the export last observed
    /// ([`events::diff`] per account, in configuration order). The first
    /// reading of an account is its baseline; an account no longer in
    /// `export` is forgotten. Events not selected still move the state, so
    /// selecting never changes which transitions are real.
    pub fn observe(&mut self, export: &Export, now: DateTime<Utc>) -> Vec<Event> {
        let mut out = Vec::new();
        for next in &export.accounts {
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
                events::diff(prev, next, &mut self.state, now)
                    .into_iter()
                    .filter(|event| {
                        self.selected
                            .as_ref()
                            .is_none_or(|selected| selected.contains(&event.event))
                    }),
            );
        }
        self.previous = export
            .accounts
            .iter()
            .map(|account| (account.id.clone(), account.clone()))
            .collect();
        self.state
            .retain_accounts(export.accounts.iter().map(|account| account.id.as_str()));
        out
    }
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
