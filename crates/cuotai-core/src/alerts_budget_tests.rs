//! PRD 0041 re-validation (D2, F-02, F-03, F-04): budget alerts keyed by
//! the budget's identity, and floors that warn per tier, through a small
//! scenario helper that feeds readings to the alert engine and keeps what
//! it told.

use super::*;
use crate::budget::{self, Budget, BudgetKind, BudgetPeriod, Floor, FloorKind};
use crate::model::{Billing, MetricKind, Provider};
use chrono::TimeZone;

fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap()
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

fn spend(used: f64) -> Metric {
    Metric {
        label: "Gasto no mês".into(),
        used: Some(used),
        limit: None,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()),
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
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
        unlimited: false,
    }
}

fn budget(period: BudgetPeriod) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount: 50.0,
        unit: Some("USD".into()),
        period,
        starts: None,
        label: None,
        row: None,
    }
}

fn floor(amount: f64) -> Floor {
    Floor {
        kind: Some(FloorKind::Credits),
        label_contains: None,
        amount,
        unit: None,
    }
}

/// The account `report` builds, with `budgets`.
fn account(budgets: Vec<Budget>) -> crate::Account {
    crate::Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets,
        ..Default::default()
    }
}

/// A spend reading of `account` at `at` with its budgets applied: the week
/// started at 0 (the weekly budget counts the whole spend).
fn budget_reading(account: &crate::Account, used: f64, at: DateTime<Utc>) -> Report {
    let mut reading = report(vec![spend(used)], at);
    let history = |_: &Metric, since: DateTime<Utc>| {
        vec![Point {
            taken_at: since + TimeDelta::minutes(1),
            used: 0.0,
        }]
    };
    budget::apply(account, &mut reading, &history, at, &Utc);
    reading
}

/// Feeds readings to the alert engine like a front end does: each reading
/// is evaluated against the previous one, then the state records it with
/// the events it produced (`claimed`: recorded with none, then each event
/// marked shown, as [`crate::resets::Ledger::claim_alerts`] and
/// `claim_shown` do).
struct Scenario {
    prefs: AlertPrefs,
    state: AlertState,
    prev: Option<Report>,
    claimed: bool,
}

impl Scenario {
    fn new() -> Self {
        Self {
            prefs: AlertPrefs::new(true, DEFAULT_THRESHOLDS).unwrap(),
            state: AlertState::default(),
            prev: None,
            claimed: false,
        }
    }

    fn with_floors(mut self, floors: Vec<Floor>) -> Self {
        self.set_floors(floors);
        self
    }

    fn set_floors(&mut self, floors: Vec<Floor>) {
        let key = report(Vec::new(), at(1, 0, 0)).state_key();
        self.prefs.floors.insert(key, floors);
    }

    fn feed(&mut self, next: Report) -> Vec<AlertEvent> {
        let now = next.checked_at;
        let events = evaluate(self.prev.as_ref(), &next, &self.prefs, &self.state, now);
        if self.claimed {
            self.state.update(&next, &self.prefs, &[], now);
            for event in &events {
                assert!(self.state.mark_shown(event, None, now), "{event:?}");
                if event.kind != AlertKind::BalanceRecovered {
                    assert!(self.state.notified(event), "{event:?}");
                }
            }
        } else {
            self.state.update(&next, &self.prefs, &events, now);
        }
        self.prev = Some(next);
        events
    }

    /// Balance readings five minutes apart from `start`, all the events.
    fn balances(&mut self, start: DateTime<Utc>, values: &[f64]) -> Vec<AlertEvent> {
        let mut all = Vec::new();
        for (i, value) in values.iter().enumerate() {
            let at = start + TimeDelta::minutes(5 * i as i64);
            all.extend(self.feed(report(vec![balance(*value)], at)));
        }
        all
    }
}

fn kinds(events: &[AlertEvent]) -> Vec<(AlertKind, u8)> {
    events.iter().map(|e| (e.kind, e.threshold)).collect()
}

fn floor_key_of(amount: f64) -> Option<String> {
    Some(floor_mark(&budget::floor_key(&floor(amount))))
}

// ---- budgets -----------------------------------------------------------------

