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

use super::estimate::{estimate, Usd};
use super::prices::Prices;
use super::{CostError, DailyUsage, SourceId};
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
    pub fn new(code: &str, rate: f64, set_on: NaiveDate) -> Result<Self, CostError> {
        let code = currency_code(code).ok_or(CostError::InvalidRate)?;
        let rate = (rate * 1e6).round() / 1e6;
        if code == super::CURRENCY || !rate.is_finite() || rate <= 0.0 || rate > MAX_RATE {
            return Err(CostError::InvalidRate);
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

    /// RF6.2: "≈ BRL 1149.00 at your rate (set 12 days ago)", plus "update
    /// your rate" from [`RATE_STALE_DAYS`] days on.
    pub fn text(&self, usd: Usd, today: NaiveDate) -> Localized {
        let days = self.age_days(today);
        let amount = format!("{} {:.2}", self.code, self.convert(usd));
        let ask = days >= RATE_STALE_DAYS;
        let (id, fallback) = if ask {
            (
                "cost.display.line_stale",
                "≈ {amount} at your rate (set {days} days ago); update your rate",
            )
        } else {
            (
                "cost.display.line",
                "≈ {amount} at your rate (set {days} days ago)",
            )
        };
        Localized::with_params(
            id,
            params([
                ("amount", amount.as_str().into()),
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
                name: account.name.clone(),
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
    /// The accounts with a plan (RF1.3: more than one means one combined
    /// line).
    pub accounts: Vec<String>,
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
    /// is in the display currency, `at_your_rate`).
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

fn amount_text(amount: &PlanAmount) -> String {
    format!("{} {:.2}", amount.currency, amount.price)
}

/// RF1: one [`PlanValue`] per source with at least one planned account, in
/// [`SourceId::ALL`] order. No plan price, nothing shown. Pure.
pub fn plan_values(input: &PlanInput<'_>) -> Vec<PlanValue> {
    let mut values = Vec::new();
    for source in SourceId::ALL {
        let planned: Vec<_> = planned(input.plans, input.accounts, input.enabled)
            .filter(|(of, _, _)| *of == source)
            .collect();
        if planned.is_empty() {
            continue;
        }
        let start = planned
            .iter()
            .map(|(_, plan, _)| cycle_start(input.today, plan.cycle_day()))
            .min()
            .expect("not empty");
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
        let (ratio, at_your_rate) = match (estimated.total, plans.as_slice()) {
            (Some(total), [only]) if only.currency == super::CURRENCY => {
                (Some(total.as_usd() / only.price), false)
            }
            (Some(total), [only]) => match input.display.filter(|d| d.code == only.currency) {
                Some(display) => (Some(total.as_usd() * display.rate / only.price), true),
                None => (None, false),
            },
            _ => (None, false),
        };
        let partial = estimated.unknown_tokens.total() > 0;
        let label = source.label();
        let accounts: Vec<String> = planned.iter().map(|(_, _, a)| a.name.clone()).collect();
        let text = plan_text(
            label,
            accounts.len() > 1,
            estimated.total,
            partial,
            &plans,
            ratio,
            at_your_rate,
        );
        values.push(PlanValue {
            source,
            label,
            accounts,
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

fn plan_text(
    label: &'static str,
    combined: bool,
    estimate: Option<Usd>,
    partial: bool,
    plans: &[PlanAmount],
    ratio: Option<f64>,
    at_your_rate: bool,
) -> Localized {
    let source: ParamValue = if combined {
        ParamValue::l10n(Localized::with_param(
            "cost.plan.combined",
            "source",
            label,
            "{source} accounts",
        ))
    } else {
        label.into()
    };
    let plans_text = plans
        .iter()
        .map(amount_text)
        .collect::<Vec<_>>()
        .join(" · ");
    let Some(estimate) = estimate else {
        return Localized::with_params(
            "cost.plan.unknown",
            params([("source", source), ("plans", plans_text.as_str().into())]),
            "{source}: cost at API prices unknown (models without a price) · plan {plans}",
        );
    };
    let amount = if partial {
        ParamValue::l10n(Localized::with_param(
            "cost.plan.partial",
            "amount",
            estimate.to_string(),
            "{amount}, without the models that have no price",
        ))
    } else {
        estimate.to_string().as_str().into()
    };
    let base = params([
        ("source", source),
        ("estimate", amount),
        ("plans", plans_text.as_str().into()),
    ]);
    match ratio {
        Some(ratio) => {
            let mut all = base;
            all.insert("ratio".to_owned(), format!("{ratio:.1}").as_str().into());
            Localized::with_params(
                if at_your_rate {
                    "cost.plan.ratio_rate"
                } else {
                    "cost.plan.ratio"
                },
                all,
                if at_your_rate {
                    "{source}: ≈ {estimate} at API prices (estimate) · plan {plans} · ≈ {ratio}× the plan price, at your rate"
                } else {
                    "{source}: ≈ {estimate} at API prices (estimate) · plan {plans} · ≈ {ratio}× the plan price"
                },
            )
        }
        None => Localized::with_params(
            "cost.plan.side",
            base,
            "{source}: ≈ {estimate} at API prices (estimate) · plan {plans}",
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
        assert_eq!(v[0].text.id, "cost.plan.ratio");
        assert!(v[0].text.fallback.contains("estimate"));
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
        assert_eq!(v[0].text.id, "cost.plan.side");
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
        assert_eq!(v[0].text.id, "cost.plan.ratio_rate");
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
        assert!(v[0].text.fallback.starts_with("Claude Code accounts"));
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
        assert!(fresh.fallback.contains("BRL 1149.04"), "{}", fresh.fallback);
        assert!(fresh.fallback.contains("12 days"));
        let old = rate.text(usd, day(11, 20));
        assert_eq!(old.id, "cost.display.line_stale");
        assert_eq!(rate.age_days(day(10, 1)), 0);
    }
}
