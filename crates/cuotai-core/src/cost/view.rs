//! RF4: what front ends show of the estimated cost: per source today,
//! yesterday and the span shown, with the breakdown by day and by model,
//! the state of the source (off, not read yet, no logs, no access, reading
//! not finished) and why there are no numbers when there are none.
//!
//! [`summarize`] is pure; [`Scanner::summary`] feeds it from the
//! preferences, the index and the prices in effect, and reads no log. The
//! same [`CostSummary`] is the desktop's cards and details, the TUI's
//! account section, `cuotai --cost --json` and `local_cost` in
//! `export::v1`. It never holds a path, a file name or anything but
//! numbers, model names and texts.

use super::estimate::{estimate, estimate_notice, Estimate, ModelEstimate, Usd};
use super::index::DailyUsage;
use super::prices::{pricing_ignored, Prices, PricingError, PricingFile, CURRENCY};
use super::scan::{no_logs_text, off_text, ScanReport, Scanner, SourceState};
use super::{CostError, Period, Retention, SourceId, Tokens};
use crate::text::{params, Localized, ParamValue};
use crate::Provider;
use chrono::{DateTime, Duration as Days, NaiveDate, Utc};
use serde::Serialize;

/// RF4.2: the local days a display covers: the saved [`Period`], or the
/// last `n` days (`cuotai --cost --days N`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Period(Period),
    Days(u32),
}

impl Span {
    /// The longest span in days: the longest retention.
    pub const MAX_DAYS: u32 = Retention::MAX_DAYS;

    /// The last `days` local days, today included (1 to [`Self::MAX_DAYS`]).
    pub fn days(days: u32) -> Result<Self, CostError> {
        if (1..=Self::MAX_DAYS).contains(&days) {
            Ok(Self::Days(days))
        } else {
            Err(CostError::DaysBounds)
        }
    }

    /// The first local day of the span ending `today`; `None` for
    /// everything the index holds.
    pub fn first_day(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Period(period) => period.first_day(today),
            Self::Days(days) => Some(today - Days::days(i64::from(days) - 1)),
        }
    }

    pub fn label(self) -> Localized {
        match self {
            Self::Period(period) => period.label(),
            Self::Days(days) => Localized::with_param(
                "cost.span.days",
                "days",
                u64::from(days),
                format!("Últimos {days} dias"),
            ),
        }
    }
}

/// The span of a [`CostSummary`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpanView {
    /// The saved period shown; `null` with a number of days.
    pub period: Option<Period>,
    /// The number of days asked for; `null` with a period.
    pub days: Option<u32>,
    pub label: Localized,
    /// `null`: everything the index holds.
    pub first_day: Option<NaiveDate>,
    /// Today, the computer's local day.
    pub last_day: NaiveDate,
}

/// What the numbers of one source stand on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CostState {
    /// Not consented: nothing is read and nothing is shown.
    Off,
    /// The last scan read every log that changed.
    Ready,
    /// The last scan stopped at a limit; the next one continues, so the
    /// numbers may still grow.
    Partial,
    /// None of the default directories exists.
    NoLogs,
    /// No directory could be read; the numbers are what the index held.
    NoAccess,
    /// On, but not scanned yet by this process; the numbers are what the
    /// index held.
    NotRead,
}

/// One local day of a [`SourceUsage`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayCost {
    pub day: NaiveDate,
    pub tokens: Tokens,
    pub requests: u64,
    /// The cost of the part with a known price; `null` when there is usage
    /// and none of it is priced, never zero for unpriced usage.
    pub cost: Option<Usd>,
    /// The tokens of models without a known price.
    pub unknown_tokens: Tokens,
}

impl DayCost {
    /// `2026-10-20: 1.2M tokens · $3.40`, for a terminal.
    pub fn line(&self) -> Localized {
        usage_line(
            self.day.to_string(),
            &self.tokens,
            self.cost,
            &self.unknown_tokens,
        )
    }
}

/// The numbers of one source that is on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceUsage {
    pub today: Estimate,
    pub yesterday: Estimate,
    /// The span shown; its `models` are the breakdown by model.
    pub period: Estimate,
    /// The breakdown by day, oldest first: every local day of the span
    /// (from its first day, or from the oldest day with usage when it is
    /// everything, to today), days without usage included with zero
    /// tokens. Empty when the span has no usage.
    pub days: Vec<DayCost>,
}

