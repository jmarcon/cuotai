//! PRD 0041 — user-defined budgets and balance floors.
//!
//! A **budget** is a limit the person sets on a metric the provider reports
//! without one (spend, usage, credits). [`apply`] turns each budget of an
//! account into one additional, *derived* quota metric (`derived_from` set)
//! with `used`, `limit`, `remaining`, `window` and `resets_at`, so the
//! generic engines (binding, pace, threshold alerts, projection, reset
//! notices, history, statistics) work on it unchanged. The target metric
//! itself is never changed.
//!
//! A **floor** is the balance level below which the person wants to be
//! warned; it adds no metric (the alert engine reads it from
//! [`crate::alerts::AlertPrefs::floors`]).
//!
//! Budgets and floors are the person's numbers: they live in the accounts
//! TOML (`[[accounts.budgets]]`, `[[accounts.floors]]`), carry no secret and
//! are never sent anywhere. Honesty rule: a budget never invents a provider
//! value; without a baseline or a target its `used` is `None` with a note,
//! never `0`.

use crate::model::{
    Account, Billing, DerivedFrom, Metric, MetricKind, MetricWindow, Provider, Report,
};
use crate::pace::Point;
use crate::text::{params, Localized, ParamValue};
use chrono::{
    DateTime, Datelike, Days, LocalResult, Months, NaiveDate, NaiveDateTime, TimeDelta, TimeZone,
    Utc, Weekday,
};
use serde::{Deserialize, Serialize};

/// RF1.1: at most this many budgets per account.
pub const MAX_BUDGETS: usize = 8;
/// RF3.1: at most this many floors per account.
pub const MAX_FLOORS: usize = 4;
/// RF1.2: the largest amount accepted.
pub const MAX_AMOUNT: f64 = 1_000_000_000.0;
/// RF1.1: the longest budget label, in characters.
pub const LABEL_MAX: usize = 32;
/// The longest `label_contains` filter, in characters.
pub const FILTER_MAX: usize = 64;
/// RF3.1: a floor re-arms once the balance rises above `amount × 1.1`.
pub const FLOOR_REARM: f64 = 1.1;
/// The raw label of a budget without its own label: rendered through
/// [`crate::metric_text::metric_text`] as "Budget · {label}". Like every
/// adapter text it stays in pt-BR, so it is stable in the history.
pub const DEFAULT_LABEL_PREFIX: &str = "Orçamento · ";
/// How far a provider's own reset may be from the budget boundary and
/// still count as the same period.
const SAME_BOUNDARY: TimeDelta = TimeDelta::hours(1);
/// A provider month (often UTC) within this distance of the local month
/// boundary is accepted for a monthly budget starting on the 1st, with a
/// note (RF2.2).
const MONTH_SLACK: TimeDelta = TimeDelta::hours(15);

/// RF1.1: the metric kinds a budget can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    Spend,
    Usage,
    Credits,
}

impl BudgetKind {
    pub const ALL: [Self; 3] = [Self::Spend, Self::Usage, Self::Credits];

    pub fn metric_kind(self) -> MetricKind {
        match self {
            Self::Spend => MetricKind::Spend,
            Self::Usage => MetricKind::Usage,
            Self::Credits => MetricKind::Credits,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Spend => "spend",
            Self::Usage => "usage",
            Self::Credits => "credits",
        }
    }
}

/// RF1.1: `scope = "account"`, the account's total spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScope {
    Account,
}

/// RF1.1: the period a budget counts over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPeriod {
    Daily,
    Weekly,
    #[default]
    Monthly,
    /// The provider's own cycle (the target's `resets_at`).
    Window,
}

impl BudgetPeriod {
    pub const ALL: [Self; 4] = [Self::Daily, Self::Weekly, Self::Monthly, Self::Window];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Window => "window",
        }
    }

    fn window(self) -> Option<MetricWindow> {
        match self {
            Self::Daily => Some(MetricWindow::Daily),
            Self::Weekly => Some(MetricWindow::Weekly),
            Self::Monthly => Some(MetricWindow::Monthly),
            Self::Window => None,
        }
    }
}

/// A weekday spelled in the TOML (`monday`..`sunday`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekdayName {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl WeekdayName {
    pub fn weekday(self) -> Weekday {
        match self {
            Self::Monday => Weekday::Mon,
            Self::Tuesday => Weekday::Tue,
            Self::Wednesday => Weekday::Wed,
            Self::Thursday => Weekday::Thu,
            Self::Friday => Weekday::Fri,
            Self::Saturday => Weekday::Sat,
            Self::Sunday => Weekday::Sun,
        }
    }
}

/// RF1.1: when a weekly (`monday`..`sunday`) or monthly (`1`..`28`) budget
/// period starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BudgetStart {
    Day(u8),
    Weekday(WeekdayName),
}

