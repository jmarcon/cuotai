//! PRD 0015 — estimated cost from local logs: sources and consent (RF1),
//! the incremental index (RF2), prices and computation (RF3) and what
//! front ends display (RF4).
//!
//! Off by default. A source ([`SourceId`]) is one command-line tool whose
//! session logs live in known directories; each has an adapter behind the
//! crate's `Source` trait (an id, its default directories, a file pattern
//! and a parser). Nothing is read before the user consents to a source
//! through its [`Disclosure`] (exactly which directories, what for, and
//! that only numbers are extracted), recorded per source by
//! [`SettingsStore`]; a changed directory list needs a new consent.
//!
//! The parsers extract only the time, the model, the token counts (input,
//! output, cache read, cache write) and a digest of the session
//! identifier; conversation content, prompts, file names and project paths
//! are never deserialized into a value, stored, logged or shown. Invalid
//! lines are skipped and counted.
//!
//! [`Scanner`] reads only what grew since the last scan into
//! [`CostIndex`] (`cost.sqlite3`, `0600`), bounded by [`Limits`];
//! [`CostWorker`] runs it on a low-priority thread of its own, never on the
//! refresh's path, and pauses in energy-saving mode. [`turn_off`] and
//! [`CostIndex::erase`] remove everything derived.
//!
//! [`estimate`] turns indexed usage into an [`Estimate`] in USD at the
//! prices in effect ([`Prices`]): the [`PriceTable`] embedded in the binary,
//! overridden model by model by the user's own `pricing.json`
//! ([`PricingFile`]). A model without a known price is "cost unknown",
//! never zero, and every estimate says it is not a charge.
//!
//! [`Scanner::summary`] ([`summarize`], pure) gives every front end the
//! same [`CostSummary`]: per source today, yesterday and the span shown,
//! the breakdown by day and by model, and why a source has no numbers.

mod claude;
mod codex;
mod estimate;
mod gemini;
mod index;
mod plan;
mod prices;
mod scan;
mod settings;
mod source;
mod system;
mod view;
mod worker;

pub use estimate::{estimate, estimate_notice, Estimate, ModelEstimate, Usd};
pub use index::{CostIndex, DailyUsage, Indexed};
pub use plan::{
    cycle_start, earliest_cycle, migrate_plan_keys, plan_accounts, plan_values, DisplayCurrency,
    PlanAccount, PlanAmount, PlanInput, PlanSetting, PlanValue, MAX_CYCLE_DAY, RATE_STALE_DAYS,
};
pub use prices::{
    pricing_ignored, Price, PriceOrigin, PriceRow, PriceTable, Prices, PricingError, PricingFile,
    Rate, Rates, TableInfo, CURRENCY, PRICE_AGE_NOTICE_DAYS,
};
pub use scan::{Limits, RootScan, RootState, ScanReport, Scanner, SourceScan, SourceState};
pub use settings::{CostSettings, Disclosure, Env, SettingsStore, SourceSettings};
pub use system::energy_saving;
pub use view::{
    by_day_heading, by_model_heading, compact_tokens, enable_hint, heading, prices_age_text,
    summarize, unpriced_text, usage_line, yesterday_label, CostState, CostSummary, DayCost,
    SourceCost, SourceUsage, Span, SpanView, SummaryInput,
};
pub use worker::{CostStatus, CostWorker, Schedule, WorkerState};

use crate::text::{params, Localized};
use crate::Provider;
use chrono::{DateTime, Datelike, Duration as Days, FixedOffset, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// A tool whose local session logs Cuotai can read (RF1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceId {
    /// Claude Code: `projects/**/*.jsonl` transcripts.
    ClaudeCode,
    /// Codex CLI: `sessions/**/rollout-*.jsonl` rollouts.
    CodexCli,
    /// Gemini CLI: `~/.gemini/tmp/<project>/chats/*.jsonl` recordings
    /// (PRD 0030 RF2).
    GeminiCli,
}

