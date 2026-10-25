//! RF3.3/RF3.4: [`estimate`], the pure computation of the estimated cost
//! of indexed usage at API prices.
//!
//! Costs are integers (pico-dollars: a token count times a price in
//! micro-dollars per million tokens), so the total is the exact sum of the
//! models' costs and rounding happens only when a value is read
//! ([`Usd::micros`], [`Usd::cents`], half up). Usage of a model without a
//! known price is counted in tokens and in the unknown share, never as a
//! cost of zero. Everything is in USD.

use super::prices::{Price, PriceOrigin, Prices, Rates, TableInfo, CURRENCY};
use super::{DailyUsage, Tokens};
use crate::text::Localized;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

const PICO_PER_MICRO: u128 = 1_000_000;
const PICO_PER_CENT: u128 = 10_000_000_000;

/// An amount in US dollars, kept exactly (pico-dollars).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Usd {
    pico: u128,
}

impl Usd {
    pub fn from_micros(micros: u64) -> Self {
        Self {
            pico: u128::from(micros) * PICO_PER_MICRO,
        }
    }

    fn rounded(self, unit: u128) -> u64 {
        u64::try_from(self.pico.saturating_add(unit / 2) / unit).unwrap_or(u64::MAX)
    }

    /// Micro-dollars, rounded half up.
    pub fn micros(self) -> u64 {
        self.rounded(PICO_PER_MICRO)
    }

    /// Cents, rounded half up.
    pub fn cents(self) -> u64 {
        self.rounded(PICO_PER_CENT)
    }

    /// Dollars to six decimals.
    pub fn as_usd(self) -> f64 {
        self.micros() as f64 / 1e6
    }
}

impl std::ops::Add for Usd {
    type Output = Usd;

    fn add(self, other: Usd) -> Usd {
        Usd {
            pico: self.pico.saturating_add(other.pico),
        }
    }
}

/// `$3.40`, half up to the cent; `<$0.01` for a positive amount below a
/// cent, so a small cost never reads as free.
impl std::fmt::Display for Usd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cents = self.cents();
        if cents == 0 && self.pico > 0 {
            return f.write_str("<$0.01");
        }
        write!(f, "${}.{:02}", cents / 100, cents % 100)
    }
}

impl Serialize for Usd {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.as_usd())
    }
}

/// The cost of `tokens` at `rates`.
fn cost(tokens: &Tokens, rates: &Rates) -> Option<Usd> {
    // PRD 0030 RF5.3: never price 1-hour writes at the 5-minute rate.
    let one_hour = match (tokens.cache_write_1h, rates.cache_write_1h) {
        (0, _) => 0,
        (count, Some(rate)) => u128::from(count) * u128::from(rate.micros_per_million()),
        (_, None) => return None,
    };
    let part = |count: u64, rate: super::prices::Rate| {
        u128::from(count) * u128::from(rate.micros_per_million())
    };
    Some(Usd {
        pico: one_hour
            + part(tokens.input, rates.input)
            + part(tokens.output, rates.output)
            + part(tokens.cache_read, rates.cache_read)
            + part(tokens.cache_write, rates.cache_write),
    })
}

/// One model's share of an [`Estimate`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelEstimate {
    /// As the logs name it.
    pub model: String,
    pub tokens: Tokens,
    pub requests: u64,
    /// `None`: no price is known for the model ("cost unknown"), never 0.
    pub cost: Option<Usd>,
    /// The price used, and which table it came from.
    pub price: Option<Price>,
}

/// RF3.3: the estimated cost of some usage at API prices.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Estimate {
    /// Always `USD`: amounts are never in nor summed with another currency.
    pub currency: &'static str,
    /// The cost of the usage whose price is known; `None` when there is
    /// usage and none of it has a known price.
    pub total: Option<Usd>,
    /// Every token, priced or not.
    pub tokens: Tokens,
    pub requests: u64,
    /// The tokens of models without a known price.
    pub unknown_tokens: Tokens,
    /// `unknown_tokens` over `tokens` (0 to 1; 0 without usage): how much
    /// of the usage `total` leaves out.
    pub unknown_share: f64,
    /// Per model, by name.
    pub models: Vec<ModelEstimate>,
    /// The date and source of each table that priced a model, the embedded
    /// one first.
    pub tables: Vec<TableInfo>,
    /// RF3.4: "estimate at API prices, not a charge".
    pub notice: Localized,
}