/// RF1.1: one `[[accounts.budgets]]` entry. The selector is `kind` (with
/// the optional `unit` and `label_contains` filters) or `scope = "account"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BudgetKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<BudgetScope>,
    /// Case-insensitive part of the target metric's label (raw or English).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_contains: Option<String>,
    pub amount: f64,
    /// ISO 4217 code for money, or the metric's unit; `None` takes the
    /// target's unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default)]
    pub period: BudgetPeriod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts: Option<BudgetStart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// Validation rejects non-finite amounts, so equality is reflexive.
impl Eq for Budget {}

/// RF3.1: one `[[accounts.floors]]` entry, on a `balance` or `credits`
/// metric (`kind` absent: the first of either).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<FloorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_contains: Option<String>,
    pub amount: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl Eq for Floor {}

/// The metric kinds a floor can watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorKind {
    Balance,
    Credits,
}

impl FloorKind {
    pub const ALL: [Self; 2] = [Self::Balance, Self::Credits];

    pub fn metric_kind(self) -> MetricKind {
        match self {
            Self::Balance => MetricKind::Balance,
            Self::Credits => MetricKind::Credits,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Balance => "balance",
            Self::Credits => "credits",
        }
    }
}

/// RF1.2: why a budget or floor was rejected. Each rule has its own
/// localized text ([`BudgetRule::note`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetRule {
    /// Neither `kind` nor `scope`, or both.
    Selector,
    /// The provider never reports a metric of this kind.
    KindNotReported,
    /// Not a currency code or the metric's unit (empty, too long).
    Unit,
    /// Not positive, not finite or above [`MAX_AMOUNT`].
    Amount,
    /// More than [`MAX_BUDGETS`] budgets or [`MAX_FLOORS`] floors.
    TooMany,
    /// Two budgets with the same selector and period.
    Duplicate,
    /// `starts` does not fit the period.
    Start,
    /// `period = "window"` on a metric that never has a reset.
    NoWindow,
    /// A label longer than [`LABEL_MAX`] characters, or blank.
    Label,
}

impl BudgetRule {
    /// The localized explanation, naming the offending `field`.
    pub fn note(self, field: &str) -> Localized {
        let (id, fallback) = match self {
            Self::Selector => (
                "budget.error.selector",
                "{field}: set either kind (spend, usage or credits) or scope = \"account\".",
            ),
            Self::KindNotReported => (
                "budget.error.kind_not_reported",
                "{field}: this provider never reports a metric of that kind.",
            ),
            Self::Unit => (
                "budget.error.unit",
                "{field}: the unit must be a currency code such as USD or the metric's own unit.",
            ),
            Self::Amount => (
                "budget.error.amount",
                "{field}: the amount must be greater than 0 and at most 1,000,000,000.",
            ),
            Self::TooMany => (
                "budget.error.too_many",
                "{field}: an account can have at most 8 budgets and 4 floors.",
            ),
            Self::Duplicate => (
                "budget.error.duplicate",
                "{field}: another budget already targets the same metric over the same period.",
            ),
            Self::Start => (
                "budget.error.start",
                "{field}: a weekly budget starts on a weekday (monday to sunday) and a monthly one on a day from 1 to 28.",
            ),
            Self::NoWindow => (
                "budget.error.no_window",
                "{field}: period = \"window\" needs a metric with its own reset; this one has none.",
            ),
            Self::Label => (
                "budget.error.label",
                "{field}: the label must have 1 to 32 characters.",
            ),
        };
        Localized::with_param(id, "field", field, fallback)
    }
}

/// A rejected budget or floor: the TOML path (`accounts[0].budgets[1].amount`)
/// and the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetError {
    pub field: String,
    pub rule: BudgetRule,
}

impl BudgetError {
    pub fn note(&self) -> Localized {
        self.rule.note(&self.field)
    }
}

/// RF1.2: the metric kinds a provider can report, when the core knows its
/// shapes; `None` (unknown: every selector is accepted and a mismatch only
/// shows at reading time as a note) otherwise.
pub fn reportable_kinds(provider: Provider) -> Option<&'static [MetricKind]> {
    use MetricKind::*;
    Some(match provider {
        Provider::OpenRouter => &[Quota, Balance, Spend, Usage],
        Provider::LiteLlm => &[Spend],
        Provider::Kimchi => &[Credits, Spend],
        Provider::ElevenLabs => &[Quota, Spend, Usage],
        Provider::JetbrainsAi => &[Usage],
        Provider::Antigravity => &[Quota],
        Provider::NineRouter => &[Quota],
        _ => return None,
    })
}

/// Whether `period = "window"` can work for `provider`: only a provider
/// whose reportable kinds are known and include no metric with a reset
/// fails ([`reportable_kinds`] with only balances). Unknown providers pass.
fn may_have_window(provider: Provider, kind: MetricKind) -> bool {
    match reportable_kinds(provider) {
        Some(kinds) => kinds.contains(&kind) && kind != MetricKind::Balance,
        None => true,
    }
}

