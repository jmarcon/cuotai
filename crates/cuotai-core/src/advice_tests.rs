//! PRD 0024 RF7 acceptance: the ranking over synthetic readings.

use super::*;
use crate::model::{Billing, MetricKind, Scope};
use crate::{Metric, MetricWindow};
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap()
}

fn limit(label: &str, used: f64, resets_in: TimeDelta) -> Metric {
    Metric {
        billing: Billing::Included,
        scope: None,
        label: label.into(),
        used: Some(used),
        limit: Some(100.0),
        remaining: None,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Window5h),
        period: None,
        resets_at: Some(now() + resets_in),
        derived_from: None,
    }
}

fn report(name: &str, status: Status, metrics: Vec<Metric>) -> Report {
    Report {
        as_of: Default::default(),
        overage_hint: Default::default(),
        sub_reports: Vec::new(),
        diagnostic: Default::default(),
        account_id: None,
        account: name.into(),
        provider: Provider::Claude,
        checked_at: now() - TimeDelta::minutes(1),
        status,
        metrics,
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        breakdown: Vec::new(),
    }
}

fn hours(n: i64) -> TimeDelta {
    TimeDelta::hours(n)
}

fn minutes(n: i64) -> TimeDelta {
    TimeDelta::minutes(n)
}

struct Rig {
    reports: Vec<Report>,
    providers: Vec<Provider>,
    groups: Vec<Option<&'static str>>,
    fresh: Vec<Freshness>,
    enabled: Vec<bool>,
}

impl Rig {
    fn new(reports: Vec<Report>) -> Self {
        let n = reports.len();
        Self {
            reports,
            providers: vec![Provider::Claude; n],
            groups: vec![None; n],
            fresh: vec![Freshness::Fresh; n],
            enabled: vec![true; n],
        }
    }

    fn best(&self) -> Vec<GroupBest> {
        let views: Vec<AccountView> = (0..self.reports.len())
            .map(|index| AccountView {
                index,
                provider: self.providers[index],
                group: self.groups[index],
                enabled: self.enabled[index],
                report: Some(&self.reports[index]),
                freshness: Some(self.fresh[index]),
            })
            .collect();
        best(&views, now())
    }
}

fn pair(a: f64, b: f64) -> Rig {
    Rig::new(vec![
        report("Work", Status::Ready, vec![limit("Session", a, hours(3))]),
        report(
            "Personal",
            Status::Ready,
            vec![limit("Session", b, hours(3))],
        ),
    ])
}

#[test]
fn the_account_with_more_headroom_is_best_and_ties_go_to_configuration_order() {
    let result = pair(80.0, 36.0).best();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].group, "provider:claude");
    assert_eq!(result[0].best, 1);
    let best = result[0].candidate(1).unwrap();
    assert!((best.remaining - 0.64).abs() < 1e-9);
    assert_eq!(pair(50.0, 50.0).best()[0].best, 0, "a tie keeps the order");
}

#[test]
fn an_exhausted_account_loses_to_the_other() {
    let result = pair(100.0, 95.0).best();
    assert_eq!(result[0].best, 1);
    assert_eq!(result[0].candidate(0).unwrap().score, 0.0);
}

#[test]
fn a_limit_renewing_in_five_minutes_is_credited_toward_full() {
    // A: 5-hour window 80% used renewing in 5 minutes, weekly 10% used far
    // away. B: weekly 70% used. A's session counts 0.2 + 0.8 × (1 − 5/30).
    let rig = Rig::new(vec![
        report(
            "Work",
            Status::Ready,
            vec![
                limit("Session", 80.0, minutes(5)),
                limit("Week", 10.0, hours(100)),
            ],
        ),
        report(
            "Personal",
            Status::Ready,
            vec![limit("Week", 70.0, hours(100))],
        ),
    ]);
    let result = rig.best();
    assert_eq!(result[0].best, 0);
    let a = result[0].candidate(0).unwrap();
    assert!(
        (a.score - (0.2 + 0.8 * (1.0 - 5.0 / 30.0))).abs() < 1e-9,
        "{a:?}"
    );
    // Without the credit (a renewal in hours) the 80% window binds and loses.
    let rig = Rig::new(vec![
        report(
            "Work",
            Status::Ready,
            vec![
                limit("Session", 80.0, hours(4)),
                limit("Week", 10.0, hours(100)),
            ],
        ),
        report(
            "Personal",
            Status::Ready,
            vec![limit("Week", 70.0, hours(100))],
        ),
    ]);
    assert_eq!(rig.best()[0].best, 1);
}

