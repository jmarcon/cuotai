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
        overage: Default::default(),
        account_key: "k".into(),
        label: "Session (5h)".into(),
        window: Some(crate::MetricWindow::Window5h),
        kind,
        threshold,
        consumed: 0.9,
        resets_at: None,
        projection: None,
        limit_change: None,
        row: None,
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
    assert_eq!(notice.lines.len(), DIGEST_LINES);
    assert_eq!(notice.hidden, 2);
    let body = notice.text("en-US");
    assert_eq!(body.lines().count(), DIGEST_LINES + 1, "{body}");
    assert!(body.ends_with("and 2 more"), "{body}");
    assert!(body.contains("Work"), "the public digest names the account");
    assert!(!body.contains("used:"), "{body}");
    // PRD 0032 re-validation C-01: the notification body keeps one line
    // per item within its cap, and "and N more" survives the cut.
    use crate::text::{LineJoin, NoticeMarkup, NOTICE_LINES_BODY_MAX};
    let native = notice.body(
        "en-US",
        NOTICE_LINES_BODY_MAX,
        NoticeMarkup::Plain,
        LineJoin::Breaks,
    );
    assert!(native.chars().count() <= NOTICE_LINES_BODY_MAX);
    assert!(native.lines().count() >= 2, "{native}");
    assert!(
        native.lines().last().unwrap().starts_with("and "),
        "{native}"
    );
    let one = notice.body("en-US", 200, NoticeMarkup::Plain, LineJoin::Separator);
    assert!(!one.contains('\n') && one.contains(" | "), "{one}");
    assert!(one.ends_with(" more"), "{one}");
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
        let text = format!("{} {}", notice.title.resolve(locale), notice.text(locale));
        assert!(!text.contains("Secret"), "{locale}: {text}");
        assert!(text.contains("90"), "{locale}: {text}");
    }
}

#[test]
fn the_strict_reading_refuses_what_the_interface_must_not_send() {
    let good = serde_json::json!({
        "enabled": true, "digest": false,
        "days": {"mon": [{"from": "22:00", "to": "08:00"}]}
    });
    let schedule = QuietSchedule::from_value_strict(&good).unwrap();
    assert!(schedule.enabled && !schedule.digest);
    assert_eq!(schedule.days[0], vec![interval("22:00", "08:00")]);
    let four = (1..=4)
        .map(|i| serde_json::json!({"from": format!("0{i}:00"), "to": format!("0{i}:30")}))
        .collect::<Vec<_>>();
    for bad in [
        serde_json::json!({"enabled": true}),
        serde_json::json!({"enabled": 1, "digest": true}),
        serde_json::json!({"enabled": true, "digest": true, "days": {"xyz": []}}),
        serde_json::json!({"enabled": true, "digest": true, "days": {"mon": four}}),
    ] {
        assert_eq!(
            QuietSchedule::from_value_strict(&bad).map_err(|e| e.code),
            Err(QuietErrorCode::Invalid),
            "{bad}"
        );
    }
    let bad_time = serde_json::json!({"enabled": true, "digest": true,
        "days": {"mon": [{"from": "7:00", "to": "08:00"}]}});
    let error = QuietSchedule::from_value_strict(&bad_time).unwrap_err();
    assert_eq!(
        (error.code, error.day, error.index, error.field),
        (QuietErrorCode::Time, Some("mon"), Some(0), Some("from"))
    );
}

fn strict_day(list: serde_json::Value) -> Result<QuietSchedule, QuietError> {
    QuietSchedule::from_value_strict(
        &serde_json::json!({"enabled": true, "digest": true, "days": {"wed": list}}),
    )
}

/// PRD 0032 re-validation C-06: an exact duplicate of a day is refused on
/// the later interval, with the earlier one named.
#[test]
fn the_strict_reading_refuses_a_duplicate() {
    let error = strict_day(serde_json::json!([
        {"from": "22:00", "to": "08:00"},
        {"from": "22:00", "to": "08:00"}
    ]))
    .unwrap_err();
    assert_eq!(
        error,
        QuietError {
            code: QuietErrorCode::Duplicate,
            day: Some("wed"),
            index: Some(1),
            field: Some("from"),
            with: Some(0),
        }
    );
    assert_eq!(
        error.to_json(),
        r#"{"code":"invalid_quiet_duplicate","day":"wed","index":1,"field":"from","with":0}"#
    );
}