fn valid_unit(unit: &str) -> bool {
    let trimmed = unit.trim();
    !trimmed.is_empty() && trimmed == unit && unit.chars().count() <= 24
}

fn valid_amount(amount: f64) -> bool {
    amount.is_finite() && amount > 0.0 && amount <= MAX_AMOUNT
}

/// RF1.2: the first problem of the budgets and floors of `account` (at
/// position `index` in the configuration, for the field path), or `Ok`.
/// [`crate::Config::validate`] and the draft validation of the editors
/// share it.
pub fn validate(index: usize, account: &Account) -> Result<(), BudgetError> {
    let base = format!("accounts[{index}]");
    if let Some(problem) = problems(account.provider, &account.budgets, &account.floors)
        .into_iter()
        .next()
    {
        return Err(BudgetError {
            field: format!("{base}.{}", problem.field),
            rule: problem.rule,
        });
    }
    Ok(())
}

/// RF5.1: every problem of a draft list of budgets and floors for an
/// account of `provider`, in row order, with paths relative to the account
/// (`budgets[1].amount`), so an editor can show each one inline.
pub fn problems(provider: Provider, budgets: &[Budget], floors: &[Floor]) -> Vec<BudgetError> {
    let mut out = Vec::new();
    let mut push = |field: String, rule| out.push(BudgetError { field, rule });
    if budgets.len() > MAX_BUDGETS {
        push("budgets".into(), BudgetRule::TooMany);
    }
    if floors.len() > MAX_FLOORS {
        push("floors".into(), BudgetRule::TooMany);
    }
    for (i, budget) in budgets.iter().enumerate() {
        let at = |key: &str| format!("budgets[{i}].{key}");
        let kind = match (budget.kind, budget.scope) {
            (Some(kind), None) => Some(kind.metric_kind()),
            (None, Some(BudgetScope::Account)) => Some(MetricKind::Spend),
            _ => {
                push(at("kind"), BudgetRule::Selector);
                None
            }
        };
        if let (Some(kind), Some(kinds)) = (kind, reportable_kinds(provider)) {
            if !kinds.contains(&kind) {
                push(at("kind"), BudgetRule::KindNotReported);
            }
        }
        if !valid_amount(budget.amount) {
            push(at("amount"), BudgetRule::Amount);
        }
        if budget.unit.as_deref().is_some_and(|unit| !valid_unit(unit)) {
            push(at("unit"), BudgetRule::Unit);
        }
        if budget
            .label_contains
            .as_deref()
            .is_some_and(|text| text.trim().is_empty() || text.chars().count() > FILTER_MAX)
        {
            push(at("label_contains"), BudgetRule::Selector);
        }
        if budget
            .label
            .as_deref()
            .is_some_and(|label| label.trim().is_empty() || label.chars().count() > LABEL_MAX)
        {
            push(at("label"), BudgetRule::Label);
        }
        let start_ok = match (budget.period, budget.starts) {
            (_, None) => true,
            (BudgetPeriod::Weekly, Some(BudgetStart::Weekday(_))) => true,
            (BudgetPeriod::Monthly, Some(BudgetStart::Day(day))) => (1..=28).contains(&day),
            _ => false,
        };
        if !start_ok {
            push(at("starts"), BudgetRule::Start);
        }
        if budget.period == BudgetPeriod::Window
            && kind.is_some_and(|kind| !may_have_window(provider, kind))
        {
            push(at("period"), BudgetRule::NoWindow);
        }
        if budgets[..i].iter().any(|other| same_target(other, budget)) {
            push(at("period"), BudgetRule::Duplicate);
        }
    }
    for (i, floor) in floors.iter().enumerate() {
        let at = |key: &str| format!("floors[{i}].{key}");
        if let (Some(kind), Some(kinds)) = (floor.kind, reportable_kinds(provider)) {
            if !kinds.contains(&kind.metric_kind()) {
                push(at("kind"), BudgetRule::KindNotReported);
            }
        }
        if !valid_amount(floor.amount) {
            push(at("amount"), BudgetRule::Amount);
        }
        if floor.unit.as_deref().is_some_and(|unit| !valid_unit(unit)) {
            push(at("unit"), BudgetRule::Unit);
        }
        if floor
            .label_contains
            .as_deref()
            .is_some_and(|text| text.trim().is_empty() || text.chars().count() > FILTER_MAX)
        {
            push(at("label_contains"), BudgetRule::Selector);
        }
    }
    out
}

fn same_target(a: &Budget, b: &Budget) -> bool {
    let folded = |text: &Option<String>| text.as_deref().map(str::to_lowercase);
    a.kind == b.kind
        && a.scope == b.scope
        && folded(&a.label_contains) == folded(&b.label_contains)
        && folded(&a.unit) == folded(&b.unit)
        && a.period == b.period
}

