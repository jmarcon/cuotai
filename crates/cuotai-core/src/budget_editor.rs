//! PRD 0041 RF5.2 and RF6: the UI-neutral rows of the budget editors and
//! the suggestions of **Set a budget…**.
//!
//! A front end shows a [`BudgetRow`] or a [`FloorRow`] as form fields
//! (typed text plus choices) and turns it back into the [`Budget`] or
//! [`Floor`] the TOML holds; the choices ([`targets`], [`starts`]), their
//! texts and the parsing live here so the TUI wizard and the desktop agree.
//! Validation stays in [`super::problems`].
//!
//! [`suggest`] prefills a draft from the person's plan price (PRD 0030 RF1)
//! and adds the "Last 30 days" reference line; it never writes anything, so
//! a budget exists only after the person saves the form.

use super::{
    recent_reference, Budget, BudgetError, BudgetKind, BudgetPeriod, BudgetScope, BudgetStart,
    Floor, FloorKind, WeekdayName,
};
use crate::cost::PlanSetting;
use crate::model::{Metric, MetricKind, Provider};
use crate::pace::Point;
use crate::text::{params, Localized, ParamValue};
use serde::Serialize;

/// What a budget row targets: a metric kind, or the account's total spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Spend,
    Usage,
    Credits,
    Account,
}

impl Target {
    pub const ALL: [Self; 4] = [Self::Spend, Self::Usage, Self::Credits, Self::Account];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Spend => "spend",
            Self::Usage => "usage",
            Self::Credits => "credits",
            Self::Account => "account",
        }
    }

    /// The localized name of the choice (`budget.target.<slug>`).
    pub fn text(self) -> Localized {
        match self {
            Self::Spend => Localized::fixed("budget.target.spend", "Spend"),
            Self::Usage => Localized::fixed("budget.target.usage", "Usage"),
            Self::Credits => Localized::fixed("budget.target.credits", "Credits"),
            Self::Account => Localized::fixed("budget.target.account", "Whole account spend"),
        }
    }

    fn metric_kind(self) -> MetricKind {
        match self {
            Self::Spend | Self::Account => MetricKind::Spend,
            Self::Usage => MetricKind::Usage,
            Self::Credits => MetricKind::Credits,
        }
    }

    fn of(budget: &Budget) -> Self {
        match (budget.scope, budget.kind) {
            (Some(BudgetScope::Account), _) => Self::Account,
            (_, Some(BudgetKind::Usage)) => Self::Usage,
            (_, Some(BudgetKind::Credits)) => Self::Credits,
            _ => Self::Spend,
        }
    }
}

/// RF5.2: the targets an account of `provider` can budget (every one when
/// the core does not know the provider's shapes), filtered by `query`
/// (case- and accent-insensitive, on the slug or the text in `locale`).
/// An empty query lists them all.
pub fn targets(provider: Provider, query: &str, locale: &str) -> Vec<Target> {
    let reportable = super::reportable_kinds(provider);
    let query = crate::text::fold(query.trim());
    Target::ALL
        .into_iter()
        .filter(|target| reportable.is_none_or(|kinds| kinds.contains(&target.metric_kind())))
        .filter(|target| {
            query.is_empty()
                || target.slug().contains(&query)
                || crate::text::fold(&target.text().resolve(locale)).contains(&query)
        })
        .collect()
}

/// The localized name of a period (`budget.period.<slug>`).
pub fn period_text(period: BudgetPeriod) -> Localized {
    match period {
        BudgetPeriod::Daily => Localized::fixed("budget.period.daily", "Daily"),
        BudgetPeriod::Weekly => Localized::fixed("budget.period.weekly", "Weekly"),
        BudgetPeriod::Monthly => Localized::fixed("budget.period.monthly", "Monthly"),
        BudgetPeriod::Window => Localized::fixed("budget.period.window", "Provider's cycle"),
    }
}

/// The periods in the order the editors offer them.
pub const PERIODS: [BudgetPeriod; 4] = [
    BudgetPeriod::Monthly,
    BudgetPeriod::Weekly,
    BudgetPeriod::Daily,
    BudgetPeriod::Window,
];

const WEEKDAYS: [WeekdayName; 7] = [
    WeekdayName::Monday,
    WeekdayName::Tuesday,
    WeekdayName::Wednesday,
    WeekdayName::Thursday,
    WeekdayName::Friday,
    WeekdayName::Saturday,
    WeekdayName::Sunday,
];

