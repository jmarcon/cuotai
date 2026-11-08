//! PRD 0046 RF1/RF3: the history of one account as it travels in a backup,
//! and its merge into this computer's history.
//!
//! A backup carries, per account, the sample table of the PRD 0006 CSV
//! export (the journal stays behind). A restore merges it by series and
//! time: a sample already stored here for the same metric and instant wins,
//! a sample more than five minutes ahead of this clock is dropped (FIX-10),
//! and nothing else is changed. Retention and pruning apply afterwards as
//! for any other sample.

use std::ops::Bound;

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, TransactionBehavior};

use super::{
    future_sample_cutoff, millis, series_digest, series_id, ExportFormat, HistoryError,
    HistoryStore,
};
use crate::Account;

/// The columns of the sample table, as [`HistoryStore::export`] writes them.
const HEADER: [&str; 12] = [
    "account",
    "provider",
    "metric",
    "metric_key",
    "unit",
    "kind",
    "window",
    "taken_at",
    "used",
    "limit",
    "remaining",
    "resets_at",
];

/// The longest text field a row may carry (labels, units, tags).
const MAX_FIELD: usize = 256;

/// What a merge found in one account's CSV.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ImportCount {
    /// Valid sample rows in the file.
    pub rows: usize,
    /// Rows this computer did not have (written by an applied merge).
    pub new: usize,
}

/// One sample row of a CSV, validated.
#[derive(Debug, Clone, PartialEq)]
struct Sample {
    provider: String,
    account: String,
    label: String,
    metric_key: String,
    unit: String,
    kind: String,
    window: Option<String>,
    taken_at: i64,
    used: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    resets_at: Option<i64>,
}

/// Split CSV `text` into records (RFC 4180 quoting, `\n` or `\r\n`).
fn records(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\r') if chars.peek() == Some(&'\n') => {}
            (false, '\n') => {
                record.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut record));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push(record);
    }
    out
}

/// A text field as the export wrote it, without the `'` that keeps a
/// spreadsheet from reading it as a formula.
fn text_field(value: &str) -> Option<String> {
    let value = match value.strip_prefix('\'') {
        Some(rest) if rest.starts_with(['=', '+', '-', '@']) => rest,
        _ => value,
    };
    (value.chars().count() <= MAX_FIELD && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn time_field(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| millis(at.with_timezone(&Utc)))
}

fn number_field(value: &str) -> Result<Option<f64>, ()> {
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Some)
        .ok_or(())
}

/// The sample rows of `csv`: the header must be the export's; the table
/// ends at the first blank line (the journal that may follow is ignored).
/// A row that does not parse makes the whole file invalid.
fn parse(csv: &[u8]) -> Result<Vec<Sample>, HistoryError> {
    let text = std::str::from_utf8(csv).map_err(|_| HistoryError::Storage)?;
    let mut rows = records(text).into_iter();
    let header = rows.next().ok_or(HistoryError::Storage)?;
    if header != HEADER {
        return Err(HistoryError::Storage);
    }
    let mut samples = Vec::new();
    for row in rows {
        if row.len() == 1 && row[0].is_empty() {
            break;
        }
        let [account, provider, metric, metric_key, unit, kind, window, taken_at, used, limit, remaining, resets_at]: [String; 12] =
            row.try_into().map_err(|_| HistoryError::Storage)?;
        let tag_ok = |value: &str| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        };
        let sample = (|| {
            if !tag_ok(&provider)
                || !tag_ok(&kind)
                || !(window.is_empty() || tag_ok(&window))
                || !tag_ok(&metric_key)
            {
                return None;
            }
            Some(Sample {
                provider,
                account: text_field(&account)?,
                label: text_field(&metric)?,
                metric_key,
                unit: text_field(&unit)?,
                kind,
                window: (!window.is_empty()).then_some(window),
                taken_at: time_field(&taken_at)?,
                used: number_field(&used).ok()?,
                limit: number_field(&limit).ok()?,
                remaining: number_field(&remaining).ok()?,
                resets_at: if resets_at.is_empty() {
                    None
                } else {
                    Some(time_field(&resets_at)?)
                },
            })
        })()
        .ok_or(HistoryError::Storage)?;
        samples.push(sample);
    }
    Ok(samples)
}