#[test]
fn stale_failing_disabled_or_unmeasured_accounts_are_not_candidates() {
    let mut rig = pair(10.0, 90.0);
    rig.fresh[0] = Freshness::Stale;
    assert!(rig.best().is_empty(), "one candidate left: no best");
    let mut rig = pair(10.0, 90.0);
    rig.reports[0].status = Status::Error;
    assert!(rig.best().is_empty());
    let mut rig = pair(10.0, 90.0);
    rig.enabled[0] = false;
    assert!(rig.best().is_empty());
    // No limit with a measured use at all.
    let mut rig = pair(10.0, 90.0);
    rig.reports[0].metrics.clear();
    assert!(rig.best().is_empty());
    // No reading is not a candidate either.
    let views = [
        AccountView {
            index: 0,
            provider: Provider::Claude,
            group: None,
            enabled: true,
            report: None,
            freshness: None,
        },
        AccountView {
            index: 1,
            provider: Provider::Claude,
            group: None,
            enabled: true,
            report: Some(&pair(10.0, 10.0).reports[0]),
            freshness: Some(Freshness::Fresh),
        },
    ];
    assert!(best(&views, now()).is_empty());
}

#[test]
fn an_aging_reading_ranks_after_every_fresh_one_even_with_more_headroom() {
    let mut rig = Rig::new(vec![
        report("A", Status::Ready, vec![limit("Session", 5.0, hours(3))]),
        report("B", Status::Ready, vec![limit("Session", 70.0, hours(3))]),
        report("C", Status::Ready, vec![limit("Session", 90.0, hours(3))]),
    ]);
    rig.fresh[0] = Freshness::Aging;
    let result = rig.best();
    let order: Vec<usize> = result[0].ranked.iter().map(|c| c.index).collect();
    assert_eq!(order, [1, 2, 0]);
    assert_eq!(result[0].best, 1);
}

#[test]
fn model_scoped_limits_and_overage_are_ignored() {
    let scoped = |used| Metric {
        scope: Some(Scope::model("opus", "Opus")),
        ..limit("Opus", used, hours(3))
    };
    let overage = Metric {
        billing: Billing::Overage,
        ..limit("Extra", 100.0, hours(3))
    };
    let rig = Rig::new(vec![
        report(
            "Work",
            Status::Ready,
            vec![limit("Session", 30.0, hours(3)), scoped(100.0), overage],
        ),
        report(
            "Personal",
            Status::Ready,
            vec![limit("Session", 60.0, hours(3))],
        ),
    ]);
    let result = rig.best();
    assert_eq!(result[0].best, 0, "the exhausted Opus limit does not count");
    assert!((result[0].candidate(0).unwrap().remaining - 0.7).abs() < 1e-9);
}

