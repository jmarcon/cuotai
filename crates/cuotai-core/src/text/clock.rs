//! PRD 0024 re-validation (V-01): a clock time in the reader's language
//! (DS-MSG-8), never an ISO date.
//!
//! A time travels as a nested [`Localized`] whose params are machine values
//! (plain integers), on the pattern of the `amount.*` ids
//! ([`super::number`]), so the `export::v1` shape of `Localized` does not
//! change:
//!
//! - `at.time` `{hour, minute}`: today, "3:37 PM", "15:37";
//! - `at.weekday` `{weekday, hour, minute}`: within the next six days,
//!   "Tuesday 3:37 PM" (`weekday` is ISO, 1 = Monday);
//! - `at.date` `{day, month, hour, minute}`: further away, "11/18 3:37 PM".
//!
//! [`clock_short`] gives the form a chip fits in its budget
//! (DS-CMP-TIME-1): the time today, otherwise only the short weekday or
//! only the date, with no `hour`/`minute` params.
//!
//! [`Localized::resolve`] renders these ids here instead of through a
//! catalog, with the hour cycle of the table below (12 hours for en-US,
//! hi-IN and ko, 24 otherwise), and the desktop renders them with
//! `Intl.DateTimeFormat`. The weekday names and the date order come from
//! the core catalogs (`budget.weekday.*`, `text.weekday.short.*`,
//! `alerts.at.*`).

use std::fmt::Display;

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};

use super::{params, Localized, ParamValue};

/// The id of a time today.
pub const TIME: &str = "at.time";
/// The id of a time on one of the next six days.
pub const WEEKDAY: &str = "at.weekday";
/// The id of a time further away.
pub const DATE: &str = "at.date";
/// The id of an instant whose local clock is out of range (read in UTC).
pub const UTC: &str = "at.utc";
/// The id of a time a caller already worded.
pub const WORDED: &str = "at.worded";

/// A time a caller already worded, shown as it is in every language.
pub fn worded(text: impl Into<String>) -> Localized {
    Localized {
        id: WORDED.into(),
        params: Default::default(),
        fallback: text.into().into(),
    }
}

/// How a language writes the hour.
#[derive(Clone, Copy)]
enum Cycle {
    /// `15:37` with this separator.
    H23(&'static str),
    /// `3:37 PM`: the morning and afternoon marks, and whether they come
    /// before the time.
    H12 {
        am: &'static str,
        pm: &'static str,
        before: bool,
    },
}

fn cycle(code: &str) -> Cycle {
    match code {
        "hi-IN" => Cycle::H12 {
            am: "am",
            pm: "pm",
            before: false,
        },
        "ko" => Cycle::H12 {
            am: "오전",
            pm: "오후",
            before: true,
        },
        "id-ID" => Cycle::H23("."),
        // en-US, the pseudo-locale and unknown codes.
        "en-US" | "en-XA" => Cycle::H12 {
            am: "AM",
            pm: "PM",
            before: false,
        },
        _ => Cycle::H23(":"),
    }
}

/// `hour:minute` in the hour cycle of `code`.
pub fn format_time(hour: u32, minute: u32, code: &str) -> String {
    match cycle(code) {
        Cycle::H23(sep) => format!("{hour:02}{sep}{minute:02}"),
        Cycle::H12 { am, pm, before } => {
            let mark = if hour < 12 { am } else { pm };
            let h = match hour % 12 {
                0 => 12,
                h => h,
            };
            if before {
                format!("{mark} {h}:{minute:02}")
            } else {
                format!("{h}:{minute:02}\u{a0}{mark}")
            }
        }
    }
}

fn source() -> &'static str {
    super::source_locale().code
}

fn with_fallback(
    id: &'static str,
    params: std::collections::BTreeMap<String, ParamValue>,
) -> Localized {
    let mut note = Localized {
        id: id.into(),
        params,
        fallback: "".into(),
    };
    note.fallback = render(&note, source()).unwrap_or_default().into();
    note
}

/// The local clock of `at` and `now` in `tz`, or the UTC text of `at`.
fn locals<Tz>(
    at: DateTime<Utc>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Result<(chrono::NaiveDateTime, i64), Localized>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    match (
        crate::instant::checked_local(at, tz),
        crate::instant::checked_local(now, tz),
    ) {
        (Some(local), Some(today)) => {
            let days = (local.date_naive() - today.date_naive()).num_days();
            Ok((local.naive_local(), days))
        }
        // An instant whose local clock is out of range reads in UTC.
        _ => Err(Localized {
            id: UTC.into(),
            params: Default::default(),
            fallback: format!("{} UTC", at.format("%Y-%m-%d %H:%M")).into(),
        }),
    }
}

