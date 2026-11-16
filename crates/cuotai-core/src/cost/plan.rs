//! PRD 0030 RF1 and RF6: what the person pays and the second currency.
//!
//! A **plan** is a price the person typed for an account (per month, or per
//! cycle starting on `cycle_day`); the **display currency** is a rate the
//! person typed ("1 USD = 5.42 BRL"). Neither is ever downloaded, written to
//! the TOML or exported: they live in the machine-local cost settings.
//!
//! [`plan_values`] compares the estimate of the current cycle with the plan
//! price. It always says "estimate at API prices", divides only when the
//! two are in the same currency (or the plan currency is the display
//! currency, "at your rate"), never sums two currencies and never splits
//! one source's estimate between two accounts.

use super::estimate::{estimate, Estimate, Usd};
use super::prices::Prices;
use super::{CostError, DailyUsage, SourceId};
use crate::text::number::{money, number};
use crate::text::{params, Localized, ParamValue};
use crate::Account;
use chrono::{Datelike, Duration as Days, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The last day of the month a cycle may start on (every month has it).
pub const MAX_CYCLE_DAY: u8 = 28;

/// The highest plan price accepted.
const MAX_PRICE: f64 = 1_000_000_000.0;

/// The highest exchange rate accepted.
const MAX_RATE: f64 = 1_000_000.0;

/// L-01: the smallest exchange rate kept (six significant digits are kept
/// above it).
const MIN_RATE: f64 = 0.000_001;

/// From this many days after it was typed, a rate asks to be updated.
pub const RATE_STALE_DAYS: u32 = 30;

/// An ISO 4217 shaped code: three ASCII letters, upper-cased.
fn currency_code(code: &str) -> Option<String> {
    let code = code.trim();
    (code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic()))
        .then(|| code.to_ascii_uppercase())
}

/// Two decimals, exactly.
fn cents(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// L-01: `value` rounded to six significant digits.
fn significant(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let scale = 10f64.powi(5 - value.abs().log10().floor() as i32);
    (value * scale).round() / scale
}

/// RF1.1: what the person pays for the plan of one account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanSetting {
    /// Greater than zero, two decimals.
    pub price: f64,
    /// An ISO 4217 code, upper case.
    pub currency: String,
    /// 1 to [`MAX_CYCLE_DAY`]; `None` is 1, the calendar month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_day: Option<u8>,
}

impl PlanSetting {
    /// A validated plan: `price` > 0 (rounded to cents), `currency` three
    /// letters, `cycle_day` 1 to 28.
    pub fn new(price: f64, currency: &str, cycle_day: Option<u8>) -> Result<Self, CostError> {
        let price = cents(price);
        let currency = currency_code(currency).ok_or(CostError::InvalidPlan)?;
        if !price.is_finite() || price <= 0.0 || price > MAX_PRICE {
            return Err(CostError::InvalidPlan);
        }
        if cycle_day.is_some_and(|day| !(1..=MAX_CYCLE_DAY).contains(&day)) {
            return Err(CostError::InvalidPlan);
        }
        Ok(Self {
            price,
            currency,
            cycle_day,
        })
    }

    /// The day of the month the cycle starts on.
    pub fn cycle_day(&self) -> u8 {
        self.cycle_day
            .filter(|day| (1..=MAX_CYCLE_DAY).contains(day))
            .unwrap_or(1)
    }
}

/// The first day of the cycle that contains `today`, for a cycle starting
/// on `cycle_day` of each month (1 to 28).
pub fn cycle_start(today: NaiveDate, cycle_day: u8) -> NaiveDate {
    let day = u32::from(cycle_day.clamp(1, MAX_CYCLE_DAY));
    let this_month = today.with_day(day).expect("1 to 28 exists in every month");
    if today.day() >= day {
        this_month
    } else {
        // The 28th of the previous month at the latest exists too.
        (today.with_day(1).expect("the first exists") - Days::days(1))
            .with_day(day)
            .expect("1 to 28 exists in every month")
    }
}

/// RF6.1: a second currency at a rate the person typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayCurrency {
    /// An ISO 4217 code other than `USD`.
    pub code: String,
    /// How many units of `code` one US dollar is.
    pub rate: f64,
    /// The local day the rate was typed.
    pub set_on: NaiveDate,
}

impl DisplayCurrency {
    /// A validated rate: `code` three letters other than `USD`, `rate`
    /// above zero (L-01: kept to six significant digits, at least
    /// [`MIN_RATE`]).
    pub fn new(code: &str, rate: f64, set_on: NaiveDate) -> Result<Self, CostError> {
        let code = currency_code(code).ok_or(CostError::InvalidRate)?;
        if code == super::CURRENCY || !rate.is_finite() || rate <= 0.0 || rate > MAX_RATE {
            return Err(CostError::InvalidRate);
        }
        let rate = significant(rate);
        if rate < MIN_RATE {
            return Err(CostError::RateTooSmall);
        }
        Ok(Self { code, rate, set_on })
    }

    /// Days since the rate was typed (0 for a date in the future).
    pub fn age_days(&self, today: NaiveDate) -> u32 {
        u32::try_from((today - self.set_on).num_days()).unwrap_or(0)
    }

