//! RF3.1/RF3.2: the per-model price table of the estimate, in USD per
//! million tokens (input, output, cache read, cache write), with the date
//! it took effect and where it came from.
//!
//! A table is embedded in the binary (`prices.json`, the providers'
//! published API prices on its effective date). The user may review and
//! override it by hand in `pricing.json` (same format) beside the cost
//! preferences: a model there wins over the embedded one; every other
//! model keeps the embedded price. Nothing is ever downloaded. A model in
//! neither table has no price, never a price of zero.

use super::CostError;
use crate::text::{params, Localized};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The only currency of every table and estimate (RF3.4).
pub const CURRENCY: &str = "USD";

/// PRD 0030 RF3.1: from this many days after a table's `effective` date the
/// cost views say how old the prices are.
pub const PRICE_AGE_NOTICE_DAYS: u32 = 60;

/// Whole days from `effective` to `today`; `None` for a date in the future.
pub(crate) fn days_since(effective: NaiveDate, today: NaiveDate) -> Option<u32> {
    u32::try_from((today - effective).num_days()).ok()
}

/// The highest price accepted, in USD per million tokens.
const MAX_USD_PER_MILLION: f64 = 10_000.0;

/// The longest `source` a table may name.
const MAX_SOURCE_CHARS: usize = 300;

static EMBEDDED: LazyLock<PriceTable> = LazyLock::new(|| {
    PriceTable::parse(include_bytes!("prices.json")).expect("the embedded price table is valid")
});

/// One price: micro-dollars per million tokens, so any price with up to
/// six decimals is exact and costs are computed in integers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rate(u64);

impl Rate {
    /// `usd` per million tokens: finite, from 0 to 10 000, with at most six
    /// decimals.
    pub fn usd_per_million(usd: f64) -> Option<Self> {
        if !usd.is_finite() || !(0.0..=MAX_USD_PER_MILLION).contains(&usd) {
            return None;
        }
        let micros = (usd * 1e6).round();
        // More than six decimals cannot be kept exactly.
        ((usd * 1e6 - micros).abs() < 1e-3).then_some(Self(micros as u64))
    }

    /// Micro-dollars per million tokens (= pico-dollars per token).
    pub fn micros_per_million(self) -> u64 {
        self.0
    }

    pub fn as_usd_per_million(self) -> f64 {
        self.0 as f64 / 1e6
    }
}

/// `3.00`, `0.075`: at least two decimals, at most the six kept.
impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fraction = format!("{:06}", self.0 % 1_000_000);
        let fraction = fraction.trim_end_matches('0');
        write!(f, "{}.{fraction:0<2}", self.0 / 1_000_000)
    }
}

impl Serialize for Rate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.as_usd_per_million())
    }
}

/// The four prices of one model, in USD per million tokens. `cache_write`
/// is the price of writing to the prompt cache (Claude's 5-minute write).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rates {
    pub input: Rate,
    pub output: Rate,
    pub cache_read: Rate,
    pub cache_write: Rate,
    /// PRD 0030 RF5.3: Claude's 1-hour write price. `None`: the table has
    /// none, and 1-hour tokens of the model are priced as unknown, never
    /// at the 5-minute rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_1h: Option<Rate>,
}

/// A price table: per model (lower case) its [`Rates`], the date the
/// prices took effect and where they came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PriceTable {
    pub currency: &'static str,
    pub effective: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub models: BTreeMap<String, Rates>,
}

/// `pricing.json` as written; validated into a [`PriceTable`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    #[serde(default)]
    currency: Option<String>,
    effective: NaiveDate,
    #[serde(default)]
    source: Option<String>,
    models: BTreeMap<String, RawRates>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRates {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    #[serde(default)]
    cache_write_1h: Option<f64>,
}