fn number(value: u32) -> ParamValue {
    ParamValue::Text(value.to_string())
}

/// `at` in `tz` as a person reads it at `now`: the time today, the weekday
/// and time within the next six days, the short date and time further away
/// (or in the past). Never an ISO date (DS-MSG-8).
pub fn clock<Tz>(at: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> Localized
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let (local, days) = match locals(at, now, tz) {
        Ok(found) => found,
        Err(utc) => return utc,
    };
    let (hour, minute) = (local.hour(), local.minute());
    match days {
        0 => with_fallback(
            TIME,
            params([("hour", number(hour)), ("minute", number(minute))]),
        ),
        1..=6 => with_fallback(
            WEEKDAY,
            params([
                ("weekday", number(local.weekday().number_from_monday())),
                ("hour", number(hour)),
                ("minute", number(minute)),
            ]),
        ),
        _ => with_fallback(
            DATE,
            params([
                ("day", number(local.day())),
                ("month", number(local.month())),
                ("hour", number(hour)),
                ("minute", number(minute)),
            ]),
        ),
    }
}

/// [`clock`] as a chip shows it within its 18-character budget
/// (DS-CMP-TIME-1): the time today, otherwise only the short weekday
/// ("Tue") or only the short date ("11/18"). The tooltip, callout and
/// notice keep the full [`clock`].
pub fn clock_short<Tz>(at: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> Localized
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let (local, days) = match locals(at, now, tz) {
        Ok(found) => found,
        Err(utc) => return utc,
    };
    match days {
        0 => clock(at, now, tz),
        1..=6 => with_fallback(
            WEEKDAY,
            params([("weekday", number(local.weekday().number_from_monday()))]),
        ),
        _ => with_fallback(
            DATE,
            params([
                ("day", number(local.day())),
                ("month", number(local.month())),
            ]),
        ),
    }
}

fn int_param(note: &Localized, key: &str, max: u32) -> Option<u32> {
    match note.params.get(key)? {
        ParamValue::Text(text) => text.parse().ok().filter(|value| *value <= max),
        ParamValue::L10n(_) => None,
    }
}

/// The full name of ISO weekday `n` (1 = Monday).
fn weekday_name(n: u32) -> Option<Localized> {
    use crate::budget::WeekdayName::*;
    let day = match n {
        1 => Monday,
        2 => Tuesday,
        3 => Wednesday,
        4 => Thursday,
        5 => Friday,
        6 => Saturday,
        7 => Sunday,
        _ => return None,
    };
    Some(crate::budget::weekday_text(day))
}

/// The short name of ISO weekday `n` (`text.weekday.short.*`).
fn weekday_short(n: u32) -> Option<Localized> {
    let (id, fallback) = match n {
        1 => ("text.weekday.short.monday", "Mon"),
        2 => ("text.weekday.short.tuesday", "Tue"),
        3 => ("text.weekday.short.wednesday", "Wed"),
        4 => ("text.weekday.short.thursday", "Thu"),
        5 => ("text.weekday.short.friday", "Fri"),
        6 => ("text.weekday.short.saturday", "Sat"),
        7 => ("text.weekday.short.sunday", "Sun"),
        _ => return None,
    };
    Some(Localized::fixed(id, fallback))
}

