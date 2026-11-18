//! PRD 0043 re-validation: the current week counts only its own days (G2)
//! and the overall figures come from the accounts alone (G3).

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

// G3: the overall figures come from the accounts alone.

#[test]
fn retaining_every_account_keeps_the_digest() {
    let dir = tempfile::tempdir().unwrap();
    let store = fixture(dir.path());
    let personal = Account {
        name: "Personal".into(),
        provider: crate::model::Provider::Claude,
        ..Default::default()
    };
    store
        .record(&personal, at(21, 22), &[daily(10.0, 100.0, 21)])
        .unwrap();
    store
        .record(&personal, at(21, 23), &[daily(90.0, 100.0, 21)])
        .unwrap();
    let accounts = [account(), personal.clone()];
    let digest = weekly(
        &store,
        &accounts,
        &[None, None],
        DigestWeek::Last,
        at(26, 9),
        &Utc,
        DaysOff::default(),
    )
    .unwrap();
    assert_eq!(digest.accounts[1].account_id, personal.state_key());
    assert_eq!(digest.retain_accounts(|_| true).as_ref(), Some(&digest));

    // Personal alone: its heaviest day and its busiest hours, not Work's.
    let alone = digest
        .retain_accounts(|id| id == personal.state_key())
        .unwrap();
    assert_eq!(alone.accounts.len(), 1);
    assert_eq!(alone.heaviest.map(|d| d.day), Some(at(21, 0).date_naive()));
    assert_eq!(alone.busiest_hours, digest.accounts[1].busiest_hours);
    assert_ne!(alone.busiest_hours, digest.busiest_hours);
    assert_eq!(alone.hits, 0);
    // Work alone: Thursday's hit and its 100% day.
    let work = digest
        .retain_accounts(|id| id == account().state_key())
        .unwrap();
    assert_eq!(work.hits, 1);
    assert_eq!(work.heaviest.map(|d| d.day), Some(at(22, 0).date_naive()));
}

#[test]
fn overall_takes_each_days_highest_peak_and_sums_the_hours() {
    let share = |day: u32, share: f64| DayShare {
        day: at(day, 0).date_naive(),
        share,
    };
    let mut one_hours = [0.0; 24];
    one_hours[8] = 10.0;
    let mut two_hours = [0.0; 24];
    two_hours[8] = 5.0;
    two_hours[15] = 12.0;
    let base = DigestAccount {
        name: String::new(),
        provider: crate::model::Provider::Claude,
        hits: 1,
        average_peak: Some(0.5),
        days_at_limit: 0,
        heaviest: None,
        lightest: None,
        busiest_hours: None,
        limit_changes: Vec::new(),
        budgets: Vec::new(),
        allowances: Vec::new(),
        account_id: "a".into(),
        days: vec![share(19, 0.2), share(20, 0.9)],
        hours: one_hours,
        extra_spent: Some(vec![ExtraSpent {
            unit: "USD".into(),
            amount: 2.0,
        }]),
    };
    let other = DigestAccount {
        account_id: "b".into(),
        days: vec![share(19, 0.6), share(21, 0.1)],
        hours: two_hours,
        ..base.clone()
    };
    let both = overall(&[base, other]);
    assert_eq!(both.hits, 2);
    assert_eq!(both.heaviest, Some(share(20, 0.9)));
    assert_eq!(both.lightest, Some(share(21, 0.1)));
    // 08:00 holds 15 points; 15:00 holds 12 (the band 06–09 or 08–11 wins).
    let (from, _) = both.busiest_hours.unwrap();
    assert!((6..=8).contains(&from), "{from}");
    let extra = both.extra_spent.unwrap();
    assert_eq!((extra.len(), extra[0].amount), (1, 4.0));
    assert!(both.recorded);
}