/// One source in a [`CostSummary`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceCost {
    pub source: SourceId,
    pub label: &'static str,
    /// RF4.1: the accounts of these providers show this source, all in the
    /// same place ([`SourceId::providers`]).
    pub providers: &'static [Provider],
    pub state: CostState,
    /// Why there are no numbers or what limits them (off, not read yet, no
    /// logs, no access, reading not finished, nothing in the span); `null`
    /// when the numbers are complete.
    pub note: Option<Localized>,
    /// `null` while the source is off.
    pub usage: Option<SourceUsage>,
}

/// RF4: the estimated cost from this computer's local logs, as every front
/// end shows it. In USD only, never summed with a provider's billing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CostSummary {
    /// Always `USD`.
    pub currency: &'static str,
    /// RF3.4: "estimate at API prices, not a charge".
    pub notice: Localized,
    pub span: SpanView,
    /// Every source, on or off, in [`SourceId::ALL`] order.
    pub sources: Vec<SourceCost>,
    /// The span over every source that is on.
    pub total: Estimate,
    /// The user's `pricing.json` is invalid and ignored as a whole: only the
    /// embedded prices are used.
    pub pricing_error: Option<Localized>,
}

impl CostSummary {
    /// Some source is on.
    pub fn any_on(&self) -> bool {
        self.sources
            .iter()
            .any(|source| source.state != CostState::Off)
    }

    pub fn source(&self, source: SourceId) -> Option<&SourceCost> {
        self.sources.iter().find(|s| s.source == source)
    }

    /// RF4.1: the source shown on the accounts of `provider`, if any.
    pub fn for_provider(&self, provider: Provider) -> Option<&SourceCost> {
        SourceId::for_provider(provider).and_then(|source| self.source(source))
    }
}

/// What [`summarize`] works from.
#[derive(Debug, Clone, Copy)]
pub struct SummaryInput<'a> {
    /// The computer's local day.
    pub today: NaiveDate,
    pub span: Span,
    /// The sources that are on; the others show nothing.
    pub enabled: &'a [SourceId],
    /// Indexed usage covering at least the span and yesterday.
    pub usage: &'a [DailyUsage],
    pub prices: &'a Prices,
    /// Why the user's `pricing.json` was ignored, if it was.
    pub pricing_error: Option<&'a PricingError>,
    /// The last scan of this process (background or foreground); `None`
    /// before one ran.
    pub last: Option<&'a ScanReport>,
}

/// RF4: the summary of `input`. Pure: the same input always gives the same
/// summary.
pub fn summarize(input: &SummaryInput<'_>) -> CostSummary {
    let today = input.today;
    let yesterday = today - Days::days(1);
    let first = input.span.first_day(today);
    let in_span =
        |row: &&DailyUsage| first.is_none_or(|first| row.day >= first) && row.day <= today;
    let on = |row: &&DailyUsage| input.enabled.contains(&row.source);
    let span_rows: Vec<DailyUsage> = input
        .usage
        .iter()
        .filter(on)
        .filter(in_span)
        .cloned()
        .collect();
    let sources = SourceId::ALL
        .into_iter()
        .map(|source| source_cost(source, input, &span_rows, yesterday))
        .collect();
    CostSummary {
        currency: CURRENCY,
        notice: estimate_notice(),
        span: SpanView {
            period: match input.span {
                Span::Period(period) => Some(period),
                Span::Days(_) => None,
            },
            days: match input.span {
                Span::Days(days) => Some(days),
                Span::Period(_) => None,
            },
            label: input.span.label(),
            first_day: first,
            last_day: today,
        },
        sources,
        total: estimate(&span_rows, input.prices),
        pricing_error: input.pricing_error.map(pricing_ignored),
    }
}

