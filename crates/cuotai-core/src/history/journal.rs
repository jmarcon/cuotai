//! PRD 0026 RF3: the persisted event journal.
//!
//! Edge events of [`crate::events`] (never `usage_updated`) are kept in a
//! `journal` table of the history store, so "recent events" survives the
//! state that produced them. Privacy follows the history's digest model:
//! rows hold the digest of the account's stable id, the event name, the
//! metric label and numbers, and the failure *category*; never a message,
//! note, URL, e-mail or token. Retention and erasure follow the history.

use super::{instant, millis, tag, untag, HistoryError, HistoryStore};
use crate::events::{Event, EventKind, FailureCategory, MetricRef, Values};
use crate::incidents::IncidentState;
use crate::models::digest;
use crate::text::Localized;
use crate::{Account, Provider};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;
use std::ops::RangeBounds;

/// Schema step: the `journal` table. `minute` is `occurred_at` rounded down
/// to the minute; the unique index keeps two processes on one profile from
/// writing the same event twice.
pub(super) const MIGRATION: &str = "CREATE TABLE journal (
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    minute INTEGER NOT NULL,
    kind TEXT NOT NULL,
    metric_digest TEXT NOT NULL,
    metric_label TEXT,
    threshold INTEGER,
    used_ratio REAL,
    resets_at INTEGER,
    category TEXT,
    incident TEXT,
    projected_at INTEGER,
    spent REAL,
    currency TEXT
);
CREATE UNIQUE INDEX journal_unique
    ON journal (config_key, account_digest, kind, metric_digest, minute);
CREATE INDEX journal_by_time ON journal (config_key, occurred_at);
CREATE INDEX journal_by_age ON journal (occurred_at);";

/// Schema step 5: the aggregator's digest for a connection's event.
pub(super) const PARENT_MIGRATION: &str = "ALTER TABLE journal ADD COLUMN parent_digest TEXT;";

const MINUTE_MS: i64 = 60_000;

/// Digest of the account id alone: the journal's account identity, equal to
/// the series digest of the same account.
pub(super) fn account_digest(account_id: &str) -> String {
    digest(format!("cuotai.series.v2\u{1f}{account_id}").as_bytes())
}

/// The aggregator's stable id inside a connection's id
/// (`<aggregator id>/<connection>`, PRD 0036 RF5.1).
fn connection_parent(account_id: &str) -> &str {
    account_id.split('/').next().unwrap_or(account_id)
}

/// One journal line, ready to show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JournalEntry {
    pub occurred_at: DateTime<Utc>,
    pub kind: EventKind,
    /// The account's current name; `None` for an account no longer in the
    /// configuration (show "Removed account", [`removed_account`]).
    pub account: Option<String>,
    /// Index of the account in the `accounts` slice given to the query.
    pub account_index: Option<usize>,
    /// PRD 0036 RF7.3: for an event of an aggregator's connection, the
    /// aggregator account's current name (`None` when the aggregator left
    /// the configuration or the event is not a connection's).
    pub parent: Option<String>,
    /// Whether the event belongs to a connection of an aggregator, even when
    /// the aggregator is no longer configured.
    pub is_connection: bool,
    pub metric: Option<String>,
    pub threshold: Option<u8>,
    pub used_ratio: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub category: Option<FailureCategory>,
    pub projected_at: Option<DateTime<Utc>>,
    pub spent: Option<f64>,
    pub currency: Option<String>,
    /// [`Event::summary`], localized.
    pub summary: Localized,
}

/// "Removed account": what stands for an account that left the configuration.
pub fn removed_account() -> Localized {
    Localized::fixed("journal.removed_account", "Removed account")
}

fn failure_category(name: &str) -> Option<FailureCategory> {
    [
        FailureCategory::Timeout,
        FailureCategory::Offline,
        FailureCategory::AuthRequired,
        FailureCategory::RateLimited,
        FailureCategory::SchemaChanged,
        FailureCategory::InvalidOutput,
        FailureCategory::ProviderError,
    ]
    .into_iter()
    .find(|category| category.name() == name)
}