fn label_matches(metric: &Metric, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::to_lowercase) else {
        return true;
    };
    let english = crate::metric_text::metric_text(&metric.label).fallback;
    metric.label.to_lowercase().contains(&filter) || english.to_lowercase().contains(&filter)
}

fn unit_matches(metric: &Metric, unit: Option<&str>) -> bool {
    unit.is_none_or(|unit| metric.unit.eq_ignore_ascii_case(unit))
}

/// The provider metric `budget` targets in `metrics`: the first one of its
/// kind (unscoped first) matching the unit and label filters. Derived
/// metrics are never targets.
pub fn target<'m>(budget: &Budget, metrics: &'m [Metric]) -> Option<&'m Metric> {
    let kind = match (budget.kind, budget.scope) {
        (Some(kind), None) => kind.metric_kind(),
        (None, Some(BudgetScope::Account)) => MetricKind::Spend,
        _ => return None,
    };
    let candidates = || {
        metrics.iter().filter(move |metric| {
            metric.derived_from.is_none()
                && metric.kind == kind
                && unit_matches(metric, budget.unit.as_deref())
                && label_matches(metric, budget.label_contains.as_deref())
        })
    };
    if budget.scope == Some(BudgetScope::Account) {
        return candidates()
            .find(|metric| metric.scope.is_none() && metric.billing == Billing::Included);
    }
    candidates()
        .find(|metric| metric.scope.is_none())
        .or_else(|| candidates().next())
}

/// RF3: the balance metric `floor` watches in `metrics`.
pub fn floor_target<'m>(floor: &Floor, metrics: &'m [Metric]) -> Option<&'m Metric> {
    metrics.iter().find(|metric| {
        metric.derived_from.is_none()
            && match floor.kind {
                Some(kind) => metric.kind == kind.metric_kind(),
                None => matches!(metric.kind, MetricKind::Balance | MetricKind::Credits),
            }
            && unit_matches(metric, floor.unit.as_deref())
            && label_matches(metric, floor.label_contains.as_deref())
    })
}

/// RF3: the amount left on a balance metric: `remaining`, else
/// `limit − used`. `None` (never zero) when unknown.
pub fn balance_left(metric: &Metric) -> Option<f64> {
    let finite = |value: Option<f64>| value.filter(|v| v.is_finite());
    finite(metric.remaining).or_else(|| Some(finite(metric.limit)? - finite(metric.used)?))
}

/// The raw label of the budget metric for `budget` on `target`.
pub fn metric_label(budget: &Budget, target_label: &str) -> String {
    match budget.label.as_deref().map(str::trim) {
        Some(label) if !label.is_empty() => label.to_owned(),
        _ => format!("{DEFAULT_LABEL_PREFIX}{target_label}"),
    }
}

/// Whether `label` names a budget metric of `account` (a default budget
/// label, or one of its budgets' own labels): a series of the person's
/// numbers, not of the provider.
pub fn is_budget_label(account: &Account, label: &str) -> bool {
    label.starts_with(DEFAULT_LABEL_PREFIX)
        || account
            .budgets
            .iter()
            .any(|budget| budget.label.as_deref().map(str::trim) == Some(label))
}

/// Local midnight of `date` in `tz` as UTC. A midnight skipped by a DST
/// gap moves forward to the first local time that exists; an ambiguous
/// one takes the earlier instant.
pub(crate) fn local_midnight<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Option<DateTime<Utc>> {
    let midnight: NaiveDateTime = date.and_hms_opt(0, 0, 0)?;
    for minutes in (0..=180).step_by(15) {
        let local = midnight + TimeDelta::minutes(minutes);
        match tz.from_local_datetime(&local) {
            LocalResult::Single(at) => return Some(at.with_timezone(&Utc)),
            LocalResult::Ambiguous(earlier, _) => return Some(earlier.with_timezone(&Utc)),
            LocalResult::None => continue,
        }
    }
    None
}

fn month_day(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, day)
}

/// RF2.1: the current period `[start, end)` of a daily, weekly or monthly
/// budget at `now`, with boundaries at local midnight in `tz` (DST-safe:
/// the arithmetic is on local dates). `None` for `period = "window"`, which
/// follows the target's own reset.
pub fn period_bounds<Tz: TimeZone>(
    period: BudgetPeriod,
    starts: Option<BudgetStart>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let today = now.with_timezone(tz).date_naive();
    let (first, next) = match period {
        BudgetPeriod::Daily => (today, today.checked_add_days(Days::new(1))?),
        BudgetPeriod::Weekly => {
            let start = match starts {
                Some(BudgetStart::Weekday(day)) => day.weekday(),
                _ => Weekday::Mon,
            };
            let back =
                (7 + today.weekday().num_days_from_monday() - start.num_days_from_monday()) % 7;
            let first = today.checked_sub_days(Days::new(u64::from(back)))?;
            (first, first.checked_add_days(Days::new(7))?)
        }
        BudgetPeriod::Monthly => {
            let day = match starts {
                Some(BudgetStart::Day(day)) if (1..=28).contains(&day) => u32::from(day),
                _ => 1,
            };
            let this = month_day(today.year(), today.month(), day)?;
            let first = if today >= this {
                this
            } else {
                this.checked_sub_months(Months::new(1))?
            };
            (first, first.checked_add_months(Months::new(1))?)
        }
        BudgetPeriod::Window => return None,
    };
    let (start, end) = (local_midnight(first, tz)?, local_midnight(next, tz)?);
    // A local midnight the DST gap moved past `now` still holds `now`.
    Some((start.min(now), end))
}