    /// From [`RATE_STALE_DAYS`] days on, the rate asks to be updated.
    pub fn stale(&self, today: NaiveDate) -> bool {
        self.age_days(today) >= RATE_STALE_DAYS
    }

    /// `usd` in the display currency, to two decimals; computed for
    /// display only, never stored or exported.
    pub fn convert(&self, usd: Usd) -> f64 {
        cents(usd.as_usd() * self.rate)
    }

    /// C-03: `usd` converted, as an `amount.money` text in the display
    /// currency.
    pub fn amount(&self, usd: Usd) -> Localized {
        money(self.convert(usd), &self.code)
    }

    /// C-08: "set today", "set yesterday" or "set N days ago".
    pub fn age_text(&self, today: NaiveDate) -> Localized {
        match self.age_days(today) {
            0 => Localized::fixed("cost.rate.age.today", "set today"),
            1 => Localized::fixed("cost.rate.age.yesterday", "set yesterday"),
            days => Localized::plural(
                "cost.rate.age.days",
                params([("count", u64::from(days).into())]),
                "set {count} day ago",
                "set {count} days ago",
            ),
        }
    }

    /// RF6.2: "≈ R$ 1.149,00 at your rate (set 12 days ago)", plus "update
    /// your rate" from [`RATE_STALE_DAYS`] days on.
    pub fn text(&self, usd: Usd, today: NaiveDate) -> Localized {
        self.line(ParamValue::l10n(self.amount(usd)), today)
    }

    /// C-07: the line for an estimate: `None` without usage or when none
    /// of it has a price; "{amount} + unknown" when part of it has none.
    pub fn text_for(&self, estimate: &Estimate, today: NaiveDate) -> Option<Localized> {
        let total = estimate.total?;
        if estimate.tokens.total() == 0 {
            return None;
        }
        let amount = self.amount(total);
        let amount = if estimate.unknown_tokens.total() > 0 {
            Localized::with_param(
                "cost.display.partial",
                "amount",
                amount,
                "{amount} + unknown",
            )
        } else {
            amount
        };
        Some(self.line(ParamValue::l10n(amount), today))
    }

    fn line(&self, amount: ParamValue, today: NaiveDate) -> Localized {
        let days = self.age_days(today);
        let (id, fallback) = if days >= RATE_STALE_DAYS {
            (
                "cost.display.line_stale",
                "≈ {amount} at your rate ({age}); update your rate",
            )
        } else {
            ("cost.display.line", "≈ {amount} at your rate ({age})")
        };
        // `days` stays for consumers written for it.
        Localized::with_params(
            id,
            params([
                ("amount", amount),
                ("age", ParamValue::l10n(self.age_text(today))),
                ("days", u64::from(days).into()),
            ]),
            fallback,
        )
    }
}

/// RF1.1: one configured account whose tool a source reads, with the plan
/// price saved for it (if any); what the front ends list to type a plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanAccount {
    /// [`Account::state_key`]: the key the plan is saved under.
    pub id: String,
    pub name: String,
    pub provider: crate::Provider,
    pub source: SourceId,
    pub label: &'static str,
    pub plan: Option<PlanSetting>,
}

/// The accounts of `accounts` shown by a source, in configuration order,
/// each with its plan (saved under its id, or still under the legacy
/// `provider:name` key of an earlier build).
pub fn plan_accounts(
    plans: &BTreeMap<String, PlanSetting>,
    accounts: &[Account],
) -> Vec<PlanAccount> {
    accounts
        .iter()
        .filter_map(|account| {
            let source = SourceId::for_provider(account.provider)?;
            Some(PlanAccount {
                id: account.state_key(),
                // L-18: an unnamed account shows its provider.
                name: account.display_name().into_owned(),
                provider: account.provider,
                source,
                label: source.label(),
                plan: plans
                    .get(&account.state_key())
                    .or_else(|| plans.get(&account.key()))
                    .cloned(),
            })
        })
        .collect()
}

/// RF1.1 / PRD 0029 RF2.3: move the plans saved under a legacy
/// `provider:name` key of `accounts` to the account id. An id that already
/// has a plan wins. Idempotent; `true` when something moved.
pub fn migrate_plan_keys(plans: &mut BTreeMap<String, PlanSetting>, accounts: &[Account]) -> bool {
    let legacy: BTreeMap<String, String> = accounts
        .iter()
        .map(|account| (account.key(), account.state_key()))
        .collect();
    crate::account_id::migrate_map(plans, &legacy)
}

/// One price a source's accounts pay, in one currency.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanAmount {
    pub currency: String,
    pub price: f64,
}