impl SourceId {
    pub const ALL: [SourceId; 3] = [
        SourceId::ClaudeCode,
        SourceId::CodexCli,
        SourceId::GeminiCli,
    ];

    /// The stable identifier (`claude_code`, `codex_cli`) of the settings,
    /// the index and the command line.
    pub fn slug(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::CodexCli => "codex_cli",
            Self::GeminiCli => "gemini_cli",
        }
    }

    /// The tool's name, shown as is in every language.
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::CodexCli => "Codex CLI",
            Self::GeminiCli => "Gemini CLI",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.slug() == slug)
    }

    /// RF4.1: the providers whose accounts are used through this tool and
    /// show its local usage: the window-based subscriptions (Claude for
    /// Claude Code, ChatGPT / Codex for Codex CLI).
    pub fn providers(self) -> &'static [Provider] {
        match self {
            Self::ClaudeCode => &[Provider::Claude],
            Self::CodexCli => &[Provider::ChatGpt],
            Self::GeminiCli => &[Provider::GeminiAi],
        }
    }

    /// The source whose local usage the accounts of `provider` show.
    pub fn for_provider(provider: Provider) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.providers().contains(&provider))
    }

    pub(crate) fn adapter(self) -> &'static dyn source::Source {
        match self {
            Self::ClaudeCode => &claude::ClaudeCode,
            Self::CodexCli => &codex::CodexCli,
            Self::GeminiCli => &gemini::GeminiCli,
        }
    }
}

/// Token counts of one request or of an aggregate. `input` excludes the
/// cached part (`cache_read`) and the part written to the cache
/// (`cache_write`, `cache_write_1h`), so they never overlap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// PRD 0030 RF5: Claude's 1-hour cache writes, apart from the 5-minute
    /// writes in `cache_write` (they cost more).
    #[serde(default)]
    pub cache_write_1h: u64,
}

impl Tokens {
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
            .saturating_add(self.cache_write_1h)
    }

    pub fn add(&mut self, other: &Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
    }
}

/// The span of days a front end shows (RF2.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Today,
    #[default]
    #[serde(rename = "last_7_days")]
    Last7Days,
    #[serde(rename = "last_30_days")]
    Last30Days,
    CurrentMonth,
    /// Everything the index still holds (bounded by the [`Retention`]).
    All,
}

impl Period {
    pub const ALL: [Period; 5] = [
        Period::Today,
        Period::Last7Days,
        Period::Last30Days,
        Period::CurrentMonth,
        Period::All,
    ];

    /// The stable identifier (`today`, `last_7_days`, `last_30_days`,
    /// `current_month`, `all`) of the settings and the command line.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Last7Days => "last_7_days",
            Self::Last30Days => "last_30_days",
            Self::CurrentMonth => "current_month",
            Self::All => "all",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|period| period.slug() == slug)
    }

    /// The first local day of the period ending `today` (inclusive);
    /// `None` for [`Period::All`].
    pub fn first_day(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Today => Some(today),
            Self::Last7Days => Some(today - Days::days(6)),
            Self::Last30Days => Some(today - Days::days(29)),
            Self::CurrentMonth => today.with_day(1),
            Self::All => None,
        }
    }

    pub fn label(self) -> Localized {
        let (id, text) = match self {
            Self::Today => ("cost.period.today", "Today"),
            Self::Last7Days => ("cost.period.last_7_days", "Last 7 days"),
            Self::Last30Days => ("cost.period.last_30_days", "Last 30 days"),
            Self::CurrentMonth => ("cost.period.current_month", "Current month"),
            Self::All => ("cost.period.all", "Everything available"),
        };
        Localized::fixed(id, text)
    }
}

/// How many local days the index keeps (RF2.3): 120 by default, from
/// [`Retention::MIN_DAYS`] to [`Retention::MAX_DAYS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Retention(u32);