/// The start of the provider window ending at `resets_at`.
fn provider_window_start(
    window: Option<MetricWindow>,
    resets_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    Some(match window? {
        MetricWindow::Window5h => resets_at - TimeDelta::hours(5),
        MetricWindow::Daily => resets_at - TimeDelta::days(1),
        MetricWindow::Weekly => resets_at - TimeDelta::weeks(1),
        MetricWindow::Monthly => resets_at.checked_sub_months(Months::new(1))?,
        MetricWindow::Other => return None,
    })
}

/// What [`apply`] reads from the history: the recorded samples of the
/// target metric (by its series) from `since` to now, in time order.
pub trait Baselines {
    fn samples(&self, target: &Metric, since: DateTime<Utc>) -> Vec<Point>;
}

/// No history: every derivation that needs a baseline yields `None`.
pub struct NoHistory;

impl Baselines for NoHistory {
    fn samples(&self, _: &Metric, _: DateTime<Utc>) -> Vec<Point> {
        Vec::new()
    }
}

impl<F: Fn(&Metric, DateTime<Utc>) -> Vec<Point>> Baselines for F {
    fn samples(&self, target: &Metric, since: DateTime<Utc>) -> Vec<Point> {
        self(target, since)
    }
}

/// Whether the target counts down (a prepaid balance): RF2.3.
fn counts_down(target: &Metric) -> bool {
    target.kind == MetricKind::Balance
        || (target.used.filter(|v| v.is_finite()).is_none()
            && target.remaining.filter(|v| v.is_finite()).is_some())
}

/// The target's current value: `used`, or `remaining` for a balance.
fn current_value(target: &Metric) -> Option<f64> {
    let finite = |value: Option<f64>| value.filter(|v| v.is_finite());
    if counts_down(target) {
        finite(target.remaining)
    } else {
        finite(target.used)
    }
}

/// How a budget's `used` was found (for the notes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Derivation {
    /// The provider's metric covers the period: its own `used`.
    Provider,
    /// The provider's month is close to, not equal to, the local one.
    ProviderMonth,
    /// Current value minus the first value recorded in the period.
    Baseline,
    /// Sum of the decreases of a balance in the period.
    Drops { topped_up: bool },
}

/// RF2.2/RF2.3: the use of `target` in the period starting at `start`
/// (ending at `end`), or `None` with the reason.
fn used_in_period(
    budget: &Budget,
    target: &Metric,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    history: &dyn Baselines,
) -> Result<(f64, Derivation), &'static str> {
    let current = current_value(target).ok_or("target_missing")?;
    let covers = |window: MetricWindow| {
        target.window == Some(window)
            && target
                .resets_at
                .is_some_and(|at| (at - end).abs() <= SAME_BOUNDARY)
    };
    if !counts_down(target) {
        if budget.period == BudgetPeriod::Window || budget.period.window().is_some_and(covers) {
            return Ok((current, Derivation::Provider));
        }
        let first_of_month = budget.period == BudgetPeriod::Monthly
            && matches!(budget.starts, None | Some(BudgetStart::Day(1)));
        if first_of_month
            && target.window == Some(MetricWindow::Monthly)
            && target
                .resets_at
                .is_none_or(|at| (at - end).abs() <= MONTH_SLACK)
        {
            return Ok((current, Derivation::ProviderMonth));
        }
    }
    let samples: Vec<Point> = history
        .samples(target, start)
        .into_iter()
        .filter(|point| point.taken_at >= start && point.used.is_finite())
        .collect();
    let Some(first) = samples.first() else {
        return Err("no_baseline");
    };
    if !counts_down(target) {
        return Ok(((current - first.used).max(0.0), Derivation::Baseline));
    }
    // A balance: add every drop; a rise is a top-up and moves the
    // baseline up by its amount instead of counting as negative use.
    let mut used = 0.0;
    let mut topped_up = false;
    let mut previous = first.used;
    for value in samples.iter().skip(1).map(|p| p.used).chain([current]) {
        if value < previous {
            used += previous - value;
        } else if value > previous {
            topped_up = true;
        }
        previous = value;
    }
    Ok((used, Derivation::Drops { topped_up }))
}

