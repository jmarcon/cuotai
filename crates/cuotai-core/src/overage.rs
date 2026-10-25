//! Paid extra usage (overage) state of an account (PRD 0023 RF3).
//!
//! A pure reading of one [`Report`]: the overage metrics
//! ([`Billing::Overage`]) plus the provider's [`OverageHint`] become one
//! state, a running amount and a cycle end. It never queries anything and
//! never turns a missing amount into zero.

use crate::model::{Billing, Metric, MetricKind, OverageHint, Report};
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Utc};
use serde::Serialize;

/// The unit label adapters use for amounts the provider does not price.
pub const PROVIDER_UNIT: &str = "unid. provedor";

/// Where an account stands with paid extra usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverageState {
    /// Extra usage is off (or capped at zero).
    Disabled,
    /// Extra usage is on and nothing has been spent.
    Armed,
    /// Something has been spent on top of the plan.
    Active,
    /// The extra budget is used up.
    CapReached,
    /// Extra usage is on but the amount is missing.
    Unknown,
}

impl OverageState {
    /// Stable snake_case name (events, export).
    pub fn slug(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Armed => "armed",
            Self::Active => "active",
            Self::CapReached => "cap_reached",
            Self::Unknown => "unknown",
        }
    }

    /// Money is being (or has been) billed: `Active` or `CapReached`.
    pub fn is_billing(self) -> bool {
        matches!(self, Self::Active | Self::CapReached)
    }
}

/// One overage metric of the account, kept when the amounts cannot be added.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverageItem {
    pub label: String,
    pub spent: Option<f64>,
    pub cap: Option<f64>,
    pub unit: String,
    pub currency: Option<String>,
}

/// The overage reading of one account.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Overage {
    pub state: OverageState,
    /// Sum of the headline group; `None` when unknown or not addable.
    pub spent: Option<f64>,
    /// Unit of `spent` and `cap` (empty when there is none).
    pub unit: String,
    /// ISO 4217 code when the unit is money.
    pub currency: Option<String>,
    pub cap: Option<f64>,
    pub cycle_ends_at: Option<DateTime<Utc>>,
    /// Label of the first source metric (empty without metrics).
    pub label: String,
    /// Every overage metric, in report order.
    pub items: Vec<OverageItem>,
}

/// The overage object of `export::v1`, the widget snapshot and the event
/// payloads: known values only, never a note.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverageExport {
    pub state: OverageState,
    pub spent: Option<f64>,
    pub unit: Option<String>,
    pub currency: Option<String>,
    pub cap: Option<f64>,
    pub cycle_ends_at: Option<DateTime<Utc>>,
}

impl From<&Overage> for OverageExport {
    fn from(overage: &Overage) -> Self {
        Self {
            state: overage.state,
            spent: overage.spent,
            unit: Some(overage.unit.clone()).filter(|unit| !unit.is_empty()),
            currency: overage.currency.clone(),
            cap: overage.cap,
            cycle_ends_at: overage.cycle_ends_at,
        }
    }
}

impl OverageExport {
    /// The amount spent, formatted for people; `None` when unknown.
    pub fn amount(&self) -> Option<ParamValue> {
        self.spent.map(|value| {
            format_amount(
                value,
                self.unit.as_deref().unwrap_or(""),
                self.currency.as_deref(),
            )
        })
    }
}

fn is_money(metric: &Metric) -> bool {
    metric.kind == MetricKind::Spend
}

