//! PRD 0043 RF3 — the opt-in weekly digest.
//!
//! [`weekly`] summarizes one week from the readings recorded on this
//! computer (the history and the latest readings the caller holds): limit
//! hits, average peak, days at the limit, extra spent, limit changes, the
//! heaviest and lightest day, the busiest hours, the person's budgets
//! (PRD 0041) and, for the coming week, the allowance of each quota with a
//! known reset. It never queries a provider and is computed only when due
//! or asked. [`Digest::short_lines`] is the notification text (at most 8
//! lines), [`Digest::markdown`] the full text.
//!
//! [`due_slot`] says which weekly slot is due; the shared ledger
//! ([`crate::resets::Ledger::claim_weekly_digest`]) makes one process
//! deliver it once across every process, and a slot missed while the app
//! was closed is delivered within [`CATCH_UP`] of it, then dropped.

use crate::history::{HistoryError, HistoryStore};
use crate::insights::{DaysOff, DigestPrefs, LimitChange, RhythmQuery, RhythmRange};
use crate::model::{Account, Report};
use crate::stats::{ExtraSpent, StatsRange};
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Datelike, NaiveDate, TimeDelta, TimeZone, Utc};
use serde::Serialize;

/// RF3.2: a slot missed while no process ran is still delivered this long
/// after it, then dropped.
pub const CATCH_UP: TimeDelta = TimeDelta::hours(48);
/// RF3.1: the notification text has at most this many lines.
pub const SHORT_LINES: usize = 8;

/// RF3.2: which week a digest covers (`--week last|current`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestWeek {
    /// The last full week, Monday to Sunday.
    Last,
    /// This week so far, from Monday 00:00 to now.
    Current,
}

impl DigestWeek {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "last" => Some(Self::Last),
            "current" => Some(Self::Current),
            _ => None,
        }
    }
}

/// One budget's state at the end of the week (PRD 0041).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetLine {
    pub label: String,
    pub label_text: Localized,
    pub used: Option<f64>,
    pub limit: f64,
    pub unit: String,
}

/// One quota's allowance for the coming week (PRD 0041 RF4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AllowanceLine {
    pub label: String,
    pub label_text: Localized,
    pub sentence: Localized,
}

/// A day and its peak share (`0.0..=1.0`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DayShare {
    pub day: NaiveDate,
    pub share: f64,
}

/// RF3.1: one account's week.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestAccount {
    pub name: String,
    pub provider: crate::Provider,
    pub hits: u32,
    pub average_peak: Option<f64>,
    pub days_at_limit: u32,
    pub heaviest: Option<DayShare>,
    pub lightest: Option<DayShare>,
    /// The local hours `[from, to)` with the most consumption.
    pub busiest_hours: Option<(u8, u8)>,
    pub limit_changes: Vec<LimitChange>,
    pub budgets: Vec<BudgetLine>,
    pub allowances: Vec<AllowanceLine>,
}

/// RF3.1: the week of every account and overall.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Digest {
    pub week: DigestWeek,
    /// First local day of the week and the last one covered.
    pub first_day: NaiveDate,
    pub last_day: NaiveDate,
    pub hits: u32,
    pub average_peak: Option<f64>,
    pub extra_spent: Option<Vec<ExtraSpent>>,
    pub heaviest: Option<DayShare>,
    pub lightest: Option<DayShare>,
    pub busiest_hours: Option<(u8, u8)>,
    pub accounts: Vec<DigestAccount>,
    /// Whether any reading was recorded in the week (else the digest is
    /// empty and the CLI exits 2).
    pub recorded: bool,
}

/// Monday of the week holding `date`.
fn monday(date: NaiveDate) -> NaiveDate {
    date - TimeDelta::days(i64::from(date.weekday().num_days_from_monday()))
}

fn extremes(days: &[crate::stats::DayPeak]) -> (Option<DayShare>, Option<DayShare>) {
    let known = || {
        days.iter().filter_map(|day| {
            day.share.map(|share| DayShare {
                day: day.day,
                share,
            })
        })
    };
    (
        known().max_by(|a, b| a.share.total_cmp(&b.share)),
        known().min_by(|a, b| a.share.total_cmp(&b.share)),
    )
}

/// The 3-hour band with the most consumption over the hours of a day.
fn busiest(values: &[f64; 24]) -> Option<(u8, u8)> {
    if values.iter().all(|v| *v <= 0.0) {
        return None;
    }
    let (start, _) = (0..24)
        .map(|h| (h, (0..3).map(|k| values[(h + k) % 24]).sum::<f64>()))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some((start as u8, ((start + 3) % 24) as u8))
}