/// RF1.2: the estimate of the current cycle next to the plan price of the
/// accounts one source shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanValue {
    pub source: SourceId,
    pub label: &'static str,
    /// The display names of the accounts with a plan.
    pub accounts: Vec<String>,
    /// C-05: the ids of every configured account this source shows (the
    /// logs cannot tell them apart).
    pub account_ids: Vec<String>,
    /// The ids of those with a plan; a front end shows the line only on
    /// these accounts.
    pub planned_ids: Vec<String>,
    /// More than one account shows the source: one combined line.
    pub combined: bool,
    /// Every account the source shows has a plan; without it there is
    /// never a ratio.
    pub complete: bool,
    /// The first day of the cycle (the earliest when the accounts differ).
    pub cycle_start: NaiveDate,
    /// The estimate at API prices since `cycle_start`; `None` when there is
    /// usage and none of it has a price.
    pub estimate: Option<Usd>,
    /// Part of the usage has no price, so `estimate` leaves it out.
    pub partial: bool,
    /// The prices, summed per currency, never across currencies.
    pub plans: Vec<PlanAmount>,
    /// `estimate / price`, only when both are in one currency (or the plan
    /// is in the display currency, `at_your_rate`), every account of the
    /// source has a plan and, combined, they share one cycle day.
    pub ratio: Option<f64>,
    pub at_your_rate: bool,
    pub text: Localized,
}

/// What [`plan_values`] works from.
#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub today: NaiveDate,
    pub plans: &'a BTreeMap<String, PlanSetting>,
    pub accounts: &'a [Account],
    /// The sources that are on.
    pub enabled: &'a [SourceId],
    /// Indexed usage covering at least the earliest cycle start.
    pub usage: &'a [DailyUsage],
    pub prices: &'a Prices,
    pub display: Option<&'a DisplayCurrency>,
}

/// The earliest first day of the cycles of the accounts that have a plan
/// and whose source is on; `None` when there is none.
pub fn earliest_cycle(
    today: NaiveDate,
    plans: &BTreeMap<String, PlanSetting>,
    accounts: &[Account],
    enabled: &[SourceId],
) -> Option<NaiveDate> {
    planned(plans, accounts, enabled)
        .map(|(_, plan, _)| cycle_start(today, plan.cycle_day()))
        .min()
}

fn planned<'a>(
    plans: &'a BTreeMap<String, PlanSetting>,
    accounts: &'a [Account],
    enabled: &'a [SourceId],
) -> impl Iterator<Item = (SourceId, &'a PlanSetting, &'a Account)> {
    accounts.iter().filter_map(move |account| {
        let source = SourceId::for_provider(account.provider)?;
        // PRD 0029 RF2: a plan follows the account id; one saved under the
        // legacy `provider:name` key by an earlier build still applies.
        let plan = plans
            .get(&account.state_key())
            .or_else(|| plans.get(&account.key()))?;
        enabled.contains(&source).then_some((source, plan, account))
    })
}

/// C-03: the plan prices as `amount.money` texts, one per currency,
/// joined by `cost.plan.list`.
fn plans_text(plans: &[PlanAmount]) -> Localized {
    let mut amounts = plans
        .iter()
        .rev()
        .map(|plan| money(plan.price, &plan.currency));
    let last = amounts
        .next()
        .unwrap_or_else(|| money(0.0, super::CURRENCY));
    amounts.fold(last, |rest, first| {
        Localized::with_params(
            "cost.plan.list",
            params([
                ("first", ParamValue::l10n(first)),
                ("rest", ParamValue::l10n(rest)),
            ]),
            "{first} · {rest}",
        )
    })
}

/// RF1: one [`PlanValue`] per source with at least one planned account, in
/// [`SourceId::ALL`] order. No plan price, nothing shown. Pure.
///
/// C-05: the logs of a source cannot be split between the accounts that
/// show it, so with several accounts the line is combined; it gives a
/// ratio only when every one of them has a plan, in one currency and on
/// one cycle day.
pub fn plan_values(input: &PlanInput<'_>) -> Vec<PlanValue> {
    let mut values = Vec::new();
    for source in SourceId::ALL {
        let planned: Vec<_> = planned(input.plans, input.accounts, input.enabled)
            .filter(|(of, _, _)| *of == source)
            .collect();
        if planned.is_empty() {
            continue;
        }
        let account_ids: Vec<String> = input
            .accounts
            .iter()
            .filter(|account| SourceId::for_provider(account.provider) == Some(source))
            .map(crate::Account::state_key)
            .collect();
        let combined = account_ids.len() > 1;
        let complete = planned.len() == account_ids.len();
        let start = planned
            .iter()
            .map(|(_, plan, _)| cycle_start(input.today, plan.cycle_day()))
            .min()
            .expect("not empty");
        let one_day = planned
            .iter()
            .all(|(_, plan, _)| plan.cycle_day() == planned[0].1.cycle_day());
        let rows: Vec<DailyUsage> = input
            .usage
            .iter()
            .filter(|row| row.source == source && row.day >= start && row.day <= input.today)
            .cloned()
            .collect();
        let estimated = estimate(&rows, input.prices);
        let mut sums: BTreeMap<&str, f64> = BTreeMap::new();
        for (_, plan, _) in &planned {
            *sums.entry(plan.currency.as_str()).or_default() += plan.price;
        }
        let plans: Vec<PlanAmount> = sums
            .into_iter()
            .map(|(currency, price)| PlanAmount {
                currency: currency.to_owned(),
                price: cents(price),
            })
            .collect();
        let divisible = complete && one_day;
        let (ratio, at_your_rate) = match (estimated.total, plans.as_slice()) {
            (Some(total), [only]) if divisible && only.currency == super::CURRENCY => {
                (Some(total.as_usd() / only.price), false)
            }
            (Some(total), [only]) if divisible => {
                match input.display.filter(|d| d.code == only.currency) {
                    Some(display) => (Some(total.as_usd() * display.rate / only.price), true),
                    None => (None, false),
                }
            }
            _ => (None, false),
        };
        let partial = estimated.unknown_tokens.total() > 0;
        let label = source.label();
        let accounts: Vec<String> = planned
            .iter()
            .map(|(_, _, a)| a.display_name().into_owned())
            .collect();
        let planned_ids: Vec<String> = planned.iter().map(|(_, _, a)| a.state_key()).collect();
        let text = plan_text(&PlanText {
            label,
            count: account_ids.len(),
            planned: planned_ids.len(),
            estimate: estimated.total,
            partial,
            plans: &plans,
            ratio,
            at_your_rate,
        });
        values.push(PlanValue {
            source,
            label,
            accounts,
            account_ids,
            planned_ids,
            combined,
            complete,
            cycle_start: start,
            estimate: estimated.total,
            partial,
            plans,
            ratio,
            at_your_rate,
            text,
        });
    }
    values
}