/// Why `pricing.json` was not used. The whole file is then ignored and
/// only the embedded table prices the estimate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PricingError {
    #[error("Could not read pricing.json.")]
    Unreadable,
    /// Not JSON, a missing or misspelled field, or a value of the wrong
    /// type (an `effective` that is not `YYYY-MM-DD` included).
    #[error("Invalid pricing.json at line {line}, column {column}.")]
    Syntax { line: usize, column: usize },
    #[error("pricing.json only accepts prices in USD.")]
    Currency,
    #[error("pricing.json has a source longer than 300 characters or with control characters.")]
    Source,
    /// A model name that is not one as logs write it (or `unknown`, the
    /// name of usage without a model).
    #[error(
        "pricing.json has an invalid model name: use 1 to 96 letters, digits or .-_:/@+ (and not \"unknown\")."
    )]
    Model,
    #[error("pricing.json repeats the model {model}.")]
    Duplicate { model: String },
    #[error(
        "Invalid price for {model} ({field}): use a number from 0 to 10000 dollars per million tokens, with up to six decimals."
    )]
    Rate { model: String, field: &'static str },
}

impl PricingError {
    pub fn l10n(&self) -> Localized {
        let text = self.to_string();
        match self {
            Self::Unreadable => Localized::fixed("cost.pricing.error.unreadable", text),
            Self::Syntax { line, column } => Localized::with_params(
                "cost.pricing.error.syntax",
                params([
                    ("line", line.to_string().into()),
                    ("column", column.to_string().into()),
                ]),
                text,
            ),
            Self::Currency => Localized::fixed("cost.pricing.error.currency", text),
            Self::Source => Localized::fixed("cost.pricing.error.source", text),
            Self::Model => Localized::fixed("cost.pricing.error.model", text),
            Self::Duplicate { model } => Localized::with_param(
                "cost.pricing.error.duplicate",
                "model",
                model.as_str(),
                text,
            ),
            Self::Rate { model, field } => Localized::with_params(
                "cost.pricing.error.rate",
                params([("model", model.as_str().into()), ("field", (*field).into())]),
                text,
            ),
        }
    }
}

/// `pricing.json` was ignored because of `error`.
pub fn pricing_ignored(error: &PricingError) -> Localized {
    let reason = error.l10n();
    Localized::with_param(
        "cost.pricing.ignored",
        "error",
        reason.clone(),
        "{error} Only the embedded table is used until the file is fixed.",
    )
}

/// A model name as the logs write it; the same rule the parsers apply.
fn model_key(name: &str) -> Option<String> {
    super::source::model_name(name)
        .map(|name| name.to_ascii_lowercase())
        .filter(|name| name != super::source::UNKNOWN_MODEL)
}

impl PriceTable {
    /// PRD 0030 RF3.1: days since the prices took effect; `None` when the
    /// date is in the future.
    pub fn age(&self, today: NaiveDate) -> Option<u32> {
        days_since(self.effective, today)
    }

    /// PRD 0030 RF4.2: a `pricing.json` fragment naming `models` with
    /// `null` rates to fill in, effective `today`. It never guesses a
    /// price, and it is valid only once every `null` is replaced.
    pub fn template(models: &[String], today: NaiveDate) -> Result<String, PricingError> {
        let mut entries = serde_json::Map::new();
        for name in models {
            let model = model_key(name).ok_or(PricingError::Model)?;
            entries.insert(
                model,
                serde_json::json!({
                    "input": null,
                    "output": null,
                    "cache_read": null,
                    "cache_write": null,
                }),
            );
        }
        let value = serde_json::json!({
            "currency": CURRENCY,
            "effective": today.to_string(),
            "source": "",
            "models": entries,
        });
        Ok(serde_json::to_string_pretty(&value).expect("a JSON value serializes") + "\n")
    }

