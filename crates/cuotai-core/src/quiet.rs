//! Quiet hours (PRD 0032 RF2, RF3): the person's do-not-disturb intervals
//! per weekday, the pure question "is a quiet window open at this instant
//! and when does it end" ([`window_at`]), the delivery decision for one
//! notice ([`decide`]) and the text of the digest sent when the window
//! ends ([`digest`]).
//!
//! Everything here is pure over its inputs, the injected clock and the
//! injected time zone: no `Local`, no `Utc::now`. Front ends pass
//! [`chrono::Local`] and the real time.

use crate::alerts::{AlertEvent, AlertKind};
use crate::model::Account;
use crate::resets::Notice;
use crate::text::{params, Localized};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde_json::{json, Map, Value};
use std::fmt::Display;

/// At most this many intervals per weekday (RF2.1).
pub const MAX_INTERVALS_PER_DAY: usize = 3;
/// The digest lists at most this many lines, then "and N more" (RF3.3).
pub const DIGEST_LINES: usize = 5;
/// Weekday keys of `desktop.json`, Monday first.
pub const DAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// One interval of a day, in the system's local wall-clock time. A `to`
/// at or before `from` crosses midnight and belongs to its start day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub from: NaiveTime,
    pub to: NaiveTime,
}

impl Interval {
    /// `HH:MM` texts (24-hour, zero padded) of `from` and `to`.
    pub fn parse(from: &str, to: &str) -> Option<Self> {
        Some(Self {
            from: parse_time(from)?,
            to: parse_time(to)?,
        })
    }

    pub fn from_text(&self) -> String {
        self.from.format("%H:%M").to_string()
    }

    pub fn to_text(&self) -> String {
        self.to.format("%H:%M").to_string()
    }
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    let (hours, minutes) = text.trim().split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(hours.parse().ok()?, minutes.parse().ok()?, 0)
}

/// The quiet hours section of `desktop.json` (`quiet`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuietSchedule {
    pub enabled: bool,
    /// Index 0 is Monday (see [`DAY_KEYS`]).
    pub days: [Vec<Interval>; 7],
    /// Send one summary when the window ends (on by default); off, the held
    /// notices are dropped.
    pub digest: bool,
}

impl Default for QuietSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            days: Default::default(),
            digest: true,
        }
    }
}

impl QuietSchedule {
    /// Each field on its own: a mistyped value is its default, an invalid
    /// interval is skipped and a day keeps at most
    /// [`MAX_INTERVALS_PER_DAY`] valid ones.
    pub fn from_value(value: &Value) -> Self {
        let default = Self::default();
        let mut days: [Vec<Interval>; 7] = Default::default();
        for (index, key) in DAY_KEYS.iter().enumerate() {
            if let Some(list) = value["days"][key].as_array() {
                days[index] = list
                    .iter()
                    .filter_map(|item| {
                        Interval::parse(item["from"].as_str()?, item["to"].as_str()?)
                    })
                    .take(MAX_INTERVALS_PER_DAY)
                    .collect();
            }
        }
        Self {
            enabled: value["enabled"].as_bool().unwrap_or(default.enabled),
            days,
            digest: value["digest"].as_bool().unwrap_or(default.digest),
        }
    }