fn source_cost(
    source: SourceId,
    input: &SummaryInput<'_>,
    span_rows: &[DailyUsage],
    yesterday: NaiveDate,
) -> SourceCost {
    let label = source.label();
    let base = SourceCost {
        source,
        label,
        providers: source.providers(),
        state: CostState::Off,
        note: Some(off_text(label)),
        usage: None,
    };
    if !input.enabled.contains(&source) {
        return base;
    }
    let rows: Vec<DailyUsage> = span_rows
        .iter()
        .filter(|row| row.source == source)
        .cloned()
        .collect();
    let of_day = |day: NaiveDate| -> Vec<DailyUsage> {
        input
            .usage
            .iter()
            .filter(|row| row.source == source && row.day == day)
            .cloned()
            .collect()
    };
    let scanned = input
        .last
        .and_then(|report| report.sources.iter().find(|s| s.source == source));
    let state = match scanned.map(|scan| (scan.state, scan.complete)) {
        Some((SourceState::Ready, true)) => CostState::Ready,
        Some((SourceState::Ready, false)) => CostState::Partial,
        Some((SourceState::NoLogs, _)) => CostState::NoLogs,
        Some((SourceState::NoAccess, _)) => CostState::NoAccess,
        // Turned on after the last scan.
        Some((SourceState::Off, _)) | None => CostState::NotRead,
    };
    let note = match state {
        CostState::Ready if rows.is_empty() => Some(Localized::with_params(
            "cost.view.empty",
            params([
                ("source", label.into()),
                ("period", ParamValue::l10n(input.span.label())),
            ]),
            format!(
                "{label}: nenhum uso nos logs lidos ({}).",
                input.span.label().fallback
            ),
        )),
        CostState::Ready | CostState::Off => None,
        CostState::Partial => Some(Localized::with_param(
            "cost.view.partial",
            "source",
            label,
            format!("{label}: a leitura dos logs ainda não terminou; os números podem crescer."),
        )),
        CostState::NoLogs => Some(no_logs_text(label)),
        CostState::NoAccess => Some(Localized::with_param(
            "cost.view.no_access",
            "source",
            label,
            format!(
                "{label}: sem acesso aos diretórios dos logs; nada foi contornado. Os números são \
                 os que o índice já tinha."
            ),
        )),
        CostState::NotRead => Some(Localized::with_param(
            "cost.view.not_read",
            "source",
            label,
            format!(
                "{label}: os logs ainda não foram lidos nesta execução; os números são os que o \
                 índice já tinha."
            ),
        )),
    };
    let days = breakdown(
        &rows,
        input.span.first_day(input.today),
        input.today,
        input.prices,
    );
    SourceCost {
        state,
        note,
        usage: Some(SourceUsage {
            today: estimate(&of_day(input.today), input.prices),
            yesterday: estimate(&of_day(yesterday), input.prices),
            period: estimate(&rows, input.prices),
            days,
        }),
        ..base
    }
}

/// Every day from `first` (or the oldest with usage) to `today`; empty
/// without usage.
fn breakdown(
    rows: &[DailyUsage],
    first: Option<NaiveDate>,
    today: NaiveDate,
    prices: &Prices,
) -> Vec<DayCost> {
    let Some(oldest) = rows.iter().map(|row| row.day).min() else {
        return Vec::new();
    };
    let mut days = Vec::new();
    let mut day = first.unwrap_or(oldest);
    while day <= today {
        let of_day: Vec<DailyUsage> = rows.iter().filter(|row| row.day == day).cloned().collect();
        let estimate = estimate(&of_day, prices);
        days.push(DayCost {
            day,
            tokens: estimate.tokens,
            requests: estimate.requests,
            cost: estimate.total,
            unknown_tokens: estimate.unknown_tokens,
        });
        day += Days::days(1);
    }
    days
}

impl Scanner {
    /// RF4: the summary at `now` of `span` (the saved period when `None`)
    /// over the index, the prices in effect (the embedded table under
    /// `pricing`) and `last`, this process's last scan. Reads no log.
    pub fn summary(
        &self,
        pricing: &PricingFile,
        last: Option<&ScanReport>,
        span: Option<Span>,
        now: DateTime<Utc>,
    ) -> Result<CostSummary, CostError> {
        let settings = self.settings().load()?;
        let today = self.zone().day(now);
        let span = span.unwrap_or(Span::Period(settings.period));
        let enabled = settings.enabled_sources(self.env());
        let first = span
            .first_day(today)
            .map(|first| first.min(today - Days::days(1)));
        let usage = if enabled.is_empty() {
            Vec::new()
        } else {
            self.index().usage(None, first, today)?
        };
        let (prices, pricing_error) = Prices::with_user_file(pricing);
        Ok(summarize(&SummaryInput {
            today,
            span,
            enabled: &enabled,
            usage: &usage,
            prices: &prices,
            pricing_error: pricing_error.as_ref(),
            last,
        }))
    }
}

