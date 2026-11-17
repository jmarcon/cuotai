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
    Account, Billing, DerivedFrom, Metric, MetricKind, MetricWindow, Provider, Report, ScopeKind,
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
/// The raw label a budget without its own label had before the PRD 0041
/// re-validation (D2): rendered through [`crate::metric_text::metric_text`]
/// as "Budget · {label}". Kept to recognize restored readings, legacy
/// alert marks and history series; new budgets name their period
/// ([`metric_label`]).
pub const DEFAULT_LABEL_PREFIX: &str = "Orçamento · ";
/// The raw prefixes of the default budget labels that name the period
/// ([`metric_label`]), each followed by the target's label. Like every
/// adapter text they stay in pt-BR, so they are stable in the history.
const DAILY_PREFIX: &str = "Orçamento diário · ";
const WEEKLY_PREFIX: &str = "Orçamento semanal · ";
const MONTHLY_PREFIX: &str = "Orçamento mensal · ";
const MONTHLY_FROM_PREFIX: &str = "Orçamento mensal desde o dia ";
const CYCLE_PREFIX: &str = "Orçamento do ciclo · ";
/// How far a provider's own reset may be from the budget boundary and
/// still count as the same period.
const SAME_BOUNDARY: TimeDelta = TimeDelta::hours(1);
/// A provider month (often UTC) within this distance of the local month
/// boundary is accepted for a monthly budget starting on the 1st, with a
/// note (RF2.2).
const MONTH_SLACK: TimeDelta = TimeDelta::hours(15);
/// The longest stretch between two readings (or between the period start
/// and its first reading) whose unrecorded use is taken as negligible: a
/// longer one across a reset or a top-up makes the result a lower bound.
const GAP: TimeDelta = TimeDelta::hours(1);

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
    /// PRD 0045 RF5.2: the breakdown row whose metric the budget follows
    /// (`row = { scope = "key", id = "..." }`); `None` for the account's
    /// own metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<RowTarget>,
}

// Validation rejects non-finite amounts, so equality is reflexive.
impl Eq for Budget {}

/// PRD 0045 RF5.2: a breakdown row by its scope and opaque id, as the
/// breakdown reports them (never its label).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowTarget {
    pub scope: ScopeKind,
    pub id: String,
}

impl RowTarget {
    /// Whether `row` is this one.
    pub fn matches(&self, row: &crate::BreakdownRow) -> bool {
        row.scope == self.scope && row.id == self.id && !row.is_others()
    }
}

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
    /// Two budgets with the same identity ([`budget_key`]): the same
    /// selector, filter, row and period, whatever their unit.
    Duplicate,
    /// `starts` does not fit the period.
    Start,
    /// `period = "window"` on a metric that never has a reset.
    NoWindow,
    /// A label longer than [`LABEL_MAX`] characters, or blank.
    Label,
    /// PRD 0045 RF5.2: a row target with `scope = "account"`, a `model`
    /// scope, an empty or too long id, or on a provider without breakdowns.
    Row,
    /// Two floors on the same balance at the same amount
    /// ([`floor_key`]).
    DuplicateFloor,
    /// Two budgets of one account with the same own label (ignoring
    /// case).
    DuplicateLabel,
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
            Self::Row => (
                "budget.error.row",
                "{field}: a budget on a breakdown row needs a kind, the row's scope (key, virtual_key, project, workspace or team) and its id, on a provider that offers breakdowns.",
            ),
            Self::DuplicateFloor => (
                "budget.error.duplicate_floor",
                "{field}: another floor already watches the same balance at the same amount.",
            ),
            Self::DuplicateLabel => (
                "budget.error.duplicate_label",
                "{field}: another budget of this account already has this label.",
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
    let mut keys = std::collections::HashSet::new();
    let mut labels = std::collections::HashSet::new();
    let mut floor_keys = std::collections::HashSet::new();
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
        if let Some(row) = &budget.row {
            let offers = crate::access::may_break_down(provider);
            if budget.scope.is_some()
                || row.scope == ScopeKind::Model
                || row.id.trim().is_empty()
                || row.id.len() > crate::breakdown::ID_MAX
                || !offers
            {
                push(at("row"), BudgetRule::Row);
            }
        }
        if !keys.insert(budget_key(budget)) {
            push(at("period"), BudgetRule::Duplicate);
        }
        if let Some(label) = budget.label.as_deref().map(str::trim) {
            if !label.is_empty() && !labels.insert(label.to_lowercase()) {
                push(at("label"), BudgetRule::DuplicateLabel);
            }
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
        if !floor_keys.insert(floor_key(floor)) {
            push(at("amount"), BudgetRule::DuplicateFloor);
        }
    }
    out
}