/// Overlaps among intervals of one start day are refused, touching ones
/// are not; an overnight interval only covers its own day's evening.
#[test]
fn the_strict_reading_refuses_an_overlap_on_the_same_day() {
    for (list, with) in [
        (
            serde_json::json!([{"from": "22:00", "to": "08:00"}, {"from": "23:00", "to": "01:00"}]),
            0,
        ),
        (
            serde_json::json!([{"from": "09:00", "to": "12:00"}, {"from": "13:00", "to": "14:00"}, {"from": "11:00", "to": "13:30"}]),
            0,
        ),
        (
            serde_json::json!([{"from": "00:00", "to": "00:00"}, {"from": "10:00", "to": "11:00"}]),
            0,
        ),
    ] {
        let error = strict_day(list.clone()).unwrap_err();
        assert_eq!(error.code, QuietErrorCode::Overlap, "{list}");
        assert_eq!(error.with, Some(with), "{list}");
        assert_eq!(error.field, Some("from"), "{list}");
    }
    // Touching intervals and the morning before an overnight one are fine.
    let schedule = strict_day(serde_json::json!([
        {"from": "22:00", "to": "08:00"},
        {"from": "08:00", "to": "09:00"},
        {"from": "09:00", "to": "10:00"}
    ]))
    .unwrap();
    assert_eq!(schedule.days[2].len(), 3);
}

/// Monday's overnight interval running into Tuesday's morning is one
/// window, not an error.
#[test]
fn the_strict_reading_accepts_an_overlap_across_days() {
    let value = serde_json::json!({"enabled": true, "digest": true, "days": {
        "mon": [{"from": "22:00", "to": "08:00"}],
        "tue": [{"from": "06:00", "to": "09:00"}]
    }});
    let schedule = QuietSchedule::from_value_strict(&value).unwrap();
    assert_eq!(schedule.days[1], vec![interval("06:00", "09:00")]);
}

/// `from == to` is refused (zero or 24 hours?), except `00:00`-`00:00`,
/// the whole day; the lenient reading keeps it as 24 hours.
#[test]
fn the_strict_reading_refuses_equal_ends_but_the_whole_day() {
    let error = strict_day(serde_json::json!([{"from": "08:00", "to": "08:00"}])).unwrap_err();
    assert_eq!(
        (error.code, error.index, error.field),
        (QuietErrorCode::Empty, Some(0), Some("to"))
    );
    let schedule = strict_day(serde_json::json!([{"from": "00:00", "to": "00:00"}])).unwrap();
    assert!(schedule.days[2][0].is_all_day());
    let lenient = QuietSchedule::from_value(&serde_json::json!({
        "enabled": true, "days": {"wed": [{"from": "08:00", "to": "08:00"}]}
    }));
    assert_eq!(lenient.days[2], vec![interval("08:00", "08:00")]);
}

/// The lenient reading keeps the first of exact duplicates and drops the
/// later ones before it counts the day's limit; overlaps stay.
#[test]
fn the_lenient_reading_drops_later_duplicates() {
    let schedule = QuietSchedule::from_value(&serde_json::json!({
        "enabled": true,
        "days": {"mon": [
            {"from": "22:00", "to": "08:00"},
            {"from": "22:00", "to": "08:00"},
            {"from": "23:00", "to": "01:00"},
            {"from": "22:00", "to": "08:00"},
            {"from": "12:00", "to": "13:00"}
        ]}
    }));
    assert_eq!(
        schedule.days[0],
        vec![
            interval("22:00", "08:00"),
            interval("23:00", "01:00"),
            interval("12:00", "13:00")
        ]
    );
}