/// The start choices of `period`: the default (`None`), then the weekdays
/// of a weekly budget or the days 1 to 28 of a monthly one; only the
/// default for the others.
pub fn starts(period: BudgetPeriod) -> Vec<Option<BudgetStart>> {
    let mut out = vec![None];
    match period {
        BudgetPeriod::Weekly => out.extend(WEEKDAYS.map(|day| Some(BudgetStart::Weekday(day)))),
        BudgetPeriod::Monthly => out.extend((1..=28).map(|day| Some(BudgetStart::Day(day)))),
        _ => {}
    }
    out
}

/// The text of a start choice: the default ("Monday" / "Day 1"),
/// a weekday, or "Day 12".
pub fn start_text(period: BudgetPeriod, start: Option<BudgetStart>) -> Localized {
    match (period, start) {
        (BudgetPeriod::Weekly, None) => Localized::fixed("budget.start.default_weekly", "Monday"),
        (BudgetPeriod::Monthly, None) => Localized::fixed("budget.start.default_monthly", "Day 1"),
        (_, None) => Localized::fixed("budget.start.none", "Not used for this period"),
        (_, Some(BudgetStart::Weekday(day))) => super::weekday_text(day),
        (_, Some(BudgetStart::Day(day))) => {
            Localized::with_param("budget.start.day", "day", day.to_string(), "Day {day}")
        }
    }
}

/// Typed amount text → number: a comma decimal is accepted; anything else
/// that is not a number reads as 0, which the validation rejects.
fn parse_amount(text: &str) -> f64 {
    text.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0)
}

fn amount_field(amount: f64) -> String {
    if amount > 0.0 && amount.is_finite() {
        format!("{amount}")
    } else {
        String::new()
    }
}

fn some_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// RF5.2: one budget as an editor holds it: choices plus typed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRow {
    pub target: Target,
    pub amount: String,
    pub unit: String,
    pub period: BudgetPeriod,
    pub start: Option<BudgetStart>,
    pub filter: String,
    pub label: String,
}

impl Default for BudgetRow {
    fn default() -> Self {
        Self::from_budget(&Budget {
            kind: Some(BudgetKind::Spend),
            scope: None,
            label_contains: None,
            amount: 0.0,
            unit: None,
            period: BudgetPeriod::Monthly,
            starts: None,
            label: None,
        })
    }
}

impl BudgetRow {
    /// A new row on the first target `provider` can budget.
    pub fn new(provider: Provider) -> Self {
        let mut row = Self::default();
        if let Some(first) = targets(provider, "", "en-US").first() {
            row.target = *first;
        }
        row
    }

    pub fn from_budget(budget: &Budget) -> Self {
        Self {
            target: Target::of(budget),
            amount: amount_field(budget.amount),
            unit: budget.unit.clone().unwrap_or_default(),
            period: budget.period,
            start: budget.starts,
            filter: budget.label_contains.clone().unwrap_or_default(),
            label: budget.label.clone().unwrap_or_default(),
        }
    }

    /// The budget the TOML holds (blank texts omitted; a start that does
    /// not fit the period dropped, as the editors offer none).
    pub fn to_budget(&self) -> Budget {
        let (kind, scope) = match self.target {
            Target::Account => (None, Some(BudgetScope::Account)),
            Target::Spend => (Some(BudgetKind::Spend), None),
            Target::Usage => (Some(BudgetKind::Usage), None),
            Target::Credits => (Some(BudgetKind::Credits), None),
        };
        let starts = self
            .start
            .filter(|start| starts(self.period).contains(&Some(*start)));
        Budget {
            kind,
            scope,
            label_contains: some_text(&self.filter),
            amount: parse_amount(&self.amount),
            unit: some_text(&self.unit),
            period: self.period,
            starts,
            label: some_text(&self.label),
        }
    }
}

/// What a floor row watches: the first balance or credits metric, or one
/// kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorTarget {
    Any,
    Balance,
    Credits,
}

impl FloorTarget {
    pub const ALL: [Self; 3] = [Self::Any, Self::Balance, Self::Credits];

    pub fn text(self) -> Localized {
        match self {
            Self::Any => Localized::fixed("budget.floor_target.any", "Any balance"),
            Self::Balance => Localized::fixed("budget.floor_target.balance", "Balance"),
            Self::Credits => Localized::fixed("budget.floor_target.credits", "Credits"),
        }
    }

    fn kind(self) -> Option<FloorKind> {
        match self {
            Self::Any => None,
            Self::Balance => Some(FloorKind::Balance),
            Self::Credits => Some(FloorKind::Credits),
        }
    }
}

/// The floor targets of `provider` (every one when its shapes are unknown).
pub fn floor_targets(provider: Provider) -> Vec<FloorTarget> {
    let reportable = super::reportable_kinds(provider);
    FloorTarget::ALL
        .into_iter()
        .filter(|target| match (target.kind(), reportable) {
            (Some(kind), Some(kinds)) => kinds.contains(&kind.metric_kind()),
            _ => true,
        })
        .collect()
}