/// `prefix` + the first 12 hex digits of the digest of `identity`.
fn short_key(prefix: char, identity: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(identity).expect("a budget identity serializes");
    let mut key = crate::models::digest(&bytes);
    key.truncate(12);
    key.insert(0, prefix);
    key
}

fn folded(text: Option<&str>) -> Option<String> {
    text.map(|text| text.trim().to_lowercase())
}

/// PRD 0041 re-validation (D2): the stable identity of `budget`, derived
/// from what it follows and its period (never written to the TOML): its
/// kind or scope, its `label_contains` filter (ignoring case), its row and
/// its period. The amount, label, unit and `starts` take no part, so
/// editing them keeps the budget's alerts and history series; following
/// another metric or period makes a new budget. Two budgets of one account
/// never share it ([`BudgetRule::Duplicate`]).
pub fn budget_key(budget: &Budget) -> String {
    let selector = match (budget.kind, budget.scope) {
        (Some(kind), None) => kind.slug(),
        (None, Some(BudgetScope::Account)) => "scope:account",
        (Some(_), Some(_)) => "both",
        (None, None) => "none",
    };
    let row = budget
        .row
        .as_ref()
        .map(|row| (serde_json::to_value(row.scope).ok(), row.id.as_str()));
    short_key(
        'b',
        &(
            selector,
            folded(budget.label_contains.as_deref()),
            budget.period.slug(),
            row,
        ),
    )
}