/// PRD 0032 re-validation C-12: a week with no gap, counting the intervals
/// that cross midnight into the next day (and Sunday's into Monday's).
#[test]
fn always_on_needs_a_week_without_gap() {
    let week = |list: serde_json::Value| {
        let days: serde_json::Map<String, serde_json::Value> = DAY_KEYS
            .iter()
            .map(|day| ((*day).to_owned(), list.clone()))
            .collect();
        QuietSchedule::from_value(&serde_json::json!({"enabled": true, "days": days}))
    };
    assert!(week(serde_json::json!([{"from": "00:00", "to": "00:00"}])).always_on());
    assert!(week(serde_json::json!([{"from": "12:00", "to": "12:00"}])).always_on());
    assert!(week(
        serde_json::json!([{"from": "20:00", "to": "08:00"}, {"from": "08:00", "to": "20:00"}])
    )
    .always_on());
    assert!(!week(serde_json::json!([{"from": "22:00", "to": "08:00"}])).always_on());
    assert!(!QuietSchedule::default().always_on());
    // Six whole days and Sunday's 24 hours from 06:00 reaching into
    // Monday: Sunday morning is still a gap.
    let mut schedule = week(serde_json::json!([{"from": "00:00", "to": "00:00"}]));
    schedule.days[6] = vec![interval("06:00", "06:00")];
    assert!(!schedule.always_on(), "Sunday 00:00-06:00 is open");
    schedule.days[6].insert(0, interval("00:00", "06:00"));
    assert!(schedule.always_on(), "touching intervals leave no gap");
    // A day missing breaks it.
    let mut schedule = week(serde_json::json!([{"from": "00:00", "to": "00:00"}]));
    schedule.days[3].clear();
    assert!(!schedule.always_on());
}

/// PRD 0032 re-validation C-02 (DS-CMP-NOTIF-1): the digest title fits 50
/// characters in every catalog for one, two, nine and 99 held alerts, and
/// a private digest's first line fits the 110-character body budget.
#[test]
fn the_digest_title_fits_the_notification_budget() {
    let work = account("Work");
    for count in [1, 2, 9, 99] {
        let events: Vec<AlertEvent> = (0..count)
            .map(|i| {
                let mut e = event(AlertKind::Threshold, 90);
                e.label = format!("Metric {i}");
                e
            })
            .collect();
        for private in [true, false] {
            let digest = digest(
                &[(&work, events.clone())],
                utc(2026, 2, 16, 8, 0),
                &chrono::Utc,
                private,
            )
            .unwrap();
            for locale in crate::text::LOCALES.iter().map(|row| row.code) {
                let title = digest.title.resolve(locale);
                assert!(
                    title.chars().count() <= 50,
                    "{locale} {count}: {title} ({})",
                    title.chars().count()
                );
                assert!(!title.contains('{'), "{locale}: {title}");
                if private {
                    let first = digest.lines[0].resolve(locale);
                    assert!(first.chars().count() <= 110, "{locale}: {first}");
                }
            }
        }
    }
}

/// PRD 0032 re-validation C-12: a week without a gap is capped at a week,
/// as documented, however many merge passes it would take.
#[test]
fn a_week_without_gap_is_capped_at_a_week() {
    let always = QuietSchedule {
        enabled: true,
        days: std::array::from_fn(|_| vec![Interval::parse("00:00", "00:00").unwrap()]),
        digest: true,
    };
    let now = utc(2026, 2, 18, 13, 37);
    let window = window_at(&always, now, &chrono::Utc).unwrap();
    assert_eq!(window.ends_at, now + chrono::TimeDelta::days(7));
}

/// PRD 0032 re-validation C-12: a time is exactly two ASCII digits on each
/// side of the colon, in both readings.
#[test]
fn times_need_two_ascii_digits_on_each_side() {
    for bad in [
        "+1:00",
        "-1:00",
        " 1:00",
        "1:00",
        "０１:００",
        "01:+0",
        "001:00",
    ] {
        assert!(Interval::parse(bad, "08:00").is_none(), "{bad}");
        let value = serde_json::json!({
            "enabled": true, "digest": true,
            "days": {"mon": [{"from": bad, "to": "08:00"}]}
        });
        assert!(QuietSchedule::from_value_strict(&value).is_err(), "{bad}");
        assert!(!QuietSchedule::from_value(&value).has_intervals(), "{bad}");
    }
    assert!(Interval::parse("22:00", "08:00").is_some());
}
