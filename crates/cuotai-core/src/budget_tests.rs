use super::*;
use crate::model::Status;
use chrono::TimeZone;
use chrono_tz::{America::New_York, America::Santiago, Europe::Berlin};

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
    }
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

// ---- RF2.1 period boundaries ------------------------------------------

#[test]
fn daily_weekly_and_monthly_periods_start_at_local_midnight() {
    let now = at(2026, 10, 21, 15, 0); // a Wednesday
    let (start, end) = period_bounds(BudgetPeriod::Daily, None, now, &Utc).unwrap();
    assert_eq!(
        (start, end),
        (at(2026, 10, 21, 0, 0), at(2026, 10, 22, 0, 0))
    );
    let (start, end) = period_bounds(BudgetPeriod::Weekly, None, now, &Utc).unwrap();
    assert_eq!(
        (start, end),
        (at(2026, 10, 19, 0, 0), at(2026, 10, 26, 0, 0))
    );
    let friday = Some(BudgetStart::Weekday(WeekdayName::Friday));
    let (start, _) = period_bounds(BudgetPeriod::Weekly, friday, now, &Utc).unwrap();
    assert_eq!(start, at(2026, 10, 16, 0, 0));
    let (start, end) = period_bounds(BudgetPeriod::Monthly, None, now, &Utc).unwrap();
    assert_eq!((start, end), (at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0)));
    let day = Some(BudgetStart::Day(25));
    let (start, end) = period_bounds(BudgetPeriod::Monthly, day, now, &Utc).unwrap();
    assert_eq!(
        (start, end),
        (at(2026, 9, 25, 0, 0), at(2026, 10, 25, 0, 0))
    );
    assert!(period_bounds(BudgetPeriod::Window, None, now, &Utc).is_none());
}

#[test]
fn boundaries_follow_dst_in_new_york_and_berlin() {
    // New York springs forward on 2026-03-08: that day has 23 hours.
    let now = at(2026, 3, 8, 18, 0);
    let (start, end) = period_bounds(BudgetPeriod::Daily, None, now, &New_York).unwrap();
    assert_eq!(start, at(2026, 3, 8, 5, 0)); // 00:00 EST
    assert_eq!(end, at(2026, 3, 9, 4, 0)); // 00:00 EDT
    assert_eq!(end - start, TimeDelta::hours(23));
    // Berlin falls back on 2026-10-25: the week holding it has 169 hours.
    let now = at(2026, 10, 24, 12, 0);
    let (start, end) = period_bounds(BudgetPeriod::Weekly, None, now, &Berlin).unwrap();
    assert_eq!(start, at(2026, 10, 18, 22, 0)); // Monday 00:00 CEST
    assert_eq!(end, at(2026, 10, 25, 23, 0)); // Monday 00:00 CET
    assert_eq!(end - start, TimeDelta::hours(169));
    // Monthly in Berlin: November starts at 23:00 UTC on October 31.
    let (_, end) = period_bounds(BudgetPeriod::Monthly, None, now, &Berlin).unwrap();
    assert_eq!(end, at(2026, 10, 31, 23, 0));
}

#[test]
fn a_midnight_skipped_by_dst_moves_to_the_first_local_time() {
    // Santiago springs forward at 24:00 on 2026-09-05: 2026-09-06 00:00
    // does not exist and the day starts at 01:00 local (04:00 UTC).
    let now = at(2026, 9, 6, 15, 0);
    let (start, end) = period_bounds(BudgetPeriod::Daily, None, now, &Santiago).unwrap();
    assert_eq!(start, at(2026, 9, 6, 4, 0));
    assert_eq!(end, at(2026, 9, 7, 3, 0));
}

// ---- RF2.2 / RF2.3 used --------------------------------------------------

#[test]
fn a_provider_metric_of_the_same_period_gives_its_own_used() {
    let now = at(2026, 10, 21, 12, 0);
    let target = spend(
        45.0,
        Some(MetricWindow::Monthly),
        Some(at(2026, 11, 1, 0, 0)),
    );
    let mut reading = report(vec![target], now);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(45.0));
    assert_eq!(metric.limit, Some(50.0));
    assert_eq!(metric.remaining, Some(5.0));
    assert_eq!(metric.kind, MetricKind::Quota);
    assert_eq!(metric.window, Some(MetricWindow::Monthly));
    assert_eq!(metric.resets_at, Some(at(2026, 11, 1, 0, 0)));
    assert_eq!(metric.label, "Orçamento · Gasto no mês");
    assert_eq!(
        crate::metric_text::metric_text(&metric.label).fallback,
        "Budget · Spending in the month"
    );
    assert!(note_ids(&reading).is_empty(), "{:?}", note_ids(&reading));
    // Applying again changes nothing.
    let before = reading.metrics.len();
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    assert_eq!(reading.metrics.len(), before);
}