/// PRD 0041 re-validation (D2): the stable identity of `floor`: its kind,
/// filter and unit (ignoring case) and its amount. A changed amount makes
/// a new floor, which warns once if the balance is already below it.
pub fn floor_key(floor: &Floor) -> String {
    short_key(
        'f',
        &(
            floor.kind.map(FloorKind::slug),
            folded(floor.label_contains.as_deref()),
            folded(floor.unit.as_deref()),
            floor.amount,
        ),
    )
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

/// The raw label of the budget metric for `budget` on `target`: its own
/// label, else a default that names the period ("Monthly budget · {label}",
/// PRD 0041 re-validation D2), so two budgets on one metric read apart.
pub fn metric_label(budget: &Budget, target_label: &str) -> String {
    match budget.label.as_deref().map(str::trim) {
        Some(label) if !label.is_empty() => label.to_owned(),
        _ => default_label(budget.period, budget.starts, target_label),
    }
}

/// The default label of a budget over `period` (starting `starts`) on the
/// metric labelled `target_label`.
fn default_label(period: BudgetPeriod, starts: Option<BudgetStart>, target_label: &str) -> String {
    match (period, starts) {
        (BudgetPeriod::Daily, _) => format!("{DAILY_PREFIX}{target_label}"),
        (BudgetPeriod::Weekly, _) => format!("{WEEKLY_PREFIX}{target_label}"),
        (BudgetPeriod::Monthly, Some(BudgetStart::Day(day))) if (2..=28).contains(&day) => {
            format!("{MONTHLY_FROM_PREFIX}{day} · {target_label}")
        }
        (BudgetPeriod::Monthly, _) => format!("{MONTHLY_PREFIX}{target_label}"),
        (BudgetPeriod::Window, _) => format!("{CYCLE_PREFIX}{target_label}"),
    }
}

/// The target label inside a default budget label (any period, or the
/// legacy "Orçamento · {label}"); `None` for any other label.
fn default_label_target(label: &str) -> Option<&str> {
    [
        DEFAULT_LABEL_PREFIX,
        DAILY_PREFIX,
        WEEKLY_PREFIX,
        MONTHLY_PREFIX,
        CYCLE_PREFIX,
    ]
    .iter()
    .find_map(|prefix| label.strip_prefix(prefix))
    .or_else(|| {
        let rest = label.strip_prefix(MONTHLY_FROM_PREFIX)?;
        let (day, target) = rest.split_once(" · ")?;
        day.parse::<u8>().ok()?;
        Some(target)
    })
    .filter(|target| !target.is_empty())
}

/// Whether `label` is a default budget label (of any period, or the
/// legacy form): a series or reading of the person's numbers, never of a
/// provider, whose labels are never written this way.
pub fn is_default_label(label: &str) -> bool {
    default_label_target(label).is_some()
}

/// The label a budget metric had before the PRD 0041 re-validation (D2):
/// its own label, or "Orçamento · {target}" for a default label. The alert
/// marks and history series written under it move to the budget's key.
pub fn legacy_label(metric: &Metric) -> String {
    match default_label_target(&metric.label) {
        Some(target) => format!("{DEFAULT_LABEL_PREFIX}{target}"),
        None => metric.label.clone(),
    }
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

/// One recorded reading of a budget's target metric: when it was taken,
/// its value (`used`, or `remaining` for a balance) and the provider's own
/// reset as reported then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub taken_at: DateTime<Utc>,
    pub value: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

impl From<Point> for Reading {
    fn from(point: Point) -> Self {
        Self {
            taken_at: point.taken_at,
            value: point.used,
            resets_at: None,
        }
    }
}

/// What [`apply`] reads from the history: the recorded readings of the
/// target metric (by its series) from `since` to now, in time order.
pub trait Baselines {
    fn samples(&self, target: &Metric, since: DateTime<Utc>) -> Vec<Reading>;
}

/// No history: every derivation that needs a baseline yields `None`.
pub struct NoHistory;

impl Baselines for NoHistory {
    fn samples(&self, _: &Metric, _: DateTime<Utc>) -> Vec<Reading> {
        Vec::new()
    }
}

impl<F, R> Baselines for F
where
    F: Fn(&Metric, DateTime<Utc>) -> Vec<R>,
    R: Into<Reading>,
{
    fn samples(&self, target: &Metric, since: DateTime<Utc>) -> Vec<Reading> {
        self(target, since).into_iter().map(Into::into).collect()
    }
}

/// Whether the target counts down (a prepaid balance): RF2.3.
fn counts_down(target: &Metric) -> bool {
    target.kind == MetricKind::Balance || target.is_balance_shaped()
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
    /// The use added up reading by reading since the period start
    /// ([`counted_use`]).
    Counted { topped_up: bool },
}

/// A budget's use in its period: the amount, and whether it is only a
/// lower bound (`partial`: part of the period was not recorded).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Use {
    used: f64,
    partial: bool,
    how: Derivation,
}

/// RF2.2/RF2.3: the use of `target` in the period starting at `start`
/// (ending at `end`) at `now`, or the reason it is unknown.
fn used_in_period(
    budget: &Budget,
    target: &Metric,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
    history: &dyn Baselines,
) -> Result<Use, &'static str> {
    let current = current_value(target).ok_or("target_missing")?;
    let covers = |window: MetricWindow| {
        target.window == Some(window)
            && target
                .resets_at
                .is_some_and(|at| (at - end).abs() <= SAME_BOUNDARY)
    };
    if !counts_down(target) {
        let first_of_month = budget.period == BudgetPeriod::Monthly
            && matches!(budget.starts, None | Some(BudgetStart::Day(1)));
        let how = if budget.period == BudgetPeriod::Window
            || budget.period.window().is_some_and(covers)
        {
            Some(Derivation::Provider)
        } else if first_of_month
            && target.window == Some(MetricWindow::Monthly)
            && target
                .resets_at
                .is_none_or(|at| (at - end).abs() <= MONTH_SLACK)
        {
            Some(Derivation::ProviderMonth)
        } else {
            None
        };
        if let Some(how) = how {
            // F-16: a negative spend is a net credit, not a use of 0 or
            // less; it is unknown rather than clamped.
            if current < 0.0 {
                return Err("negative");
            }
            return Ok(Use {
                used: current,
                partial: false,
                how,
            });
        }
    }
    let readings: Vec<Reading> = history
        .samples(target, start)
        .into_iter()
        .filter(|reading| {
            reading.taken_at >= start && reading.taken_at <= now && reading.value.is_finite()
        })
        .collect();
    if readings.is_empty() {
        return Err("no_baseline");
    }
    let latest = Reading {
        taken_at: now,
        value: current,
        resets_at: target.resets_at,
    };
    counted_use(&readings, latest, start, target)
}