/// `950`, `12.3K`, `1.23M`, `4.5B`: a token count in a few characters.
pub fn compact_tokens(count: u64) -> String {
    let scaled = |divisor: f64, suffix: &str| {
        let value = count as f64 / divisor;
        let text = if value < 10.0 {
            format!("{value:.2}")
        } else if value < 100.0 {
            format!("{value:.1}")
        } else {
            format!("{value:.0}")
        };
        let text = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        };
        format!("{text}{suffix}")
    };
    match count {
        0..=999 => count.to_string(),
        1_000..=999_999 => scaled(1e3, "K"),
        1_000_000..=999_999_999 => scaled(1e6, "M"),
        _ => scaled(1e9, "B"),
    }
}

/// RF4.2: one labelled amount for a terminal: `Hoje: 1.2M tokens · $3.40`,
/// with the part without a known price named ("cost unknown"), never
/// counted as zero.
pub fn usage_line(
    label: impl Into<ParamValue>,
    tokens: &Tokens,
    cost: Option<Usd>,
    unknown: &Tokens,
) -> Localized {
    let label = label.into();
    let shown = |value: &ParamValue| match value {
        ParamValue::Text(text) => text.clone(),
        ParamValue::L10n(text) => text.fallback.to_string(),
    };
    let fallback_label = shown(&label);
    let count = compact_tokens(tokens.total());
    let hidden = compact_tokens(unknown.total());
    match cost {
        _ if tokens.total() == 0 => Localized::with_params(
            "cost.line.none",
            params([("label", label)]),
            format!("{fallback_label}: sem uso"),
        ),
        None => Localized::with_params(
            "cost.line.unknown",
            params([("label", label), ("tokens", count.as_str().into())]),
            format!("{fallback_label}: {count} tokens · custo desconhecido"),
        ),
        Some(cost) if unknown.total() > 0 => Localized::with_params(
            "cost.line.partial",
            params([
                ("label", label),
                ("tokens", count.as_str().into()),
                ("cost", cost.to_string().into()),
                ("unknown", hidden.as_str().into()),
            ]),
            format!(
                "{fallback_label}: {count} tokens · {cost} + custo desconhecido de {hidden} tokens"
            ),
        ),
        Some(cost) => Localized::with_params(
            "cost.line.priced",
            params([
                ("label", label),
                ("tokens", count.as_str().into()),
                ("cost", cost.to_string().into()),
            ]),
            format!("{fallback_label}: {count} tokens · {cost}"),
        ),
    }
}

impl Estimate {
    /// `label: tokens · cost`, see [`usage_line`].
    pub fn line(&self, label: impl Into<ParamValue>) -> Localized {
        usage_line(label, &self.tokens, self.total, &self.unknown_tokens)
    }
}

impl ModelEstimate {
    /// `model: tokens · cost`, see [`usage_line`].
    pub fn line(&self) -> Localized {
        let unknown = if self.cost.is_some() {
            Tokens::default()
        } else {
            self.tokens
        };
        usage_line(self.model.as_str(), &self.tokens, self.cost, &unknown)
    }
}

/// "Yesterday", beside [`Period::Today`]'s label.
pub fn yesterday_label() -> Localized {
    Localized::fixed("cost.view.yesterday", "Ontem")
}

/// The heading of a source's section: "Local usage of Claude Code on this
/// computer".
pub fn heading(source: SourceId) -> Localized {
    let label = source.label();
    Localized::with_param(
        "cost.view.heading",
        "source",
        label,
        format!("Uso local do {label} neste computador"),
    )
}

/// The heading of the breakdown by model over `span`.
pub fn by_model_heading(span: &SpanView) -> Localized {
    Localized::with_params(
        "cost.view.by_model",
        params([("period", ParamValue::l10n(span.label.clone()))]),
        format!("Por modelo ({})", span.label.fallback),
    )
}

/// The heading of the breakdown by day.
pub fn by_day_heading() -> Localized {
    Localized::fixed("cost.view.by_day", "Por dia")
}

