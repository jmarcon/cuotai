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
use crate::text::{params, Localized};
use chrono::{
    DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
};
use serde::Serialize;
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
    // Exactly two ASCII digits on each side: `u32::from_str` alone would
    // take a sign ("+1:00") (PRD 0032 re-validation C-12).
    let two_digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(hours) || !two_digits(minutes) {
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

/// Why [`QuietSchedule::from_value_strict`] refused a schedule (PRD 0032
/// RF2.3). The interface names the field from `day`, `index` and `field`
/// and words the reason from `code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuietErrorCode {
    /// A flag that is not a boolean, an unknown weekday, a list that is not
    /// one or holds more than [`MAX_INTERVALS_PER_DAY`] intervals.
    #[serde(rename = "invalid_quiet")]
    Invalid,
    /// A time that is not `HH:MM` (`field` names which).
    #[serde(rename = "invalid_quiet_time")]
    Time,
    /// The same `from` and `to` as an earlier interval of the day (`with`).
    #[serde(rename = "invalid_quiet_duplicate")]
    Duplicate,
    /// Overlaps an earlier interval of the same start day (`with`).
    /// Intervals of different days that run into each other are accepted
    /// and count as one window.
    #[serde(rename = "invalid_quiet_overlap")]
    Overlap,
    /// `from` equals `to` (zero or 24 hours?), except `00:00`–`00:00`,
    /// which is the whole day.
    #[serde(rename = "invalid_quiet_empty")]
    Empty,
}

/// A refused schedule: the reason and, when it is one interval's, where.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct QuietError {
    pub code: QuietErrorCode,
    /// The weekday key ([`DAY_KEYS`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<&'static str>,
    /// The interval's position in its day, from 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// `from` or `to`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<&'static str>,
    /// For a duplicate or an overlap: the earlier interval of the same day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<usize>,
}

impl QuietError {
    const fn invalid() -> Self {
        Self {
            code: QuietErrorCode::Invalid,
            day: None,
            index: None,
            field: None,
            with: None,
        }
    }

    const fn at(
        code: QuietErrorCode,
        day: &'static str,
        index: usize,
        field: &'static str,
    ) -> Self {
        Self {
            code,
            day: Some(day),
            index: Some(index),
            field: Some(field),
            with: None,
        }
    }

    /// The JSON object the desktop command returns as its error.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "invalid_quiet".to_owned())
    }
}

impl Display for QuietError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_json())
    }
}

impl std::error::Error for QuietError {}

const DAY_MINUTES: u32 = 24 * 60;

impl Interval {
    /// Minutes from its start day's midnight to its start and its end: an
    /// end at or before the start is on the next day, so `from == to` reads
    /// as 24 hours, as [`window_at`] reads it.
    fn span(&self) -> (u32, u32) {
        let minutes = |t: NaiveTime| t.num_seconds_from_midnight() / 60;
        let (from, to) = (minutes(self.from), minutes(self.to));
        (from, if to > from { to } else { to + DAY_MINUTES })
    }

    /// `00:00`–`00:00`: the whole day.
    pub fn is_all_day(&self) -> bool {
        self.from == NaiveTime::MIN && self.to == NaiveTime::MIN
    }
}

/// Why the interval at `index` of a day cannot join the ones before it, in
/// the strict reading's order: equal ends, a duplicate, an overlap.
fn day_conflict(
    list: &[Interval],
    index: usize,
) -> Option<(QuietErrorCode, &'static str, Option<usize>)> {
    let interval = list[index];
    if interval.from == interval.to && !interval.is_all_day() {
        return Some((QuietErrorCode::Empty, "to", None));
    }
    let (start, end) = interval.span();
    list[..index]
        .iter()
        .enumerate()
        .find_map(|(other, earlier)| {
            if *earlier == interval {
                return Some((QuietErrorCode::Duplicate, "from", Some(other)));
            }
            let (s, e) = earlier.span();
            (start < e && s < end).then_some((QuietErrorCode::Overlap, "from", Some(other)))
        })
}