/// RF2.2/RF2.3 (design D1): the use added up segment by segment over the
/// readings of the period (`readings`, in time order, the first one being
/// the baseline) and the current reading `latest`. A counter that restarts
/// at the provider's reset adds the value after the reset; a balance adds
/// its drops and treats a rise as a top-up. A stretch longer than [`GAP`]
/// whose use cannot be known (before the first reading, across a reset or
/// a top-up, or a counter that went down) makes the result a lower bound.
fn counted_use(
    readings: &[Reading],
    latest: Reading,
    start: DateTime<Utc>,
    target: &Metric,
) -> Result<Use, &'static str> {
    let first = readings.first().ok_or("no_baseline")?;
    let mut partial = first.taken_at - start > GAP;
    let mut used = 0.0;
    let mut topped_up = false;
    let balance = counts_down(target);
    let mut previous = *first;
    for next in readings.iter().skip(1).copied().chain([latest]) {
        let (a, b) = (previous, next);
        let gap = b.taken_at - a.taken_at;
        if balance {
            if b.value <= a.value {
                used += a.value - b.value;
            } else {
                // A top-up: use and a top-up inside one long gap hide
                // each other.
                topped_up = true;
                partial |= gap > GAP;
            }
        } else {
            // A reset is known when the instant `a` announced has passed
            // by `b`, or when `b` belongs to a later instance and the
            // counter went down. A `resets_at` that only rolls forward
            // with the clock is not a reset.
            let reset = a.resets_at.filter(|at| {
                (*at > a.taken_at && *at <= b.taken_at)
                    || (b.value < a.value
                        && b.resets_at.is_some_and(|next| next > *at + SAME_BOUNDARY))
            });
            if let Some(reset) = reset.map(|at| at.min(b.taken_at)) {
                // The use between `a` and the reset is not recorded.
                partial |= reset - a.taken_at > GAP;
                // Neither is an instance that came and went in between.
                let skipped = b
                    .resets_at
                    .and_then(|next| provider_window_start(target.window, next))
                    .is_some_and(|began| began > reset + SAME_BOUNDARY);
                partial |= skipped;
                used += b.value.max(0.0);
            } else if b.value >= a.value {
                used += b.value - a.value;
            } else {
                let same_instance = match (a.resets_at, b.resets_at) {
                    (Some(x), Some(y)) => (x - y).abs() <= SAME_BOUNDARY,
                    _ => false,
                };
                if same_instance {
                    // A refund or a revised estimate: the counter is not
                    // monotonic, so what was used is not known exactly.
                    partial = true;
                } else {
                    // A reset at an unknown time: the counter restarted.
                    used += b.value.max(0.0);
                    partial |= gap > GAP;
                }
            }
        }
        previous = next;
    }
    if !used.is_finite() {
        return Err("not_finite");
    }
    Ok(Use {
        used,
        partial,
        how: Derivation::Counted { topped_up },
    })
}

/// A budget note naming the budget by its display text (`metric_text` of
/// its label), so the name reads in the reader's language too (PRD 0041
/// re-validation F-09).
fn note(id: &'static str, label: &str, fallback: &'static str) -> Localized {
    Localized::with_param(
        id,
        "budget",
        ParamValue::l10n(crate::metric_text::metric_text(label)),
        fallback,
    )
}