impl HistoryStore {
    /// RF3.2: append `events` to the journal, skipping `usage_updated`,
    /// sample events and any entry already written (same account, event,
    /// metric and minute). Returns how many rows were new. Creates the store
    /// when needed; the caller decides whether recording is on.
    pub fn journal_record(&self, events: &[Event]) -> Result<usize, HistoryError> {
        let events: Vec<&Event> = events
            .iter()
            .filter(|event| event.event != EventKind::UsageUpdated && !event.sample)
            .collect();
        if events.is_empty() {
            return Ok(0);
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut added = 0;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO journal (config_key, account_digest, occurred_at, minute,
                    kind, metric_digest, metric_label, threshold, used_ratio, resets_at, category,
                    incident, projected_at, spent, currency, parent_digest)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL, NULL, ?14)",
            )?;
            for event in events {
                let at = millis(event.occurred_at);
                let label = event.metric.as_ref().map(|metric| metric.label.as_str());
                let metric_digest = event.metric.as_ref().map_or_else(String::new, |metric| {
                    digest(format!("{}\u{1f}{:?}", metric.label, metric.window).as_bytes())
                });
                let values = &event.current;
                added += insert.execute(rusqlite::params![
                    self.config_key,
                    account_digest(&event.account_id),
                    at,
                    at.div_euclid(MINUTE_MS),
                    event.event.name(),
                    metric_digest,
                    label,
                    values.threshold.map(i64::from),
                    values.used_ratio.filter(|ratio| ratio.is_finite()),
                    values.resets_at.map(millis),
                    values.category.map(FailureCategory::name),
                    values.incident.map(tag),
                    values.projected_at.map(millis),
                    event
                        .parent
                        .as_ref()
                        .map(|_| account_digest(connection_parent(&event.account_id))),
                ])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// RF3.4: the newest `limit` entries of this configuration that
    /// occurred within `range`, newest first. `accounts` resolves digests to
    /// current names; entries of accounts not in it carry `account: None`.
    /// A store that does not exist yet has an empty journal.
    pub fn journal(
        &self,
        range: impl RangeBounds<DateTime<Utc>>,
        accounts: &[Account],
        limit: usize,
    ) -> Result<Vec<JournalEntry>, HistoryError> {
        let (start, end) = super::span(range);
        self.journal_between(start, end, accounts, limit)
    }

    /// [`Self::journal`] over millisecond bounds.
    pub(super) fn journal_between(
        &self,
        start: i64,
        end: i64,
        accounts: &[Account],
        limit: usize,
    ) -> Result<Vec<JournalEntry>, HistoryError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Vec::new());
        };
        let mut query = db.prepare(
            "SELECT account_digest, occurred_at, kind, metric_label, threshold, used_ratio,
                    resets_at, category, incident, projected_at, spent, currency, parent_digest
             FROM journal WHERE config_key = ?1 AND occurred_at BETWEEN ?2 AND ?3
             ORDER BY occurred_at DESC, rowid DESC LIMIT ?4",
        )?;
        let digests: Vec<String> = accounts
            .iter()
            .map(|account| account_digest(&account.state_key()))
            .collect();
        let mut rows = query.query(rusqlite::params![
            self.config_key,
            start,
            end,
            i64::try_from(limit).unwrap_or(i64::MAX)
        ])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let Some(kind) = EventKind::from_name(&row.get::<_, String>(2)?) else {
                continue; // an event of a newer release: not shown, not guessed
            };
            let who: String = row.get(0)?;
            let account_index = digests.iter().position(|known| *known == who);
            let parent_digest: Option<String> = row.get(12)?;
            let parent_index = parent_digest
                .as_deref()
                .and_then(|wanted| digests.iter().position(|known| known == wanted));
            let category = row
                .get::<_, Option<String>>(7)?
                .and_then(|name| failure_category(&name));
            let incident = row
                .get::<_, Option<String>>(8)?
                .map(untag::<IncidentState>)
                .transpose()?;
            let metric_label: Option<String> = row.get(3)?;
            let occurred_at = instant(row.get(1)?)?;
            let current = Values {
                threshold: row
                    .get::<_, Option<i64>>(4)?
                    .and_then(|value| u8::try_from(value).ok()),
                used_ratio: row.get(5)?,
                resets_at: row.get::<_, Option<i64>>(6)?.map(instant).transpose()?,
                category,
                incident,
                projected_at: row.get::<_, Option<i64>>(9)?.map(instant).transpose()?,
                ..Values::default()
            };
            let event = Event {
                parent: Default::default(),
                schema: crate::events::SCHEMA,
                event: kind,
                occurred_at,
                account_key: String::new(),
                account_id: String::new(),
                account_name: String::new(),
                provider: Provider::ALL[0],
                accounts: Vec::new(),
                metric: metric_label
                    .clone()
                    .map(|label| MetricRef::new(label, None)),
                current: current.clone(),
                previous: None,
                sample: false,
            };
            entries.push(JournalEntry {
                occurred_at,
                kind,
                account: account_index.map(|index| accounts[index].name.clone()),
                account_index,
                parent: parent_index.map(|index| accounts[index].name.clone()),
                is_connection: parent_digest.is_some(),
                metric: metric_label,
                threshold: current.threshold,
                used_ratio: current.used_ratio,
                resets_at: current.resets_at,
                category,
                projected_at: current.projected_at,
                spent: row.get(10)?,
                currency: row.get(11)?,
                summary: event.summary(),
            });
        }
        Ok(entries)
    }
}

/// RF3.3: remove the journal rows older than `before`.
pub(super) fn prune(db: &Connection, before: i64) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM journal WHERE occurred_at < ?1", [before])
}

/// RF3.3: remove the journal rows of one account.
pub(super) fn erase_account(
    db: &Connection,
    config_key: &str,
    account_id: &str,
) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM journal WHERE config_key = ?1
           AND (account_digest = ?2 OR parent_digest = ?2)",
        rusqlite::params![config_key, account_digest(account_id)],
    )
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