fn note(id: &'static str, label: &str, fallback: &'static str) -> Localized {
    Localized::with_param(id, "budget", label, fallback)
}

fn push_note(report: &mut Report, note: Localized) {
    report.notes.push(note.fallback.clone().into_owned());
    report.notes_l10n.push(note);
}

/// RF2.1: the budget metrics of `account` added to `report` (after the
/// adapter, before classification). Earlier derived metrics are dropped
/// first, so applying twice changes nothing. A report that is not `Ready`
/// is left as it is. `tz` is the person's time zone (local midnight
/// boundaries); `history` gives the baselines.
pub fn apply<Tz: TimeZone>(
    account: &Account,
    report: &mut Report,
    history: &dyn Baselines,
    now: DateTime<Utc>,
    tz: &Tz,
) {
    report
        .metrics
        .retain(|metric| metric.derived_from.is_none());
    if account.budgets.is_empty() || report.status != crate::Status::Ready {
        return;
    }
    let mut added = Vec::new();
    for (index, budget) in account.budgets.iter().enumerate() {
        let target = target(budget, &report.metrics).cloned();
        let target_label = target.as_ref().map_or_else(
            || {
                budget
                    .label_contains
                    .clone()
                    .or_else(|| budget.kind.map(|k| k.slug().to_owned()))
                    .unwrap_or_else(|| "spend".into())
            },
            |metric| metric.label.clone(),
        );
        let label = metric_label(budget, &target_label);
        let display = crate::metric_text::metric_text(&label)
            .fallback
            .into_owned();
        let unit = budget
            .unit
            .clone()
            .or_else(|| target.as_ref().map(|metric| metric.unit.clone()))
            .unwrap_or_default();
        let bounds = match budget.period {
            BudgetPeriod::Window => target.as_ref().and_then(|metric| {
                let end = metric.resets_at?;
                Some((
                    provider_window_start(metric.window, end).unwrap_or(now.min(end)),
                    end,
                ))
            }),
            period => period_bounds(period, budget.starts, now, tz),
        };
        let window = match budget.period {
            BudgetPeriod::Window => target.as_ref().and_then(|metric| metric.window),
            period => period.window(),
        };
        let mut metric = Metric {
            label,
            used: None,
            limit: Some(budget.amount),
            remaining: None,
            unit,
            kind: MetricKind::Quota,
            window,
            period: bounds.map(|(start, _)| start.to_rfc3339()),
            resets_at: bounds.map(|(_, end)| end),
            // RF2.4: the budget is a plan limit for every generic engine.
            billing: Billing::Included,
            scope: target.as_ref().and_then(|metric| metric.scope.clone()),
            derived_from: Some(DerivedFrom {
                kind: target
                    .as_ref()
                    .map(|metric| metric.kind)
                    .or_else(|| budget.kind.map(BudgetKind::metric_kind))
                    .unwrap_or(MetricKind::Spend),
                label: target_label,
                budget: index,
            }),
        };
        match (&target, bounds) {
            (None, _) => push_note(
                report,
                note(
                    "note.budget.target_missing",
                    &display,
                    "{budget}: the metric this budget follows is missing from this reading.",
                ),
            ),
            (Some(_), None) => push_note(
                report,
                note(
                    "note.budget.no_window",
                    &display,
                    "{budget}: the metric has no reset of its own, so period = \"window\" cannot count.",
                ),
            ),
            (Some(target), Some((start, end))) => {
                if budget.unit.as_deref().is_some_and(|unit| !target.unit.eq_ignore_ascii_case(unit)) {
                    push_note(
                        report,
                        note(
                            "note.budget.unit_mismatch",
                            &display,
                            "{budget}: the metric is reported in another unit, so this budget cannot count.",
                        ),
                    );
                } else {
                    match used_in_period(budget, target, start, end, history) {
                        Ok((used, how)) => {
                            metric.used = Some(used);
                            metric.remaining = Some((budget.amount - used).max(0.0));
                            match how {
                                Derivation::ProviderMonth => push_note(
                                    report,
                                    note(
                                        "note.budget.provider_month",
                                        &display,
                                        "{budget}: counted over the provider's billing month, which may start in another time zone.",
                                    ),
                                ),
                                Derivation::Drops { topped_up: true } => push_note(
                                    report,
                                    note(
                                        "note.budget.topup_detected",
                                        &display,
                                        "{budget}: a top-up was detected; it does not count as use.",
                                    ),
                                ),
                                _ => {}
                            }
                        }
                        Err("no_baseline") => push_note(
                            report,
                            note(
                                "note.budget.no_baseline",
                                &display,
                                "{budget}: counting starts with the first reading of this period.",
                            ),
                        ),
                        Err(_) => push_note(
                            report,
                            note(
                                "note.budget.target_missing",
                                &display,
                                "{budget}: the metric this budget follows is missing from this reading.",
                            ),
                        ),
                    }
                }
            }
        }
        added.push(metric);
    }
    report.metrics.extend(added);
}