/// PRD 0041 re-validation (F-06): the start of a budget period as a person
/// reads it, "since Tue" or "since 11/01", in `tz` at `now`.
pub fn period_since<Tz: TimeZone>(start: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> Localized {
    let day = crate::text::clock::day(
        start.with_timezone(tz).date_naive(),
        now.with_timezone(tz).date_naive(),
    );
    Localized::with_param(
        "budget.period_since",
        "day",
        ParamValue::l10n(day),
        "since {day}",
    )
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
    let mut notes = Vec::new();
    for (index, budget) in account.budgets.iter().enumerate() {
        if budget.row.is_none() {
            added.push(derive(
                index,
                budget,
                &report.metrics,
                history,
                now,
                tz,
                &mut notes,
            ));
        }
    }
    report.metrics.extend(added);
    for note in notes {
        push_note(report, note);
    }
}

/// PRD 0045 RF5.2: the baselines of a row's metric since a time (the row
/// unit's series).
pub type RowBaselines<'a> =
    dyn Fn(&crate::BreakdownRow, &Metric, DateTime<Utc>) -> Vec<Reading> + 'a;

/// PRD 0045 RF5.2: the budgets of `account` that follow a breakdown row,
/// each added to its row as a derived quota (after the row's own metrics,
/// beyond [`crate::breakdown::MAX_ROW_METRICS`]), with its notes on the
/// row. `history` gives the baselines of a row's metric (the row unit's
/// series, [`crate::breakdown::row_account`]). A row absent from the
/// reading adds the account note `note.budget.target_missing`. Earlier
/// derived row metrics are dropped first.
pub fn apply_rows<Tz: TimeZone>(
    account: &Account,
    report: &mut Report,
    history: &RowBaselines,
    now: DateTime<Utc>,
    tz: &Tz,
) {
    for row in &mut report.breakdown {
        row.metrics.retain(|metric| metric.derived_from.is_none());
    }
    if report.status != crate::Status::Ready {
        return;
    }
    let mut missing = Vec::new();
    for (index, budget) in account.budgets.iter().enumerate() {
        let Some(wanted) = &budget.row else {
            continue;
        };
        let Some(row) = report.breakdown.iter_mut().find(|row| wanted.matches(row)) else {
            let label = metric_label(budget, budget.label_contains.as_deref().unwrap_or("spend"));
            missing.push(note(
                "note.budget.target_missing",
                &label,
                "{budget}: the metric this budget follows is missing from this reading.",
            ));
            continue;
        };
        let snapshot = row.clone();
        let baselines = |metric: &Metric, since: DateTime<Utc>| history(&snapshot, metric, since);
        let mut notes = Vec::new();
        let metric = derive(
            index,
            budget,
            &snapshot.metrics,
            &baselines,
            now,
            tz,
            &mut notes,
        );
        row.metrics.push(metric);
        row.notes.extend(notes);
    }
    for note in missing {
        push_note(report, note);
    }
}

fn push_note_to(notes: &mut Vec<Localized>, note: Localized) {
    notes.push(note);
}

/// One budget's derived quota over `metrics` (the account's or a row's),
/// with its notes pushed to `notes`.
fn derive<Tz: TimeZone>(
    index: usize,
    budget: &Budget,
    metrics: &[Metric],
    history: &dyn Baselines,
    now: DateTime<Utc>,
    tz: &Tz,
    notes: &mut Vec<Localized>,
) -> Metric {
    let target = target(budget, metrics).cloned();
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
    let display = label.clone();
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
            partial: false,
            key: budget_key(budget),
            period_start: bounds.map(|(start, _)| start),
            period_text: bounds.map(|(start, _)| period_since(start, now, tz)),
        }),
        unlimited: false,
    };
    match (&target, bounds) {
        (None, _) => push_note_to(
            notes,
            note(
                "note.budget.target_missing",
                &display,
                "{budget}: the metric this budget follows is missing from this reading.",
            ),
        ),
        (Some(_), None) => push_note_to(
            notes,
            note(
                "note.budget.no_window",
                &display,
                "{budget}: the metric has no reset of its own, so period = \"window\" cannot count.",
            ),
        ),
        (Some(target), Some((start, end))) => {
            if budget.unit.as_deref().is_some_and(|unit| !target.unit.eq_ignore_ascii_case(unit)) {
                push_note_to(
                    notes,
                    note(
                        "note.budget.unit_mismatch",
                        &display,
                        "{budget}: the metric is reported in another unit, so this budget cannot count.",
                    ),
                );
            } else {
                match used_in_period(budget, target, start, end, now, history) {
                    Ok(Use { used, partial, how }) => {
                        metric.used = Some(used);
                        if partial {
                            // A lower bound: what is left is not known.
                            if let Some(from) = &mut metric.derived_from {
                                from.partial = true;
                            }
                            push_note_to(
                                notes,
                                note(
                                    "note.budget.partial",
                                    &display,
                                    "{budget}: part of this period's use was not recorded, so at least this much was used.",
                                ),
                            );
                        } else {
                            metric.remaining = Some((budget.amount - used).max(0.0));
                        }
                        match how {
                            Derivation::ProviderMonth => push_note_to(
                                notes,
                                note(
                                    "note.budget.provider_month",
                                    &display,
                                    "{budget}: counted over the provider's billing month, which may start in another time zone.",
                                ),
                            ),
                            Derivation::Counted { topped_up: true } => push_note_to(
                                notes,
                                note(
                                    "note.budget.topup_detected",
                                    &display,
                                    "{budget}: a top-up was detected; it does not count as use.",
                                ),
                            ),
                            _ => {}
                        }
                    }
                    Err("negative") => push_note_to(
                        notes,
                        note(
                            "note.budget.negative",
                            &display,
                            "{budget}: the metric shows a net credit this period, so this budget cannot count it.",
                        ),
                    ),
                    Err("no_baseline") => push_note_to(
                        notes,
                        note(
                            "note.budget.no_baseline",
                            &display,
                            "{budget}: counting starts with the first reading of this period.",
                        ),
                    ),
                    Err(_) => push_note_to(
                        notes,
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
    metric
}

/// A report restored from the history (PRD 0010 RF2.3) carries the budget
/// metrics as plain quotas: mark the ones whose label is one of the
/// account's budgets again (the label the budget gives now, or the one it
/// gave before the PRD 0041 re-validation), so every surface still labels
/// them as the person's budget. A quota with a default budget label that
/// no budget claims (the budget was removed) is dropped: a removed budget
/// never reads as a provider limit (F-26). `now` and `tz` word the period
/// again ("since Tue").
pub fn mark_restored<Tz: TimeZone>(
    account: &Account,
    report: &mut Report,
    now: DateTime<Utc>,
    tz: &Tz,
) {
    let originals: Vec<Metric> = report
        .metrics
        .iter()
        .filter(|metric| metric.derived_from.is_none() && metric.kind != MetricKind::Quota)
        .cloned()
        .collect();
    let candidates: Vec<(usize, &Budget, Option<&Metric>, String)> = account
        .budgets
        .iter()
        .enumerate()
        .filter(|(_, budget)| budget.row.is_none())
        .map(|(index, budget)| {
            let target = target(budget, &originals);
            let label = target.map_or_else(
                || budget.label.clone().unwrap_or_default(),
                |target| metric_label(budget, &target.label),
            );
            (index, budget, target, label)
        })
        .collect();
    let mut claimed = vec![false; account.budgets.len()];
    report.metrics.retain_mut(|metric| {
        if metric.derived_from.is_some() || metric.kind != MetricKind::Quota {
            return true;
        }
        let legacy = legacy_label(metric);
        let found = candidates.iter().find(|(index, budget, target, label)| {
            !claimed[*index]
                && !label.is_empty()
                && (metric.label == *label
                    || target.is_some_and(|target| {
                        budget.label.is_none()
                            && legacy == format!("{DEFAULT_LABEL_PREFIX}{}", target.label)
                    }))
        });
        let Some((index, budget, target, _)) = found else {
            return !is_default_label(&metric.label);
        };
        claimed[*index] = true;
        let period_start = metric
            .period
            .as_deref()
            .and_then(|period| DateTime::parse_from_rfc3339(period).ok())
            .map(|start| start.with_timezone(&Utc));
        metric.derived_from = Some(DerivedFrom {
            period_start,
            period_text: period_start.map(|start| period_since(start, now, tz)),
            kind: target
                .map(|t| t.kind)
                .or_else(|| budget.kind.map(BudgetKind::metric_kind))
                .unwrap_or(MetricKind::Spend),
            label: target.map_or_else(String::new, |t| t.label.clone()),
            budget: *index,
            partial: false,
            key: budget_key(budget),
        });
        true
    });
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
        && !account.budgets.iter().any(|budget| {
            budget.row.is_none()
                && target(budget, &report.metrics).is_some_and(|t| t.label == metric.label)
        })
}

/// PRD 0045 RF5.2: whether the details offer **Set a budget…** on `row`:
/// not the "Others" row, the account has room for one more budget and no
/// budget follows the row yet.
pub fn offers_row_budget(account: &Account, row: &crate::BreakdownRow) -> bool {
    !row.is_others()
        && account.budgets.len() < MAX_BUDGETS
        && !account
            .budgets
            .iter()
            .filter_map(|budget| budget.row.as_ref())
            .any(|wanted| wanted.matches(row))
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
        row: None,
    }
}

/// PRD 0045 RF5.2: a budget prefilled for the primary metric of `row` (its
/// kind, unit and period, the row's scope and id); `None` for the "Others"
/// row or a row without a spend, usage or credits metric.
pub fn draft_for_row(row: &crate::BreakdownRow) -> Option<Budget> {
    if row.is_others() {
        return None;
    }
    let metric = row.metrics.iter().find(|m| {
        matches!(
            m.kind,
            MetricKind::Spend | MetricKind::Usage | MetricKind::Credits
        )
    })?;
    Some(Budget {
        row: Some(RowTarget {
            scope: row.scope,
            id: row.id.clone(),
        }),
        ..draft_for(metric)
    })
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

#[cfg(test)]
#[path = "budget_reset_tests.rs"]
mod reset_tests;

#[cfg(test)]
#[path = "budget_identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "budget_l10n_tests.rs"]
mod l10n_tests;