    /// The strict reading of what the interface submits: `enabled` and
    /// `digest` must be booleans, `days` an object of known weekday keys
    /// with at most [`MAX_INTERVALS_PER_DAY`] `{from, to}` intervals in
    /// `HH:MM` each. Error: `invalid_quiet`.
    pub fn from_value_strict(value: &Value) -> Result<Self, &'static str> {
        let flag = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_bool)
                .ok_or("invalid_quiet")
        };
        let mut days: [Vec<Interval>; 7] = Default::default();
        if let Some(object) = value.get("days") {
            let object = object.as_object().ok_or("invalid_quiet")?;
            for (key, list) in object {
                let index = DAY_KEYS
                    .iter()
                    .position(|day| day == key)
                    .ok_or("invalid_quiet")?;
                let list = list.as_array().ok_or("invalid_quiet")?;
                if list.len() > MAX_INTERVALS_PER_DAY {
                    return Err("invalid_quiet");
                }
                days[index] = list
                    .iter()
                    .map(|item| {
                        Interval::parse(
                            item["from"].as_str().ok_or("invalid_quiet")?,
                            item["to"].as_str().ok_or("invalid_quiet")?,
                        )
                        .ok_or("invalid_quiet")
                    })
                    .collect::<Result<_, _>>()?;
            }
        }
        Ok(Self {
            enabled: flag("enabled")?,
            days,
            digest: flag("digest")?,
        })
    }

    /// The JSON form stored in `desktop.json`: only the days that have
    /// intervals.
    pub fn to_value(&self) -> Value {
        let mut days = Map::new();
        for (index, key) in DAY_KEYS.iter().enumerate() {
            if !self.days[index].is_empty() {
                let list: Vec<Value> = self.days[index]
                    .iter()
                    .map(|i| json!({ "from": i.from_text(), "to": i.to_text() }))
                    .collect();
                days.insert((*key).to_owned(), Value::Array(list));
            }
        }
        json!({ "enabled": self.enabled, "days": days, "digest": self.digest })
    }

    /// Whether any interval exists (an enabled schedule without one never
    /// holds anything).
    pub fn has_intervals(&self) -> bool {
        self.days.iter().any(|day| !day.is_empty())
    }
}

/// An open quiet window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietWindow {
    /// When the person's silence ends: the digest goes out then. Adjacent
    /// or overlapping intervals count as one window.
    pub ends_at: DateTime<Utc>,
}

#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

/// The instant `local` is in `tz`. A repeated hour takes the earlier
/// reading for a start and the later for an end (the window lengthens); a
/// skipped hour takes the first instant after the gap (the window
/// shortens).
fn resolve<Tz: TimeZone>(tz: &Tz, local: NaiveDateTime, edge: Edge) -> Option<DateTime<Utc>> {
    use chrono::LocalResult;
    let pick = |result: LocalResult<DateTime<Tz>>| match (result, edge) {
        (LocalResult::Single(at), _) => Some(at.with_timezone(&Utc)),
        (LocalResult::Ambiguous(first, second), Edge::Start) => {
            Some(first.min(second).with_timezone(&Utc))
        }
        (LocalResult::Ambiguous(first, second), Edge::End) => {
            Some(first.max(second).with_timezone(&Utc))
        }
        (LocalResult::None, _) => None,
    };
    pick(tz.from_local_datetime(&local)).or_else(|| {
        (1..=180)
            .find_map(|minutes| pick(tz.from_local_datetime(&(local + Duration::minutes(minutes)))))
    })
}

/// The window containing `at`, without merging neighbours.
fn containing<Tz: TimeZone>(
    schedule: &QuietSchedule,
    at: DateTime<Utc>,
    tz: &Tz,
) -> Option<DateTime<Utc>> {
    let today: NaiveDate = at.with_timezone(tz).date_naive();
    let mut best: Option<DateTime<Utc>> = None;
    for date in [today.pred_opt()?, today, today.succ_opt()?] {
        let day = date.weekday().num_days_from_monday() as usize;
        for interval in &schedule.days[day] {
            let end_date = if interval.to > interval.from {
                date
            } else {
                date.succ_opt()?
            };
            let (Some(start), Some(end)) = (
                resolve(tz, date.and_time(interval.from), Edge::Start),
                resolve(tz, end_date.and_time(interval.to), Edge::End),
            ) else {
                continue;
            };
            if start <= at && at < end && best.is_none_or(|current| end > current) {
                best = Some(end);
            }
        }
    }
    best
}