#[test]
fn a_utc_provider_month_counts_for_a_local_month_with_a_note() {
    let now = at(2026, 10, 21, 12, 0);
    let target = spend(
        10.0,
        Some(MetricWindow::Monthly),
        Some(at(2026, 11, 1, 0, 0)),
    );
    let mut reading = report(vec![target], now);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    apply(&account, &mut reading, &NoHistory, now, &New_York);
    assert_eq!(derived(&reading).used, Some(10.0));
    assert_eq!(note_ids(&reading), ["note.budget.provider_month"]);
}

#[test]
fn another_period_subtracts_the_first_sample_of_the_period() {
    let now = at(2026, 10, 21, 12, 0);
    let target = spend(
        45.0,
        Some(MetricWindow::Monthly),
        Some(at(2026, 11, 1, 0, 0)),
    );
    let mut reading = report(vec![target], now);
    let account = account(vec![budget(BudgetKind::Spend, 10.0, BudgetPeriod::Daily)]);
    let history = |_: &Metric, since: DateTime<Utc>| {
        assert_eq!(since, at(2026, 10, 21, 0, 0));
        vec![
            Point {
                taken_at: at(2026, 10, 21, 0, 5),
                used: 41.0,
            },
            Point {
                taken_at: at(2026, 10, 21, 6, 0),
                used: 43.0,
            },
        ]
    };
    apply(&account, &mut reading, &history, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(4.0));
    assert_eq!(metric.window, Some(MetricWindow::Daily));
}

#[test]
fn without_a_baseline_used_is_unknown_with_a_note_never_zero() {
    let now = at(2026, 10, 21, 12, 0);
    let mut reading = report(vec![spend(45.0, None, None)], now);
    let account = account(vec![budget(BudgetKind::Spend, 10.0, BudgetPeriod::Weekly)]);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, None);
    assert_eq!(metric.remaining, None);
    assert_eq!(note_ids(&reading), ["note.budget.no_baseline"]);
    assert!(!crate::alerts::eligible(metric), "never alerts");
}