/// RF3.1: the digest of `week` at `now` in `tz` for `accounts`, with
/// `latest` the latest reading of each account (budgets and allowances;
/// `None` where there is none). Reads the history only.
pub fn weekly<Tz: TimeZone>(
    store: &HistoryStore,
    accounts: &[Account],
    latest: &[Option<Report>],
    week: DigestWeek,
    now: DateTime<Utc>,
    tz: &Tz,
    days_off: DaysOff,
) -> Result<Digest, HistoryError> {
    let today = now.with_timezone(tz).date_naive();
    let this_monday = monday(today);
    let (first_day, last_day, end) = match week {
        DigestWeek::Last => {
            let end = crate::budget::local_midnight(this_monday, tz).unwrap_or(now);
            (
                this_monday - TimeDelta::days(7),
                this_monday - TimeDelta::days(1),
                end - TimeDelta::milliseconds(1),
            )
        }
        DigestWeek::Current => (this_monday, today, now),
    };
    let start = crate::budget::local_midnight(first_day, tz).unwrap_or(end);
    // PRD 0026 statistics over the 7 days ending at `end`, trimmed to the week.
    let summary = crate::stats::summarize(store, accounts, StatsRange::Days7, end, tz)?;
    let in_week = |day: &crate::stats::DayPeak| day.day >= first_day && day.day <= last_day;
    let mut out_accounts = Vec::new();
    let mut recorded = false;
    for (index, (account, stats)) in accounts.iter().zip(&summary.accounts).enumerate() {
        let days: Vec<crate::stats::DayPeak> =
            stats.days.iter().filter(|d| in_week(d)).copied().collect();
        recorded |= days.iter().any(|d| d.share.is_some());
        let (heaviest, lightest) = extremes(&days);
        let query = RhythmQuery {
            scope: Some(index),
            metric: None,
            range: RhythmRange::Typical30d,
            days_off,
        };
        let busiest_hours = weekly_hours(store, accounts, &query, start, end, tz)?;
        let limit_changes = store.limit_changes(account, start..=end)?;
        let reading = latest.get(index).and_then(Option::as_ref);
        let budgets = reading
            .map(|report| {
                report
                    .metrics
                    .iter()
                    .filter(|metric| metric.derived_from.is_some())
                    .filter_map(|metric| {
                        Some(BudgetLine {
                            label: metric.label.clone(),
                            label_text: crate::metric_text::metric_text(&metric.label),
                            used: metric.used,
                            limit: metric.limit?,
                            unit: metric.unit.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let allowances = reading
            .map(|report| {
                report
                    .metrics
                    .iter()
                    .filter_map(|metric| {
                        let allowance = crate::pace::allowance(metric, now, None)?;
                        Some(AllowanceLine {
                            label: metric.label.clone(),
                            label_text: crate::metric_text::metric_text(&metric.label),
                            sentence: allowance.sentence(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out_accounts.push(DigestAccount {
            name: account.display_name().into_owned(),
            provider: account.provider,
            hits: stats.hits,
            average_peak: mean(days.iter().filter_map(|d| d.share)),
            days_at_limit: days
                .iter()
                .filter(|d| d.share.is_some_and(|s| s >= 0.999))
                .count() as u32,
            heaviest,
            lightest,
            busiest_hours,
            limit_changes,
            budgets,
            allowances,
        });
    }
    let overall: Vec<crate::stats::DayPeak> = summary
        .daily
        .iter()
        .filter(|d| in_week(d))
        .copied()
        .collect();
    let (heaviest, lightest) = extremes(&overall);
    let query = RhythmQuery {
        scope: None,
        metric: None,
        range: RhythmRange::Typical30d,
        days_off,
    };
    Ok(Digest {
        week,
        first_day,
        last_day,
        hits: out_accounts.iter().map(|a| a.hits).sum(),
        average_peak: mean(out_accounts.iter().filter_map(|a| a.average_peak)),
        extra_spent: summary.extra_spent.clone(),
        heaviest,
        lightest,
        busiest_hours: weekly_hours(store, accounts, &query, start, end, tz)?,
        accounts: out_accounts,
        recorded,
    })
}

/// The busiest 3-hour band of the consumption between `start` and `end`
/// (the hours of day summed over the week).
fn weekly_hours<Tz: TimeZone>(
    store: &HistoryStore,
    accounts: &[Account],
    query: &RhythmQuery<'_>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    tz: &Tz,
) -> Result<Option<(u8, u8)>, HistoryError> {
    let hours = crate::insights::hour_totals(store, accounts, query.scope, start, end, tz)?;
    Ok(busiest(&hours))
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0u32), |(sum, n), v| (sum + v, n + 1));
    (count > 0).then(|| sum / f64::from(count))
}

fn percent(share: f64) -> ParamValue {
    ParamValue::Text(format!("{:.0}", (share * 100.0).round()))
}

fn day_text(day: NaiveDate, locale: &str) -> String {
    let name = crate::budget::weekday_text(crate::insights::weekday_name(day.weekday()));
    format!("{} {}", name.resolve(locale), day.format("%Y-%m-%d"))
}

fn amounts(extra: &[ExtraSpent]) -> String {
    extra
        .iter()
        .map(|e| {
            let currency = crate::budget::is_currency(&e.unit).then_some(e.unit.as_str());
            match crate::overage::format_amount(e.amount, &e.unit, currency) {
                ParamValue::Text(text) => text,
                ParamValue::L10n(text) => text.fallback.into_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl Digest {
    /// "Your week in Cuotai: 2026-10-19 – 2026-10-25".
    pub fn title(&self) -> Localized {
        Localized::with_params(
            "digest.title",
            params([
                (
                    "from",
                    ParamValue::Text(self.first_day.format("%Y-%m-%d").to_string()),
                ),
                (
                    "to",
                    ParamValue::Text(self.last_day.format("%Y-%m-%d").to_string()),
                ),
            ]),
            "Your week in Cuotai: {from} – {to}",
        )
    }

    /// The lines both texts share, overall, in `locale`.
    fn overall_lines(&self, locale: &str) -> Vec<Localized> {
        let mut lines = vec![Localized::plural(
            "digest.line.hits",
            params([("count", (self.hits as usize).into())]),
            "{count} limit hit",
            "{count} limit hits",
        )];
        if let Some(peak) = self.average_peak {
            lines.push(Localized::with_param(
                "digest.line.peak",
                "percent",
                percent(peak),
                "Average peak: {percent}%",
            ));
        }
        if let Some(extra) = self.extra_spent.as_deref().filter(|e| !e.is_empty()) {
            lines.push(Localized::with_param(
                "digest.line.extra",
                "amounts",
                amounts(extra),
                "Extra spent: {amounts}",
            ));
        }
        if let (Some(heavy), Some(light)) = (self.heaviest, self.lightest) {
            lines.push(Localized::with_params(
                "digest.line.days",
                params([
                    ("heavy", ParamValue::Text(day_text(heavy.day, locale))),
                    ("heavy_percent", percent(heavy.share)),
                    ("light", ParamValue::Text(day_text(light.day, locale))),
                    ("light_percent", percent(light.share)),
                ]),
                "Heaviest day: {heavy} ({heavy_percent}%) · lightest: {light} ({light_percent}%)",
            ));
        }
        if let Some((from, to)) = self.busiest_hours {
            lines.push(Localized::with_params(
                "digest.line.hours",
                params([
                    ("from", ParamValue::Text(format!("{from:02}:00"))),
                    ("to", ParamValue::Text(format!("{to:02}:00"))),
                ]),
                "Busiest hours: {from}–{to}",
            ));
        }
        let changes: usize = self.accounts.iter().map(|a| a.limit_changes.len()).sum();
        if changes > 0 {
            lines.push(Localized::plural(
                "digest.line.limit_changes",
                params([("count", changes.into())]),
                "{count} limit change",
                "{count} limit changes",
            ));
        }
        let budgets: Vec<&BudgetLine> = self.accounts.iter().flat_map(|a| &a.budgets).collect();
        if !budgets.is_empty() {
            let over = budgets
                .iter()
                .filter(|b| b.used.is_some_and(|used| used >= b.limit))
                .count();
            lines.push(Localized::with_params(
                "digest.line.budgets",
                params([("count", budgets.len().into()), ("over", over.into())]),
                "Budgets: {over} of {count} used up",
            ));
        }
        lines
    }

    /// The source line every text ends with (RNF honesty).
    pub fn source_line() -> Localized {
        Localized::fixed(
            "digest.line.source",
            "From the readings recorded on this computer.",
        )
    }

    /// The text when nothing was recorded in the week.
    pub fn empty_line() -> Localized {
        Localized::fixed(
            "digest.line.empty",
            "No readings were recorded on this computer this week.",
        )
    }

    /// RF3.1: the notification text: at most [`SHORT_LINES`] lines, no
    /// account names (PRD 0032 private mode).
    pub fn short_lines(&self, locale: &str) -> Vec<Localized> {
        if !self.recorded {
            return vec![Self::empty_line()];
        }
        let mut lines = self.overall_lines(locale);
        lines.truncate(SHORT_LINES - 1);
        lines.push(Self::source_line());
        lines
    }

    /// RF3.1: the full Markdown text in `locale`, with account names (it
    /// stays local unless the person sends it somewhere they own).
    pub fn markdown(&self, locale: &str) -> String {
        let mut out = format!("# {}\n\n", self.title().resolve(locale));
        if !self.recorded {
            out.push_str(&format!("{}\n", Self::empty_line().resolve(locale)));
            return out;
        }
        for line in self.overall_lines(locale) {
            out.push_str(&format!("- {}\n", line.resolve(locale)));
        }
        for account in &self.accounts {
            out.push_str(&format!(
                "\n## {} · {}\n\n",
                account.name,
                crate::text::provider_l10n(account.provider).resolve(locale)
            ));
            let one = Digest {
                hits: account.hits,
                average_peak: account.average_peak,
                extra_spent: None,
                heaviest: account.heaviest,
                lightest: account.lightest,
                busiest_hours: account.busiest_hours,
                accounts: vec![DigestAccount {
                    budgets: Vec::new(),
                    ..account.clone()
                }],
                ..self.clone()
            };
            for line in one.overall_lines(locale) {
                out.push_str(&format!("- {}\n", line.resolve(locale)));
            }
            if account.days_at_limit > 0 {
                out.push_str(&format!(
                    "- {}\n",
                    Localized::plural(
                        "digest.line.days_at_limit",
                        params([("count", (account.days_at_limit as usize).into())]),
                        "{count} day at the limit",
                        "{count} days at the limit",
                    )
                    .resolve(locale)
                ));
            }
            for change in &account.limit_changes {
                let note = change.note(|at| at.format("%Y-%m-%d").to_string());
                out.push_str(&format!("- {}\n", note.resolve(locale)));
            }
            for budget in &account.budgets {
                let used = budget
                    .used
                    .map_or_else(|| "?".to_owned(), crate::insights::plain);
                out.push_str(&format!(
                    "- {}\n",
                    Localized::with_params(
                        "digest.line.budget",
                        params([
                            ("budget", ParamValue::l10n(budget.label_text.clone())),
                            ("used", ParamValue::Text(used)),
                            (
                                "limit",
                                ParamValue::Text(crate::insights::plain(budget.limit))
                            ),
                            (
                                "unit",
                                ParamValue::l10n(crate::metric_text::metric_text(&budget.unit))
                            ),
                        ]),
                        "{budget}: {used} of {limit} {unit}",
                    )
                    .resolve(locale)
                ));
            }
            for allowance in &account.allowances {
                out.push_str(&format!(
                    "- {}: {}\n",
                    allowance.label_text.resolve(locale),
                    allowance.sentence.resolve(locale)
                ));
            }
        }
        out.push_str(&format!("\n_{}_\n", Self::source_line().resolve(locale)));
        out
    }
}

/// RF3.2: the latest weekly slot (`prefs.weekday` at `prefs.hour`, local)
/// at or before `now`, when the digest is on and that slot is at most
/// [`CATCH_UP`] old; `None` otherwise.
pub fn due_slot<Tz: TimeZone>(
    prefs: &DigestPrefs,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Option<DateTime<Utc>> {
    if !prefs.enabled {
        return None;
    }
    let today = now.with_timezone(tz).date_naive();
    let monday = monday(today);
    let slot_at = |week_monday: NaiveDate| {
        let date = week_monday + TimeDelta::days(i64::from(prefs.weekday.min(6)));
        crate::budget::local_midnight(date, tz)
            .map(|midnight| midnight + TimeDelta::hours(i64::from(prefs.hour.min(23))))
    };
    let this_week = slot_at(monday)?;
    let slot = if this_week <= now {
        this_week
    } else {
        slot_at(monday - TimeDelta::days(7))?
    };
    (now - slot <= CATCH_UP).then_some(slot)
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