/// The quiet window open at `now` in `tz`, if any, with the instant it ends
/// (RF2.2). Intervals that touch or overlap are one window; a week with no
/// gap is capped at a week. Disabled or empty schedules have none.
pub fn window_at<Tz: TimeZone>(
    schedule: &QuietSchedule,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Option<QuietWindow> {
    if !schedule.enabled || !schedule.has_intervals() {
        return None;
    }
    let mut ends_at = containing(schedule, now, tz)?;
    // Merge the windows that start where this one ends.
    for _ in 0..(7 * MAX_INTERVALS_PER_DAY) {
        match containing(schedule, ends_at, tz) {
            Some(next) if next > ends_at => ends_at = next,
            _ => break,
        }
    }
    Some(QuietWindow { ends_at })
}

/// What a notice is, for the delivery decision.
#[derive(Debug, Clone, Copy)]
pub enum Subject<'a> {
    /// A usage alert (any [`AlertKind`]).
    Alert(&'a AlertEvent),
    /// A quota reset notice (PRD 0003) for the reset at `resets_at`.
    Reset { resets_at: DateTime<Utc> },
}

/// What to do with a notice that is due now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Deliver,
    /// Hold it until then (the digest).
    Defer(DateTime<Utc>),
    /// Never send it.
    Drop,
}

/// RF3.1: the fate of `subject` when `window` is the quiet window open now
/// (`None` outside quiet hours). `Exhausted` breaks through; thresholds,
/// recoveries, session expiries, overage and projected exhaustion wait for
/// the end of the window; a reset that happens inside the window is
/// dropped (the card already shows the new window), one that comes after
/// it waits for the window to end.
pub fn decide(subject: Subject<'_>, window: Option<QuietWindow>) -> Decision {
    let Some(window) = window else {
        return Decision::Deliver;
    };
    match subject {
        Subject::Alert(event) if event.kind == AlertKind::Exhausted => Decision::Deliver,
        Subject::Alert(_) => Decision::Defer(window.ends_at),
        Subject::Reset { resets_at } if resets_at < window.ends_at => Decision::Drop,
        Subject::Reset { .. } => Decision::Defer(window.ends_at),
    }
}

/// RF3.3: the one notification that lists the `groups` of alerts that still
/// hold when quiet hours end: at most [`DIGEST_LINES`] lines (one per
/// event, in account order) plus "and N more". With `private` on the lines
/// are the lock-screen-safe titles (provider, window and percentage); off,
/// they name the account and the metric. `None` without events.
pub fn digest<Tz>(
    groups: &[(&Account, Vec<AlertEvent>)],
    now: DateTime<Utc>,
    tz: &Tz,
    private: bool,
) -> Option<Notice>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let lines: Vec<Localized> = groups
        .iter()
        .flat_map(|(account, events)| events.iter().map(move |event| (*account, event)))
        .map(|(account, event)| {
            let single =
                crate::alerts::notice_with(&[(account, vec![event.clone()])], now, tz, private)?;
            if private {
                return Some(single.title);
            }
            Some(Localized::with_params(
                "quiet.digest.line",
                params([("title", single.title.into()), ("body", single.body.into())]),
                "{title}: {body}",
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let count = lines.len();
    if count == 0 {
        return None;
    }
    let mut shown = lines.into_iter();
    let first = shown.next()?;
    let mut body = first;
    for line in shown.by_ref().take(DIGEST_LINES - 1) {
        body = join(body, line);
    }
    let more = shown.count();
    if more > 0 {
        body = join(
            body,
            Localized::plural(
                "quiet.digest.more",
                params([("count", more.into())]),
                "and {count} more",
                "and {count} more",
            ),
        );
    }
    Some(Notice {
        title: Localized::plural(
            "quiet.digest.title",
            params([("count", count.into())]),
            "Cuotai: {count} usage alert held during quiet hours",
            "Cuotai: {count} usage alerts held during quiet hours",
        ),
        body,
    })
}

fn join(first: Localized, rest: Localized) -> Localized {
    Localized::with_params(
        "quiet.digest.join",
        params([("first", first.into()), ("rest", rest.into())]),
        "{first}\n{rest}",
    )
}

#[cfg(test)]
#[path = "quiet_tests.rs"]
mod tests;
