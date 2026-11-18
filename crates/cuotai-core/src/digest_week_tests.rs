//! PRD 0043 re-validation G2: the current week counts only its own days.

use super::tests::{account, at, daily, fixture};
use super::*;
use crate::model::{Metric, MetricKind, MetricWindow};

fn overage(used: f64, day: u32) -> Metric {
    Metric {
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Monthly),
        limit: None,
        ..daily(used, 1.0, day)
    }
    .overage()
}

#[test]
fn current_week_counts_only_its_own_days() {
    let dir = tempfile::tempdir().unwrap();
    let store = fixture(dir.path());
    let work = account();
    // Extra spent on the Thursday before (last week), and one reading on
    // this week's Tuesday.
    store
        .record(&work, at(22, 12), &[overage(5.0, 22)])
        .unwrap();
    store
        .record(&work, at(27, 10), &[daily(20.0, 150.0, 27)])
        .unwrap();
    let digest = weekly(
        &store,
        &[work],
        &[None],
        DigestWeek::Current,
        at(28, 12), // Wednesday
        &Utc,
        DaysOff::default(),
    )
    .unwrap();
    assert_eq!(
        (digest.first_day, digest.last_day),
        (
            NaiveDate::from_ymd_opt(2026, 10, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 28).unwrap()
        )
    );
    assert_eq!(digest.hits, 0, "Thursday's hit belongs to last week");
    assert_eq!(digest.accounts[0].hits, 0);
    assert_eq!(
        digest.extra_spent, None,
        "no extra spent from before Monday"
    );
    assert_eq!(digest.accounts[0].days_at_limit, 0);
    assert!(digest.recorded);
    assert_eq!(
        digest.heaviest.map(|d| d.day),
        NaiveDate::from_ymd_opt(2026, 10, 27)
    );
    let markdown = digest.markdown("en");
    assert!(markdown.contains("- 0 limit hits"), "{markdown}");
}

#[test]
fn last_week_still_counts_its_hit_and_extra() {
    let dir = tempfile::tempdir().unwrap();
    let store = fixture(dir.path());
    let work = account();
    store
        .record(&work, at(22, 12), &[overage(5.0, 22)])
        .unwrap();
    let digest = weekly(
        &store,
        &[work],
        &[None],
        DigestWeek::Last,
        at(28, 12),
        &Utc,
        DaysOff::default(),
    )
    .unwrap();
    assert_eq!(digest.hits, 1);
    let extra = digest.extra_spent.expect("extra spent recorded");
    assert_eq!((extra[0].unit.as_str(), extra[0].amount), ("USD", 5.0));
}