#[test]
fn two_budgets_on_one_spend_tell_each_threshold_once() {
    let account = account(vec![
        budget(BudgetPeriod::Monthly),
        budget(BudgetPeriod::Weekly),
    ]);
    let mut scenario = Scenario::new();
    let mut events = Vec::new();
    for (minute, used) in [(0, 10.0), (5, 40.0), (10, 41.0), (15, 46.0), (20, 47.0)] {
        events.extend(scenario.feed(budget_reading(&account, used, at(21, 12, minute))));
    }
    assert_eq!(
        kinds(&events),
        [
            (AlertKind::Threshold, 75),
            (AlertKind::Threshold, 75),
            (AlertKind::Threshold, 90),
            (AlertKind::Threshold, 90),
        ]
    );
    let keys: BTreeSet<_> = events.iter().map(|e| e.mark_key.clone()).collect();
    assert_eq!(keys.len(), 2, "one identity per budget");
    assert_ne!(events[0].label, events[1].label);
    // The marks follow the budget, not its label: renaming it keeps them.
    let renamed = crate::Account {
        budgets: vec![
            Budget {
                label: Some("Team".into()),
                ..budget(BudgetPeriod::Monthly)
            },
            budget(BudgetPeriod::Weekly),
        ],
        ..account.clone()
    };
    let later = scenario.feed(budget_reading(&renamed, 48.0, at(21, 12, 25)));
    assert!(later.is_empty(), "{later:?}");
}

/// A reading as a release before the re-validation recorded it: the
/// budget metrics carry their legacy label and no key.
fn legacy(mut reading: Report) -> Report {
    for metric in &mut reading.metrics {
        if metric.derived_from.is_some() {
            metric.label = budget::legacy_label(metric);
        }
        if let Some(from) = &mut metric.derived_from {
            from.key.clear();
        }
    }
    reading
}

#[test]
fn a_legacy_mark_of_one_budget_is_adopted() {
    let account = account(vec![budget(BudgetPeriod::Monthly)]);
    let mut scenario = Scenario::new();
    scenario.feed(legacy(budget_reading(&account, 10.0, at(21, 12, 0))));
    let told = scenario.feed(legacy(budget_reading(&account, 40.0, at(21, 12, 5))));
    assert_eq!(kinds(&told), [(AlertKind::Threshold, 75)]);
    let key = scenario.prev.as_ref().unwrap().state_key();
    let old = key_digest(&key, "Orçamento · Gasto no mês");
    assert!(scenario.state.marks.iter().any(|mark| mark.metric == old));

    // The upgraded release reads the same budget: 75 % stays told.
    let upgraded = scenario.feed(budget_reading(&account, 41.0, at(21, 12, 10)));
    assert!(upgraded.is_empty(), "{upgraded:?}");
    assert!(!scenario.state.marks.iter().any(|mark| mark.metric == old));
    let next = scenario.feed(budget_reading(&account, 46.0, at(21, 12, 15)));
    assert_eq!(kinds(&next), [(AlertKind::Threshold, 90)]);
}

#[test]
fn a_legacy_mark_two_budgets_shared_is_dropped() {
    let account = account(vec![
        budget(BudgetPeriod::Monthly),
        budget(BudgetPeriod::Weekly),
    ]);
    let mut scenario = Scenario::new();
    scenario.feed(legacy(budget_reading(&account, 10.0, at(21, 12, 0))));
    let told = scenario.feed(legacy(budget_reading(&account, 40.0, at(21, 12, 5))));
    assert!(!told.is_empty());
    let key = scenario.prev.as_ref().unwrap().state_key();
    let old = key_digest(&key, "Orçamento · Gasto no mês");
    // Ambiguous: each budget tells its 75 % once more, then never again.
    let upgraded = scenario.feed(budget_reading(&account, 41.0, at(21, 12, 10)));
    assert_eq!(
        kinds(&upgraded),
        [(AlertKind::Threshold, 75), (AlertKind::Threshold, 75)]
    );
    assert!(!scenario.state.marks.iter().any(|mark| mark.metric == old));
    let quiet = scenario.feed(budget_reading(&account, 42.0, at(21, 12, 15)));
    assert!(quiet.is_empty(), "{quiet:?}");
}

// ---- floors ------------------------------------------------------------------