/// How to turn a source on from a terminal.
pub fn enable_hint() -> Localized {
    Localized::fixed(
        "cost.view.enable_hint",
        "Para ligar uma fonte: cuotai --cost-sources mostra o que cada uma lê e cuotai \
         --cost-enable FONTE pede a confirmação.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::{PriceTable, Rate, Rates, RootScan, RootState, SourceScan};
    use chrono::TimeZone;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn row(source: SourceId, d: u32, model: &str, input: u64) -> DailyUsage {
        DailyUsage {
            source,
            day: day(d),
            model: model.into(),
            tokens: Tokens {
                input,
                ..Tokens::default()
            },
            requests: 1,
            sessions: 1,
        }
    }

    /// `known` at $1 per million input tokens; nothing else priced.
    fn prices() -> Prices {
        let one = Rate::usd_per_million(1.0).unwrap();
        let zero = Rate::usd_per_million(0.0).unwrap();
        Prices::with_tables(
            PriceTable {
                currency: CURRENCY,
                effective: day(1),
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

    fn report(source: SourceId, state: SourceState, complete: bool) -> ScanReport {
        ScanReport {
            scanned_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap(),
            sources: vec![SourceScan {
                roots: vec![RootScan {
                    path: "/Users/ana/.claude/projects".into(),
                    state: RootState::NoAccess,
                }],
                complete,
                ..scan_of(source, state)
            }],
            complete,
        }
    }

    fn scan_of(source: SourceId, state: SourceState) -> SourceScan {
        SourceScan {
            source,
            state,
            roots: Vec::new(),
            files: 0,
            read: 0,
            restarted: 0,
            unreadable: 0,
            foreign: 0,
            lines: 0,
            requests: 0,
            duplicates: 0,
            invalid: 0,
            expired: 0,
            complete: true,
        }
    }

    fn summary(
        span: Span,
        enabled: &[SourceId],
        usage: &[DailyUsage],
        last: Option<&ScanReport>,
    ) -> CostSummary {
        summarize(&SummaryInput {
            today: day(20),
            span,
            enabled,
            usage,
            prices: &prices(),
            pricing_error: None,
            last,
        })
    }

    #[test]
    fn today_yesterday_and_the_span_split_by_local_day_and_source() {
        let claude = SourceId::ClaudeCode;
        let usage = [
            row(claude, 12, "known", 9_000_000), // before the 7 days
            row(claude, 14, "known", 1_000_000), // first day of the 7 days
            row(claude, 19, "known", 2_000_000),
            row(claude, 20, "known", 3_000_000),
            row(claude, 20, "mystery", 500),
            row(SourceId::CodexCli, 20, "known", 7_000_000),
        ];
        let last = report(claude, SourceState::Ready, true);
        let s = summary(
            Span::Period(Period::Last7Days),
            &[claude],
            &usage,
            Some(&last),
        );
        assert_eq!(s.span.first_day, Some(day(14)));
        let c = s.source(claude).unwrap();
        assert_eq!((c.state, &c.note), (CostState::Ready, &None));
        let u = c.usage.as_ref().unwrap();
        assert_eq!(u.today.total.map(Usd::micros), Some(3_000_000));
        assert_eq!(u.today.unknown_tokens.input, 500, "never priced as zero");
        assert_eq!(u.yesterday.total.map(Usd::micros), Some(2_000_000));
        assert_eq!(u.period.total.map(Usd::micros), Some(6_000_000));
        assert_eq!(u.period.unknown_models().collect::<Vec<_>>(), ["mystery"]);
        // Every day of the span, the empty ones at zero tokens.
        assert_eq!(u.days.len(), 7);
        assert_eq!(u.days[0].day, day(14));
        assert_eq!(u.days[1].tokens.total(), 0);
        assert_eq!(u.days[1].cost.map(Usd::micros), Some(0));
        assert_eq!(u.days[6].unknown_tokens.input, 500);
        // A source that is off shows nothing, even with rows left behind.
        let codex = s.source(SourceId::CodexCli).unwrap();
        assert_eq!((codex.state, codex.usage.is_none()), (CostState::Off, true));
        assert_eq!(s.total.total.map(Usd::micros), Some(6_000_000));
        assert!(s.any_on());
        assert_eq!(s.for_provider(Provider::Claude).unwrap().source, claude);
        assert!(s.for_provider(Provider::AnthropicApi).is_none());
    }

    #[test]
    fn a_number_of_days_and_everything_bound_the_breakdown() {
        let claude = SourceId::ClaudeCode;
        let usage = [row(claude, 5, "known", 1), row(claude, 19, "known", 1)];
        let s = summary(Span::days(2).unwrap(), &[claude], &usage, None);
        assert_eq!((s.span.days, s.span.period), (Some(2), None));
        let u = s.sources[0].usage.as_ref().unwrap();
        assert_eq!(
            u.days.iter().map(|d| d.day).collect::<Vec<_>>(),
            [day(19), day(20)]
        );
        let all = summary(Span::Period(Period::All), &[claude], &usage, None);
        let u = all.sources[0].usage.as_ref().unwrap();
        assert_eq!((u.days.len(), u.days[0].day), (16, day(5)));
        assert_eq!(Span::days(0), Err(CostError::DaysBounds));
        assert_eq!(Span::days(731), Err(CostError::DaysBounds));
        assert!(Span::days(730).is_ok());
    }

    #[test]
    fn every_state_without_complete_numbers_is_explained() {
        let claude = SourceId::ClaudeCode;
        let en = |s: &CostSummary| s.sources[0].note.as_ref().map(|n| n.resolve("en-US"));
        let none = summary(Span::Period(Period::Today), &[], &[], None);
        assert!(!none.any_on());
        assert_eq!(en(&none).unwrap(), "Claude Code: off; nothing is read.");

        let not_read = summary(Span::Period(Period::Today), &[claude], &[], None);
        assert_eq!(not_read.sources[0].state, CostState::NotRead);

        let ready = report(claude, SourceState::Ready, true);
        let empty = summary(Span::Period(Period::Today), &[claude], &[], Some(&ready));
        assert_eq!(
            en(&empty).unwrap(),
            "Claude Code: no usage in the logs read (Today)."
        );
        assert!(empty.sources[0].usage.as_ref().unwrap().days.is_empty());

        let partial = report(claude, SourceState::Ready, false);
        let s = summary(Span::Period(Period::Today), &[claude], &[], Some(&partial));
        assert_eq!(s.sources[0].state, CostState::Partial);

        let denied = report(claude, SourceState::NoAccess, true);
        let s = summary(Span::Period(Period::Today), &[claude], &[], Some(&denied));
        assert_eq!(s.sources[0].state, CostState::NoAccess);
        // The summary never names a directory, even the refused one.
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("/Users/ana"), "{json}");
        for locale in ["pt-BR", "en-US", "es", "it"] {
            assert!(!s.sources[0]
                .note
                .as_ref()
                .unwrap()
                .resolve(locale)
                .contains("/Users"));
        }
    }

    #[test]
    fn lines_name_the_unknown_part_and_never_count_it_as_zero() {
        let tokens = |input| Tokens {
            input,
            ..Tokens::default()
        };
        let priced = usage_line(
            "Today",
            &tokens(1_234_567),
            Some(Usd::from_micros(3_400_000)),
            &tokens(0),
        );
        assert_eq!(priced.resolve("en-US"), "Today: 1.23M tokens · $3.40");
        let partial = usage_line(
            "Today",
            &tokens(2_000),
            Some(Usd::from_micros(4_000)),
            &tokens(1_000),
        );
        assert_eq!(
            partial.resolve("en-US"),
            "Today: 2K tokens · <$0.01 + unknown cost of 1K tokens"
        );
        let unknown = usage_line("Today", &tokens(950), None, &tokens(950));
        assert_eq!(
            unknown.resolve("es"),
            "Today: 950 tokens · costo desconocido"
        );
        let none = usage_line(
            yesterday_label(),
            &tokens(0),
            Some(Usd::default()),
            &tokens(0),
        );
        assert_eq!(none.resolve("pt-BR"), "Ontem: sem uso");
        assert_eq!(none.resolve("en-US"), "Yesterday: no usage");
        assert_eq!(compact_tokens(12_345), "12.3K");
        assert_eq!(compact_tokens(4_500_000_000), "4.5B");
        assert_eq!(Usd::from_micros(1_005_000).to_string(), "$1.01");
        let model = ModelEstimate {
            model: "mystery".into(),
            tokens: tokens(10),
            requests: 1,
            cost: None,
            price: None,
        };
        assert_eq!(
            model.line().resolve("en-US"),
            "mystery: 10 tokens · unknown cost"
        );
    }
}