/// Reads the overage state of a report. `None` when the provider exposes no
/// overage data at all.
pub fn assess(report: &Report) -> Option<Overage> {
    let metrics: Vec<&Metric> = report
        .metrics
        .iter()
        .filter(|m| m.billing == Billing::Overage)
        .collect();
    if metrics.is_empty() {
        let state = match report.overage_hint? {
            OverageHint::Disabled => OverageState::Disabled,
            OverageHint::Armed => OverageState::Armed,
            OverageHint::Unknown => OverageState::Unknown,
        };
        return Some(Overage {
            state,
            spent: None,
            unit: String::new(),
            currency: None,
            cap: None,
            cycle_ends_at: None,
            label: String::new(),
            items: Vec::new(),
        });
    }
    let items = metrics
        .iter()
        .map(|m| OverageItem {
            label: m.label.clone(),
            spent: m.used.filter(|v| v.is_finite()),
            cap: m.limit.filter(|v| v.is_finite()),
            unit: m.unit.clone(),
            currency: is_money(m).then(|| m.unit.to_ascii_uppercase()),
        })
        .collect();
    // The headline group: money when any metric is money (the amount a
    // person pays), every metric otherwise. It is only added up when the
    // group shares one unit.
    let money: Vec<&Metric> = metrics.iter().copied().filter(|m| is_money(m)).collect();
    let group = if money.is_empty() { &metrics } else { &money };
    let unit = group[0].unit.clone();
    let addable = group.iter().all(|m| m.unit.eq_ignore_ascii_case(&unit));
    let (spent, cap) = if addable {
        let spent = group
            .iter()
            .filter_map(|m| m.used)
            .filter(|v| v.is_finite());
        let spent = spent.fold(None, |acc: Option<f64>, v| Some(acc.unwrap_or(0.0) + v));
        let cap = group
            .iter()
            .map(|m| m.limit.filter(|v| v.is_finite()))
            .collect::<Option<Vec<f64>>>()
            .map(|caps| caps.iter().sum::<f64>());
        (spent, cap)
    } else {
        (None, None)
    };
    let state = if addable {
        classify(spent, cap, report.overage_hint)
    } else {
        // Mixed units: the items speak for themselves.
        let any_spent = group.iter().any(|m| m.used.is_some_and(|v| v > 0.0));
        let any_known = group.iter().any(|m| m.used.is_some());
        match (any_spent, any_known) {
            (true, _) => OverageState::Active,
            (false, true) => OverageState::Armed,
            (false, false) => OverageState::Unknown,
        }
    };
    let state = escalate(state, &metrics);
    Some(Overage {
        state,
        spent,
        currency: (addable && !money.is_empty()).then(|| unit.to_ascii_uppercase()),
        unit: if addable { unit } else { String::new() },
        cap,
        cycle_ends_at: group.iter().find_map(|m| m.resets_at),
        label: metrics[0].label.clone(),
        items,
    })
}

/// Test helper: assess loose metrics as if they were a report's.
#[cfg(test)]
pub(crate) fn assess_metrics(metrics: &[Metric], hint: Option<OverageHint>) -> Option<Overage> {
    use crate::model::{Provider, Status};
    assess(&Report {
        sub_reports: Vec::new(),
        account_id: None,
        as_of: Default::default(),
        diagnostic: None,
        account: String::new(),
        provider: Provider::Claude,
        checked_at: Utc::now(),
        status: Status::Ready,
        metrics: metrics.to_vec(),
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        overage_hint: hint,
    })
}

/// PRD 0037 RF5.5: the headline group (money) cannot see a cap reached or
/// extra usage counted in another unit (credits). Any overage item at its
/// positive cap makes the account `CapReached`; any item above zero turns an
/// `Armed` headline into `Active`. `Disabled` and `Unknown` stay.
fn escalate(state: OverageState, metrics: &[&Metric]) -> OverageState {
    if !matches!(state, OverageState::Armed | OverageState::Active) {
        return state;
    }
    let finite = |v: Option<f64>| v.filter(|v| v.is_finite());
    let capped = metrics.iter().any(|m| {
        matches!((finite(m.used), finite(m.limit)), (Some(used), Some(cap)) if cap > 0.0 && used >= cap)
    });
    if capped {
        return OverageState::CapReached;
    }
    if state == OverageState::Armed
        && metrics
            .iter()
            .any(|m| finite(m.used).is_some_and(|v| v > 0.0))
    {
        return OverageState::Active;
    }
    state
}

fn classify(spent: Option<f64>, cap: Option<f64>, hint: Option<OverageHint>) -> OverageState {
    // A zero cap means on-demand is off, whatever was reported as used.
    if cap == Some(0.0) || (hint == Some(OverageHint::Disabled) && spent.is_none_or(|v| v <= 0.0)) {
        return OverageState::Disabled;
    }
    let Some(spent) = spent else {
        return OverageState::Unknown;
    };
    match cap {
        Some(cap) if cap > 0.0 && spent >= cap => OverageState::CapReached,
        _ if spent > 0.0 => OverageState::Active,
        _ => OverageState::Armed,
    }
}