#[test]
fn two_floors_around_ten_warn_once() {
    let mut scenario = Scenario::new().with_floors(vec![floor(10.0), floor(5.0)]);
    let events = scenario.balances(at(21, 12, 0), &[12.0, 9.5, 10.5, 9.5, 10.4, 9.8, 9.6]);
    assert_eq!(kinds(&events), [(AlertKind::BalanceBelowFloor, 0)]);
    assert_eq!(events[0].mark_key, floor_key_of(10.0));
}

#[test]
fn one_drop_across_two_floors_names_the_lowest_and_a_lower_crossing_warns_again() {
    let mut scenario = Scenario::new().with_floors(vec![floor(10.0), floor(5.0)]);
    let events = scenario.balances(at(21, 12, 0), &[12.0, 4.0, 4.2]);
    assert_eq!(kinds(&events), [(AlertKind::BalanceBelowFloor, 0)]);
    assert_eq!(events[0].mark_key, floor_key_of(5.0));
    assert!((events[0].consumed - 0.2).abs() < 1e-9);

    let mut tiers = Scenario::new().with_floors(vec![floor(10.0), floor(5.0)]);
    let events = tiers.balances(at(21, 12, 0), &[12.0, 9.0, 8.0, 4.5, 4.4]);
    assert_eq!(
        kinds(&events),
        [
            (AlertKind::BalanceBelowFloor, 0),
            (AlertKind::BalanceBelowFloor, 0)
        ]
    );
    assert_eq!(events[0].mark_key, floor_key_of(10.0));
    assert_eq!(events[1].mark_key, floor_key_of(5.0));

    // Back above the lower floor only: silent. Above the higher: one
    // recovery, naming it.
    let rising = tiers.balances(at(21, 13, 0), &[6.0, 9.0, 11.5, 12.0]);
    assert_eq!(kinds(&rising), [(AlertKind::BalanceRecovered, 0)]);
    assert_eq!(rising[0].mark_key, floor_key_of(10.0));
}

#[test]
fn a_new_floor_above_the_balance_warns_once_and_a_changed_one_again() {
    for claimed in [false, true] {
        let mut scenario = Scenario::new();
        scenario.claimed = claimed;
        assert!(scenario.balances(at(21, 12, 0), &[3.4, 3.2]).is_empty());
        scenario.set_floors(vec![floor(10.0)]);
        let events = scenario.balances(at(21, 12, 10), &[3.2, 3.1, 3.0]);
        assert_eq!(
            kinds(&events),
            [(AlertKind::BalanceBelowFloor, 0)],
            "claimed {claimed}"
        );
        scenario.set_floors(vec![floor(8.0)]);
        let changed = scenario.balances(at(21, 12, 30), &[3.0, 2.9]);
        assert_eq!(
            kinds(&changed),
            [(AlertKind::BalanceBelowFloor, 0)],
            "claimed {claimed}"
        );
        let back = scenario.balances(at(21, 13, 0), &[9.0, 12.0]);
        assert_eq!(
            kinds(&back),
            [(AlertKind::BalanceRecovered, 0)],
            "claimed {claimed}"
        );
    }
}

#[test]
fn a_legacy_floor_mark_becomes_one_mark_per_floor() {
    let mut scenario = Scenario::new().with_floors(vec![floor(10.0), floor(5.0)]);
    scenario.balances(at(21, 12, 0), &[12.0]);
    let key = scenario.prev.as_ref().unwrap().state_key();
    let legacy = key_digest(&key, &legacy_floor_mark("Saldo de créditos"));
    scenario.state.marks.push(Mark {
        account: account_digest(&key),
        metric: legacy.clone(),
        kind: AlertKind::BalanceBelowFloor,
        threshold: 0,
        resets_at: None,
        notified: true,
        at: at(20, 0, 0),
        lapsed: false,
        deferred_until: None,
        acknowledged: false,
        projection: None,
    });
    // Below both floors: the legacy mark stands for both, silently.
    assert!(scenario.balances(at(21, 12, 5), &[4.0]).is_empty());
    assert!(!scenario
        .state
        .marks
        .iter()
        .any(|mark| mark.metric == legacy));
    let floors = scenario
        .state
        .marks
        .iter()
        .filter(|mark| mark.kind == AlertKind::BalanceBelowFloor && mark.notified)
        .count();
    assert_eq!(floors, 2);
    let back = scenario.balances(at(21, 12, 10), &[6.0, 11.5]);
    assert_eq!(kinds(&back), [(AlertKind::BalanceRecovered, 0)]);
}