impl Retention {
    pub const DEFAULT_DAYS: u32 = 120;
    pub const MIN_DAYS: u32 = 7;
    pub const MAX_DAYS: u32 = 730;

    pub fn days(days: u32) -> Result<Self, CostError> {
        if (Self::MIN_DAYS..=Self::MAX_DAYS).contains(&days) {
            Ok(Self(days))
        } else {
            Err(CostError::RetentionBounds)
        }
    }

    pub fn keep_days(self) -> u32 {
        self.0
    }

    /// The oldest local day kept when today is `today`.
    pub fn first_day(self, today: NaiveDate) -> NaiveDate {
        today - Days::days(i64::from(self.0) - 1)
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self(Self::DEFAULT_DAYS)
    }
}

/// Which clock decides a usage's local day: the computer's (default) or a
/// fixed offset (tests). A change of time zone does not move days already
/// indexed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DayZone {
    #[default]
    Local,
    Fixed(FixedOffset),
}

impl DayZone {
    pub fn day(self, at: DateTime<Utc>) -> NaiveDate {
        match self {
            Self::Local => at.with_timezone(&Local).date_naive(),
            Self::Fixed(offset) => at.with_timezone(&offset).date_naive(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CostError {
    /// The index could not be opened, read or written.
    #[error("Could not read or write the local cost index.")]
    Storage,
    /// The cost preferences could not be read or written.
    #[error("Could not read or write the local cost preferences.")]
    Settings,
    #[error("The cost index retention ranges from 7 to 730 days.")]
    RetentionBounds,
    /// The directories of a source changed since its disclosure was shown.
    #[error(
        "The directories of this source changed since they were shown; review and confirm again."
    )]
    ConsentChanged,
    /// `cuotai --cost --days` out of bounds.
    #[error("The number of days ranges from 1 to 730.")]
    DaysBounds,
    /// A chosen directory that is neither absolute nor under `~/`.
    #[error("Use a directory that is absolute or starts with ~/.")]
    RelativePath,
    /// PRD 0030 RF1.1: a plan price, currency or cycle day out of bounds.
    #[error(
        "Invalid plan: a price above zero, a three-letter currency and a cycle day from 1 to 28."
    )]
    InvalidPlan,
    /// PRD 0030 RF6.1: a rate or currency code out of bounds.
    #[error("Invalid rate: a three-letter currency other than USD and a rate above zero.")]
    InvalidRate,
}

impl CostError {
    pub fn l10n(&self) -> Localized {
        match self {
            Self::Storage => Localized::fixed("cost.error.storage", self.to_string()),
            Self::Settings => Localized::fixed("cost.error.settings", self.to_string()),
            Self::RetentionBounds => Localized::with_params(
                "cost.error.retention_bounds",
                params([
                    ("min", Retention::MIN_DAYS.to_string().into()),
                    ("max", Retention::MAX_DAYS.to_string().into()),
                ]),
                "The cost index retention ranges from {min} to {max} days.",
            ),
            Self::ConsentChanged => {
                Localized::fixed("cost.error.consent_changed", self.to_string())
            }
            Self::DaysBounds => Localized::with_params(
                "cost.error.days_bounds",
                params([
                    ("min", 1u64.into()),
                    ("max", u64::from(Span::MAX_DAYS).into()),
                ]),
                "The number of days ranges from {min} to {max}.",
            ),
            Self::RelativePath => Localized::fixed("cost.error.relative_path", self.to_string()),
            Self::InvalidPlan => Localized::fixed("cost.error.invalid_plan", self.to_string()),
            Self::InvalidRate => Localized::fixed("cost.error.invalid_rate", self.to_string()),
        }
    }
}

impl From<rusqlite::Error> for CostError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

/// The name given is not a source.
pub fn unknown_source(name: &str) -> Localized {
    let known = SourceId::ALL.map(SourceId::slug).join(", ");
    Localized::with_params(
        "cost.error.unknown_source",
        params([("name", name.into()), ("known", known.as_str().into())]),
        "Unknown local cost source: {name}. Sources: {known}.",
    )
}