/// Formats an amount for a person: `$6.25`, `6.25 EUR`, `12.5 unid. provedor`.
/// Provider units get a localizable wrapper.
pub fn format_amount(value: f64, unit: &str, currency: Option<&str>) -> ParamValue {
    match currency {
        Some(code) if code.eq_ignore_ascii_case("USD") => ParamValue::Text(format!("${value:.2}")),
        Some(code) => ParamValue::Text(format!("{value:.2} {code}")),
        None if unit == PROVIDER_UNIT => ParamValue::l10n(Localized::with_param(
            "overage.amount.provider_units",
            "value",
            plain_number(value),
            "{value} provider units",
        )),
        None if unit.is_empty() => ParamValue::Text(plain_number(value)),
        None => ParamValue::Text(format!("{} {unit}", plain_number(value))),
    }
}

fn plain_number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl Overage {
    /// The amount spent, formatted; `None` when it is not known.
    pub fn amount(&self) -> Option<ParamValue> {
        self.spent
            .map(|v| format_amount(v, &self.unit, self.currency.as_deref()))
    }

    /// The pt-BR fallback of [`Self::amount`] (for plain text consumers).
    pub fn amount_text(&self) -> Option<String> {
        match self.amount()? {
            ParamValue::Text(t) => Some(t),
            ParamValue::L10n(l) => Some(l.fallback.into_owned()),
        }
    }

    /// The cap, formatted like the amount; `None` without one.
    pub fn cap_amount(&self) -> Option<ParamValue> {
        self.cap
            .filter(|cap| *cap > 0.0)
            .map(|cap| format_amount(cap, &self.unit, self.currency.as_deref()))
    }

    /// The honesty caveat of an amount in provider units: it is not money.
    pub fn units_caveat(&self) -> Option<Localized> {
        (self.currency.is_none() && self.unit == PROVIDER_UNIT && self.spent.is_some()).then(|| {
            Localized::fixed(
                "overage.units_caveat",
                "Amounts are in provider units, not money.",
            )
        })
    }

    /// The state line: "On extra · $6.25", "Extra usage on · nothing spent",
    /// "Extra budget used up", "Extra usage amount unknown", "Extra usage off".
    pub fn text(&self) -> Localized {
        match (self.state, self.amount()) {
            (OverageState::Active, Some(amount)) => {
                on_extra("overage.on_extra", amount, "On extra · {amount}")
            }
            (OverageState::CapReached, _) => {
                Localized::fixed("overage.cap_reached", "Extra budget used up")
            }
            (OverageState::Armed, _) => {
                Localized::fixed("overage.armed", "Extra usage on · nothing spent")
            }
            (OverageState::Disabled, _) => Localized::fixed("overage.disabled", "Extra usage off"),
            _ => Localized::fixed("overage.unknown", "Extra usage amount unknown"),
        }
    }

    /// The combined card summary with the binding state:
    /// "Limit hit · on extra $6.25". Falls back to [`Self::text`] when the
    /// amount is missing or nothing is being billed.
    pub fn summary_with_limit_hit(&self) -> Localized {
        match (self.state.is_billing(), self.amount()) {
            (true, Some(amount)) => on_extra(
                "overage.limit_hit_on_extra",
                amount,
                "Limit hit · on extra {amount}",
            ),
            _ => self.text(),
        }
    }
}