/// A report restored from the history (PRD 0010 RF2.3) carries the budget
/// metrics as plain quotas: mark the ones whose label is one of the
/// account's budgets again, so every surface still labels them as the
/// person's budget.
pub fn mark_restored(account: &Account, report: &mut Report) {
    if account.budgets.is_empty() {
        return;
    }
    let originals: Vec<Metric> = report
        .metrics
        .iter()
        .filter(|metric| metric.derived_from.is_none())
        .cloned()
        .collect();
    for metric in &mut report.metrics {
        if metric.derived_from.is_some() || metric.kind != MetricKind::Quota {
            continue;
        }
        for (index, budget) in account.budgets.iter().enumerate() {
            let target = target(budget, &originals);
            let label = target.map_or_else(
                || budget.label.clone().unwrap_or_default(),
                |target| metric_label(budget, &target.label),
            );
            if !label.is_empty() && metric.label == label {
                metric.derived_from = Some(DerivedFrom {
                    kind: target
                        .map(|t| t.kind)
                        .or_else(|| budget.kind.map(BudgetKind::metric_kind))
                        .unwrap_or(MetricKind::Spend),
                    label: target.map_or_else(String::new, |t| t.label.clone()),
                    budget: index,
                });
                break;
            }
        }
    }
}

fn amount_text(amount: f64, unit: &str) -> ParamValue {
    let currency = is_currency(unit).then_some(unit);
    crate::overage::format_amount(amount, unit, currency)
}

/// Whether `unit` is an ISO 4217-shaped code (three ASCII letters).
pub fn is_currency(unit: &str) -> bool {
    unit.len() == 3 && unit.chars().all(|c| c.is_ascii_alphabetic())
}

/// RF5.1: the live sentence of a budget row: "Cuotai will show a Budget
/// quota of $50 a month, starting on the 1st". `unit` is the unit shown
/// (the budget's, else the target metric's).
pub fn describe(budget: &Budget, unit: &str) -> Localized {
    let amount = amount_text(budget.amount, unit);
    let (id, fallback, start) = match (budget.period, budget.starts) {
        (BudgetPeriod::Daily, _) => (
            "budget.describe.daily",
            "Cuotai will show a Budget quota of {amount} a day, from midnight.",
            None,
        ),
        (BudgetPeriod::Weekly, starts) => {
            let day = match starts {
                Some(BudgetStart::Weekday(day)) => day,
                _ => WeekdayName::Monday,
            };
            (
                "budget.describe.weekly",
                "Cuotai will show a Budget quota of {amount} a week, starting on {start}.",
                Some(ParamValue::l10n(weekday_text(day))),
            )
        }
        (BudgetPeriod::Monthly, starts) => {
            let day = match starts {
                Some(BudgetStart::Day(day)) => day,
                _ => 1,
            };
            (
                "budget.describe.monthly",
                "Cuotai will show a Budget quota of {amount} a month, starting on day {start}.",
                Some(ParamValue::Text(day.to_string())),
            )
        }
        (BudgetPeriod::Window, _) => (
            "budget.describe.window",
            "Cuotai will show a Budget quota of {amount} over the provider's own cycle.",
            None,
        ),
    };
    let mut values = params([("amount", amount)]);
    if let Some(start) = start {
        values.insert("start".into(), start);
    }
    Localized::with_params(id, values, fallback)
}

/// The localized name of a weekday (`budget.weekday.<name>`).
pub fn weekday_text(day: WeekdayName) -> Localized {
    match day {
        WeekdayName::Monday => Localized::fixed("budget.weekday.monday", "Monday"),
        WeekdayName::Tuesday => Localized::fixed("budget.weekday.tuesday", "Tuesday"),
        WeekdayName::Wednesday => Localized::fixed("budget.weekday.wednesday", "Wednesday"),
        WeekdayName::Thursday => Localized::fixed("budget.weekday.thursday", "Thursday"),
        WeekdayName::Friday => Localized::fixed("budget.weekday.friday", "Friday"),
        WeekdayName::Saturday => Localized::fixed("budget.weekday.saturday", "Saturday"),
        WeekdayName::Sunday => Localized::fixed("budget.weekday.sunday", "Sunday"),
    }
}

/// RF3.2: the semantic role of a floor chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorRole {
    /// Above the floor: no chip, only the details line.
    Ok,
    /// Below the floor.
    Warn,
    /// At or below 25% of the floor.
    Bad,
}

/// RF3.2: one floor of an account as a reading shows it: the card chip
/// ("Balance low · $3.20 left", only when below) and the details line under
/// the balance ("Floor $10.00 · $3.20 left").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FloorStatus {
    /// The balance metric's raw label and its display text.
    pub label: String,
    pub label_text: Localized,
    pub left: f64,
    pub amount: f64,
    pub unit: String,
    pub role: FloorRole,
    /// The card chip; `None` above the floor.
    pub chip: Option<Localized>,
    /// The details line.
    pub line: Localized,
}

