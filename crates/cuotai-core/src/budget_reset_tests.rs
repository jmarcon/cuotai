//! PRD 0041 re-validation, design D1 (F-01, F-16): a budget counts across
//! the provider's reset, and a period with an unrecorded stretch gives a
//! lower bound (`derived_from.partial`), never a value that looks exact.

use super::*;
use crate::history::HistoryStore;
use crate::model::Status;
use chrono::TimeZone;
use chrono_tz::America::Sao_Paulo;

fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
}

fn spend(used: f64, window: Option<MetricWindow>, resets_at: Option<DateTime<Utc>>) -> Metric {
    Metric {
        label: "Gasto no mês".into(),
        used: Some(used),
        limit: None,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window,
        period: None,
        resets_at,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn monthly(used: f64, resets_at: DateTime<Utc>) -> Metric {
    spend(used, Some(MetricWindow::Monthly), Some(resets_at))
}

fn balance(left: f64) -> Metric {
    Metric {
        label: "Saldo de créditos".into(),
        used: None,
        limit: None,
        remaining: Some(left),
        unit: "USD".into(),
        kind: MetricKind::Credits,
        window: None,
        period: None,
        resets_at: None,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn report(metrics: Vec<Metric>, checked_at: DateTime<Utc>) -> Report {
    Report {
        account: "Work".into(),
        account_id: None,
        provider: Provider::OpenAiApi,
        checked_at,
        status: Status::Ready,
        metrics,
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        diagnostic: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
        breakdown: Vec::new(),
    }
}

fn budget(kind: BudgetKind, amount: f64, period: BudgetPeriod) -> Budget {
    Budget {
        kind: Some(kind),
        scope: None,
        label_contains: None,
        amount,
        unit: Some("USD".into()),
        period,
        starts: None,
        label: None,
        row: None,
    }
}

fn account(budgets: Vec<Budget>) -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets,
        ..Default::default()
    }
}

fn note_ids(report: &Report) -> Vec<&str> {
    report
        .notes_l10n
        .iter()
        .map(|note| note.id.as_ref())
        .collect()
}

fn derived(report: &Report) -> &Metric {
    report
        .metrics
        .iter()
        .find(|metric| metric.derived_from.is_some())
        .expect("a budget metric")
}

fn partial(metric: &Metric) -> bool {
    metric
        .derived_from
        .as_ref()
        .is_some_and(|from| from.partial)
}

fn sample(taken_at: DateTime<Utc>, value: f64, resets_at: Option<DateTime<Utc>>) -> Reading {
    Reading {
        taken_at,
        value,
        resets_at,
    }
}

/// A temporary history store holding `metrics` (each recorded at its
/// time), as `Monitor::apply_budgets` reads it.
fn recorded(
    dir: &std::path::Path,
    account: &Account,
    metrics: &[(DateTime<Utc>, Metric)],
) -> HistoryStore {
    let store = HistoryStore::new(
        dir.join("history.sqlite3"),
        &crate::config_file::ConfigIdentity::Explicit("budget-reset-tests".into()),
    );
    for (taken_at, metric) in metrics {
        assert_eq!(
            store.record(account, *taken_at, std::slice::from_ref(metric)),
            Ok(1)
        );
    }
    store
}

/// The baselines `Monitor::apply_budgets` gives: the store's readings of
/// the target from `since` up to `now`.
fn read_back(
    store: &HistoryStore,
    account: &Account,
    now: DateTime<Utc>,
) -> impl Fn(&Metric, DateTime<Utc>) -> Vec<Reading> {
    let (store, account) = (store.clone(), account.clone());
    move |target: &Metric, since: DateTime<Utc>| {
        store
            .readings(&account, &crate::history::metric_key(target), since..=now)
            .unwrap()
    }
}

// 1. The review's reproduction (S-01): a weekly budget across the monthly
//    reset of the provider's spend.
#[test]
fn a_weekly_budget_keeps_counting_across_the_providers_monthly_reset() {
    let now = at(2026, 11, 1, 12, 0); // a Sunday: the week began on 10-26
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let (october, november) = (at(2026, 11, 1, 0, 0), at(2026, 12, 1, 0, 0));
    let dir = tempfile::tempdir().unwrap();
    let store = recorded(
        dir.path(),
        &account,
        &[
            (at(2026, 10, 26, 0, 20), monthly(100.0, october)),
            (at(2026, 10, 31, 23, 0), monthly(140.0, october)),
            (at(2026, 11, 1, 1, 0), monthly(2.0, november)),
        ],
    );
    let mut reading = report(vec![monthly(5.0, november)], now);
    apply(
        &account,
        &mut reading,
        &read_back(&store, &account, now),
        now,
        &Utc,
    );
    let metric = derived(&reading);
    // 40 before the reset, the 1 h tail is within GAP, then 5 after it.
    assert_eq!(metric.used, Some(45.0));
    assert_eq!(metric.remaining, Some(5.0));
    assert!(!partial(metric));
    assert!(note_ids(&reading).is_empty(), "{:?}", note_ids(&reading));
    assert!(crate::pace::allowance(metric, now, None).is_some());
}

// 2. A daily budget in São Paulo (UTC−3) across the provider's UTC reset,
//    which falls at 21:00 local.
#[test]
fn a_local_daily_budget_counts_across_a_utc_reset_in_the_evening() {
    let now = at(2026, 11, 1, 2, 0); // 23:00 local on 10-31
    let account = account(vec![budget(BudgetKind::Spend, 10.0, BudgetPeriod::Daily)]);
    let (october, november) = (at(2026, 11, 1, 0, 0), at(2026, 12, 1, 0, 0));
    let history = |_: &Metric, since: DateTime<Utc>| {
        assert_eq!(since, at(2026, 10, 31, 3, 0)); // local midnight
        vec![
            sample(at(2026, 10, 31, 3, 10), 140.0, Some(october)), // 00:10
            sample(at(2026, 10, 31, 23, 50), 144.0, Some(october)), // 20:50
        ]
    };
    let mut reading = report(vec![monthly(0.5, november)], now);
    apply(&account, &mut reading, &history, now, &Sao_Paulo);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(4.5));
    assert_eq!(metric.remaining, Some(5.5));
    assert!(!partial(metric));
    assert!(note_ids(&reading).is_empty(), "{:?}", note_ids(&reading));
}

// 3. No reading near the reset: the use before it is unknown, so the
//    result is a lower bound with no remaining and no allowance.
#[test]
fn a_gap_across_the_reset_gives_a_lower_bound() {
    let now = at(2026, 11, 1, 12, 0);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let (october, november) = (at(2026, 11, 1, 0, 0), at(2026, 12, 1, 0, 0));
    let history = |_: &Metric, _: DateTime<Utc>| {
        vec![
            sample(at(2026, 10, 26, 0, 20), 100.0, Some(october)),
            sample(at(2026, 10, 29, 12, 0), 120.0, Some(october)),
            sample(at(2026, 11, 1, 9, 0), 3.0, Some(november)),
        ]
    };
    let mut reading = report(vec![monthly(5.0, november)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(25.0));
    assert_eq!(metric.remaining, None);
    assert!(partial(metric));
    assert_eq!(note_ids(&reading), ["note.budget.partial"]);
    assert_eq!(
        reading.notes_l10n[0].fallback,
        "Weekly budget · Spending in the month: part of this period's use was not recorded, so at least this much was used."
    );
    assert!(crate::pace::allowance(metric, now, None).is_none());
    // A lower bound that crossed a threshold has truly crossed it.
    assert!(crate::alerts::eligible(metric));
    // The export says so.
    let json = serde_json::to_value(metric).unwrap();
    assert_eq!(json["derived_from"]["partial"], true);
}

// 4. A daily provider window resets every night inside a weekly budget.
#[test]
fn several_resets_in_one_period_add_up_day_by_day() {
    let now = at(2026, 10, 29, 12, 0); // Thursday
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let daily = |used: f64, resets_at: DateTime<Utc>| {
        spend(used, Some(MetricWindow::Daily), Some(resets_at))
    };
    let midnight = |day: u32| at(2026, 10, day, 0, 0);
    let history = |_: &Metric, _: DateTime<Utc>| {
        vec![
            sample(at(2026, 10, 26, 0, 15), 0.5, Some(midnight(27))),
            sample(at(2026, 10, 26, 23, 45), 10.0, Some(midnight(27))),
            sample(at(2026, 10, 27, 23, 45), 7.0, Some(midnight(28))),
            sample(at(2026, 10, 28, 23, 45), 4.0, Some(midnight(29))),
        ]
    };
    let mut reading = report(vec![daily(2.0, midnight(30))], now);
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    // 9.5 on Monday after the baseline, then 7, 4 and 2 so far today.
    assert_eq!(metric.used, Some(22.5));
    assert!(!partial(metric));
    assert!(note_ids(&reading).is_empty(), "{:?}", note_ids(&reading));

    // Reading only once a day at noon: each night's tail is unknown.
    let sparse = |_: &Metric, _: DateTime<Utc>| {
        vec![
            sample(at(2026, 10, 26, 0, 15), 0.5, Some(midnight(27))),
            sample(at(2026, 10, 27, 12, 0), 3.0, Some(midnight(28))),
        ]
    };
    let mut reading = report(vec![daily(2.0, midnight(30))], now);
    apply(&account, &mut reading, &sparse, now, &Utc);
    let metric = derived(&reading);
    assert!(partial(metric));
    assert_eq!(metric.remaining, None);
}

// 5. A balance: drops count, a top-up does not; a top-up right after a
//    reading is exact, the same top-up after a 3-day gap is a lower bound.
#[test]
fn a_balance_top_up_is_exact_within_the_gap_and_partial_across_days() {
    let now = at(2026, 10, 21, 12, 0);
    let account = account(vec![budget(
        BudgetKind::Credits,
        20.0,
        BudgetPeriod::Monthly,
    )]);
    let dir = tempfile::tempdir().unwrap();
    let store = recorded(
        dir.path(),
        &account,
        &[
            (at(2026, 10, 1, 0, 30), balance(30.0)),
            (at(2026, 10, 10, 10, 0), balance(22.0)),
            (at(2026, 10, 10, 10, 30), balance(50.0)),
        ],
    );
    let mut reading = report(vec![balance(47.0)], now);
    apply(
        &account,
        &mut reading,
        &read_back(&store, &account, now),
        now,
        &Utc,
    );
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(11.0));
    assert_eq!(metric.remaining, Some(9.0));
    assert!(!partial(metric));
    assert_eq!(note_ids(&reading), ["note.budget.topup_detected"]);

    let gap = tempfile::tempdir().unwrap();
    let store = recorded(
        gap.path(),
        &account,
        &[
            (at(2026, 10, 1, 0, 30), balance(30.0)),
            (at(2026, 10, 10, 10, 0), balance(22.0)),
            (at(2026, 10, 13, 10, 0), balance(50.0)),
        ],
    );
    let mut reading = report(vec![balance(47.0)], now);
    apply(
        &account,
        &mut reading,
        &read_back(&store, &account, now),
        now,
        &Utc,
    );
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(11.0));
    assert_eq!(metric.remaining, None);
    assert!(partial(metric));
    assert_eq!(
        note_ids(&reading),
        ["note.budget.partial", "note.budget.topup_detected"]
    );
}