impl Overage {
    /// The card summary when some model-scoped plan limits are exhausted
    /// while the account itself is not (PRD 0023 RF10): "Opus limit hit ·
    /// on extra $6.25". Falls back to [`Self::text`] when nothing is billed,
    /// no amount is known or no scope is blocked.
    pub fn summary_with_scope_hit(&self, blocks: &[crate::metrics::ScopeBlock]) -> Localized {
        let models = blocks
            .iter()
            .map(|block| block.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        match (self.state.is_billing(), self.amount(), models.is_empty()) {
            (true, Some(amount), false) => Localized::with_params(
                "overage.scope_limit_hit_on_extra",
                params([("model", models.as_str().into()), ("amount", amount)]),
                "{model} limit hit · on extra {amount}",
            ),
            _ => self.text(),
        }
    }
}

fn on_extra(id: &'static str, amount: ParamValue, template: &'static str) -> Localized {
    Localized::with_params(id, params([("amount", amount)]), template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MetricWindow, Provider, Status};

    fn report(metrics: Vec<Metric>, hint: Option<OverageHint>) -> Report {
        Report {
            sub_reports: Vec::new(),
            account_id: None,
            as_of: Default::default(),
            diagnostic: None,
            account: "Work".into(),
            provider: Provider::Claude,
            checked_at: Utc::now(),
            status: Status::Ready,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            overage_hint: hint,
        }
    }

    fn spend(label: &str, used: Option<f64>, limit: Option<f64>, unit: &str) -> Metric {
        let mut m = Metric::usage(
            label,
            0.0,
            unit,
            MetricKind::Spend,
            Some(MetricWindow::Monthly),
        );
        m.used = used;
        m.limit = limit;
        m.overage()
    }

    fn units(used: Option<f64>, limit: Option<f64>) -> Metric {
        let mut m = Metric::usage("extra", 0.0, PROVIDER_UNIT, MetricKind::Quota, None);
        m.used = used;
        m.limit = limit;
        m.overage()
    }

    #[test]
    fn no_overage_data_is_none() {
        assert!(assess(&report(Vec::new(), None)).is_none());
        let plan = Metric::usage("5h", 10.0, "%", MetricKind::Quota, None);
        assert!(assess(&report(vec![plan], None)).is_none());
    }

    #[test]
    fn hint_without_metrics_gives_the_state() {
        for (hint, state) in [
            (OverageHint::Disabled, OverageState::Disabled),
            (OverageHint::Armed, OverageState::Armed),
            (OverageHint::Unknown, OverageState::Unknown),
        ] {
            let o = assess(&report(Vec::new(), Some(hint))).unwrap();
            assert_eq!(o.state, state);
            assert_eq!(o.spent, None);
        }
    }

    #[test]
    fn every_state_from_metrics() {
        let state = |m: Metric| assess(&report(vec![m], None)).unwrap().state;
        assert_eq!(state(units(Some(0.0), Some(50.0))), OverageState::Armed);
        assert_eq!(state(units(Some(12.5), Some(50.0))), OverageState::Active);
        assert_eq!(state(units(Some(12.5), None)), OverageState::Active);
        assert_eq!(
            state(units(Some(50.0), Some(50.0))),
            OverageState::CapReached
        );
        assert_eq!(
            state(units(Some(70.0), Some(50.0))),
            OverageState::CapReached
        );
        assert_eq!(state(units(None, Some(50.0))), OverageState::Unknown);
        assert_eq!(state(units(Some(3.0), Some(0.0))), OverageState::Disabled);
        assert_eq!(
            state(spend("od", None, Some(0.0), "USD")),
            OverageState::Disabled
        );
    }

    #[test]
    fn disabled_hint_wins_only_without_spend() {
        let m = units(Some(0.0), None);
        let o = assess(&report(vec![m], Some(OverageHint::Disabled))).unwrap();
        assert_eq!(o.state, OverageState::Disabled);
        let m = units(Some(2.0), None);
        let o = assess(&report(vec![m], Some(OverageHint::Disabled))).unwrap();
        assert_eq!(o.state, OverageState::Active);
    }

    #[test]
    fn money_metrics_are_summed_per_currency_and_headline_the_group() {
        let metrics = vec![
            units(Some(30.0), None),
            spend("a", Some(1.5), None, "USD"),
            spend("b", Some(2.0), None, "usd"),
        ];
        let o = assess(&report(metrics, None)).unwrap();
        assert_eq!(o.spent, Some(3.5));
        assert_eq!(o.currency.as_deref(), Some("USD"));
        assert_eq!(o.items.len(), 3);
        assert_eq!(o.amount_text().as_deref(), Some("$3.50"));
    }

    #[test]
    fn mixed_currencies_are_listed_not_added() {
        let metrics = vec![
            spend("a", Some(1.5), None, "USD"),
            spend("b", Some(2.0), None, "EUR"),
        ];
        let o = assess(&report(metrics, None)).unwrap();
        assert_eq!(o.state, OverageState::Active);
        assert_eq!(o.spent, None);
        assert_eq!(o.currency, None);
        assert_eq!(o.items.len(), 2);
    }

    /// PRD 0023 RF10: a model at its limit while extra usage bills.
    #[test]
    fn a_model_limit_hit_summary_names_the_model_and_the_amount() {
        use crate::metrics::ScopeBlock;
        let blocks = vec![ScopeBlock {
            index: 0,
            label: "Opus".into(),
            resets_at: None,
        }];
        let active = assess(&report(vec![spend("a", Some(6.25), None, "USD")], None)).unwrap();
        let text = active.summary_with_scope_hit(&blocks);
        assert_eq!(text.id, "overage.scope_limit_hit_on_extra");
        assert_eq!(text.resolve("pt-BR"), "Opus no limite · no extra $6.25");
        assert_eq!(text.resolve("en-US"), "Opus limit hit · on extra $6.25");
        assert_eq!(
            text.resolve("es"),
            "Límite de Opus alcanzado · en extra $6.25"
        );
        assert_eq!(
            text.resolve("it"),
            "Limite di Opus raggiunto · in extra $6.25"
        );
        // Nothing blocked, or nothing billing: the plain state line.
        assert_eq!(active.summary_with_scope_hit(&[]).id, "overage.on_extra");
        let armed = assess(&report(vec![spend("a", Some(0.0), Some(5.0), "USD")], None)).unwrap();
        assert_eq!(armed.summary_with_scope_hit(&blocks).id, "overage.armed");
    }

    #[test]
    fn a_missing_amount_is_never_zero() {
        let o = assess(&report(vec![units(None, None)], None)).unwrap();
        assert_eq!(o.state, OverageState::Unknown);
        assert_eq!(o.spent, None);
        assert!(o.amount().is_none());
    }

    #[test]
    fn a_cap_needs_every_metric_to_report_one() {
        let metrics = vec![
            spend("a", Some(1.0), Some(5.0), "USD"),
            spend("b", Some(1.0), None, "USD"),
        ];
        let o = assess(&report(metrics, None)).unwrap();
        assert_eq!(o.cap, None);
        let metrics = vec![
            spend("a", Some(1.0), Some(5.0), "USD"),
            spend("b", Some(1.0), Some(5.0), "USD"),
        ];
        assert_eq!(assess(&report(metrics, None)).unwrap().cap, Some(10.0));
    }

    #[test]
    fn cycle_end_comes_from_the_headline_group() {
        let at = Utc::now();
        let mut m = spend("a", Some(1.0), None, "USD");
        m.resets_at = Some(at);
        assert_eq!(
            assess(&report(vec![m], None)).unwrap().cycle_ends_at,
            Some(at)
        );
    }

    #[test]
    fn texts_in_the_four_languages() {
        let active = assess(&report(
            vec![spend("a", Some(6.25), Some(50.0), "USD")],
            None,
        ))
        .unwrap();
        let t = active.text();
        assert_eq!(t.resolve("pt-BR"), "No extra · $6.25");
        assert_eq!(t.resolve("en-US"), "On extra · $6.25");
        assert_eq!(t.resolve("es"), "En extra · $6.25");
        assert_eq!(t.resolve("it"), "Extra in uso · $6.25");
        let s = active.summary_with_limit_hit();
        assert_eq!(s.resolve("pt-BR"), "Limite atingido · no extra $6.25");
        assert_eq!(s.resolve("en-US"), "Limit hit · on extra $6.25");
        assert_eq!(s.resolve("es"), "Límite alcanzado · en extra $6.25");
        assert_eq!(s.resolve("it"), "Limite raggiunto · in extra $6.25");

        let units_active = assess(&report(vec![units(Some(12.5), None)], None)).unwrap();
        assert_eq!(
            units_active.text().resolve("en-US"),
            "On extra · 12.5 provider units"
        );

        let cases = [
            (units(Some(0.0), Some(5.0)), "overage.armed"),
            (units(Some(5.0), Some(5.0)), "overage.cap_reached"),
            (units(None, None), "overage.unknown"),
            (units(Some(0.0), Some(0.0)), "overage.disabled"),
        ];
        for (metric, id) in cases {
            let o = assess(&report(vec![metric], None)).unwrap();
            let text = o.text();
            assert_eq!(text.id, id);
            for locale in crate::text::catalog_locales() {
                assert_ne!(text.resolve(locale), text.fallback, "{id} in {locale}");
            }
        }
    }
}