impl QuietSchedule {
    /// Each field on its own: a mistyped value is its default, an invalid
    /// interval is skipped, a later exact duplicate of a day is dropped and
    /// a day keeps at most [`MAX_INTERVALS_PER_DAY`] valid ones. Overlaps
    /// and equal ends are kept (an older file keeps its behaviour:
    /// [`window_at`] merges overlaps and reads `from == to` as 24 hours).
    pub fn from_value(value: &Value) -> Self {
        let default = Self::default();
        let mut days: [Vec<Interval>; 7] = Default::default();
        for (index, key) in DAY_KEYS.iter().enumerate() {
            if let Some(list) = value["days"][key].as_array() {
                let mut kept: Vec<Interval> = Vec::new();
                for interval in list.iter().filter_map(|item| {
                    Interval::parse(item["from"].as_str()?, item["to"].as_str()?)
                }) {
                    if kept.len() == MAX_INTERVALS_PER_DAY {
                        break;
                    }
                    if !kept.contains(&interval) {
                        kept.push(interval);
                    }
                }
                days[index] = kept;
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
    /// `HH:MM` each. Within a day, an interval may not repeat or overlap an
    /// earlier one, nor start where it ends (unless it is `00:00`–`00:00`,
    /// the whole day); intervals of different days may run into each other.
    pub fn from_value_strict(value: &Value) -> Result<Self, QuietError> {
        let flag = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_bool)
                .ok_or(QuietError::invalid())
        };
        let mut days: [Vec<Interval>; 7] = Default::default();
        if let Some(object) = value.get("days") {
            let object = object.as_object().ok_or(QuietError::invalid())?;
            for (key, list) in object {
                let index = DAY_KEYS
                    .iter()
                    .position(|day| day == key)
                    .ok_or(QuietError::invalid())?;
                let day = DAY_KEYS[index];
                let list = list.as_array().ok_or(QuietError {
                    day: Some(day),
                    ..QuietError::invalid()
                })?;
                if list.len() > MAX_INTERVALS_PER_DAY {
                    return Err(QuietError {
                        day: Some(day),
                        ..QuietError::invalid()
                    });
                }
                let mut parsed = Vec::with_capacity(list.len());
                for (position, item) in list.iter().enumerate() {
                    let time = |field: &'static str| {
                        item[field]
                            .as_str()
                            .and_then(parse_time)
                            .ok_or(QuietError::at(QuietErrorCode::Time, day, position, field))
                    };
                    parsed.push(Interval {
                        from: time("from")?,
                        to: time("to")?,
                    });
                    if let Some((code, field, with)) = day_conflict(&parsed, position) {
                        return Err(QuietError {
                            with,
                            ..QuietError::at(code, day, position, field)
                        });
                    }
                }
                days[index] = parsed;
            }
        }
        Ok(Self {
            enabled: flag("enabled")?,
            days,
            digest: flag("digest")?,
        })
    }