/// The word that accepts a [`Disclosure`] in a terminal.
pub fn confirm_answer() -> Localized {
    Localized::fixed("cost.confirm.answer", "yes")
}

/// The question after the disclosure, asking for [`confirm_answer`].
pub fn confirm_question(answer: &str) -> Localized {
    Localized::with_param(
        "cost.confirm.question",
        "answer",
        answer,
        "To turn it on, type \"{answer}\" and press Enter (any other answer declines):",
    )
}

/// The user answered anything but [`confirm_answer`].
pub fn declined() -> Localized {
    Localized::fixed(
        "cost.error.declined",
        "Source not turned on; no log was read.",
    )
}

/// Nobody is there to read the disclosure and confirm it.
pub fn needs_terminal() -> Localized {
    Localized::fixed(
        "cost.error.needs_terminal",
        "Turning a local cost source on needs a confirmation in an interactive terminal.",
    )
}

/// RF2.4: turn `source` off, then remove everything derived from it from
/// `index`. The preference is written first, so a scan running elsewhere
/// (another front end) sees the source off before it could write again.
/// Returns the index rows removed.
pub fn turn_off(
    settings: &SettingsStore,
    index: &CostIndex,
    source: SourceId,
) -> Result<usize, CostError> {
    settings.disable(source)?;
    index.erase_source(source)
}

/// RF4.1: read `paths` (empty: the defaults) for `source` from now on; each
/// is absolute or starts with `~/`. The consent covered the directories it
/// showed, so a list that differs from the saved one turns the source off
/// first, erasing everything derived from it (as [`turn_off`]); turning it
/// on again shows the new list. Returns the index rows removed.
pub fn choose_paths(
    settings: &SettingsStore,
    index: &CostIndex,
    source: SourceId,
    paths: Vec<std::path::PathBuf>,
    env: &Env,
) -> Result<usize, CostError> {
    let mut chosen: Vec<std::path::PathBuf> = Vec::with_capacity(paths.len());
    for path in paths {
        let text = path.to_string_lossy();
        if !(path.is_absolute() || text == "~" || text.starts_with("~/")) {
            return Err(CostError::RelativePath);
        }
        if !chosen.contains(&path) {
            chosen.push(path);
        }
    }
    let saved = settings.load()?;
    let current = saved.sources.get(&source).map_or(&[][..], |s| &s.paths);
    if current == chosen.as_slice() {
        return Ok(0);
    }
    let removed = if saved.enabled(source, env) {
        turn_off(settings, index, source)?
    } else {
        0
    };
    settings.set_paths(source, chosen)?;
    Ok(removed)
}

/// `source` was turned on.
pub fn enabled_text(source: SourceId) -> Localized {
    let label = source.label();
    Localized::with_param(
        "cost.enabled",
        "source",
        label,
        "{source} turned on. The logs are read in the background while the TUI or the desktop app is open, or now with cuotai --cost-scan.",
    )
}

/// `source` was turned off and `removed` derived rows were erased.
pub fn disabled_text(source: SourceId, removed: usize) -> Localized {
    let label = source.label();
    Localized::with_params(
        "cost.disabled",
        params([("source", label.into()), ("removed", removed.into())]),
        "{source} turned off; {removed} derived record(s) erased.",
    )
}

/// The whole index was erased (`removed` rows).
pub fn erased_text(removed: usize) -> Localized {
    Localized::with_param(
        "cost.erased",
        "removed",
        removed,
        "Local cost index erased: {removed} record(s).",
    )
}

/// `name` is not a period.
pub fn unknown_period(name: &str) -> Localized {
    let known = Period::ALL.map(Period::slug).join(", ");
    Localized::with_params(
        "cost.error.unknown_period",
        params([("name", name.into()), ("known", known.as_str().into())]),
        "Unknown period: {name}. Periods: {known}.",
    )
}

#[cfg(test)]
mod tests;