/// The provider slug and account name of the first sample of `csv`, so a
/// restore can find the account when its id differs on this computer.
pub fn csv_account(csv: &[u8]) -> Option<(String, String)> {
    parse(csv)
        .ok()?
        .into_iter()
        .next()
        .map(|sample| (sample.provider, sample.account))
}

impl HistoryStore {
    /// The samples of `account` taken since `since`, as the CSV sample table
    /// of [`Self::export`] (no journal); `None` without any sample.
    pub fn export_account_csv(
        &self,
        account: &Account,
        since: DateTime<Utc>,
    ) -> Result<Option<Vec<u8>>, HistoryError> {
        let mut out = Vec::new();
        let rows = self.export(
            std::slice::from_ref(account),
            None,
            (Bound::Included(since), Bound::Unbounded),
            ExportFormat::Csv,
            &mut out,
        )?;
        if rows == 0 {
            return Ok(None);
        }
        // The journal follows the samples after a blank line.
        if let Some(end) = out.windows(2).position(|pair| pair == b"\n\n") {
            out.truncate(end + 1);
        }
        Ok(Some(out))
    }

    /// Merge the samples of `csv` into the history of `account`; with
    /// `apply` false nothing is written (and a missing file is not
    /// created), only counted. A sample already stored for the same metric
    /// and time wins; one more than five minutes ahead of the clock is
    /// dropped.
    pub fn import_csv(
        &self,
        account: &Account,
        csv: &[u8],
        apply: bool,
    ) -> Result<ImportCount, HistoryError> {
        self.import_csv_at(account, csv, apply, Utc::now())
    }