#[test]
fn a_balance_counts_its_drops_and_a_top_up_is_not_use() {
    let now = at(2026, 10, 21, 12, 0);
    let account = account(vec![budget(
        BudgetKind::Credits,
        20.0,
        BudgetPeriod::Monthly,
    )]);
    let samples = vec![
        Point {
            taken_at: at(2026, 10, 1, 1, 0),
            used: 30.0,
        },
        Point {
            taken_at: at(2026, 10, 10, 1, 0),
            used: 22.0,
        },
    ];
    let history = |_: &Metric, _: DateTime<Utc>| samples.clone();
    let mut reading = report(vec![balance(18.0)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    assert_eq!(derived(&reading).used, Some(12.0));
    assert!(note_ids(&reading).is_empty());

    // A top-up to 50 after 22: the 8 before it still count, the rise not.
    let topped = vec![
        samples[0],
        samples[1],
        Point {
            taken_at: at(2026, 10, 15, 1, 0),
            used: 50.0,
        },
    ];
    let history = |_: &Metric, _: DateTime<Utc>| topped.clone();
    let mut reading = report(vec![balance(47.0)], now);
    apply(&account, &mut reading, &history, now, &Utc);
    assert_eq!(derived(&reading).used, Some(11.0));
    assert_eq!(note_ids(&reading), ["note.budget.topup_detected"]);
}

#[test]
fn a_missing_target_or_unit_never_shows_zero() {
    let now = at(2026, 10, 21, 12, 0);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    let mut reading = report(vec![balance(5.0)], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    assert_eq!(derived(&reading).used, None);
    assert_eq!(note_ids(&reading), ["note.budget.target_missing"]);

    // The unit filter selects nothing: the target is missing too.
    let mut eur = budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly);
    eur.unit = Some("EUR".into());
    let mut reading = report(vec![spend(10.0, None, None)], now);
    apply(
        &super::tests::account(vec![eur]),
        &mut reading,
        &NoHistory,
        now,
        &Utc,
    );
    assert_eq!(derived(&reading).used, None);

    // A failed reading gets no budget metric.
    let mut failed = report(vec![], now);
    failed.status = Status::Error;
    apply(&account, &mut failed, &NoHistory, now, &Utc);
    assert!(failed.metrics.is_empty());
}

#[test]
fn window_budgets_follow_the_providers_reset() {
    let now = at(2026, 10, 21, 12, 0);
    let target = spend(
        3.0,
        Some(MetricWindow::Weekly),
        Some(at(2026, 10, 23, 7, 0)),
    );
    let account = account(vec![budget(BudgetKind::Spend, 10.0, BudgetPeriod::Window)]);
    let mut reading = report(vec![target], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    let metric = derived(&reading);
    assert_eq!(metric.used, Some(3.0));
    assert_eq!(metric.resets_at, Some(at(2026, 10, 23, 7, 0)));
    assert_eq!(metric.window, Some(MetricWindow::Weekly));

    let mut reading = report(vec![spend(3.0, None, None)], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    assert_eq!(derived(&reading).used, None);
    assert_eq!(note_ids(&reading), ["note.budget.no_window"]);
}

#[test]
fn restored_readings_mark_their_budget_metrics_again() {
    let now = at(2026, 10, 21, 12, 0);
    let target = spend(
        45.0,
        Some(MetricWindow::Monthly),
        Some(at(2026, 11, 1, 0, 0)),
    );
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    let mut reading = report(vec![target], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    for metric in &mut reading.metrics {
        metric.derived_from = None;
    }
    mark_restored(&account, &mut reading);
    assert_eq!(
        reading
            .metrics
            .iter()
            .filter(|metric| metric.derived_from.is_some())
            .count(),
        1
    );
}

// ---- RF2 end to end ---------------------------------------------------------

#[test]
fn a_monthly_budget_at_ninety_percent_alerts_and_paces() {
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    let resets = at(2026, 11, 1, 0, 0);
    let prev_at = at(2026, 10, 20, 11, 55);
    let now = at(2026, 10, 20, 12, 0);
    let mut prev = report(
        vec![spend(40.0, Some(MetricWindow::Monthly), Some(resets))],
        prev_at,
    );
    let mut next = report(
        vec![spend(45.0, Some(MetricWindow::Monthly), Some(resets))],
        now,
    );
    apply(&account, &mut prev, &NoHistory, prev_at, &Utc);
    apply(&account, &mut next, &NoHistory, now, &Utc);
    let prefs = crate::alerts::AlertPrefs::new(true, crate::alerts::DEFAULT_THRESHOLDS).unwrap();
    let mut state = crate::alerts::AlertState::default();
    // A first reading in this state only sets the baseline.
    state.update(&prev, &prefs, &[], prev_at);
    let events = crate::alerts::evaluate(Some(&prev), &next, &prefs, &state, now);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].kind, crate::alerts::AlertKind::Threshold);
    assert_eq!(events[0].threshold, 90);
    assert_eq!(events[0].label, "Orçamento · Gasto no mês");

    // 45 of 50 in 19.5 of 31 days runs out before the 1st (≈ the 22nd).
    let pace = crate::pace::assess(derived(&next), now, None).expect("a pace");
    let out = pace.projected_exhaustion_at.expect("runs out");
    assert!(out < resets && out > now, "{out}");
    let chip = pace.outlook_chip(|at| at.format("%d").to_string()).unwrap();
    assert_eq!(chip.text.id, "pace.chip.runs_out");

    // RF4: ≈ $0.43/day left in the budget (5 dollars over 11.5 days).
    let allowance = crate::pace::allowance(derived(&next), now, None).unwrap();
    assert_eq!(allowance.per, crate::pace::AllowancePer::Day);
    assert!((allowance.amount - 5.0 / 11.5).abs() < 1e-9);
    assert_eq!(allowance.sentence().id, "pace.allowance.budget_day");
    assert_eq!(
        allowance.sentence().fallback,
        "≈ $0.43/day left in the budget"
    );
}

// ---- RF1.2 validation -------------------------------------------------------

#[test]
fn validation_rejects_each_rule_with_its_field() {
    let ok = budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly);
    let check = |budgets: Vec<Budget>, provider: Provider| {
        problems(provider, &budgets, &[])
            .into_iter()
            .map(|problem| (problem.field, problem.rule))
            .collect::<Vec<_>>()
    };
    assert!(check(vec![ok.clone()], Provider::OpenAiApi).is_empty());
    let with = |edit: fn(&mut Budget)| {
        let mut budget = ok.clone();
        edit(&mut budget);
        budget
    };
    let cases: Vec<(Budget, &str, BudgetRule)> = vec![
        (
            with(|b| b.kind = None),
            "budgets[0].kind",
            BudgetRule::Selector,
        ),
        (
            with(|b| b.scope = Some(BudgetScope::Account)),
            "budgets[0].kind",
            BudgetRule::Selector,
        ),
        (
            with(|b| b.amount = 0.0),
            "budgets[0].amount",
            BudgetRule::Amount,
        ),
        (
            with(|b| b.amount = 2e9),
            "budgets[0].amount",
            BudgetRule::Amount,
        ),
        (
            with(|b| b.amount = f64::NAN),
            "budgets[0].amount",
            BudgetRule::Amount,
        ),
        (
            with(|b| b.unit = Some(" ".into())),
            "budgets[0].unit",
            BudgetRule::Unit,
        ),
        (
            with(|b| b.label = Some("x".repeat(33))),
            "budgets[0].label",
            BudgetRule::Label,
        ),
        (
            with(|b| b.starts = Some(BudgetStart::Day(29))),
            "budgets[0].starts",
            BudgetRule::Start,
        ),
        (
            with(|b| b.starts = Some(BudgetStart::Weekday(WeekdayName::Monday))),
            "budgets[0].starts",
            BudgetRule::Start,
        ),
    ];
    for (budget, field, rule) in cases {
        assert_eq!(
            check(vec![budget], Provider::OpenAiApi),
            vec![(field.to_owned(), rule)],
            "{field} {rule:?}"
        );
    }
    assert_eq!(
        check(vec![ok.clone(), ok.clone()], Provider::OpenAiApi),
        vec![("budgets[1].period".to_owned(), BudgetRule::Duplicate)]
    );
    assert_eq!(
        check(vec![ok.clone(); 9], Provider::OpenAiApi)[0],
        ("budgets".to_owned(), BudgetRule::TooMany)
    );
    // A provider whose shapes are known: no spend on JetBrains AI.
    assert_eq!(
        check(vec![ok.clone()], Provider::JetbrainsAi),
        vec![("budgets[0].kind".to_owned(), BudgetRule::KindNotReported)]
    );
    // period = "window" on a provider with only resetless balances is
    // impossible; unknown shapes are accepted.
    let window = with(|b| {
        b.kind = Some(BudgetKind::Credits);
        b.period = BudgetPeriod::Window;
    });
    assert_eq!(
        check(vec![window.clone()], Provider::JetbrainsAi)[0].1,
        BudgetRule::KindNotReported
    );
    assert!(check(vec![window], Provider::OpenAiApi).is_empty());

    let floor = Floor {
        kind: Some(FloorKind::Balance),
        label_contains: None,
        amount: -1.0,
        unit: None,
    };
    let floors = problems(Provider::OpenAiApi, &[], &vec![floor; 5]);
    assert_eq!(floors[0].rule, BudgetRule::TooMany);
    assert_eq!(floors[1].field, "floors[0].amount");
    // Every rule has a localized text naming the field.
    let note = BudgetRule::Amount.note("accounts[0].budgets[0].amount");
    assert_eq!(note.id, "budget.error.amount");
    assert!(note
        .resolve("pt-BR")
        .contains("accounts[0].budgets[0].amount"));
}

#[test]
fn the_editor_sentence_and_reference_line() {
    let mut weekly = budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly);
    assert_eq!(
        describe(&weekly, "USD").resolve("en"),
        "Cuotai will show a Budget quota of $50.00 a month, starting on day 1."
    );
    weekly.period = BudgetPeriod::Weekly;
    weekly.starts = Some(BudgetStart::Weekday(WeekdayName::Friday));
    assert_eq!(
        describe(&weekly, "USD").resolve("en"),
        "Cuotai will show a Budget quota of $50.00 a week, starting on Friday."
    );
    let points = [(1, 10.0), (10, 30.0), (12, 2.0), (20, 8.2)].map(|(day, used)| Point {
        taken_at: at(2026, 10, day, 0, 0),
        used,
    });
    // 20 before the reset, then 2 and 6.2 after it.
    assert_eq!(
        recent_reference(&points, "USD").unwrap().resolve("en"),
        "Last 30 days: $28.20"
    );
    assert!(recent_reference(&points[..1], "USD").is_none());
}

#[test]
fn quick_actions_offer_a_budget_or_floor_only_where_none_exists() {
    let now = at(2026, 10, 21, 12, 0);
    let reading = report(vec![spend(10.0, None, None), balance(5.0)], now);
    let none = account(Vec::new());
    assert!(offers_budget(&none, &reading, &reading.metrics[0]));
    // Credits are both: a budget on use and a floor on the balance.
    assert!(offers_budget(&none, &reading, &reading.metrics[1]));
    assert!(offers_floor(&none, &reading, &reading.metrics[1]));
    assert!(!offers_floor(&none, &reading, &reading.metrics[0]));
    let with = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    assert!(!offers_budget(&with, &reading, &reading.metrics[0]));
    let draft = draft_for(&reading.metrics[0]);
    assert_eq!(draft.kind, Some(BudgetKind::Spend));
    assert_eq!(draft.unit.as_deref(), Some("USD"));
}

// ---- RF4 allowance -------------------------------------------------------

fn quota(used: f64, window: MetricWindow, resets_at: DateTime<Utc>) -> Metric {
    Metric {
        label: "Semanal".into(),
        used: Some(used),
        limit: Some(100.0),
        remaining: None,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(window),
        period: None,
        resets_at: Some(resets_at),
        billing: Billing::Included,
        scope: None,
        derived_from: None,
    }
}

#[test]
fn allowance_per_day_switches_to_hours_under_two_days() {
    use crate::pace::{allowance, AllowancePer};
    let now = at(2026, 10, 21, 12, 0);
    // 6 days left, 40% used: 10%/day.
    let six = allowance(
        &quota(40.0, MetricWindow::Weekly, now + TimeDelta::days(6)),
        now,
        None,
    )
    .unwrap();
    assert_eq!(six.per, AllowancePer::Day);
    assert!((six.percent - 10.0).abs() < 1e-9);
    assert_eq!(six.sentence().resolve("en"), "≈ 10%/day to the reset");
    // 1 day left, 76% used: 1%/h.
    let one = allowance(
        &quota(76.0, MetricWindow::Weekly, now + TimeDelta::days(1)),
        now,
        None,
    )
    .unwrap();
    assert_eq!(one.per, AllowancePer::Hour);
    assert!((one.percent - 1.0).abs() < 1e-9);
    assert_eq!(one.sentence().resolve("en"), "≈ 1%/h to the reset");
    // 30 minutes left: the rest within the half hour, per hour.
    let soon = allowance(
        &quota(90.0, MetricWindow::Weekly, now + TimeDelta::minutes(30)),
        now,
        None,
    )
    .unwrap();
    assert!((soon.percent - 20.0).abs() < 1e-9);
    // No allowance: unknown reset, a 5-hour window, a rate limit, unknown use.
    let mut unknown = quota(10.0, MetricWindow::Weekly, now);
    unknown.resets_at = None;
    assert!(allowance(&unknown, now, None).is_none());
    assert!(allowance(
        &quota(10.0, MetricWindow::Window5h, now + TimeDelta::hours(3)),
        now,
        None
    )
    .is_none());
    let mut rate = quota(10.0, MetricWindow::Daily, now + TimeDelta::days(3));
    rate.kind = MetricKind::RateLimit;
    assert!(allowance(&rate, now, None).is_none());
    let mut unmeasured = quota(10.0, MetricWindow::Weekly, now + TimeDelta::days(3));
    unmeasured.used = None;
    assert!(allowance(&unmeasured, now, None).is_none());
}

#[test]
fn allowance_compares_with_the_recent_use_when_known() {
    use crate::pace::allowance;
    let now = at(2026, 10, 21, 12, 0);
    let metric = quota(40.0, MetricWindow::Weekly, now + TimeDelta::days(6));
    // 1% per hour over the last 3 hours: 24%/day against 10%/day.
    let recent: Vec<Point> = (0..4)
        .map(|h| Point {
            taken_at: now - TimeDelta::hours(3 - h),
            used: 37.0 + h as f64,
        })
        .collect();
    let with = allowance(&metric, now, Some(&recent)).unwrap();
    let ratio = with.vs_recent.unwrap();
    assert!((ratio - 2.4).abs() < 1e-6, "{ratio}");
    assert_eq!(
        with.sentence().resolve("en"),
        "≈ 10%/day to the reset · recently 24%/day"
    );
    assert!(allowance(&metric, now, Some(&[]))
        .unwrap()
        .vs_recent
        .is_none());
    // Every catalog words the four sentences.
    for locale in crate::text::catalog_locales() {
        for sentence in [
            with.sentence(),
            allowance(&metric, now, None).unwrap().sentence(),
        ] {
            assert_ne!(sentence.resolve(locale), sentence.fallback, "{locale}");
        }
    }
}

// ---- RF3 floors -----------------------------------------------------------

fn floor_prefs(amount: f64) -> crate::alerts::AlertPrefs {
    let mut prefs =
        crate::alerts::AlertPrefs::new(true, crate::alerts::DEFAULT_THRESHOLDS).unwrap();
    let account = Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        floors: vec![Floor {
            kind: Some(FloorKind::Credits),
            label_contains: None,
            amount,
            unit: None,
        }],
        ..Default::default()
    };
    prefs.floors.insert(
        report(vec![], at(2026, 1, 1, 0, 0)).state_key(),
        account.floors.clone(),
    );
    prefs
}