/// RF5.2: one floor as an editor holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorRow {
    pub target: FloorTarget,
    pub amount: String,
    pub unit: String,
    pub filter: String,
}

impl Default for FloorRow {
    fn default() -> Self {
        Self {
            target: FloorTarget::Any,
            amount: String::new(),
            unit: String::new(),
            filter: String::new(),
        }
    }
}

impl FloorRow {
    pub fn from_floor(floor: &Floor) -> Self {
        Self {
            target: match floor.kind {
                None => FloorTarget::Any,
                Some(FloorKind::Balance) => FloorTarget::Balance,
                Some(FloorKind::Credits) => FloorTarget::Credits,
            },
            amount: amount_field(floor.amount),
            unit: floor.unit.clone().unwrap_or_default(),
            filter: floor.label_contains.clone().unwrap_or_default(),
        }
    }

    pub fn to_floor(&self) -> Floor {
        Floor {
            kind: self.target.kind(),
            label_contains: some_text(&self.filter),
            amount: parse_amount(&self.amount),
            unit: some_text(&self.unit),
        }
    }
}

/// The first problem of row `index` of `list` (`"budgets"` or `"floors"`)
/// among `problems` (paths relative to the account), as its note.
pub fn row_problem(problems: &[BudgetError], list: &str, index: usize) -> Option<Localized> {
    let prefix = format!("{list}[{index}]");
    problems
        .iter()
        .find(|problem| {
            problem.field == list
                || problem
                    .field
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
        .map(BudgetError::note)
}

/// RF6: what **Set a budget…** opens with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// The draft row; its amount is the plan price only with `from_plan`.
    pub budget: Budget,
    /// RF6.1: "From your plan price…", when the amount came from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_plan: Option<Localized>,
    /// RF6.2: "Last 30 days: $38.20", a reference line, never an amount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<Localized>,
}

/// RF6: the draft for `metric` ([`super::draft_for`]), its amount and
/// cycle day prefilled from `plan` when the plan is in the metric's
/// currency and the metric is a spend (RF6.1), and the reference line of
/// the last 30 days from `recent` (the recorded samples of a spend or
/// usage metric, RF6.2). Pure: nothing is saved.
pub fn suggest(metric: &Metric, plan: Option<&PlanSetting>, recent: &[Point]) -> Suggestion {
    let mut budget = super::draft_for(metric);
    let plan = plan.filter(|plan| {
        metric.kind == MetricKind::Spend && plan.currency.eq_ignore_ascii_case(&metric.unit)
    });
    let from_plan = plan.map(|plan| {
        budget.amount = plan.price;
        budget.unit = Some(plan.currency.clone());
        budget.period = BudgetPeriod::Monthly;
        let day = plan.cycle_day();
        budget.starts = (day != 1).then_some(BudgetStart::Day(day));
        Localized::with_params(
            "budget.prefill.plan",
            params([
                (
                    "amount",
                    crate::overage::format_amount(plan.price, &plan.currency, Some(&plan.currency)),
                ),
                ("day", ParamValue::Text(day.to_string())),
            ]),
            "From your plan price: {amount} a month from day {day}. Check it before saving.",
        )
    });
    let reference = matches!(metric.kind, MetricKind::Spend | MetricKind::Usage)
        .then(|| recent_reference(recent, &metric.unit))
        .flatten();
    Suggestion {
        budget,
        from_plan,
        reference,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Metric;
    use chrono::{TimeZone, Utc};

    fn spend(unit: &str) -> Metric {
        Metric::usage("Monthly spend", 12.0, unit, MetricKind::Spend, None)
    }

    #[test]
    fn targets_follow_the_provider_and_the_typed_filter() {
        assert_eq!(
            targets(Provider::LiteLlm, "", "en-US"),
            [Target::Spend, Target::Account]
        );
        assert_eq!(targets(Provider::OpenRouter, "", "en-US").len(), 3);
        // Unknown shapes: every target.
        assert_eq!(targets(Provider::OpenAiApi, "", "en-US"), Target::ALL);
        assert_eq!(
            targets(Provider::OpenAiApi, "cred", "en-US"),
            [Target::Credits]
        );
        assert_eq!(
            targets(Provider::OpenAiApi, "WHOLE", "en-US"),
            [Target::Account]
        );
        assert!(targets(Provider::OpenAiApi, "zzz", "en-US").is_empty());
        assert_eq!(
            floor_targets(Provider::OpenRouter),
            [FloorTarget::Any, FloorTarget::Balance]
        );
    }

    #[test]
    fn rows_round_trip_and_parse_typed_text() {
        let budget = Budget {
            kind: None,
            scope: Some(BudgetScope::Account),
            label_contains: Some("gpt".into()),
            amount: 50.5,
            unit: Some("USD".into()),
            period: BudgetPeriod::Weekly,
            starts: Some(BudgetStart::Weekday(WeekdayName::Friday)),
            label: Some("Team".into()),
        };
        assert_eq!(BudgetRow::from_budget(&budget).to_budget(), budget);
        let mut row = BudgetRow::new(Provider::OpenAiApi);
        assert_eq!(row.target, Target::Spend);
        row.amount = " 12,5 ".into();
        row.unit = "  ".into();
        row.start = Some(BudgetStart::Day(3));
        row.period = BudgetPeriod::Weekly;
        let built = row.to_budget();
        assert_eq!(built.amount, 12.5);
        assert_eq!(built.unit, None);
        assert_eq!(built.starts, None, "a day does not start a week");
        row.amount = "fifty".into();
        assert_eq!(row.to_budget().amount, 0.0);
        let floor = Floor {
            kind: Some(FloorKind::Credits),
            label_contains: None,
            amount: 5.0,
            unit: Some("USD".into()),
        };
        assert_eq!(FloorRow::from_floor(&floor).to_floor(), floor);
        assert_eq!(FloorRow::default().to_floor().kind, None);
    }

    #[test]
    fn starts_and_their_texts() {
        assert_eq!(starts(BudgetPeriod::Weekly).len(), 8);
        assert_eq!(starts(BudgetPeriod::Monthly).len(), 29);
        assert_eq!(starts(BudgetPeriod::Daily), [None]);
        assert_eq!(
            start_text(BudgetPeriod::Monthly, Some(BudgetStart::Day(12))).resolve("en-US"),
            "Day 12"
        );
        assert_eq!(
            start_text(BudgetPeriod::Weekly, None).resolve("en-US"),
            "Monday"
        );
    }

    #[test]
    fn row_problem_finds_the_rows_own_rule() {
        let budgets = vec![
            BudgetRow::default().to_budget(),
            Budget {
                amount: 10.0,
                period: BudgetPeriod::Weekly,
                ..BudgetRow::default().to_budget()
            },
        ];
        let problems = super::super::problems(Provider::OpenAiApi, &budgets, &[]);
        assert!(row_problem(&problems, "budgets", 0).is_some());
        assert!(row_problem(&problems, "budgets", 1).is_none());
        assert!(row_problem(&problems, "floors", 0).is_none());
        // budgets[1] is not budgets[10].
        let ten = vec![BudgetError {
            field: "budgets[10].amount".into(),
            rule: super::super::BudgetRule::Amount,
        }];
        assert!(row_problem(&ten, "budgets", 1).is_none());
    }

    #[test]
    fn suggest_prefills_from_a_plan_in_the_same_currency_only() {
        let plan = PlanSetting::new(20.0, "usd", Some(5)).unwrap();
        let s = suggest(&spend("USD"), Some(&plan), &[]);
        assert_eq!(s.budget.amount, 20.0);
        assert_eq!(s.budget.starts, Some(BudgetStart::Day(5)));
        assert_eq!(s.budget.period, BudgetPeriod::Monthly);
        assert_eq!(
            s.from_plan.unwrap().resolve("en-US"),
            "From your plan price: $20.00 a month from day 5. Check it before saving."
        );
        assert!(s.reference.is_none());
        // Another currency: no prefill, the amount stays for the person.
        let other = suggest(&spend("EUR"), Some(&plan), &[]);
        assert_eq!(other.budget.amount, 0.0);
        assert!(other.from_plan.is_none());
        // Calendar month plan: no start written.
        let calendar = PlanSetting::new(20.0, "USD", None).unwrap();
        assert_eq!(
            suggest(&spend("USD"), Some(&calendar), &[]).budget.starts,
            None
        );
    }

    #[test]
    fn suggest_adds_the_reference_line_but_never_as_the_amount() {
        let at = |day| Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap();
        let points = [
            Point {
                taken_at: at(1),
                used: 10.0,
            },
            Point {
                taken_at: at(20),
                used: 48.2,
            },
        ];
        let s = suggest(&spend("USD"), None, &points);
        assert_eq!(s.budget.amount, 0.0);
        assert_eq!(
            s.reference.unwrap().resolve("en-US"),
            "Last 30 days: $38.20"
        );
        let mut balance = spend("USD");
        balance.kind = MetricKind::Credits;
        assert!(suggest(&balance, None, &points).reference.is_none());
    }
}