// 6. A counter that goes down inside one provider instance (a refund or a
//    revised estimate) is not monotonic: the result is a lower bound.
#[test]
fn a_revised_estimate_inside_one_instance_is_partial() {
    let now = at(2026, 10, 28, 12, 0);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let october = at(2026, 11, 1, 0, 0);
    let history = |_: &Metric, _: DateTime<Utc>| {
        vec![
            sample(at(2026, 10, 26, 0, 20), 100.0, Some(october)),
            sample(at(2026, 10, 27, 9, 0), 90.0, Some(october)),
        ]
    };
    let mut reading = report(vec![monthly(95.0, october)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(5.0));
    assert_eq!(metric.remaining, None);
    assert!(partial(metric));
    assert_eq!(note_ids(&reading), ["note.budget.partial"]);
}

// 7. RF2.2: without a reading at the period start, the use before the
//    first one is unknown.
#[test]
fn a_first_reading_days_after_the_start_is_partial() {
    let now = at(2026, 10, 29, 12, 0);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let october = at(2026, 11, 1, 0, 0);
    let history =
        |_: &Metric, _: DateTime<Utc>| vec![sample(at(2026, 10, 28, 0, 0), 100.0, Some(october))];
    let mut reading = report(vec![monthly(110.0, october)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(10.0));
    assert_eq!(metric.remaining, None);
    assert!(partial(metric));
    assert_eq!(note_ids(&reading), ["note.budget.partial"]);
}

// 8. F-16: a negative provider spend (a net credit) is unknown, never a
//    negative use with more than the budget left.
#[test]
fn a_negative_provider_spend_is_unknown_with_a_note() {
    let now = at(2026, 10, 21, 12, 0);
    for period in [BudgetPeriod::Monthly, BudgetPeriod::Window] {
        let account = account(vec![budget(BudgetKind::Spend, 50.0, period)]);
        let mut reading = report(vec![monthly(-5.0, at(2026, 11, 1, 0, 0))], now);
        apply(&account, &mut reading, &NoHistory, now, &Utc);
        let metric = derived(&reading);
        assert_eq!(metric.used, None, "{period:?}");
        assert_eq!(metric.remaining, None, "{period:?}");
        assert_eq!(note_ids(&reading), ["note.budget.negative"], "{period:?}");
        assert!(!crate::alerts::eligible(metric));
    }
}

// 9. A sample written while the clock was ahead is not this period's.
#[test]
fn a_sample_after_now_is_ignored() {
    let now = at(2026, 10, 28, 12, 0);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let october = at(2026, 11, 1, 0, 0);
    let history = |_: &Metric, _: DateTime<Utc>| {
        vec![
            sample(at(2026, 10, 26, 0, 20), 100.0, Some(october)),
            sample(at(2026, 10, 30, 0, 0), 1000.0, Some(october)),
        ]
    };
    let mut reading = report(vec![monthly(110.0, october)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(10.0));
    assert!(!partial(metric));
    assert!(note_ids(&reading).is_empty(), "{:?}", note_ids(&reading));
}

// 10. The store gives the budget what it needs: `resets_at`, a balance as
//     its `remaining`, and nothing after the range's end.
#[test]
fn the_history_reads_back_resets_and_balances_up_to_now() {
    let account = account(Vec::new());
    let now = at(2026, 10, 28, 12, 0);
    let october = at(2026, 11, 1, 0, 0);
    let dir = tempfile::tempdir().unwrap();
    let store = recorded(
        dir.path(),
        &account,
        &[
            (at(2026, 10, 26, 0, 20), monthly(100.0, october)),
            (at(2026, 10, 27, 0, 0), balance(12.5)),
            (at(2026, 10, 28, 6, 0), monthly(105.0, october)),
            (at(2026, 10, 29, 0, 0), monthly(1000.0, october)),
        ],
    );
    let spend_key = crate::history::metric_key(&monthly(0.0, october));
    let readings = store
        .readings(&account, &spend_key, at(2026, 10, 26, 0, 0)..=now)
        .unwrap();
    assert_eq!(
        readings,
        [
            sample(at(2026, 10, 26, 0, 20), 100.0, Some(october)),
            sample(at(2026, 10, 28, 6, 0), 105.0, Some(october)),
        ]
    );
    let balance_key = crate::history::metric_key(&balance(0.0));
    let readings = store
        .readings(&account, &balance_key, at(2026, 10, 26, 0, 0)..=now)
        .unwrap();
    assert_eq!(readings, [sample(at(2026, 10, 27, 0, 0), 12.5, None)]);
    // `series` is the same read without the resets.
    let points = store
        .series(&account, &spend_key, at(2026, 10, 26, 0, 0)..=now)
        .unwrap();
    assert_eq!(
        points.iter().map(|point| point.used).collect::<Vec<_>>(),
        [100.0, 105.0]
    );
}

// Unverified in the review (Security): a sum that overflows is unknown.
#[test]
fn a_sum_that_is_not_finite_is_unknown() {
    let now = at(2026, 11, 1, 12, 0);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly)]);
    let (october, november) = (at(2026, 11, 1, 0, 0), at(2026, 12, 1, 0, 0));
    let history = |_: &Metric, _: DateTime<Utc>| {
        vec![
            sample(at(2026, 10, 26, 0, 20), 0.0, Some(october)),
            sample(at(2026, 10, 31, 23, 30), 1.7e308, Some(october)),
        ]
    };
    let mut reading = report(vec![monthly(1.7e308, november)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, None);
    assert_eq!(metric.remaining, None);
    assert_eq!(note_ids(&reading), ["note.budget.target_missing"]);
}