#[test]
fn a_floor_alerts_once_per_crossing_with_hysteresis_and_top_up() {
    use crate::alerts::{evaluate, AlertKind, AlertState};
    let prefs = floor_prefs(10.0);
    let mut state = AlertState::default();
    let step = |minutes: i64| at(2026, 10, 21, 12, 0) + TimeDelta::minutes(minutes);
    let mut prev = report(vec![balance(12.0)], step(0));
    state.update(&prev, &prefs, &[], step(0));
    let mut run = |left: f64, minute: i64, state: &mut AlertState| {
        let next = report(vec![balance(left)], step(minute));
        let events = evaluate(Some(&prev), &next, &prefs, state, step(minute));
        state.update(&next, &prefs, &events, step(minute));
        prev = next;
        events.into_iter().map(|e| e.kind).collect::<Vec<_>>()
    };
    assert_eq!(run(9.0, 5, &mut state), [AlertKind::BalanceBelowFloor]);
    assert!(run(8.0, 10, &mut state).is_empty(), "once per crossing");
    // A top-up (a rise beyond 10% of the floor) landing above it re-arms.
    assert_eq!(run(10.2, 15, &mut state), [AlertKind::BalanceRecovered]);
    assert_eq!(run(9.5, 20, &mut state), [AlertKind::BalanceBelowFloor]);
    // A small rise just above the floor (within 10%) is not a recovery…
    assert!(run(10.3, 25, &mut state).is_empty(), "hysteresis");
    // …and going back below it is not a new crossing…
    assert!(run(9.9, 30, &mut state).is_empty());
    // …but rising above 11 is, and re-arms the floor.
    assert_eq!(run(11.5, 35, &mut state), [AlertKind::BalanceRecovered]);
}