#[test]
fn groups_compare_across_providers_only_when_the_user_says_so() {
    let mut rig = pair(80.0, 20.0);
    rig.providers[1] = Provider::ChatGpt;
    assert!(rig.best().is_empty(), "ungrouped accounts of two providers");
    rig.groups = vec![Some("coding"), Some("coding")];
    let result = rig.best();
    assert_eq!(result[0].group, "coding");
    assert_eq!(result[0].best, 1);
    // A single-member group has no best.
    rig.groups = vec![Some("coding"), Some("other")];
    assert!(rig.best().is_empty());
    // Groups come in the order of their first member.
    let mut rig = Rig::new(vec![
        report("A", Status::Ready, vec![limit("S", 10.0, hours(3))]),
        report("B", Status::Ready, vec![limit("S", 10.0, hours(3))]),
        report("C", Status::Ready, vec![limit("S", 50.0, hours(3))]),
        report("D", Status::Ready, vec![limit("S", 20.0, hours(3))]),
    ]);
    rig.groups = vec![Some("b"), Some("a"), Some("a"), Some("b")];
    let result = rig.best();
    assert_eq!(
        result.iter().map(|g| g.group.as_str()).collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!((result[0].best, result[1].best), (0, 1));
}

#[test]
fn the_texts_carry_the_names_and_percentages_and_never_write_anything() {
    let chip = best_chip();
    assert_eq!(chip.id, "advice.best");
    let runs_out = switch_text("Work", "Personal", 0.64, Some("18:53".into()));
    assert_eq!(runs_out.id, "advice.switch");
    assert!(runs_out.fallback.contains("Work") && runs_out.fallback.contains("64%"));
    assert_eq!(
        switch_text("Work", "Personal", 0.64, None).id,
        "advice.switch.exhausted"
    );
    // The module is pure: it takes shared references only, so it cannot
    // reach the configuration or the cycle (checked by the signatures).
    let _: fn(&[AccountView], DateTime<Utc>) -> Vec<GroupBest> = best;
}

/// PRD 0024 RF7.1: `[[groups]]` may list member account ids (PRD 0029);
/// the account's own `group` still wins, and an id cannot sit in two groups.
#[test]
fn groups_may_list_member_account_ids() {
    let toml = r#"
[[accounts]]
id = "work-claude"
name = "Work"
provider = "groq"
credential_env = "GROQ_WORK"
[[accounts]]
id = "home-claude"
name = "Home"
provider = "groq"
credential_env = "GROQ_HOME"
group = "personal"
[[accounts]]
id = "lab-account"
name = "Lab"
provider = "groq"
credential_env = "GROQ_LAB"

[[groups]]
id = "coding"
label = "Coding"
members = ["work-claude", "gone-account"]

[[groups]]
id = "personal"
members = ["home-claude"]
"#;
    let config = Config::from_toml(toml).unwrap();
    let [work, home, lab] = &config.accounts[..] else {
        panic!("three accounts");
    };
    assert_eq!(config.group_of(work), Some("coding"));
    assert_eq!(config.group_of(home), Some("personal"));
    assert_eq!(config.group_of(lab), None);
    assert_eq!(group_label(&config, work), Some("Coding"));
    let views = views(&config, |_| None, |_| true, now());
    assert_eq!(views[0].group, Some("coding"));
    assert_eq!(views[2].group, None);

    // Listed by two groups, or by a group other than its own `group`.
    let twice = toml.replace(
        r#"members = ["home-claude"]"#,
        r#"members = ["work-claude"]"#,
    );
    assert!(matches!(
        Config::from_toml(&twice),
        Err(crate::ConfigError::Group)
    ));
    let clash = toml.replace(r#"group = "personal""#, r#"group = "coding""#);
    assert!(matches!(
        Config::from_toml(&clash),
        Err(crate::ConfigError::Group)
    ));
}

/// PRD 0024 re-validation (V-05): the advice reads the strict forecast, so
/// an average-only projection never counts as running out, while a steep
/// recent slope does.
#[test]
fn only_the_strict_forecast_runs_out_for_the_advice() {
    // 80 used with half of the 5-hour window left: the average runs out early.
    let metric = limit("5h", 80.0, TimeDelta::minutes(150));
    let interval = Duration::from_secs(300);
    let checked = now() - TimeDelta::minutes(1);
    let average = crate::pace::assess(&metric, checked, None).unwrap();
    assert!(average.projected_exhaustion_at.is_some());
    assert_eq!(running_out(&metric, checked, interval, now(), &[]), None);
    let point = |ago: i64, used| crate::pace::Point {
        taken_at: checked - TimeDelta::minutes(ago),
        used,
    };
    let steep = [point(40, 40.0), point(20, 60.0), point(0, 80.0)];
    assert!(running_out(&metric, checked, interval, now(), &steep).is_some());
}

/// PRD 0024 re-validation (R-02): a projected or exhausted notice about one
/// account carries the best account of its group as a second line, never
/// when private, for the best account itself or for another kind.
#[test]
fn a_notice_carries_the_advice_as_its_second_line() {
    let account = |name: &str| Account {
        name: name.into(),
        provider: Provider::Claude,
        ..Account::default()
    };
    let config = Config {
        accounts: vec![account("Work"), account("Personal")],
        ..Config::default()
    };
    let reports = [
        report(
            "Work",
            Status::Ready,
            vec![limit("Session", 80.0, hours(3))],
        ),
        report(
            "Personal",
            Status::Ready,
            vec![limit("Session", 36.0, hours(3))],
        ),
    ];
    let event = |index: usize, kind| crate::alerts::AlertEvent {
        overage: Default::default(),
        account_key: config.accounts[index].state_key(),
        label: "Session".into(),
        window: Some(MetricWindow::Window5h),
        kind,
        threshold: 0,
        consumed: 0.8,
        resets_at: Some(now() + hours(3)),
        projection: (kind == crate::alerts::AlertKind::ProjectedExhaustion).then(|| {
            crate::alerts::Projection {
                projected_at: now() + minutes(40),
                lead_minutes: 60,
                margin_seconds: 8_000,
                source: crate::pace::PaceSource::Recent,
            }
        }),
        limit_change: None,
        row: None,
    };
    let reading = |index: usize| reports.get(index);
    let line = |events: &[crate::alerts::AlertEvent], private| {
        notice_line(&config, reading, events, private, now(), &Utc)
    };
    use crate::alerts::AlertKind::{Exhausted, ProjectedExhaustion, Threshold};
    let projected = line(&[event(0, ProjectedExhaustion)], false).unwrap();
    assert_eq!(
        projected.fallback,
        "Work runs out ≈12:40\u{a0}PM · Personal has 64% left"
    );
    assert_eq!(
        line(&[event(0, Exhausted)], false).unwrap().fallback,
        "Work is exhausted · Personal has 64% left"
    );
    assert_eq!(line(&[event(0, ProjectedExhaustion)], true), None);
    assert_eq!(line(&[event(1, ProjectedExhaustion)], false), None);
    assert_eq!(line(&[event(0, Threshold)], false), None);
    let notice = crate::resets::Notice {
        title: Localized::fixed("x.title", "Title"),
        body: Localized::fixed("x.body", "Body"),
    };
    let both = with_line(notice.clone(), Some(projected));
    assert_eq!(
        both.body.fallback,
        "Body · Work runs out ≈12:40\u{a0}PM · Personal has 64% left"
    );
    assert_eq!(with_line(notice.clone(), None), notice);
}