    /// The table embedded in this build.
    pub fn embedded() -> &'static PriceTable {
        &EMBEDDED
    }

    /// Validate a table in the `pricing.json` format: `currency` (`USD`,
    /// optional), `effective` (`YYYY-MM-DD`), `source` (optional) and
    /// `models`, each with `input`, `output`, `cache_read` and
    /// `cache_write` in USD per million tokens. Any invalid part rejects
    /// the whole table.
    pub fn parse(bytes: &[u8]) -> Result<Self, PricingError> {
        let raw: RawTable =
            serde_json::from_slice(bytes).map_err(|error| PricingError::Syntax {
                line: error.line(),
                column: error.column(),
            })?;
        if raw
            .currency
            .as_deref()
            .is_some_and(|currency| !currency.eq_ignore_ascii_case(CURRENCY))
        {
            return Err(PricingError::Currency);
        }
        let source = raw.source.map(|source| source.trim().to_owned());
        if source.as_deref().is_some_and(|source| {
            source.chars().count() > MAX_SOURCE_CHARS || source.chars().any(char::is_control)
        }) {
            return Err(PricingError::Source);
        }
        let mut models = BTreeMap::new();
        for (name, raw) in raw.models {
            let model = model_key(&name).ok_or(PricingError::Model)?;
            let rate = |field: &'static str, usd: f64| {
                Rate::usd_per_million(usd).ok_or_else(|| PricingError::Rate {
                    model: model.clone(),
                    field,
                })
            };
            let rates = Rates {
                input: rate("input", raw.input)?,
                output: rate("output", raw.output)?,
                cache_read: rate("cache_read", raw.cache_read)?,
                cache_write: rate("cache_write", raw.cache_write)?,
                cache_write_1h: raw
                    .cache_write_1h
                    .map(|usd| rate("cache_write_1h", usd))
                    .transpose()?,
            };
            if models.insert(model.clone(), rates).is_some() {
                return Err(PricingError::Duplicate { model });
            }
        }
        Ok(Self {
            currency: CURRENCY,
            effective: raw.effective,
            source: source.filter(|source| !source.is_empty()),
            models,
        })
    }
}

/// Which table a price came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceOrigin {
    /// The table embedded in the binary.
    Embedded,
    /// The user's `pricing.json`.
    User,
}

/// The price of one model and where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Price {
    /// The table entry that matched (the model, or the model without its
    /// date suffix).
    pub entry: String,
    pub origin: PriceOrigin,
    pub rates: Rates,
}

/// The date and the source of one table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableInfo {
    pub origin: PriceOrigin,
    pub effective: NaiveDate,
    pub source: Option<String>,
}

impl TableInfo {
    /// PRD 0030 RF3.1: days since the prices took effect.
    pub fn age(&self, today: NaiveDate) -> Option<u32> {
        days_since(self.effective, today)
    }
}

/// One row of the table in effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PriceRow {
    pub model: String,
    pub origin: PriceOrigin,
    pub rates: Rates,
}

/// The prices in effect: the user's table over the embedded one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prices {
    embedded: Cow<'static, PriceTable>,
    user: Option<PriceTable>,
}

/// `name` without a dated snapshot suffix (`-20250929`, `-2025-08-07`).
fn undated(name: &str) -> Option<&str> {
    let digits = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    let (rest, last) = name.rsplit_once('-')?;
    if digits(last, 8) {
        return Some(rest);
    }
    let (rest, month) = rest.rsplit_once('-')?;
    let (base, year) = rest.rsplit_once('-')?;
    (digits(year, 4) && digits(month, 2) && digits(last, 2)).then_some(base)
}

impl Prices {
    /// Only the embedded table.
    pub fn embedded() -> Self {
        Self::new(None)
    }

    /// The embedded table, overridden by `user` where it names a model.
    pub fn new(user: Option<PriceTable>) -> Self {
        Self {
            embedded: Cow::Borrowed(PriceTable::embedded()),
            user,
        }
    }

    /// `user` over `embedded` (tests and other embedders).
    pub fn with_tables(embedded: PriceTable, user: Option<PriceTable>) -> Self {
        Self {
            embedded: Cow::Owned(embedded),
            user,
        }
    }

    /// RF3.2: the embedded table under `file`'s, when it exists and is
    /// valid; an invalid file is ignored as a whole and its error returned.
    pub fn with_user_file(file: &PricingFile) -> (Self, Option<PricingError>) {
        match file.load() {
            Ok(user) => (Self::new(user), None),
            Err(error) => (Self::embedded(), Some(error)),
        }
    }

    pub fn user(&self) -> Option<&PriceTable> {
        self.user.as_ref()
    }

    fn tables(&self) -> impl Iterator<Item = (PriceOrigin, &PriceTable)> {
        self.user
            .iter()
            .map(|table| (PriceOrigin::User, table))
            .chain([(PriceOrigin::Embedded, self.embedded.as_ref())])
    }