    /// Whether the intervals leave no gap in the week, with the ones that
    /// cross midnight running into the next day (Sunday's into Monday's).
    /// Such a schedule, once enabled, holds everything but exhaustion and
    /// never ends, so no digest is sent (PRD 0032 re-validation C-12).
    /// `enabled` is not looked at.
    pub fn always_on(&self) -> bool {
        const WEEK: u32 = 7 * DAY_MINUTES;
        let mut spans: Vec<(u32, u32)> = Vec::new();
        for (day, list) in (0u32..).zip(&self.days) {
            for interval in list {
                let (start, end) = interval.span();
                let (start, end) = (day * DAY_MINUTES + start, day * DAY_MINUTES + end);
                if end > WEEK {
                    spans.push((start, WEEK));
                    spans.push((0, end - WEEK));
                } else {
                    spans.push((start, end));
                }
            }
        }
        spans.sort_unstable();
        let mut covered = 0;
        for (start, end) in spans {
            if start > covered {
                return false;
            }
            covered = covered.max(end);
        }
        covered >= WEEK
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
    let cap = now + chrono::TimeDelta::days(7);
    // Merge the windows that start where this one ends, up to a week.
    for _ in 0..(7 * MAX_INTERVALS_PER_DAY) {
        match containing(schedule, ends_at, tz) {
            Some(next) if next > ends_at && ends_at < cap => ends_at = next,
            _ => break,
        }
    }
    Some(QuietWindow {
        ends_at: ends_at.min(cap),
    })
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

/// When a reset notice planned for `notify_at` is shown (PRD 0003 RF3.4
/// with RF3.1): then outside quiet hours, at the window's end when the
/// reset comes after it, never (`None`) when the reset happens inside the
/// window. The same rule as [`decide`], for a notice still ahead.
pub fn reset_shown_at<Tz: TimeZone>(
    schedule: &QuietSchedule,
    tz: &Tz,
    notify_at: DateTime<Utc>,
    resets_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match decide(
        Subject::Reset { resets_at },
        window_at(schedule, notify_at, tz),
    ) {
        Decision::Deliver => Some(notify_at),
        Decision::Defer(at) => Some(at),
        Decision::Drop => None,
    }
}

/// RF3.3: the notification that closes quiet hours, as data: its title,
/// at most [`DIGEST_LINES`] lines (one per event, in account order) and the
/// count of the events beyond them. Each channel joins the lines its own
/// way ([`Digest::body`]), so they never run together (PRD 0032
/// re-validation C-01).
#[derive(Debug, Clone)]
pub struct Digest {
    pub title: Localized,
    pub lines: Vec<Localized>,
    pub hidden: usize,
}

impl Digest {
    /// "and N more".
    pub fn more(count: usize) -> Localized {
        Localized::plural(
            "quiet.digest.more",
            params([("count", count.into())]),
            "and {count} more",
            "and {count} more",
        )
    }

    /// The body for a notification: whole lines within `max` characters,
    /// each cleaned, joined by `join`, ending with "and N more" when lines
    /// are left out ([`crate::text::notice_lines`]).
    pub fn body(
        &self,
        locale: &str,
        max: usize,
        markup: crate::text::NoticeMarkup,
        join: crate::text::LineJoin,
    ) -> String {
        let lines: Vec<String> = self.lines.iter().map(|line| line.resolve(locale)).collect();
        crate::text::notice_lines(
            &lines,
            self.hidden,
            |count| Self::more(count).resolve(locale),
            max,
            markup,
            join,
        )
    }

    /// The whole text, one line per item and "and N more", for the
    /// channels' JSON (no length cap).
    pub fn text(&self, locale: &str) -> String {
        let mut lines: Vec<String> = self.lines.iter().map(|line| line.resolve(locale)).collect();
        if self.hidden > 0 {
            lines.push(Self::more(self.hidden).resolve(locale));
        }
        lines.join("\n")
    }
}

/// RF3.3: the [`Digest`] of the `groups` of alerts that still hold when
/// quiet hours end. With `private` on the lines are the lock-screen-safe
/// titles (provider, window and percentage); off, they name the account
/// and the metric ("{title} — {body}"). `None` without events.
pub fn digest<Tz>(
    groups: &[(&Account, Vec<AlertEvent>)],
    now: DateTime<Utc>,
    tz: &Tz,
    private: bool,
) -> Option<Digest>
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
                "{title} — {body}",
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let count = lines.len();
    if count == 0 {
        return None;
    }
    let hidden = count.saturating_sub(DIGEST_LINES);
    let lines: Vec<Localized> = lines.into_iter().take(DIGEST_LINES).collect();
    Some(Digest {
        title: Localized::plural(
            "quiet.digest.title",
            params([("count", count.into())]),
            "Cuotai: {count} usage alert held during quiet hours",
            "Cuotai: {count} usage alerts held during quiet hours",
        ),
        lines,
        hidden,
    })
}

#[cfg(test)]
#[path = "quiet_tests.rs"]
mod tests;