struct PlanText<'a> {
    label: &'static str,
    /// N: the accounts the source shows.
    count: usize,
    /// P: those with a plan.
    planned: usize,
    estimate: Option<Usd>,
    partial: bool,
    plans: &'a [PlanAmount],
    ratio: Option<f64>,
    at_your_rate: bool,
}

/// C-04: the ratio texts, in the order single, single at your rate,
/// summed plans, summed plans at your rate.
const TIMES: [(&str, &str); 4] = [
    ("cost.plan.times", "≈ {ratio}× the plan price"),
    (
        "cost.plan.times_rate",
        "≈ {ratio}× the plan price, at your rate",
    ),
    ("cost.plan.times_plans", "≈ {ratio}× the plans' price"),
    (
        "cost.plan.times_plans_rate",
        "≈ {ratio}× the plans' price, at your rate",
    ),
];
const SHARE: [(&str, &str); 4] = [
    ("cost.plan.share", "≈ {percent}% of the plan price"),
    (
        "cost.plan.share_rate",
        "≈ {percent}% of the plan price, at your rate",
    ),
    ("cost.plan.share_plans", "≈ {percent}% of the plans' price"),
    (
        "cost.plan.share_plans_rate",
        "≈ {percent}% of the plans' price, at your rate",
    ),
];
const SHARE_SMALL: [(&str, &str); 4] = [
    ("cost.plan.share_small", "less than 1% of the plan price"),
    (
        "cost.plan.share_small_rate",
        "less than 1% of the plan price, at your rate",
    ),
    (
        "cost.plan.share_small_plans",
        "less than 1% of the plans' price",
    ),
    (
        "cost.plan.share_small_plans_rate",
        "less than 1% of the plans' price, at your rate",
    ),
];

/// C-04: "≈ 1.5× the plan price" from 0.1×; "≈ 4% of the plan price" from
/// 1% to under 10%; "less than 1% of the plan price" below. `plural` is
/// the wording for summed plans.
fn ratio_text(ratio: f64, at_your_rate: bool, plural: bool) -> Localized {
    let index = usize::from(plural) * 2 + usize::from(at_your_rate);
    if ratio >= 0.1 {
        let (id, fallback) = TIMES[index];
        let rounded = (ratio * 10.0).round() / 10.0;
        Localized::with_param(id, "ratio", number(rounded), fallback)
    } else if ratio >= 0.01 {
        let (id, fallback) = SHARE[index];
        let percent = (ratio * 100.0).round().max(1.0);
        Localized::with_param(id, "percent", number(percent), fallback)
    } else {
        let (id, fallback) = SHARE_SMALL[index];
        Localized::fixed(id, fallback)
    }
}