    /// The price of `model` as the logs name it: the user's table first,
    /// then the embedded one; in each, the exact name (any case), then the
    /// name without a dated snapshot suffix. `None`: the cost is unknown.
    pub fn price(&self, model: &str) -> Option<Price> {
        let model = model.to_ascii_lowercase();
        let names: Vec<&str> = [Some(model.as_str()), undated(&model)]
            .into_iter()
            .flatten()
            .collect();
        self.tables().find_map(|(origin, table)| {
            names.iter().find_map(|name| {
                table.models.get(*name).map(|rates| Price {
                    entry: (*name).to_owned(),
                    origin,
                    rates: *rates,
                })
            })
        })
    }

    /// The date and source of each table in use, the embedded one first.
    pub fn info(&self) -> Vec<TableInfo> {
        let mut info: Vec<_> = self
            .tables()
            .map(|(origin, table)| TableInfo {
                origin,
                effective: table.effective,
                source: table.source.clone(),
            })
            .collect();
        info.reverse();
        info
    }

    /// Every model with a price, by name, with the price in effect.
    pub fn rows(&self) -> Vec<PriceRow> {
        let mut rows = BTreeMap::new();
        for (origin, table) in self.tables() {
            for (model, rates) in &table.models {
                rows.entry(model.clone()).or_insert(PriceRow {
                    model: model.clone(),
                    origin,
                    rates: *rates,
                });
            }
        }
        rows.into_values().collect()
    }
}

/// Where the user's table lives; injectable so tests never touch OS dirs.
#[derive(Debug, Clone)]
pub struct PricingFile {
    path: PathBuf,
}

