//! PRD 0041 re-validation (F-13, F-17, F-18): the allowance never rounds a
//! positive rest to 0, says nothing once a budget is used up, and gives a
//! money budget's recent pace in money.

use super::*;
use crate::model::{Billing, DerivedFrom, MetricKind};
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap()
}

fn quota(used: f64, limit: f64, unit: &str, left: TimeDelta) -> Metric {
    Metric {
        label: "Semanal".into(),
        used: Some(used),
        limit: Some(limit),
        remaining: None,
        unit: unit.into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Weekly),
        period: None,
        resets_at: Some(now() + left),
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn budget(used: f64, limit: f64, left: TimeDelta) -> Metric {
    let mut metric = quota(used, limit, "USD", left);
    metric.label = "Orçamento mensal · Gasto no mês".into();
    metric.window = Some(MetricWindow::Monthly);
    metric.derived_from = Some(DerivedFrom {
        kind: MetricKind::Spend,
        label: "Gasto no mês".into(),
        budget: 0,
        partial: false,
        key: String::new(),
        period_start: None,
        period_text: None,
    });
    metric
}

fn sentence(metric: &Metric, recent: Option<&[Point]>) -> Localized {
    allowance(metric, now(), recent)
        .expect("an allowance")
        .sentence()
}

fn golden(text: &Localized, expected: [(&str, &str); 4]) {
    for (locale, line) in expected {
        assert_eq!(text.resolve(locale), line, "{locale}");
    }
}

/// F-13: a positive allowance that would read 0 says "less than".
#[test]
fn a_tiny_allowance_says_less_than_instead_of_zero() {
    // 0.1 point over 6 days: 0.017 %/day.
    let percent = sentence(&quota(99.9, 100.0, "%", TimeDelta::days(6)), None);
    assert_eq!(percent.id, "pace.allowance.day_less");
    golden(
        &percent,
        [
            ("en-US", "less than 0.1%/day to the reset"),
            ("de", "weniger als 0,1\u{a0}%/Tag bis zur Erneuerung"),
            ("ru", "меньше 0,1\u{a0}%/день до обновления"),
            ("ja", "リセットまで 1 日 0.1% 未満"),
        ],
    );
    let hourly = sentence(&quota(99.99, 100.0, "%", TimeDelta::hours(30)), None);
    assert_eq!(hourly.id, "pace.allowance.hour_less");
    // 1 cent over 10 days: 0.1 cent a day.
    let money = sentence(&budget(49.99, 50.0, TimeDelta::days(10)), None);
    assert_eq!(money.id, "pace.allowance.budget_day_less");
    golden(
        &money,
        [
            ("en-US", "less than $0.01/day left in the budget"),
            ("de", "weniger als 0,01\u{a0}$/Tag im Budget übrig"),
            ("ru", "меньше 0,01\u{a0}$/день осталось в бюджете"),
            ("ja", "1 日 $0.01 未満 予算に残り"),
        ],
    );
    let money = sentence(&budget(49.999, 50.0, TimeDelta::hours(20)), None);
    assert_eq!(money.id, "pace.allowance.budget_hour_less");
    // An allowance that shows a figure keeps the "≈" form.
    let plain = sentence(&quota(99.0, 100.0, "%", TimeDelta::days(6)), None);
    assert_eq!(plain.resolve("en-US"), "≈ 0.2%/day to the reset");
    for locale in crate::text::catalog_locales() {
        for text in [&percent, &money, &hourly] {
            let line = text.resolve(locale);
            assert_ne!(line, text.fallback, "{locale}");
            assert!(!line.contains('{'), "{locale}: {line}");
        }
    }
}

/// F-18: a budget at or over its limit has no allowance; the exhausted
/// state already says it.
#[test]
fn no_allowance_once_the_budget_is_used_up() {
    for used in [50.0, 60.0] {
        assert!(allowance(&budget(used, 50.0, TimeDelta::days(10)), now(), None).is_none());
        assert!(allowance(
            &quota(used * 2.0, 100.0, "%", TimeDelta::days(3)),
            now(),
            None
        )
        .is_none());
    }
    // Known only by its remaining: the same.
    let mut over = budget(0.0, 50.0, TimeDelta::days(10));
    over.used = None;
    over.remaining = Some(0.0);
    assert!(allowance(&over, now(), None).is_none());
}

/// F-17: a money budget compares its recent pace in money, a percent quota
/// in percent.
#[test]
fn a_money_budget_gives_its_recent_pace_in_money() {
    // $1 an hour over the last 3 hours: $24/day against $2/day.
    let recent: Vec<Point> = (0..4)
        .map(|h| Point {
            taken_at: now() - TimeDelta::hours(3 - h),
            used: 27.0 + h as f64,
        })
        .collect();
    let money = sentence(&budget(30.0, 50.0, TimeDelta::days(10)), Some(&recent));
    assert_eq!(money.id, "pace.allowance.budget_day_recent_amount");
    golden(
        &money,
        [
            (
                "en-US",
                "≈ $2.00/day left in the budget · recently $24.00/day",
            ),
            (
                "de",
                "≈ 2,00\u{a0}$/Tag im Budget übrig · zuletzt 24,00\u{a0}$/Tag",
            ),
            (
                "ru",
                "≈ 2,00\u{a0}$/день осталось в бюджете · недавно 24,00\u{a0}$/день",
            ),
            ("ja", "≈ 1 日 $2.00 予算に残り · 最近は 1 日 $24.00"),
        ],
    );
    let hourly = sentence(&budget(30.0, 50.0, TimeDelta::hours(40)), Some(&recent));
    assert_eq!(hourly.id, "pace.allowance.budget_hour_recent_amount");
    assert!(
        !hourly.resolve("en-US").contains('%'),
        "{}",
        hourly.fallback
    );
    // A percent quota keeps the percent comparison.
    let percent = sentence(&quota(40.0, 100.0, "%", TimeDelta::days(6)), Some(&recent));
    assert_eq!(percent.id, "pace.allowance.day_recent");
    for locale in crate::text::catalog_locales() {
        for text in [&money, &hourly] {
            assert_ne!(text.resolve(locale), text.fallback, "{locale}");
        }
    }
}