fn plan_text(line: &PlanText<'_>) -> Localized {
    let combined = line.count > 1;
    let count: ParamValue = line.count.into();
    let plans = ParamValue::l10n(plans_text(line.plans));
    let Some(estimate) = line.estimate else {
        let source: ParamValue = if combined {
            ParamValue::l10n(Localized::plural(
                "cost.plan.combined_label",
                params([("source", line.label.into()), ("count", count.clone())]),
                "{source}, {count} account combined",
                "{source}, {count} accounts combined",
            ))
        } else {
            line.label.into()
        };
        return Localized::with_params(
            "cost.plan.unknown",
            params([("source", source), ("plans", plans)]),
            "{source}: cost at API prices unknown (models without a price) · plan {plans}",
        );
    };
    let amount = ParamValue::l10n(estimate.money());
    let estimate = ParamValue::l10n(if line.partial {
        Localized::with_params(
            "cost.plan.estimate_partial",
            params([("amount", amount)]),
            "≈ {amount} at API prices for the models with a price (estimate)",
        )
    } else {
        Localized::with_params(
            "cost.plan.estimate",
            params([("amount", amount)]),
            "≈ {amount} at API prices (estimate)",
        )
    });
    let source: ParamValue = line.label.into();
    if combined && line.planned < line.count {
        return Localized::plural(
            "cost.plan.combined_some",
            params([
                ("source", source),
                ("count", count),
                ("planned", line.planned.into()),
                ("estimate", estimate),
                ("plans", plans),
            ]),
            "{source}, {count} account combined: {estimate} · plans of {planned} of them: {plans}; the logs cannot be split, so there is no ratio",
            "{source}, {count} accounts combined: {estimate} · plans of {planned} of them: {plans}; the logs cannot be split, so there is no ratio",
        );
    }
    match (line.ratio, combined) {
        (Some(ratio), false) => Localized::with_params(
            "cost.plan.single",
            params([
                ("source", source),
                ("estimate", estimate),
                ("plans", plans),
                (
                    "value",
                    ParamValue::l10n(ratio_text(ratio, line.at_your_rate, false)),
                ),
            ]),
            "{source}: {estimate} · plan {plans} · {value}",
        ),
        (None, false) => Localized::with_params(
            "cost.plan.single_side",
            params([("source", source), ("estimate", estimate), ("plans", plans)]),
            "{source}: {estimate} · plan {plans}",
        ),
        (Some(ratio), true) => Localized::plural(
            "cost.plan.combined",
            params([
                ("source", source),
                ("count", count),
                ("estimate", estimate),
                ("plans", plans),
                (
                    "value",
                    ParamValue::l10n(ratio_text(ratio, line.at_your_rate, true)),
                ),
            ]),
            "{source}, {count} account combined: {estimate} · plans {plans} · {value}",
            "{source}, {count} accounts combined: {estimate} · plans {plans} · {value}",
        ),
        (None, true) => Localized::plural(
            "cost.plan.combined_side",
            params([
                ("source", source),
                ("count", count),
                ("estimate", estimate),
                ("plans", plans),
            ]),
            "{source}, {count} account combined: {estimate} · plans {plans}",
            "{source}, {count} accounts combined: {estimate} · plans {plans}",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::prices::{PriceTable, Rate, Rates, CURRENCY};
    use crate::cost::Tokens;
    use crate::Provider;

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            group: Default::default(),
            id: None,
            name: name.into(),
            provider,
            credential_env: None,
            oauth: None,
            session: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            budgets: Vec::new(),
            floors: Vec::new(),
            breakdown: Vec::new(),
        }
    }

    /// `known` at $1 per million input tokens.
    fn prices() -> Prices {
        let one = Rate::usd_per_million(1.0).unwrap();
        let zero = Rate::usd_per_million(0.0).unwrap();
        Prices::with_tables(
            PriceTable {
                currency: CURRENCY,
                effective: day(10, 1),
                source: None,
                models: [(
                    "known".to_owned(),
                    Rates {
                        input: one,
                        output: zero,
                        cache_read: zero,
                        cache_write: zero,
                        cache_write_1h: None,
                    },
                )]
                .into(),
            },
            None,
        )
    }

    fn usage(d: NaiveDate, millions: u64) -> DailyUsage {
        DailyUsage {
            source: SourceId::ClaudeCode,
            day: d,
            model: "known".into(),
            tokens: Tokens {
                input: millions * 1_000_000,
                ..Tokens::default()
            },
            requests: 1,
            sessions: 1,
        }
    }

    fn values(
        plans: &[(&str, PlanSetting)],
        accounts: &[Account],
        rows: &[DailyUsage],
        display: Option<&DisplayCurrency>,
    ) -> Vec<PlanValue> {
        let plans: BTreeMap<String, PlanSetting> = plans
            .iter()
            .map(|(key, plan)| ((*key).to_owned(), plan.clone()))
            .collect();
        plan_values(&PlanInput {
            today: day(10, 20),
            plans: &plans,
            accounts,
            enabled: &[SourceId::ClaudeCode],
            usage: rows,
            prices: &prices(),
            display,
        })
    }

    fn usd_plan(price: f64) -> PlanSetting {
        PlanSetting::new(price, "usd", None).unwrap()
    }

    #[test]
    fn no_plan_price_shows_nothing() {
        let accounts = [account("a", Provider::Claude)];
        assert!(values(&[], &accounts, &[usage(day(10, 5), 3)], None).is_empty());
    }

    #[test]
    fn the_same_currency_gives_a_ratio_of_the_cycle_estimate() {
        let accounts = [account("a", Provider::Claude)];
        let rows = [usage(day(9, 30), 50), usage(day(10, 5), 150)];
        let v = values(&[("claude:a", usd_plan(100.0))], &accounts, &rows, None);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].cycle_start, day(10, 1));
        assert_eq!(v[0].estimate.unwrap().as_usd(), 150.0, "September is out");
        assert_eq!(v[0].ratio, Some(1.5));
        assert_eq!(v[0].text.id, "cost.plan.single");
        assert_eq!(
            v[0].text.fallback,
            "Claude Code: ≈ $150.00 at API prices (estimate) · plan $100.00 · ≈ 1.5× the plan price"
        );
        assert!(!v[0].combined && v[0].complete);
        assert_eq!(v[0].planned_ids, [accounts[0].state_key()]);
    }

    #[test]
    fn another_currency_without_a_rate_has_no_ratio_and_no_sum() {
        let accounts = [
            account("a", Provider::Claude),
            account("b", Provider::Claude),
        ];
        let brl = PlanSetting::new(500.0, "BRL", None).unwrap();
        let v = values(
            &[("claude:a", brl), ("claude:b", usd_plan(100.0))],
            &accounts,
            &[usage(day(10, 5), 3)],
            None,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].ratio, None);
        let currencies: Vec<_> = v[0].plans.iter().map(|p| p.currency.as_str()).collect();
        assert_eq!(currencies, ["BRL", "USD"], "never summed across currencies");
        assert_eq!(v[0].text.id, "cost.plan.combined_side");
        assert!(
            v[0].text.fallback.contains("plans R$500.00 · $100.00"),
            "{}",
            v[0].text.fallback
        );
    }

    #[test]
    fn a_plan_in_the_display_currency_divides_at_your_rate() {
        let accounts = [account("a", Provider::Claude)];
        let brl = PlanSetting::new(500.0, "brl", None).unwrap();
        let rate = DisplayCurrency::new("BRL", 5.0, day(10, 1)).unwrap();
        let v = values(
            &[("claude:a", brl)],
            &accounts,
            &[usage(day(10, 5), 200)],
            Some(&rate),
        );
        assert_eq!(v[0].ratio, Some(2.0));
        assert!(v[0].at_your_rate);
        assert_eq!(v[0].text.id, "cost.plan.single");
        assert!(v[0]
            .text
            .fallback
            .ends_with("≈ 2× the plan price, at your rate"));
    }

    #[test]
    fn two_accounts_of_one_source_give_one_combined_line_with_summed_plans() {
        let accounts = [
            account("a", Provider::Claude),
            account("b", Provider::Claude),
        ];
        let v = values(
            &[("claude:a", usd_plan(100.0)), ("claude:b", usd_plan(100.0))],
            &accounts,
            &[usage(day(10, 5), 212)],
            None,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].accounts, ["a", "b"]);
        assert_eq!(v[0].plans.len(), 1);
        assert_eq!(v[0].plans[0].price, 200.0);
        assert!(v[0].combined && v[0].complete);
        assert_eq!(v[0].ratio, Some(1.06));
        assert_eq!(
            v[0].text.fallback,
            "Claude Code, 2 accounts combined: ≈ $212.00 at API prices (estimate) · plans $200.00 · ≈ 1.1× the plans' price"
        );
    }

    /// PRD 0029 RF2: a plan saved under the account id applies, and so does
    /// one an earlier build saved under the legacy `provider:name` key.
    #[test]
    fn a_plan_is_found_by_the_account_id_or_the_legacy_key() {
        let accounts = [account("a", Provider::Claude)];
        let id = accounts[0].state_key();
        let rows = [usage(day(10, 5), 50)];
        let by_id = values(&[(id.as_str(), usd_plan(100.0))], &accounts, &rows, None);
        assert_eq!(by_id.len(), 1);
        let legacy = values(&[("claude:a", usd_plan(100.0))], &accounts, &rows, None);
        assert_eq!(legacy.len(), 1);
    }

    #[test]
    fn a_cycle_day_of_fifteen_splits_the_month_at_the_fifteenth() {
        assert_eq!(cycle_start(day(10, 20), 15), day(10, 15));
        assert_eq!(cycle_start(day(10, 14), 15), day(9, 15));
        assert_eq!(
            cycle_start(day(1, 3), 15),
            NaiveDate::from_ymd_opt(2025, 12, 15).unwrap()
        );
        let accounts = [account("a", Provider::Claude)];
        let plan = PlanSetting::new(100.0, "USD", Some(15)).unwrap();
        let rows = [
            usage(day(10, 14), 40),
            usage(day(10, 15), 10),
            usage(day(10, 19), 5),
        ];
        let v = values(&[("claude:a", plan)], &accounts, &rows, None);
        assert_eq!(v[0].cycle_start, day(10, 15));
        assert_eq!(v[0].estimate.unwrap().as_usd(), 15.0);
    }

    #[test]
    fn invalid_plans_and_rates_are_refused() {
        assert!(PlanSetting::new(0.0, "USD", None).is_err());
        assert!(PlanSetting::new(-1.0, "USD", None).is_err());
        assert!(PlanSetting::new(10.0, "US", None).is_err());
        assert!(PlanSetting::new(10.0, "USD", Some(29)).is_err());
        assert!(PlanSetting::new(10.0, "USD", Some(0)).is_err());
        assert_eq!(PlanSetting::new(9.999, "usd", None).unwrap().price, 10.0);
        assert!(DisplayCurrency::new("USD", 5.0, day(10, 1)).is_err());
        assert!(DisplayCurrency::new("BRL", 0.0, day(10, 1)).is_err());
        assert!(DisplayCurrency::new("BRL", f64::NAN, day(10, 1)).is_err());
    }

    #[test]
    fn the_texts_exist_in_the_four_languages() {
        let rate = DisplayCurrency::new("BRL", 5.42, day(10, 8)).unwrap();
        let usd = Usd::from_micros(10_000_000);
        let line = rate.text(usd, day(10, 20));
        assert!(line.resolve("pt-BR").contains("no seu câmbio"));
        assert!(line
            .resolve("en-US")
            .contains("at your rate (set 12 days ago)"));
        assert!(line.resolve("es").contains("a su tipo de cambio"));
        assert!(line.resolve("it").contains("al tuo cambio"));
        let accounts = [account("a", Provider::Claude)];
        let v = values(
            &[("claude:a", usd_plan(100.0))],
            &accounts,
            &[usage(day(10, 5), 100)],
            None,
        );
        for (locale, word) in [
            ("en-US", "API prices"),
            ("es", "precios de API"),
            ("it", "prezzi API"),
            ("pt-BR", "preços de API"),
        ] {
            let text = v[0].text.resolve(locale);
            assert!(
                text.contains(word) && !text.contains('{'),
                "{locale}: {text}"
            );
        }
    }

    #[test]
    fn the_rate_and_plans_stay_out_of_the_usd_only_summary() {
        use crate::cost::view::{summarize, Span, SummaryInput};
        use crate::cost::Period;
        let summary = summarize(&SummaryInput {
            today: day(10, 20),
            span: Span::Period(Period::Today),
            enabled: &[SourceId::ClaudeCode],
            usage: &[usage(day(10, 20), 1)],
            prices: &prices(),
            pricing_error: None,
            last: None,
        });
        let json = serde_json::to_string(&summary).unwrap();
        assert_eq!(summary.currency, "USD");
        for word in ["plans", "display_currency", "\"rate\"", "BRL"] {
            assert!(!json.contains(word), "{word} leaked into the summary");
        }
    }

    #[test]
    fn plans_and_the_rate_persist_in_the_settings_file_only() {
        use crate::cost::SettingsStore;
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path().join("cost/settings.json"));
        store.set_plan("claude:a", Some(usd_plan(100.0))).unwrap();
        store
            .set_display_currency(Some(DisplayCurrency::new("BRL", 5.42, day(10, 8)).unwrap()))
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.plans["claude:a"].price, 100.0);
        assert_eq!(loaded.display_currency.as_ref().unwrap().rate, 5.42);
        store.set_plan("claude:a", None).unwrap();
        store.set_display_currency(None).unwrap();
        let cleared = store.load().unwrap();
        assert!(cleared.plans.is_empty() && cleared.display_currency.is_none());
        // An older file without the new keys still loads.
        std::fs::write(store.path(), br#"{"period":"today"}"#).unwrap();
        assert!(store.load().unwrap().plans.is_empty());
    }

    #[test]
    fn the_rate_text_says_its_age_and_asks_for_an_update_after_thirty_days() {
        let rate = DisplayCurrency::new("brl", 5.42, day(10, 8)).unwrap();
        let usd = Usd::from_micros(212_000_000);
        let fresh = rate.text(usd, day(10, 20));
        assert_eq!(fresh.id, "cost.display.line");
        assert_eq!(
            fresh.fallback,
            "≈ R$1,149.04 at your rate (set 12 days ago)"
        );
        assert!(
            fresh.resolve("pt-BR").contains("R$\u{a0}1.149,04"),
            "{}",
            fresh.resolve("pt-BR")
        );
        let old = rate.text(usd, day(11, 20));
        assert_eq!(old.id, "cost.display.line_stale");
        assert_eq!(rate.age_days(day(10, 1)), 0);
        // C-08: today, yesterday, then a real plural.
        assert_eq!(rate.age_text(day(10, 8)).fallback, "set today");
        assert_eq!(rate.age_text(day(10, 9)).fallback, "set yesterday");
        assert_eq!(rate.age_text(day(10, 10)).fallback, "set 2 days ago");
        for (days, ru) in [(21, "21 день"), (22, "22 дня"), (25, "25 дней")] {
            let text = rate.age_text(day(10, 8) + Days::days(days)).resolve("ru");
            assert!(text.contains(ru), "{days}: {text}");
        }
    }

    fn estimate_of(rows: &[DailyUsage]) -> Estimate {
        estimate(rows, &prices())
    }

    /// C-07: a partial estimate converts with "+ unknown"; no usage, or
    /// nothing priced, converts nothing.
    #[test]
    fn a_conversion_keeps_the_unknown_part_and_needs_usage() {
        let rate = DisplayCurrency::new("BRL", 5.0, day(10, 18)).unwrap();
        let priced = estimate_of(&[usage(day(10, 20), 2)]);
        assert_eq!(
            rate.text_for(&priced, day(10, 20)).unwrap().fallback,
            "≈ R$10.00 at your rate (set 2 days ago)"
        );
        let mut unknown = usage(day(10, 20), 1);
        unknown.model = "mystery".into();
        let partial = estimate_of(&[usage(day(10, 20), 2), unknown.clone()]);
        assert_eq!(
            rate.text_for(&partial, day(10, 20)).unwrap().fallback,
            "≈ R$10.00 + unknown at your rate (set 2 days ago)"
        );
        assert!(rate.text_for(&estimate_of(&[]), day(10, 20)).is_none());
        assert!(rate
            .text_for(&estimate_of(&[unknown]), day(10, 20))
            .is_none());
    }

    /// C-05: two accounts share one source's logs and only one has a plan:
    /// one combined line on that account only, and never a ratio.
    #[test]
    fn some_accounts_without_a_plan_give_a_combined_line_without_a_ratio() {
        let accounts = [
            account("a", Provider::Claude),
            account("b", Provider::Claude),
        ];
        let v = values(
            &[("claude:a", usd_plan(100.0))],
            &accounts,
            &[usage(day(10, 5), 50)],
            None,
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].combined && !v[0].complete);
        assert_eq!(v[0].ratio, None);
        assert_eq!(v[0].planned_ids, [accounts[0].state_key()]);
        assert_eq!(
            v[0].account_ids,
            [accounts[0].state_key(), accounts[1].state_key()]
        );
        assert_eq!(v[0].text.id, "cost.plan.combined_some");
        assert_eq!(
            v[0].text.fallback,
            "Claude Code, 2 accounts combined: ≈ $50.00 at API prices (estimate) · plans of 1 of them: $100.00; the logs cannot be split, so there is no ratio"
        );
    }

    /// C-05: with different cycle days the estimate window does not match
    /// the plans, so there is no ratio; one shared day gives one.
    #[test]
    fn combined_plans_divide_only_on_one_cycle_day() {
        let accounts = [
            account("a", Provider::Claude),
            account("b", Provider::Claude),
        ];
        let rows = [usage(day(10, 16), 100)];
        let fifteenth = PlanSetting::new(100.0, "USD", Some(15)).unwrap();
        let apart = values(
            &[
                ("claude:a", usd_plan(100.0)),
                ("claude:b", fifteenth.clone()),
            ],
            &accounts,
            &rows,
            None,
        );
        assert_eq!(apart[0].ratio, None);
        assert_eq!(apart[0].text.id, "cost.plan.combined_side");
        let together = values(
            &[("claude:a", fifteenth.clone()), ("claude:b", fifteenth)],
            &accounts,
            &rows,
            None,
        );
        assert_eq!(together[0].ratio, Some(0.5));
        assert_eq!(together[0].text.id, "cost.plan.combined");
    }

    /// C-04: small ratios read as a share of the plan, never "0.0×".
    #[test]
    fn small_ratios_read_as_a_share_of_the_plan() {
        let text = |ratio: f64| ratio_text(ratio, false, false).fallback.into_owned();
        assert_eq!(text(0.009), "less than 1% of the plan price");
        assert_eq!(text(0.015), "≈ 2% of the plan price");
        assert_eq!(text(0.0999), "≈ 10% of the plan price");
        assert_eq!(text(0.1), "≈ 0.1× the plan price");
        assert_eq!(text(7604.2), "≈ 7,604.2× the plan price");
        assert_eq!(
            ratio_text(0.009, true, true).fallback,
            "less than 1% of the plans' price, at your rate"
        );
        assert!(ratio_text(7604.2, false, false)
            .resolve("de")
            .contains("7.604,2"));
    }

    /// The cycle starts on day 28 at the month ends.
    #[test]
    fn a_cycle_on_the_twenty_eighth_crosses_month_ends() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(cycle_start(date(2026, 3, 1), 28), date(2026, 2, 28));
        assert_eq!(cycle_start(date(2028, 2, 29), 28), date(2028, 2, 28));
        assert_eq!(cycle_start(date(2026, 1, 15), 28), date(2025, 12, 28));
    }

    /// L-01: six significant digits, and a too-small rate has its own error.
    #[test]
    fn a_rate_keeps_six_significant_digits() {
        let rate = |value| DisplayCurrency::new("XYZ", value, day(10, 1)).map(|d| d.rate);
        assert_eq!(rate(0.000_123_456_78), Ok(0.000_123_457));
        assert_eq!(rate(25_412.345_6), Ok(25_412.3));
        assert_eq!(rate(0.000_000_4), Err(CostError::RateTooSmall));
    }

    /// L-18: an unnamed account is listed by its provider.
    #[test]
    fn an_unnamed_account_is_listed_by_its_provider() {
        let accounts = [account("", Provider::Claude)];
        let listed = plan_accounts(&BTreeMap::new(), &accounts);
        assert_eq!(listed[0].name, "Claude");
    }

    /// C-08: every cost text that carries a count is a real plural in the
    /// catalogs: 1, 2 and 5 read in ru, pt-BR and de with no placeholder
    /// left and no "(s)".
    #[test]
    fn cost_counts_are_real_plurals() {
        use crate::cost::{disabled_text, erased_text};
        let texts = |count: usize| {
            vec![
                disabled_text(SourceId::ClaudeCode, count),
                erased_text(count),
                crate::cost::block_hours_text(SourceId::ClaudeCode, count as u8),
                crate::cost::folder_hint_text(SourceId::ClaudeCode, false, count),
                DisplayCurrency::new("BRL", 5.0, day(1, 1))
                    .unwrap()
                    .age_text(day(1, 1) + Days::days(count as i64)),
                ratio_text(0.5, false, true),
            ]
        };
        for locale in ["ru", "pt-BR", "de"] {
            for count in [2, 5, 21] {
                for text in texts(count) {
                    let out = text.resolve(locale);
                    assert!(
                        !out.contains('{') && !out.contains("(s)") && !out.contains("(е)"),
                        "{locale} {count}: {out}"
                    );
                }
            }
        }
        let ru = |count| erased_text(count).resolve("ru");
        assert_ne!(ru(2), ru(5).replace('5', "2"), "few and many differ in ru");
    }
}