#[test]
fn floor_hysteresis_keeps_a_bouncing_balance_quiet() {
    use crate::alerts::{evaluate, AlertState};
    let prefs = floor_prefs(10.0);
    let mut state = AlertState::default();
    let step = |minutes: i64| at(2026, 10, 21, 12, 0) + TimeDelta::minutes(minutes);
    let mut prev = report(vec![balance(12.0)], step(0));
    state.update(&prev, &prefs, &[], step(0));
    let mut fired = 0;
    // Spending makes the balance only go down: 9.9 then down, then a
    // provider rounding back to 10.05 (a "rise") is a top-up above the
    // floor; a rise that stays below the floor never is.
    for (minute, left) in [(5, 9.9), (10, 9.8), (15, 9.85), (20, 9.7)] {
        let next = report(vec![balance(left)], step(minute));
        let events = evaluate(Some(&prev), &next, &prefs, &state, step(minute));
        fired += events.len();
        state.update(&next, &prefs, &events, step(minute));
        prev = next;
    }
    assert_eq!(fired, 1, "one crossing, no noise below the floor");
}

#[test]
fn floor_notices_follow_the_private_mode() {
    use crate::alerts::{notice_with as notice, AlertEvent, AlertKind};
    let account = account(Vec::new());
    let event = AlertEvent {
        account_key: account.state_key(),
        label: "Saldo de créditos".into(),
        window: None,
        kind: AlertKind::BalanceBelowFloor,
        threshold: 0,
        consumed: 0.1,
        resets_at: None,
        projection: None,
        overage: None,
        limit_change: None,
    };
    let now = at(2026, 10, 21, 12, 0);
    let private = notice(&[(&account, vec![event.clone()])], now, &Utc, true).unwrap();
    assert_eq!(
        private.title.resolve("en"),
        "OpenAI API: balance below your floor"
    );
    assert!(!private.body.resolve("en").contains("Work"));
    let open = notice(&[(&account, vec![event])], now, &Utc, false).unwrap();
    assert!(open.body.resolve("en").contains("Work"));
}