/// The text of an `at.*` note in `locale`; `None` for any other id or a
/// malformed param (the caller then shows the fallback).
pub(super) fn render(note: &Localized, locale: &str) -> Option<String> {
    let id = &*note.id;
    if !id.starts_with("at.") || id == UTC || id == WORDED {
        return None;
    }
    let code = super::Locale::resolve(locale).code();
    let code = if super::is_pseudo(locale) {
        "en-XA"
    } else {
        code
    };
    let time = match (int_param(note, "hour", 23), int_param(note, "minute", 59)) {
        (Some(hour), Some(minute)) => Some(format_time(hour, minute, code)),
        (None, None) if !note.params.contains_key("hour") => None,
        _ => return None,
    };
    let text = match id {
        TIME => return time,
        WEEKDAY => {
            let n = int_param(note, "weekday", 7)?;
            match time {
                Some(time) => Localized::with_params(
                    "alerts.at.weekday",
                    params([
                        ("weekday", ParamValue::l10n(weekday_name(n)?)),
                        ("time", time.into()),
                    ]),
                    "{weekday} {time}",
                ),
                None => weekday_short(n)?,
            }
        }
        DATE => {
            let day = format!("{:02}", int_param(note, "day", 31).filter(|d| *d > 0)?);
            let month = format!("{:02}", int_param(note, "month", 12).filter(|m| *m > 0)?);
            match time {
                Some(time) => Localized::with_params(
                    "alerts.at.date",
                    params([
                        ("day", day.into()),
                        ("month", month.into()),
                        ("time", time.into()),
                    ]),
                    "{month}/{day} {time}",
                ),
                None => Localized::with_params(
                    "alerts.at.day",
                    params([("day", day.into()), ("month", month.into())]),
                    "{month}/{day}",
                ),
            }
        }
        _ => return None,
    };
    Some(text.resolve(locale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn the_hour_cycle_follows_each_language() {
        assert_eq!(format_time(15, 7, "en-US"), "3:07\u{a0}PM");
        assert_eq!(format_time(0, 5, "en-US"), "12:05\u{a0}AM");
        assert_eq!(format_time(12, 59, "hi-IN"), "12:59\u{a0}pm");
        assert_eq!(format_time(15, 7, "ko"), "오후 3:07");
        assert_eq!(format_time(9, 5, "de"), "09:05");
        assert_eq!(format_time(15, 7, "id-ID"), "15.07");
    }

    #[test]
    fn today_a_weekday_and_a_date_resolve_without_iso() {
        let tz = FixedOffset::east_opt(0).unwrap();
        let now = utc("2026-11-16T10:00:00Z"); // a Monday
        let today = clock(utc("2026-11-16T15:37:00Z"), now, &tz);
        assert_eq!(today.id, TIME);
        assert_eq!(today.fallback, "3:37\u{a0}PM");
        assert_eq!(today.resolve("de"), "15:37");
        let tuesday = clock(utc("2026-11-17T15:37:00Z"), now, &tz);
        assert_eq!(tuesday.fallback, "Tuesday 3:37\u{a0}PM");
        assert_eq!(tuesday.resolve("de"), "Dienstag, 15:37");
        assert_eq!(tuesday.resolve("pt-BR"), "terça-feira 15:37");
        let later = clock(utc("2026-11-28T09:05:00Z"), now, &tz);
        assert_eq!(later.fallback, "11/28 9:05\u{a0}AM");
        assert_eq!(later.resolve("de"), "28.11., 09:05");
        assert_eq!(later.resolve("ja"), "11/28 09:05");
        let json = serde_json::to_value(&tuesday).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "at.weekday",
                "params": {"weekday": "2", "hour": "15", "minute": "37"},
                "fallback": "Tuesday 3:37\u{a0}PM"
            })
        );
        // The chip form drops the time beyond today.
        assert_eq!(
            clock_short(utc("2026-11-17T15:37:00Z"), now, &tz).fallback,
            "Tue"
        );
        assert_eq!(
            clock_short(utc("2026-11-17T15:37:00Z"), now, &tz).resolve("de"),
            "Di."
        );
        assert_eq!(
            clock_short(utc("2026-11-28T09:05:00Z"), now, &tz).resolve("de"),
            "28.11."
        );
        assert_eq!(
            clock_short(utc("2026-11-16T15:37:00Z"), now, &tz).resolve("de"),
            "15:37"
        );
        // Nested inside another text, the time follows the outer locale.
        let outer = Localized::with_param("x.y", "at", today, "Runs out ≈{at}");
        assert_eq!(outer.fallback, "Runs out ≈3:37\u{a0}PM");
    }

    #[test]
    fn a_malformed_or_out_of_range_clock_never_invents_a_time() {
        let mut broken = clock(
            utc("2026-11-16T15:37:00Z"),
            utc("2026-11-16T10:00:00Z"),
            &Utc,
        );
        broken
            .params
            .insert("hour".into(), ParamValue::Text("25".into()));
        assert_eq!(broken.resolve("de"), broken.fallback);
        let east = FixedOffset::east_opt(14 * 3600).unwrap();
        let max = DateTime::<Utc>::MAX_UTC;
        let text = clock(max, utc("2026-11-16T10:00:00Z"), &east);
        assert!(text.resolve("de").ends_with(" UTC"), "{}", text.fallback);
    }
}