/// RF3.2: the floors of `account` against `report` (floors whose balance is
/// missing or unknown are left out: an unknown balance is never zero).
pub fn floor_statuses(account: &Account, report: &Report) -> Vec<FloorStatus> {
    if report.status != crate::Status::Ready {
        return Vec::new();
    }
    account
        .floors
        .iter()
        .filter_map(|floor| {
            let metric = floor_target(floor, &report.metrics)?;
            let left = balance_left(metric)?;
            let role = if left <= floor.amount * 0.25 {
                FloorRole::Bad
            } else if left < floor.amount {
                FloorRole::Warn
            } else {
                FloorRole::Ok
            };
            let left_text = amount_text(left, &metric.unit);
            let floor_text = amount_text(floor.amount, &metric.unit);
            Some(FloorStatus {
                label: metric.label.clone(),
                label_text: crate::metric_text::metric_text(&metric.label),
                left,
                amount: floor.amount,
                unit: metric.unit.clone(),
                role,
                chip: (role != FloorRole::Ok).then(|| {
                    Localized::with_param(
                        "budget.floor.chip",
                        "left",
                        left_text.clone(),
                        "Balance low · {left} left",
                    )
                }),
                line: Localized::with_params(
                    "budget.floor.line",
                    params([("floor", floor_text), ("left", left_text)]),
                    "Floor {floor} · {left} left",
                ),
            })
        })
        .collect()
}

/// The budget chip text every surface shows beside a derived metric
/// (RNF honesty: the person's number, never a provider limit).
pub fn budget_badge() -> Localized {
    Localized::fixed("budget.badge", "Your budget")
}

/// RF5.3: whether the details of `metric` (of an account with `account`'s
/// budgets and floors) offer **Set a budget…**: a spend, usage or credits
/// metric no budget targets yet.
pub fn offers_budget(account: &Account, report: &Report, metric: &Metric) -> bool {
    matches!(
        metric.kind,
        MetricKind::Spend | MetricKind::Usage | MetricKind::Credits
    ) && metric.derived_from.is_none()
        && account.budgets.len() < MAX_BUDGETS
        && !account
            .budgets
            .iter()
            .any(|budget| target(budget, &report.metrics).is_some_and(|t| t.label == metric.label))
}

/// RF5.3: whether the details of `metric` offer **Set a floor…**: a balance
/// or credits metric no floor watches yet.
pub fn offers_floor(account: &Account, report: &Report, metric: &Metric) -> bool {
    matches!(metric.kind, MetricKind::Balance | MetricKind::Credits)
        && account.floors.len() < MAX_FLOORS
        && !account.floors.iter().any(|floor| {
            floor_target(floor, &report.metrics).is_some_and(|t| t.label == metric.label)
        })
}

/// RF5.3: a budget prefilled for `metric` (its kind, unit and label, the
/// monthly period); the amount is left for the person (RF6.1 prefills it
/// only from a plan price).
pub fn draft_for(metric: &Metric) -> Budget {
    Budget {
        kind: match metric.kind {
            MetricKind::Usage => Some(BudgetKind::Usage),
            MetricKind::Credits => Some(BudgetKind::Credits),
            _ => Some(BudgetKind::Spend),
        },
        scope: None,
        label_contains: Some(
            crate::metric_text::metric_text(&metric.label)
                .fallback
                .into_owned(),
        ),
        amount: 0.0,
        unit: Some(metric.unit.clone()).filter(|unit| !unit.is_empty()),
        period: match metric.window {
            Some(MetricWindow::Daily) => BudgetPeriod::Daily,
            Some(MetricWindow::Weekly) => BudgetPeriod::Weekly,
            _ => BudgetPeriod::Monthly,
        },
        starts: None,
        label: None,
    }
}

/// RF6.2: the reference line "last 30 days: $38.20" from the recorded
/// samples of a cumulative metric (its rise over the span, resets
/// included); `None` without two samples. Never a prefilled amount.
pub fn recent_reference(samples: &[Point], unit: &str) -> Option<Localized> {
    let (first, rest) = samples.split_first()?;
    if rest.is_empty() {
        return None;
    }
    let mut total = 0.0;
    let mut previous = first.used;
    for point in rest {
        if point.used > previous {
            total += point.used - previous;
        } else if point.used < previous {
            // A reset: the new window's value is all new use.
            total += point.used.max(0.0);
        }
        previous = point.used;
    }
    Some(Localized::with_params(
        "budget.reference.last_30_days",
        params([("amount", amount_text(total, unit))]),
        "Last 30 days: {amount}",
    ))
}

#[path = "budget_editor.rs"]
pub mod editor;

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