impl Estimate {
    /// The models without a known price.
    pub fn unknown_models(&self) -> impl Iterator<Item = &str> {
        self.models
            .iter()
            .filter(|model| model.cost.is_none())
            .map(|model| model.model.as_str())
    }
}

/// RF3.4: what every estimate says about itself.
pub fn estimate_notice() -> Localized {
    Localized::fixed(
        "cost.estimate.notice",
        "Estimativa a preços de API, não é uma cobrança; valores em USD.",
    )
}

/// RF3.3: the estimated cost of `usage` (any sources, days and models) at
/// `prices`. Pure: the same usage and prices always give the same result.
pub fn estimate(usage: &[DailyUsage], prices: &Prices) -> Estimate {
    let mut by_model: BTreeMap<&str, (Tokens, u64)> = BTreeMap::new();
    for row in usage {
        let (tokens, requests) = by_model.entry(row.model.as_str()).or_default();
        tokens.add(&row.tokens);
        *requests = requests.saturating_add(row.requests);
    }
    let mut tokens = Tokens::default();
    let mut unknown_tokens = Tokens::default();
    let mut requests = 0u64;
    let mut total = Usd::default();
    let mut priced = false;
    let mut origins = Vec::new();
    let models: Vec<ModelEstimate> = by_model
        .into_iter()
        .map(|(model, (model_tokens, model_requests))| {
            let price = prices.price(model);
            let model_cost = price
                .as_ref()
                .and_then(|price| cost(&model_tokens, &price.rates));
            tokens.add(&model_tokens);
            requests = requests.saturating_add(model_requests);
            match (&price, model_cost) {
                (Some(price), Some(model_cost)) => {
                    total = total + model_cost;
                    priced = true;
                    origins.push(price.origin);
                }
                _ => unknown_tokens.add(&model_tokens),
            }
            ModelEstimate {
                model: model.to_owned(),
                tokens: model_tokens,
                requests: model_requests,
                cost: model_cost,
                price,
            }
        })
        .collect();
    let unknown_share = if tokens.total() == 0 {
        0.0
    } else {
        unknown_tokens.total() as f64 / tokens.total() as f64
    };
    let used = |origin: PriceOrigin| origins.contains(&origin);
    Estimate {
        currency: CURRENCY,
        total: (priced || models.is_empty()).then_some(total),
        tokens,
        requests,
        unknown_tokens,
        unknown_share,
        tables: prices
            .info()
            .into_iter()
            .filter(|info| used(info.origin))
            .collect(),
        models,
        notice: estimate_notice(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::{PriceTable, SourceId};
    use chrono::NaiveDate;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn usage(source: SourceId, d: u32, model: &str, tokens: [u64; 4], requests: u64) -> DailyUsage {
        let [input, output, cache_read, cache_write] = tokens;
        DailyUsage {
            source,
            day: day(d),
            model: model.to_owned(),
            tokens: Tokens {
                input,
                output,
                cache_read,
                cache_write,
                cache_write_1h: 0,
            },
            requests,
            sessions: 1,
        }
    }

    fn table(json: &str) -> PriceTable {
        PriceTable::parse(json.as_bytes()).unwrap()
    }

    /// Sonnet $3 in, $15 out, $0.30 cache read, $3.75 cache write; a cheap
    /// model for rounding.
    fn prices(user: Option<PriceTable>) -> Prices {
        Prices::with_tables(
            table(
                r#"{"effective": "2026-10-01", "source": "vendor", "models": {
                    "claude-sonnet-4-5": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75},
                    "gpt-5-nano": {"input": 0.05, "output": 0.4, "cache_read": 0.005, "cache_write": 0.05}}}"#,
            ),
            user,
        )
    }

    #[test]
    fn known_tokens_at_known_prices_including_cache() {
        let usage = [
            usage(
                SourceId::ClaudeCode,
                20,
                "claude-sonnet-4-5-20250929",
                [1_000_000, 0, 0, 0],
                3,
            ),
            usage(
                SourceId::ClaudeCode,
                21,
                "claude-sonnet-4-5-20250929",
                [0, 200_000, 2_000_000, 400_000],
                2,
            ),
        ];
        let estimate = estimate(&usage, &prices(None));
        // 1M × $3 + 0.2M × $15 + 2M × $0.30 + 0.4M × $3.75 = $8.10.
        assert_eq!(estimate.total, Some(Usd::from_micros(8_100_000)));
        assert_eq!(estimate.total.unwrap().cents(), 810);
        assert_eq!(estimate.currency, "USD");
        assert_eq!(estimate.requests, 5);
        assert_eq!(estimate.tokens.total(), 3_600_000);
        assert_eq!(estimate.unknown_share, 0.0);
        let [model] = estimate.models.as_slice() else {
            panic!("one model expected")
        };
        assert_eq!(model.model, "claude-sonnet-4-5-20250929");
        assert_eq!(model.cost, estimate.total);
        assert_eq!(model.price.as_ref().unwrap().entry, "claude-sonnet-4-5");
        let [table] = estimate.tables.as_slice() else {
            panic!("one table expected")
        };
        assert_eq!(
            (table.origin, table.effective),
            (PriceOrigin::Embedded, day(1))
        );
    }

    #[test]
    fn an_unknown_model_is_unknown_never_zero() {
        let usage = [
            usage(
                SourceId::CodexCli,
                20,
                "gpt-5-nano",
                [1_000_000, 0, 0, 0],
                1,
            ),
            usage(SourceId::CodexCli, 20, "unknown", [3_000_000, 0, 0, 0], 4),
            usage(
                SourceId::ClaudeCode,
                21,
                "claude-haiku-5-5",
                [0, 0, 0, 0],
                1,
            ),
        ];
        let estimate = estimate(&usage, &prices(None));
        assert_eq!(estimate.total, Some(Usd::from_micros(50_000)));
        assert_eq!(estimate.unknown_tokens.input, 3_000_000);
        assert_eq!(estimate.unknown_share, 0.75);
        assert_eq!(
            estimate.unknown_models().collect::<Vec<_>>(),
            ["claude-haiku-5-5", "unknown"]
        );
        let unknown = estimate
            .models
            .iter()
            .find(|m| m.model == "unknown")
            .unwrap();
        assert_eq!(
            (unknown.cost, &unknown.price, unknown.requests),
            (None, &None, 4)
        );

        // Usage whose models are all unknown has no total at all.
        let only_unknown = super::estimate(&usage[1..2], &prices(None));
        assert_eq!(only_unknown.total, None);
        assert_eq!(only_unknown.unknown_share, 1.0);
        assert!(only_unknown.tables.is_empty());
        // No usage costs nothing.
        let none = super::estimate(&[], &prices(None));
        assert_eq!(
            (none.total, none.unknown_share),
            (Some(Usd::default()), 0.0)
        );
    }

    #[test]
    fn amounts_sum_exactly_and_round_half_up_when_read() {
        // 1 output token of gpt-5-nano is $0.0000004: below a micro-dollar,
        // yet a thousand of them sum to exactly $0.0004.
        let one = estimate(
            &[usage(SourceId::CodexCli, 20, "gpt-5-nano", [0, 1, 0, 0], 1)],
            &prices(None),
        );
        assert_eq!(one.total.unwrap().micros(), 0);
        let many: Vec<_> = (0..1_000)
            .map(|i| {
                usage(
                    SourceId::CodexCli,
                    1 + i % 28,
                    "gpt-5-nano",
                    [0, 1, 0, 0],
                    1,
                )
            })
            .collect();
        assert_eq!(estimate(&many, &prices(None)).total.unwrap().micros(), 400);
        // $0.005 is half a cent: up; $0.0049996 is down.
        let half = estimate(
            &[usage(
                SourceId::CodexCli,
                20,
                "gpt-5-nano",
                [100_000, 0, 0, 0],
                1,
            )],
            &prices(None),
        );
        assert_eq!(
            (half.total.unwrap().micros(), half.total.unwrap().cents()),
            (5_000, 1)
        );
        let below = estimate(
            &[usage(
                SourceId::CodexCli,
                20,
                "gpt-5-nano",
                [0, 12_499, 0, 0],
                1,
            )],
            &prices(None),
        );
        assert_eq!(
            (below.total.unwrap().micros(), below.total.unwrap().cents()),
            (5_000, 0)
        );
        assert_eq!(
            serde_json::to_value(half.total).unwrap(),
            serde_json::json!(0.005)
        );
    }

    #[test]
    fn the_users_table_overrides_the_embedded_one_and_is_reported() {
        let user = table(
            r#"{"effective": "2026-10-15", "source": "my contract", "models": {
                "claude-sonnet-4-5": {"input": 1, "output": 5, "cache_read": 0.1, "cache_write": 1.25},
                "claude-haiku-5-5": {"input": 0.5, "output": 2.5, "cache_read": 0.05, "cache_write": 0.625}}}"#,
        );
        let usage = [
            usage(
                SourceId::ClaudeCode,
                20,
                "claude-sonnet-4-5-20250929",
                [1_000_000, 0, 0, 0],
                1,
            ),
            usage(
                SourceId::ClaudeCode,
                20,
                "claude-haiku-5-5",
                [0, 1_000_000, 0, 0],
                1,
            ),
            usage(
                SourceId::CodexCli,
                20,
                "gpt-5-nano",
                [1_000_000, 0, 0, 0],
                1,
            ),
        ];
        let estimate = estimate(&usage, &prices(Some(user)));
        // $1 (user's Sonnet) + $2.50 (user's Haiku) + $0.05 (embedded).
        assert_eq!(estimate.total, Some(Usd::from_micros(3_550_000)));
        let origins: Vec<_> = estimate
            .models
            .iter()
            .map(|m| m.price.as_ref().unwrap().origin)
            .collect();
        assert_eq!(
            origins,
            [PriceOrigin::User, PriceOrigin::User, PriceOrigin::Embedded]
        );
        let tables: Vec<_> = estimate
            .tables
            .iter()
            .map(|t| (t.origin, t.effective, t.source.as_deref()))
            .collect();
        assert_eq!(
            tables,
            [
                (PriceOrigin::Embedded, day(1), Some("vendor")),
                (PriceOrigin::User, day(15), Some("my contract"))
            ]
        );
    }

    #[test]
    fn every_estimate_says_it_is_not_a_charge_in_every_language() {
        let notice = estimate(&[], &Prices::embedded()).notice;
        assert_eq!(
            notice.resolve("pt-BR"),
            "Estimativa a preços de API, não é uma cobrança; valores em USD."
        );
        assert_eq!(
            notice.resolve("en-US"),
            "Estimate at API prices, not a charge; amounts in USD."
        );
        assert_eq!(
            notice.resolve("es"),
            "Estimación a precios de API, no es un cargo; importes en USD."
        );
    }

    /// PRD 0030 RF5.3: 1-hour writes use their own rate; without one the
    /// model's cost is unknown, never the 5-minute price.
    #[test]
    fn one_hour_writes_use_their_rate_or_make_the_cost_unknown() {
        let hour = |model: &str| {
            let mut row = usage(SourceId::ClaudeCode, 20, model, [0, 0, 0, 0], 1);
            row.tokens.cache_write_1h = 1_000_000;
            row
        };
        let priced = Prices::with_tables(
            table(
                r#"{"effective": "2026-10-01", "models": {
                    "claude-sonnet-4-5": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75, "cache_write_1h": 6}}}"#,
            ),
            None,
        );
        let with_rate = estimate(&[hour("claude-sonnet-4-5")], &priced);
        assert_eq!(with_rate.models[0].cost.unwrap().to_string(), "$6.00");
        let without = estimate(&[hour("claude-sonnet-4-5")], &prices(None));
        assert!(without.models[0].cost.is_none());
        assert!(without.total.is_none());
    }
}
