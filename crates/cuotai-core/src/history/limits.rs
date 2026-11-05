//! PRD 0043 RF2.2: the limit changes recorded in the history, for the
//! 48-hour card note and the weekly digest. Network-free reads only; the
//! history keeps every sample's `limit`, so no extra storage is needed.

use super::{instant, series_digest, span, untag, HistoryError, HistoryStore};
use crate::insights::{is_limit_change, LimitChange};
use crate::{Account, MetricKind, MetricWindow};
use chrono::{DateTime, Utc};
use std::ops::RangeBounds;

impl HistoryStore {
    /// The limit changes of `account`'s provider metrics observed within
    /// `range`, oldest first: consecutive samples of one series whose
    /// finite positive limits differ beyond
    /// [`crate::insights::is_limit_change`]. Rate limits and the person's
    /// budget metrics (PRD 0041) never count. Empty when nothing was
    /// recorded; the file is never created by this read.
    pub fn limit_changes(
        &self,
        account: &Account,
        range: impl RangeBounds<DateTime<Utc>>,
    ) -> Result<Vec<LimitChange>, HistoryError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Vec::new());
        };
        let (start, end) = span(range);
        let account_digest = series_digest(&db, &self.config_key, account)?;
        let mut series_query = db.prepare(
            "SELECT id, label, unit, kind, \"window\" FROM series
             WHERE config_key = ?1 AND account_digest = ?2",
        )?;
        let series: Vec<(i64, String, String, MetricKind, Option<MetricWindow>)> = series_query
            .query_map(rusqlite::params![self.config_key, account_digest], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    untag(row.get(3)?)?,
                    row.get::<_, Option<String>>(4)?
                        .map(untag::<MetricWindow>)
                        .transpose()?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        // The sample just before the range, then the ones in it.
        let mut samples = db.prepare(
            "SELECT taken_at, \"limit\" FROM samples
             WHERE series_id = ?1 AND \"limit\" IS NOT NULL
               AND taken_at >= COALESCE((SELECT MAX(taken_at) FROM samples
                    WHERE series_id = ?1 AND \"limit\" IS NOT NULL AND taken_at < ?2), ?2)
               AND taken_at <= ?3
             ORDER BY taken_at",
        )?;
        let mut changes = Vec::new();
        for (id, label, unit, kind, window) in series {
            if kind == MetricKind::RateLimit || crate::budget::is_budget_label(account, &label) {
                continue;
            }
            let points: Vec<(DateTime<Utc>, f64)> = samples
                .query_map(rusqlite::params![id, start, end], |row| {
                    Ok((instant(row.get(0)?)?, row.get::<_, f64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .filter(|(_, limit)| limit.is_finite() && *limit > 0.0)
                .collect();
            for pair in points.windows(2) {
                let ((_, from), (at, to)) = (pair[0], pair[1]);
                if is_limit_change(from, to) {
                    changes.push(LimitChange {
                        label_text: crate::metric_text::metric_text(&label),
                        label: label.clone(),
                        window,
                        from,
                        to,
                        unit: unit.clone(),
                        observed_at: at,
                    });
                }
            }
        }
        changes.sort_by_key(|change| change.observed_at);
        Ok(changes)
    }
}