impl PricingFile {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<OS local data dir>/cuotai/cost/pricing.json`, beside the cost
    /// preferences.
    pub fn in_data_dir() -> Result<Self, CostError> {
        dirs::data_local_dir()
            .map(|dir| Self::at(dir.join("cuotai").join("cost").join("pricing.json")))
            .ok_or(CostError::Settings)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The user's table; `None` when there is no file. Never written by
    /// Cuotai.
    pub fn load(&self) -> Result<Option<PriceTable>, PricingError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => PriceTable::parse(&bytes).map(Some),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err(PricingError::Unreadable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(json: &str) -> Result<PriceTable, PricingError> {
        PriceTable::parse(json.as_bytes())
    }

    #[test]
    fn age_counts_days_and_ignores_future_dates() {
        let t = table(r#"{"effective":"2026-10-01","models":{}}"#).unwrap();
        let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        assert_eq!(t.age(day(1)), Some(0));
        assert_eq!(t.age(day(31)), Some(30));
        assert_eq!(t.age(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()), None);
    }

    #[test]
    fn the_template_has_null_rates_and_is_not_valid_until_filled() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        let text =
            PriceTable::template(&["Claude-Foo-6".into(), "gpt-x-mini".into()], today).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["effective"], "2026-10-25");
        assert_eq!(json["currency"], "USD");
        assert!(json["models"]["claude-foo-6"]["input"].is_null());
        assert!(json["models"]["gpt-x-mini"]["cache_write"].is_null());
        assert!(table(&text).is_err(), "nulls must be filled in");
        let filled = text.replace("null", "1");
        let parsed = table(&filled).unwrap();
        assert_eq!(parsed.models.len(), 2);
        assert!(PriceTable::template(&["bad name".into()], today).is_err());
    }

    fn usd(rate: Rate) -> f64 {
        rate.as_usd_per_million()
    }

    #[test]
    fn the_embedded_table_prices_the_current_models_of_both_tools() {
        let prices = Prices::embedded();
        let opus = prices.price("claude-opus-4-1-20250805").unwrap();
        assert_eq!(
            (opus.entry.as_str(), opus.origin),
            ("claude-opus-4-1", PriceOrigin::Embedded)
        );
        assert_eq!(
            [
                opus.rates.input,
                opus.rates.output,
                opus.rates.cache_read,
                opus.rates.cache_write
            ]
            .map(usd),
            [15.0, 75.0, 1.5, 18.75]
        );
        assert!(prices.price("gpt-5.3-codex").is_some());
        assert!(prices.price("GPT-5-2025-08-07").is_some());
        // Priced by prompt length per request, which the daily index does
        // not keep: unknown rather than a guess.
        assert_eq!(prices.price("claude-haiku-5-5"), None);
        assert_eq!(prices.price("unknown"), None);
        let [info] = prices.info().try_into().unwrap();
        assert_eq!(info.origin, PriceOrigin::Embedded);
        assert!(info.source.unwrap().contains("platform.claude.com"));
    }

    #[test]
    fn a_dated_suffix_falls_back_to_the_undated_entry_but_never_to_a_sibling() {
        let prices = Prices::with_tables(
            table(
                r#"{"effective": "2026-01-01", "models": {
                    "gpt-5": {"input": 1, "output": 2, "cache_read": 0.1, "cache_write": 1},
                    "gpt-5-2025-08-07": {"input": 9, "output": 9, "cache_read": 9, "cache_write": 9}}}"#,
            )
            .unwrap(),
            None,
        );
        assert_eq!(
            prices.price("gpt-5-2025-08-07").unwrap().entry,
            "gpt-5-2025-08-07"
        );
        assert_eq!(prices.price("gpt-5-2026-01-31").unwrap().entry, "gpt-5");
        assert_eq!(prices.price("gpt-5-20260131").unwrap().entry, "gpt-5");
        for other in [
            "gpt-5-mini",
            "gpt-5-codex",
            "gpt-5-2026",
            "gpt-5-2026-1-31",
            "gpt-5x",
        ] {
            assert_eq!(prices.price(other), None, "{other}");
        }
    }

    #[test]
    fn the_users_table_wins_model_by_model() {
        let embedded = table(
            r#"{"effective": "2026-01-01", "source": "vendor", "models": {
                "claude-sonnet-4-5": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75},
                "claude-sonnet-4-5-20250929": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75},
                "gpt-5": {"input": 1.25, "output": 10, "cache_read": 0.125, "cache_write": 1.25}}}"#,
        )
        .unwrap();
        let user = table(
            r#"{"currency": "usd", "effective": "2026-10-01", "models": {
                "Claude-Sonnet-4-5": {"input": 2, "output": 10, "cache_read": 0.2, "cache_write": 2.5},
                "my-local-model": {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}}}"#,
        )
        .unwrap();
        let prices = Prices::with_tables(embedded, Some(user));
        // The user's undated entry beats even an exact embedded snapshot.
        let sonnet = prices.price("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(
            (sonnet.entry.as_str(), sonnet.origin),
            ("claude-sonnet-4-5", PriceOrigin::User)
        );
        assert_eq!(usd(sonnet.rates.input), 2.0);
        assert_eq!(prices.price("gpt-5").unwrap().origin, PriceOrigin::Embedded);
        // A price of zero written by the user is a known price.
        assert_eq!(
            prices.price("my-local-model").unwrap().rates.output,
            Rate(0)
        );
        let rows: Vec<_> = prices
            .rows()
            .into_iter()
            .map(|r| (r.model, r.origin))
            .collect();
        assert_eq!(
            rows,
            [
                ("claude-sonnet-4-5".to_owned(), PriceOrigin::User),
                (
                    "claude-sonnet-4-5-20250929".to_owned(),
                    PriceOrigin::Embedded
                ),
                ("gpt-5".to_owned(), PriceOrigin::Embedded),
                ("my-local-model".to_owned(), PriceOrigin::User),
            ]
        );
        let info: Vec<_> = prices
            .info()
            .into_iter()
            .map(|i| (i.origin, i.effective.to_string(), i.source))
            .collect();
        assert_eq!(
            info,
            [
                (
                    PriceOrigin::Embedded,
                    "2026-01-01".to_owned(),
                    Some("vendor".to_owned())
                ),
                (PriceOrigin::User, "2026-10-01".to_owned(), None),
            ]
        );
    }

    #[test]
    fn an_invalid_users_table_is_rejected_as_a_whole() {
        let rates = r#"{"input": 1, "output": 2, "cache_read": 0.1, "cache_write": 1}"#;
        let with =
            |models: &str| format!(r#"{{"effective": "2026-10-01", "models": {{{models}}}}}"#);
        assert_eq!(
            table("{ nope"),
            Err(PricingError::Syntax { line: 1, column: 3 })
        );
        assert!(matches!(
            table(r#"{"effective": "21/10/2026", "models": {}}"#),
            Err(PricingError::Syntax { .. })
        ));
        // A misspelled field is not silently a missing price.
        assert!(matches!(
            table(&with(
                r#""gpt-5": {"input": 1, "output": 2, "cache_reads": 0.1, "cache_write": 1}"#
            )),
            Err(PricingError::Syntax { .. })
        ));
        assert!(matches!(
            table(&with(r#""gpt-5": {"input": 1, "output": 2}"#)),
            Err(PricingError::Syntax { .. })
        ));
        assert_eq!(
            table(&format!(
                r#"{{"currency": "BRL", "effective": "2026-10-01", "models": {{"gpt-5": {rates}}}}}"#
            )),
            Err(PricingError::Currency)
        );
        // Escaped in JSON, so the control character reaches the check.
        for name in ["", "unknown", "a model", r"gpt\u001b[2J"] {
            assert_eq!(
                table(&with(&format!(r#""{name}": {rates}"#))),
                Err(PricingError::Model),
                "{name:?}"
            );
        }
        assert_eq!(
            table(&with(&format!(r#""GPT-5": {rates}, "gpt-5": {rates}"#))),
            Err(PricingError::Duplicate {
                model: "gpt-5".into()
            })
        );
        let invalid: [(&'static str, &str); 3] = [
            ("input", "-1"),
            ("output", "10000.5"),
            ("cache_read", "0.0000001"),
        ];
        for (field, value) in invalid {
            let mut raw: serde_json::Value = serde_json::from_str(rates).unwrap();
            raw[field] = serde_json::from_str(value).unwrap();
            assert_eq!(
                table(&with(&format!(r#""gpt-5": {raw}"#))),
                Err(PricingError::Rate {
                    model: "gpt-5".into(),
                    field
                }),
                "{field}"
            );
        }
        assert_eq!(
            table(&format!(
                r#"{{"effective": "2026-10-01", "source": "line\nbreak", "models": {{"gpt-5": {rates}}}}}"#
            )),
            Err(PricingError::Source)
        );
    }

    #[test]
    fn the_users_file_is_optional_and_an_invalid_one_falls_back_to_the_embedded_table() {
        let dir = tempfile::tempdir().unwrap();
        let file = PricingFile::at(dir.path().join("pricing.json"));
        assert_eq!(file.load(), Ok(None));
        let (prices, error) = Prices::with_user_file(&file);
        assert_eq!((prices, error), (Prices::embedded(), None));

        std::fs::write(file.path(), r#"{"effective": "2026-10-01", "models": {"claude-haiku-5-5": {"input": 0.5, "output": 2.5, "cache_read": 0.05, "cache_write": 0.625}}}"#).unwrap();
        let (prices, error) = Prices::with_user_file(&file);
        assert_eq!(error, None);
        assert_eq!(
            prices.price("claude-haiku-5-5").unwrap().origin,
            PriceOrigin::User
        );

        std::fs::write(file.path(), r#"{"effective": "2026-10-01", "models": {"claude-haiku-5-5": {"input": -1, "output": 2.5, "cache_read": 0.05, "cache_write": 0.625}}}"#).unwrap();
        let (prices, error) = Prices::with_user_file(&file);
        assert_eq!(prices, Prices::embedded());
        let error = error.unwrap();
        assert_eq!(
            pricing_ignored(&error).resolve("en-US"),
            "Invalid price for claude-haiku-5-5 (input): use a number from 0 to 10000 dollars per million tokens, with up to six decimals. Only the embedded table is used until the file is fixed."
        );
    }

    #[test]
    fn rates_keep_six_decimals_and_print_at_least_two() {
        let text = |usd: f64| Rate::usd_per_million(usd).unwrap().to_string();
        assert_eq!(text(3.0), "3.00");
        assert_eq!(text(12.5), "12.50");
        assert_eq!(text(0.075), "0.075");
        assert_eq!(text(0.000001), "0.000001");
        assert_eq!(
            Rate::usd_per_million(0.3).unwrap().micros_per_million(),
            300_000
        );
        assert_eq!(Rate::usd_per_million(f64::NAN), None);
        assert_eq!(
            Rate::usd_per_million(10_000.0)
                .unwrap()
                .micros_per_million(),
            10_000_000_000
        );
    }
}