    fn import_csv_at(
        &self,
        account: &Account,
        csv: &[u8],
        apply: bool,
        now: DateTime<Utc>,
    ) -> Result<ImportCount, HistoryError> {
        let cutoff = future_sample_cutoff(now);
        let samples: Vec<Sample> = parse(csv)?
            .into_iter()
            .filter(|sample| sample.taken_at <= cutoff)
            .collect();
        let rows = samples.len();
        if !apply && !self.path.try_exists().map_err(|_| HistoryError::Storage)? {
            return Ok(ImportCount { rows, new: rows });
        }
        let mut db = self.open()?;
        let account_digest = series_digest(&db, &self.config_key, account)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut new = 0;
        {
            let mut insert_series = tx.prepare(
                "INSERT INTO series (config_key, account_digest, metric_key, label, position,
                    seen_at, unit, kind, \"window\")
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (config_key, account_digest, metric_key) DO NOTHING",
            )?;
            let mut insert_sample = tx.prepare(
                "INSERT OR IGNORE INTO samples (series_id, taken_at, used, \"limit\", remaining,
                    resets_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut positions: Vec<String> = Vec::new();
            for sample in &samples {
                let position = match positions.iter().position(|k| *k == sample.metric_key) {
                    Some(position) => position,
                    None => {
                        positions.push(sample.metric_key.clone());
                        positions.len() - 1
                    }
                };
                let seen_at = samples
                    .iter()
                    .filter(|other| other.metric_key == sample.metric_key)
                    .map(|other| other.taken_at)
                    .max()
                    .unwrap_or(sample.taken_at);
                let id =
                    match series_id(&tx, &self.config_key, &account_digest, &sample.metric_key)? {
                        Some(id) => id,
                        None => {
                            insert_series.execute(rusqlite::params![
                                self.config_key,
                                account_digest,
                                sample.metric_key,
                                sample.label,
                                i64::try_from(position).map_err(|_| HistoryError::Storage)?,
                                seen_at,
                                sample.unit,
                                sample.kind,
                                sample.window,
                            ])?;
                            series_id(&tx, &self.config_key, &account_digest, &sample.metric_key)?
                                .ok_or(HistoryError::Storage)?
                        }
                    };
                let exists = tx
                    .query_row(
                        "SELECT 1 FROM samples WHERE series_id = ?1 AND taken_at = ?2",
                        rusqlite::params![id, sample.taken_at],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if exists {
                    continue;
                }
                new += insert_sample.execute(rusqlite::params![
                    id,
                    sample.taken_at,
                    sample.used,
                    sample.limit,
                    sample.remaining,
                    sample.resets_at,
                ])?;
            }
        }
        if apply {
            tx.commit()?;
        }
        Ok(ImportCount { rows, new })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{config, first_account, t0, CONFIG};
    use super::*;
    use crate::{Metric, MetricKind, MetricWindow};
    use chrono::TimeDelta;

    fn store(dir: &std::path::Path, name: &str) -> HistoryStore {
        let identity = crate::config_file::ConfigIdentity::Explicit("transfer".into());
        HistoryStore::new(dir.join(name), &identity)
    }

    fn metric(used: f64) -> Metric {
        Metric {
            label: "=Requests, today".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "req".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Daily),
            period: None,
            resets_at: None,
            billing: Default::default(),
            scope: None,
            derived_from: None,
        }
    }

    #[test]
    fn an_account_history_round_trips_and_merges_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let account = first_account(config(CONFIG));
        let source = store(dir.path(), "a.sqlite3");
        let now = Utc::now();
        for (n, used) in [(3, 10.0), (2, 20.0), (1, 30.0)] {
            source
                .record(&account, now - TimeDelta::hours(n), &[metric(used)])
                .unwrap();
        }
        let csv = source
            .export_account_csv(&account, now - TimeDelta::days(90))
            .unwrap()
            .unwrap();
        let text = String::from_utf8(csv.clone()).unwrap();
        assert!(text.starts_with("account,provider,metric,"));
        assert!(!text.contains("occurred_at"), "the journal stays behind");
        assert_eq!(text.lines().count(), 4);
        assert_eq!(
            csv_account(&csv),
            Some(("groq".to_owned(), "alpha".to_owned()))
        );

        let target = store(dir.path(), "b.sqlite3");
        // A dry run on a fresh computer counts every row and creates nothing.
        assert_eq!(
            target.import_csv(&account, &csv, false).unwrap(),
            ImportCount { rows: 3, new: 3 }
        );
        assert!(!dir.path().join("b.sqlite3").exists());
        // The local sample at the same instant wins.
        target
            .record(&account, now - TimeDelta::hours(1), &[metric(99.0)])
            .unwrap();
        assert_eq!(
            target.import_csv(&account, &csv, true).unwrap(),
            ImportCount { rows: 3, new: 2 }
        );
        assert_eq!(
            target.import_csv(&account, &csv, true).unwrap(),
            ImportCount { rows: 3, new: 0 },
            "a second merge adds nothing"
        );
        let back = target
            .export_account_csv(&account, now - TimeDelta::days(90))
            .unwrap()
            .unwrap();
        let back = String::from_utf8(back).unwrap();
        assert_eq!(back.lines().count(), 4);
        assert!(back.contains(",99,100,"), "{back}");
        assert!(back.contains("'=Requests"), "the label keeps its guard");
    }

    #[test]
    fn invalid_files_and_future_rows_are_refused_or_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let account = first_account(config(CONFIG));
        let target = store(dir.path(), "c.sqlite3");
        for bad in [
            &b"not,a,history\n"[..],
            b"",
            b"account,provider,metric,metric_key,unit,kind,window,taken_at,used,limit,remaining,resets_at\nalpha,groq,x,abc,req,quota,,yesterday,1,,,\n",
            b"account,provider,metric,metric_key,unit,kind,window,taken_at,used,limit,remaining,resets_at\nalpha,gr oq,x,abc,req,quota,,2026-10-01T12:00:00+00:00,1,,,\n",
        ] {
            assert_eq!(
                target.import_csv(&account, bad, true),
                Err(HistoryError::Storage)
            );
        }
        let future = (t0() + TimeDelta::hours(1)).to_rfc3339();
        let past = (t0() - TimeDelta::hours(1)).to_rfc3339();
        let csv = format!(
            "account,provider,metric,metric_key,unit,kind,window,taken_at,used,limit,remaining,resets_at\n\
             alpha,groq,\"Tokens, month\",abc,tok,quota,monthly,{past},1,2,,\n\
             alpha,groq,\"Tokens, month\",abc,tok,quota,monthly,{future},3,,,\n\
             \noccurred_at,event\n"
        );
        assert_eq!(
            target
                .import_csv_at(&account, csv.as_bytes(), true, t0())
                .unwrap(),
            ImportCount { rows: 1, new: 1 }
        );
    }
}
