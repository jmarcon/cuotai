//! PRD 0032 RF2/RF3: the quiet window across midnight, the week wrap and
//! DST changes, the delivery decision and the digest text.

use super::*;
use crate::alerts::AlertKind;
use crate::model::Provider;
use chrono::{FixedOffset, LocalResult, Offset, TimeDelta};

/// A zone with one summer-time rule: `std` seconds east of UTC, plus an
/// hour between `dst_from` and `dst_to` (UTC instants).
#[derive(Clone, Copy, Debug)]
struct Zone {
    std: i32,
    dst_from: DateTime<Utc>,
    dst_to: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Off(i32);

impl Offset for Off {
    fn fix(&self) -> FixedOffset {
        FixedOffset::east_opt(self.0).unwrap()
    }
}

impl std::fmt::Display for Off {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:+}s", self.0)
    }
}

impl Zone {
    fn at_utc(&self, utc: NaiveDateTime) -> Off {
        let utc = utc.and_utc();
        Off(if (self.dst_from..self.dst_to).contains(&utc) {
            self.std + 3600
        } else {
            self.std
        })
    }
}

impl TimeZone for Zone {
    type Offset = Off;

    fn from_offset(offset: &Off) -> Self {
        Self {
            std: offset.0,
            dst_from: DateTime::<Utc>::MAX_UTC,
            dst_to: DateTime::<Utc>::MAX_UTC,
        }
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<Off> {
        self.offset_from_local_datetime(&local.and_time(NaiveTime::MIN))
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Off> {
        // The summer offset gives the earlier instant.
        let candidates = [Off(self.std + 3600), Off(self.std)];
        let valid: Vec<Off> = candidates
            .into_iter()
            .filter(|off| self.at_utc(*local - TimeDelta::seconds(i64::from(off.0))) == *off)
            .collect();
        match valid.as_slice() {
            [] => LocalResult::None,
            [one] => LocalResult::Single(*one),
            [first, second, ..] => LocalResult::Ambiguous(*first, *second),
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> Off {
        self.at_utc(utc.and_time(NaiveTime::MIN))
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Off {
        self.at_utc(*utc)
    }
}

/// United States Eastern in 2026: UTC−5, summer time from 2026-03-08
/// 02:00 local to 2026-11-01 02:00 local.
fn eastern() -> Zone {
    Zone {
        std: -5 * 3600,
        dst_from: utc(2026, 3, 8, 7, 0),
        dst_to: utc(2026, 11, 1, 6, 0),
    }
}

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
}

fn interval(from: &str, to: &str) -> Interval {
    Interval::parse(from, to).unwrap()
}

/// 22:00–08:00 on every day.
fn nights() -> QuietSchedule {
    QuietSchedule {
        enabled: true,
        days: std::array::from_fn(|_| vec![interval("22:00", "08:00")]),
        digest: true,
    }
}

#[test]
fn a_night_crosses_midnight_and_ends_at_eight() {
    let tz = FixedOffset::west_opt(3 * 3600).unwrap();
    let schedule = nights();
    // 23:00 local on Monday 2026-02-16 is 02:00 UTC on the 17th.
    let at = |h, m| utc(2026, 2, 17, h, m);
    let window = window_at(&schedule, at(2, 0), &tz).expect("23:00 is quiet");
    assert_eq!(window.ends_at, utc(2026, 2, 17, 11, 0), "08:00 local");
    // 03:00 local (after midnight) belongs to the start day's interval.
    assert_eq!(
        window_at(&schedule, at(6, 0), &tz).unwrap().ends_at,
        utc(2026, 2, 17, 11, 0)
    );
    // Noon and 21:59 are not quiet; 22:00 is.
    assert_eq!(window_at(&schedule, at(15, 0), &tz), None);
    assert_eq!(window_at(&schedule, utc(2026, 2, 17, 0, 59), &tz), None);
    assert!(window_at(&schedule, utc(2026, 2, 17, 1, 0), &tz).is_some());
    // 08:00 sharp is already outside.
    assert_eq!(window_at(&schedule, at(11, 0), &tz), None);
}

#[test]
fn only_the_listed_weekdays_are_quiet_and_the_week_wraps() {
    let tz = FixedOffset::east_opt(0).unwrap();
    let mut schedule = QuietSchedule {
        enabled: true,
        ..QuietSchedule::default()
    };
    // Sunday night into Monday only.
    schedule.days[6] = vec![interval("22:00", "07:00")];
    // 2026-02-15 is a Sunday.
    let w = window_at(&schedule, utc(2026, 2, 15, 23, 0), &tz).unwrap();
    assert_eq!(w.ends_at, utc(2026, 2, 16, 7, 0));
    assert!(window_at(&schedule, utc(2026, 2, 16, 3, 0), &tz).is_some());
    // The same hour on Monday night is not quiet.
    assert_eq!(window_at(&schedule, utc(2026, 2, 16, 23, 0), &tz), None);
    // Saturday night neither: the interval belongs to its start day.
    assert_eq!(window_at(&schedule, utc(2026, 2, 14, 23, 0), &tz), None);
}

#[test]
fn disabled_or_empty_schedules_hold_nothing() {
    let tz = FixedOffset::east_opt(0).unwrap();
    let mut schedule = nights();
    schedule.enabled = false;
    assert_eq!(window_at(&schedule, utc(2026, 2, 15, 23, 0), &tz), None);
    let empty = QuietSchedule {
        enabled: true,
        ..QuietSchedule::default()
    };
    assert_eq!(window_at(&empty, utc(2026, 2, 15, 23, 0), &tz), None);
}

#[test]
fn adjacent_intervals_are_one_window() {
    let tz = FixedOffset::east_opt(0).unwrap();
    let mut schedule = QuietSchedule {
        enabled: true,
        ..QuietSchedule::default()
    };
    schedule.days[0] = vec![interval("22:00", "08:00")];
    schedule.days[1] = vec![interval("08:00", "09:30"), interval("12:00", "13:00")];
    // Monday 2026-02-16 23:00: the night runs into the Tuesday morning.
    let w = window_at(&schedule, utc(2026, 2, 16, 23, 0), &tz).unwrap();
    assert_eq!(w.ends_at, utc(2026, 2, 17, 9, 30));
    let lunch = window_at(&schedule, utc(2026, 2, 17, 12, 30), &tz).unwrap();
    assert_eq!(lunch.ends_at, utc(2026, 2, 17, 13, 0));
}

#[test]
fn spring_forward_shortens_the_night() {
    let tz = eastern();
    let schedule = nights();
    // Saturday 2026-03-07 22:00 EST = 03:00 UTC on the 8th. The night of
    // the change ends 08:00 EDT = 12:00 UTC: nine hours instead of ten.
    let w = window_at(&schedule, utc(2026, 3, 8, 3, 0), &tz).unwrap();
    assert_eq!(w.ends_at, utc(2026, 3, 8, 12, 0));
    // 03:30 EDT, right after the skipped hour, is quiet.
    assert!(window_at(&schedule, utc(2026, 3, 8, 7, 30), &tz).is_some());
    // A window ending inside the gap ends where the gap does.
    let mut gap = QuietSchedule {
        enabled: true,
        ..QuietSchedule::default()
    };
    gap.days[5] = vec![interval("22:00", "02:30")];
    let w = window_at(&gap, utc(2026, 3, 8, 4, 0), &tz).unwrap();
    assert_eq!(w.ends_at, utc(2026, 3, 8, 7, 0), "03:00 EDT");
}

#[test]
fn fall_back_lengthens_the_night() {
    let tz = eastern();
    let schedule = nights();
    // Saturday 2026-10-31 22:00 EDT = 02:00 UTC on 1 Nov; the night ends
    // 08:00 EST = 13:00 UTC: eleven hours.
    let w = window_at(&schedule, utc(2026, 11, 1, 2, 0), &tz).unwrap();
    assert_eq!(w.ends_at, utc(2026, 11, 1, 13, 0));
    // The repeated 01:30 (06:30 UTC) is inside too.
    assert!(window_at(&schedule, utc(2026, 11, 1, 6, 30), &tz).is_some());
    // An end in the repeated hour takes the later reading.
    let mut repeated = QuietSchedule {
        enabled: true,
        ..QuietSchedule::default()
    };
    repeated.days[5] = vec![interval("22:00", "01:30")];
    let w = window_at(&repeated, utc(2026, 11, 1, 3, 0), &tz).unwrap();
    assert_eq!(w.ends_at, utc(2026, 11, 1, 6, 30), "01:30 EST");
}

#[test]
fn the_codec_is_lenient_and_round_trips() {
    let value = serde_json::json!({
        "enabled": true,
        "digest": "yes",
        "days": {
            "mon": [
                {"from": "22:00", "to": "08:00"},
                {"from": "25:00", "to": "08:00"},
                {"from": "9:00", "to": "10:00"},
                {"from": "12:00", "to": "13:00"},
                {"from": "14:00", "to": "15:00"},
                {"from": "16:00", "to": "17:00"}
            ],
            "tue": "nope",
            "xyz": [{"from": "01:00", "to": "02:00"}]
        }
    });
    let schedule = QuietSchedule::from_value(&value);
    assert!(schedule.enabled);
    assert!(schedule.digest, "a mistyped flag reads as its default");
    assert_eq!(schedule.days[0].len(), MAX_INTERVALS_PER_DAY);
    assert_eq!(schedule.days[0][0], interval("22:00", "08:00"));
    assert!(schedule.days[1].is_empty());
    assert_eq!(
        QuietSchedule::from_value(&schedule.to_value()),
        schedule,
        "round trip"
    );
    assert_eq!(
        QuietSchedule::from_value(&Value::Null),
        QuietSchedule::default()
    );
}

fn event(kind: AlertKind, threshold: u8) -> AlertEvent {
    AlertEvent {
        account_key: "k".into(),
        label: "Session (5h)".into(),
        window: Some(crate::MetricWindow::Window5h),
        kind,
        threshold,
        consumed: 0.9,
        resets_at: None,
        projection: None,
    }
}

#[test]
fn exhaustion_breaks_through_and_the_rest_waits() {
    let window = Some(QuietWindow {
        ends_at: utc(2026, 2, 16, 8, 0),
    });
    let end = utc(2026, 2, 16, 8, 0);
    let decide_alert = |kind, window| {
        let e = event(kind, 90);
        decide(Subject::Alert(&e), window)
    };
    assert_eq!(
        decide_alert(AlertKind::Exhausted, window),
        Decision::Deliver
    );
    for kind in [
        AlertKind::Threshold,
        AlertKind::Recovered,
        AlertKind::SessionExpiring,
        AlertKind::ProjectedExhaustion,
    ] {
        assert_eq!(decide_alert(kind, window), Decision::Defer(end), "{kind:?}");
        assert_eq!(decide_alert(kind, None), Decision::Deliver);
    }
}

#[test]
fn a_reset_inside_the_window_is_dropped_and_a_later_one_waits() {
    let window = Some(QuietWindow {
        ends_at: utc(2026, 2, 16, 8, 0),
    });
    let at = |resets_at| decide(Subject::Reset { resets_at }, window);
    assert_eq!(at(utc(2026, 2, 16, 3, 0)), Decision::Drop);
    assert_eq!(
        at(utc(2026, 2, 16, 8, 5)),
        Decision::Defer(utc(2026, 2, 16, 8, 0))
    );
    assert_eq!(
        decide(
            Subject::Reset {
                resets_at: utc(2026, 2, 16, 3, 0)
            },
            None
        ),
        Decision::Deliver
    );
}

fn account(name: &str) -> Account {
    Account {
        id: None,
        name: name.into(),
        provider: Provider::Claude,
        ..Account::default()
    }
}

#[test]
fn the_digest_lists_five_lines_and_counts_the_rest() {
    let work = account("Work");
    let events: Vec<AlertEvent> = (0..7)
        .map(|i| {
            let mut e = event(AlertKind::Threshold, 90);
            e.label = format!("Metric {i}");
            e
        })
        .collect();
    let now = utc(2026, 2, 16, 8, 0);
    let notice = digest(&[(&work, events)], now, &chrono::Utc, false).unwrap();
    assert_eq!(
        notice.title.resolve("en-US"),
        "Cuotai: 7 usage alerts held during quiet hours"
    );
    let body = notice.body.resolve("en-US");
    assert_eq!(body.lines().count(), DIGEST_LINES + 1, "{body}");
    assert!(body.ends_with("and 2 more"), "{body}");
    assert!(body.contains("Work"), "the public digest names the account");
    assert!(digest(&[(&work, Vec::new())], now, &chrono::Utc, false).is_none());
}

#[test]
fn the_private_digest_names_no_account() {
    let work = account("Work Secret");
    let one = event(AlertKind::Threshold, 90);
    let notice = digest(
        &[(&work, vec![one])],
        utc(2026, 2, 16, 8, 0),
        &chrono::Utc,
        true,
    )
    .unwrap();
    for locale in ["en-US", "es", "it", "pt-BR"] {
        let text = format!(
            "{} {}",
            notice.title.resolve(locale),
            notice.body.resolve(locale)
        );
        assert!(!text.contains("Secret"), "{locale}: {text}");
        assert!(text.contains("90"), "{locale}: {text}");
    }
}
