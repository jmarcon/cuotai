//! Cards-style quota dashboard.
//!
//! Behavioral classification of a report (`Available`/`Exhausted`/`Unmeasured`)
//! and per-metric rendering state live in `cuotai_core::metrics` so other
//! consumers (CLI, web) can reuse the same semantics. This module only owns
//! colors, labels, layout, and the unmeasured toggle. No credentials are read
//! or stored here.

use crate::l10n::{tr, trf};
use crate::motion::{eighth_bar, Anim};
use crate::palette;
use crate::ui::{
    age_text, data_as_of_suffix, local_time, metric_text, resolve_l10n, safe, tui_locale,
};
use chrono::{DateTime, Local, Utc};
use cuotai_core::access;
use cuotai_core::cost::{self, SourceCost};
use cuotai_core::history::{self, HistoryRange, MetricHistory, Trend};
use cuotai_core::incidents::{IncidentState, ProviderStatus};
use cuotai_core::metrics::{
    binding, classify_metric, classify_report, scope_blocks, AccountClass, MetricState,
};
use cuotai_core::overage;
use cuotai_core::pace::{self, Pace, PaceStage, PaceTone, Point};
#[cfg(test)]
use cuotai_core::Snapshot;
use cuotai_core::{Account, Metric, Provider, Report, Status};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MIN_CARD_HEIGHT: u16 = 8;
const NARROW_WIDTH: u16 = 45;
const NARROW_HEIGHT: u16 = 18;
const WIDE_COLUMNS_WIDTH: u16 = 110;
const HEADER_HEIGHT: u16 = 4;
/// The footer shows at least two rows (navigation, actions) and flows the
/// keys that do not fit the width onto up to two more, so a narrow
/// terminal still names every key instead of clipping the rows mid-word.
const FOOTER_MIN_ROWS: usize = 2;
const FOOTER_MAX_ROWS: usize = 4;
const METRICS_VISIBLE_IN_CARD: usize = 2;
/// PRD 0006 RF6.1: sparkline glyphs, lowest to highest level.
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Below this many cells the history line is text only.
const SPARK_MIN_CELLS: usize = 8;
const SPARK_MAX_CELLS: usize = 48;
fn resolve_card_l10n(value: &cuotai_core::text::Localized) -> String {
    resolve_l10n(value)
}

// Dracula palette: background, foreground, selection, muted, and status accents.
const BG: Color = palette::BG;
const FG: Color = palette::FG;
const MUTED: Color = palette::MUTED;
const PURPLE: Color = palette::INFO;
const CYAN: Color = palette::ACCENT;
const GREEN: Color = palette::GOOD;
const YELLOW: Color = palette::WARN;
const RED: Color = palette::BAD;
const PINK: Color = palette::PINK;

/// Localized label for an account class. Behavioral classification lives in
/// `cuotai_core::metrics`; this function only owns the labels the
/// dashboard prints.
pub(crate) fn class_label(class: AccountClass) -> &'static str {
    match class {
        AccountClass::Available => tr("dashboard.class.available", "Available"),
        AccountClass::Exhausted => tr("dashboard.class.exhausted", "Exhausted"),
        AccountClass::Unmeasured => tr("dashboard.class.unmeasured", "No data"),
    }
}

/// Card text for a synced catalog size; an empty catalog is a real result,
/// shown as such instead of hidden.
fn model_count_label(count: usize) -> String {
    match count {
        0 => tr("dashboard.models.none", "no models").to_string(),
        1 => tr("dashboard.models.one", "1 model").to_string(),
        n => trf("dashboard.models.many", "{n} models", &[("n", &n)]),
    }
}

fn status_color(status: Status) -> Color {
    match status {
        Status::Ready => GREEN,
        Status::NeedsConfiguration | Status::Unsupported => YELLOW,
        Status::AuthenticationError | Status::Error => RED,
        Status::RateLimited => PINK,
    }
}

/// Classify an account using the latest report, if any. Behavioral logic
/// (per-metric finite checks, unknown-sentinel handling, negative-debt
/// exhaustion) lives in `cuotai_core::metrics::classify_report`. A `None`
/// report or a non-`Ready` status is never classified as available there.
pub(crate) fn classification(report: Option<&Report>) -> AccountClass {
    classify_report(report)
}

/// Filter by case-insensitive substring on name or provider label, hide
/// unmeasured entries when requested, then return the surviving indices in
/// configuration order. Class is only used to decide whether the unmeasured
/// toggle drops the row; visible order is never rearranged.
pub(crate) fn visible_indices(
    accounts: &[Account],
    reports: &[Option<Report>],
    filter: &str,
    show_unmeasured: bool,
) -> Vec<usize> {
    let needle = cuotai_core::text::fold(filter);
    // PRD 0036 RF9.1: an aggregator stays visible when one of its
    // connections matches (label or upstream).
    let connection_matches = |index: usize| {
        reports
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|report| {
                report.sub_reports.iter().any(|sub| {
                    cuotai_core::text::fold(&sub.label).contains(&needle)
                        || cuotai_core::text::fold(&sub.upstream).contains(&needle)
                })
            })
    };
    accounts
        .iter()
        .enumerate()
        .filter(|(index, account)| {
            needle.is_empty()
                || cuotai_core::text::fold(&account.display_name()).contains(&needle)
                || cuotai_core::text::fold(account.provider.label()).contains(&needle)
                || (account.provider.is_aggregator() && connection_matches(*index))
        })
        .filter(|(index, _)| {
            show_unmeasured
                || !classification(reports.get(*index).and_then(Option::as_ref)).is_unmeasured()
        })
        .map(|(index, _)| index)
        .collect()
}

/// Map a metric state to the color its gauge/text should use. UI mapping only.
fn metric_state_color(state: MetricState) -> Option<Color> {
    match state {
        MetricState::Unmeasured => None,
        MetricState::Exhausted { .. } => Some(RED),
        MetricState::Available { ratio, uncapped } => {
            if uncapped {
                Some(MUTED)
            } else {
                // Single source of the thresholds: core `tray::Level` (DS-ID-5).
                Some(match cuotai_core::tray::Level::of(ratio) {
                    cuotai_core::tray::Level::Good => GREEN,
                    cuotai_core::tray::Level::Warn => YELLOW,
                    cuotai_core::tray::Level::Bad => RED,
                })
            }
        }
    }
}

/// Build a short, locale-neutral countdown ("3d 4h", "5h 02m", "12m 03s",
/// "47s"). Once `resets_at` is at or before now, return the renewal hint
/// (the word after "renova") without inventing a cycle.
fn reset_text(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> &'static str {
    if resets_at <= now {
        return tr("dashboard.reset.now", "now");
    }
    tr("dashboard.reset.soon", "soon")
}

fn reset_seconds(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> Option<i64> {
    let diff = (resets_at - now).num_seconds();
    if diff <= 0 {
        None
    } else {
        Some(diff)
    }
}

fn render_reset(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> Line<'static> {
    let text = match reset_seconds(resets_at, now) {
        Some(total) => format_reset_remaining(total),
        None => reset_text(resets_at, now).to_string(),
    };
    Line::from(vec![
        Span::styled(renews(), Style::default().fg(MUTED)),
        Span::styled(text, Style::default().fg(CYAN)),
    ])
}

fn format_reset_remaining(total: i64) -> String {
    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;
    let seconds = total % 60;
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

#[derive(Debug, Clone)]
struct MetricRow {
    label: String,
    text: String,
    state: MetricState,
    period: Option<String>,
    resets_at: Option<DateTime<Utc>>,
    /// PRD 0007 RF4: the core's pace; `None` for ineligible metrics, freshly
    /// opened windows and the preserved (stale) reading.
    pace: Option<Pace>,
}

/// The reading a pace is read from (PRD 0024 FIX-11): its `checked_at`,
/// the account's effective refresh `interval` and the drawing clock `now`.
#[derive(Debug, Clone, Copy)]
struct PaceClock {
    checked_at: DateTime<Utc>,
    interval: std::time::Duration,
    now: DateTime<Utc>,
}

/// The clock for the pace of account `account_index`'s current `report`;
/// `None` for the preserved (stale) reading, which shows no pace.
fn pace_clock(
    view: &View<'_>,
    account_index: usize,
    report: &Report,
    preserved: bool,
    now: DateTime<Utc>,
) -> Option<PaceClock> {
    let seconds = view
        .accounts
        .get(account_index)
        .map_or(view.refresh_seconds, |account| {
            account.effective_refresh_seconds(view.refresh_seconds)
        });
    (!preserved).then_some(PaceClock {
        checked_at: report.checked_at,
        interval: std::time::Duration::from_secs(seconds),
        now,
    })
}

/// `clock` is the reading the pace is read from; `None` shows no pace. The
/// core evaluates it at the reading's `checked_at` and only while the
/// reading is fresh. `recent` are the recorded points of the metric (PRD
/// 0006), which project the pace from the recent slope when there are
/// enough of them (PRD 0007 RF1.5).
fn metric_row(metric: &Metric, clock: Option<PaceClock>, recent: &[Point]) -> MetricRow {
    MetricRow {
        label: crate::ui::metric_label_text(&metric.label),
        text: metric_text(metric),
        state: classify_metric(metric),
        period: metric.period.as_deref().map(crate::ui::unit_text),
        resets_at: metric.resets_at,
        pace: clock.and_then(|clock| {
            pace::assess_reading(
                metric,
                clock.checked_at,
                clock.interval,
                clock.now,
                Some(recent),
            )
        }),
    }
}

/// The recorded points of each metric of account `index`'s current report.
fn recent_of<'a>(view: &View<'a>, index: usize) -> &'a [Vec<Point>] {
    view.recent.get(index).map_or(&[], Vec::as_slice)
}

fn points(recent: &[Vec<Point>], metric: usize) -> &[Point] {
    recent.get(metric).map_or(&[], Vec::as_slice)
}

/// PRD 0024 RF6.2: the narrowest card text area that also shows the pace chip.
const CHIP_MIN_WIDTH: u16 = 60;

/// PRD 0024 RF1.3: the unit every pace text of this process uses; set once
/// from `--pace-unit` or `CUOTAI_PACE_UNIT` before anything draws.
static PACE_UNIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_pace_unit(unit: pace::PaceUnit) {
    PACE_UNIT.store(
        unit == pace::PaceUnit::Percent,
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub(crate) fn pace_unit() -> pace::PaceUnit {
    if PACE_UNIT.load(std::sync::atomic::Ordering::Relaxed) {
        pace::PaceUnit::Percent
    } else {
        pace::PaceUnit::Points
    }
}

/// PRD 0024 RF7: the best account of each group of comparable accounts at
/// the drawing clock, over the current readings (advisory only).
fn advice_groups(view: &View<'_>) -> Vec<cuotai_core::advice::GroupBest> {
    let views: Vec<cuotai_core::advice::AccountView<'_>> = view
        .accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let report = view.reports.get(index).and_then(Option::as_ref);
            let interval = std::time::Duration::from_secs(
                account.effective_refresh_seconds(view.refresh_seconds),
            );
            cuotai_core::advice::AccountView {
                index,
                provider: account.provider,
                group: account.group.as_deref(),
                enabled: true,
                report,
                freshness: report.map(|report| {
                    cuotai_core::freshness::assess(report.checked_at, interval, view.now)
                }),
            }
        })
        .collect();
    cuotai_core::advice::best(&views, view.now)
}

/// PRD 0024 RF7.5: the "Best now" chip text when `account_index` is the
/// best of its group.
fn best_chip_of(view: &View<'_>, account_index: usize) -> Option<String> {
    advice_groups(view)
        .iter()
        .any(|group| group.best == account_index)
        .then(|| cuotai_core::advice::best_chip().resolve(tui_locale()))
}

/// PRD 0024 RF7.5: the details sentence: "Best now" for the best account,
/// and for an exhausted one or one projected to run out, what the best
/// account of its group has left ("Work runs out 18:53 · Personal has 64%
/// left").
fn advice_sentence(
    view: &View<'_>,
    account_index: usize,
    rows: &[MetricRow],
    exhausted: bool,
) -> Option<String> {
    let groups = advice_groups(view);
    let group = groups
        .iter()
        .find(|group| group.candidate(account_index).is_some())?;
    if group.best == account_index {
        return Some(cuotai_core::advice::best_chip().resolve(tui_locale()));
    }
    let runs_out = rows
        .iter()
        .filter_map(|row| row.pace.as_ref())
        .filter(|pace| !pace.lasts_until_reset)
        .filter_map(|pace| pace.projected_exhaustion_at)
        .min()
        .map(local_time);
    if !exhausted && runs_out.is_none() {
        return None;
    }
    let best = view.accounts.get(group.best)?;
    let left = group.candidate(group.best)?.remaining;
    let from = view.accounts.get(account_index)?;
    Some(
        cuotai_core::advice::switch_text(
            &from.name,
            &best.name,
            left,
            runs_out.as_deref().filter(|_| !exhausted),
        )
        .resolve(tui_locale()),
    )
}

/// PRD 0007 RF4: the core's pace texts joined as "stage · outlook", with
/// `at` formatting the projected time and the margin as `11h 10m`.
pub(crate) fn pace_text(
    pace: &Pace,
    locale: &str,
    at: impl FnOnce(DateTime<Utc>) -> String,
) -> String {
    // Points keep the stage sentence; percent names the comparison with the
    // core's chip ("20% faster than pace"), PRD 0024 RF1.3.
    let stage = match pace_unit() {
        pace::PaceUnit::Points => pace.stage_label().resolve(locale),
        unit => pace.chip(unit).text.resolve(locale),
    };
    let margin = |span: chrono::TimeDelta| format_reset_remaining(span.num_seconds().max(0));
    match pace.outlook_label(at, margin) {
        Some(outlook) => format!("{stage} · {}", outlook.resolve(locale)),
        None => stage,
    }
}

/// Discreet stage icon: the shape carries the stage, the color only repeats it.
fn pace_icon(pace: &Pace) -> Span<'static> {
    let icon = match pace.stage {
        PaceStage::Reserve => "↓ ",
        PaceStage::OnPace => "→ ",
        PaceStage::Ahead => "↑ ",
        PaceStage::Exhausted => "✕ ",
    };
    let color = match pace.tone {
        PaceTone::Good => GREEN,
        PaceTone::Neutral => FG,
        PaceTone::Warn => YELLOW,
        PaceTone::Bad => RED,
    };
    Span::styled(
        icon,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

/// PRD 0006 RF6.1: the selected account's recorded metrics over the chosen
/// span, read by `ui.rs` from the core when the account, the span or its
/// reading changes (never per frame).
pub(crate) struct DetailHistory {
    /// Account index the metrics belong to.
    pub(crate) account: usize,
    pub(crate) range: HistoryRange,
    /// The span read: the time axis of the sparklines.
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: DateTime<Utc>,
    /// The core's metrics, or the storage failure to show.
    pub(crate) metrics: Result<Vec<MetricHistory>, String>,
}

pub(crate) fn range_label(range: HistoryRange) -> &'static str {
    match range {
        HistoryRange::Day => "24 h",
        HistoryRange::Week => "7 d",
        HistoryRange::Month => "30 d",
    }
}

/// A value with at most two decimals and no trailing zeros (`40`, `12.5`).
pub(crate) fn compact(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// "renews " before a reset countdown.
fn renews() -> &'static str {
    tr("dashboard.reset.renews", "renews ")
}

/// RF6.1 range summary: `min 5 · max 80 · last 40 %`.
pub(crate) fn trend_text(trend: &Trend, unit: &str) -> String {
    trf(
        "dashboard.history.trend",
        "min {min} · max {max} · last {last} {unit}",
        &[
            ("min", &compact(trend.min)),
            ("max", &compact(trend.max)),
            ("last", &compact(trend.last.used)),
            ("unit", &safe(&crate::ui::unit_text(unit))),
        ],
    )
}

/// RF6.1: `cells` glyphs over `start..=end` from the core's levels; a
/// bucket without a sample is a blank, never the lowest bar.
pub(crate) fn sparkline(
    metric: &MetricHistory,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    cells: usize,
) -> String {
    metric
        .levels(start, end, cells, SPARK.len() as u8)
        .into_iter()
        .map(|level| level.map_or(' ', |level| SPARK[usize::from(level)]))
        .collect()
}

/// RF6.1: the history line of one metric in `width` cells: the span, the
/// sparkline and the summary, or only the summary (minimum, maximum and
/// last value) when the sparkline would not fit. The span leads, so a
/// leading gap is never trimmed away.
fn history_line(
    metric: Option<&MetricHistory>,
    detail: &DetailHistory,
    width: u16,
) -> Line<'static> {
    let range = range_label(detail.range);
    let Some((metric, trend)) = metric.and_then(|m| m.trend().map(|trend| (m, trend))) else {
        return Line::styled(
            trf(
                "dashboard.history.empty",
                "  history {range}: no recorded measurements",
                &[("range", &range)],
            ),
            Style::default().fg(MUTED),
        );
    };
    let summary = trend_text(&trend, &metric.unit);
    let cells = usize::from(width)
        .saturating_sub(6 + cells(range) + cells(&summary))
        .min(SPARK_MAX_CELLS);
    if cells < SPARK_MIN_CELLS {
        return Line::styled(
            trf(
                "dashboard.history.summary",
                "  history {range}: {summary}",
                &[("range", &range), ("summary", &summary)],
            ),
            Style::default().fg(MUTED),
        );
    }
    Line::from(vec![
        Span::styled(format!("  {range}  "), Style::default().fg(MUTED)),
        Span::styled(
            sparkline(metric, detail.start, detail.end, cells),
            Style::default().fg(CYAN),
        ),
        Span::styled(format!("  {summary}"), Style::default().fg(MUTED)),
    ])
}

/// RF6.1: the span picker of the details, the chosen one highlighted.
fn history_heading(detail: &DetailHistory) -> Line<'static> {
    let mut spans = vec![Span::styled(
        tr("dashboard.history.heading", "  HISTORY  "),
        Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
    )];
    for (index, range) in HistoryRange::ALL.into_iter().enumerate() {
        let label = format!("{} {}", index + 1, range_label(range));
        spans.push(if range == detail.range {
            Span::styled(
                format!("[{label}]"),
                Style::default().fg(PINK).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {label} "), Style::default().fg(MUTED))
        });
        spans.push(Span::raw(" "));
    }
    if let Err(error) = &detail.metrics {
        spans.push(Span::styled(
            format!(" {}", safe(error)),
            Style::default().fg(YELLOW),
        ));
    }
    Line::from(spans)
}

/// Input to `draw`. Field names match the spec; `selected` is the position
/// inside `visible`, not an account index. `next_refresh` carries the time
/// remaining until the next scheduled refresh: `None` when the timer is
/// not armed (after a refresh just fired, before scheduling the next one,
/// or while a manual refresh is in flight).
pub(crate) struct View<'a> {
    pub(crate) accounts: &'a [Account],
    /// Per-account report lookup. Always has the same length as `accounts`
    /// — an `None` entry means "no fresh report yet" (cycle is fresh or
    /// this slot was just edited). The dashboard renders an
    /// "Aguardando" placeholder for these slots instead of leaking a
    /// stale row from the previous configuration.
    pub(crate) reports: &'a [Option<Report>],
    /// Models in each account's synced catalog (local cache, no network);
    /// `None` when never synced or the provider cannot list. Same length
    /// as `accounts` or empty.
    pub(crate) model_counts: &'a [Option<usize>],
    pub(crate) visible: &'a [usize],
    pub(crate) selected: Option<usize>,
    pub(crate) details: bool,
    pub(crate) detail_scroll: u16,
    pub(crate) show_unmeasured: bool,
    pub(crate) refreshing: bool,
    pub(crate) location: &'a str,
    pub(crate) filter: &'a str,
    pub(crate) filtering: bool,
    pub(crate) next_refresh: Option<Duration>,
    pub(crate) error: Option<&'a str>,
    /// RF4 effects; `None` (animations off) renders exactly as before.
    pub(crate) anim: Option<&'a Anim>,
    /// PRD 0010 RF5: the core's last valid reading per account
    /// (`RefreshCycle::last_good_report`), `Some` only while the current
    /// report failed. Same length as `accounts` or empty. Display only.
    pub(crate) last_good: &'a [Option<&'a Report>],
    /// PRD 0007 RF1.5: recorded points per account and metric of its
    /// current report (PRD 0006); same length as `accounts` or empty.
    pub(crate) recent: &'a [Vec<Vec<Point>>],
    /// The configuration's global `refresh_seconds`, from which each
    /// account's effective interval (and so its freshness) follows; the
    /// pace shows only for a fresh reading (PRD 0024 FIX-11).
    pub(crate) refresh_seconds: u64,
    /// PRD 0006 RF6.1: the selected account's history in the details;
    /// `None` without a history (nothing is shown).
    pub(crate) history: Option<&'a DetailHistory>,
    /// PRD 0003 RF4: the last reset notice of the session (`--notify`),
    /// shown on its own header line while `Some`.
    pub(crate) banner: Option<&'a str>,
    /// PRD 0011 RF5: the configured providers' public status (one entry
    /// per provider with a verified source), empty while status queries
    /// are off.
    pub(crate) incidents: &'a [ProviderStatus],
    /// PRD 0015 RF4.2: this computer's local cost, shown in the details of
    /// every account of a source's tool; `None` shows nothing.
    pub(crate) local_cost: Option<&'a crate::cost::CostPanel>,
    /// Clock for ages and countdowns, injected so rendering is testable.
    pub(crate) now: DateTime<Utc>,
    /// PRD 0036 RF9.1: per account (same length as `accounts`, or empty),
    /// the connection cards of an aggregator account, grouped by upstream.
    pub(crate) sub_cards: &'a [Vec<SubCard>],
    /// PRD 0036 RF9.1: `g` folds every aggregator's connections away.
    pub(crate) aggregators_collapsed: bool,
    /// PRD 0036 RF4.4: the connection card in focus (account index, card
    /// index), the target of `R`.
    pub(crate) connection_focus: Option<(usize, usize)>,
    /// The account whose usage access the cards name instead of their own
    /// (a connection card reads through its aggregator).
    pub(crate) access_account: Option<&'a Account>,
}

/// PRD 0036 RF9.1: one connection of an aggregator, drawn as a card of its
/// own after the aggregator's (never selectable on its own: actions act on
/// the aggregator account).
#[derive(Debug, Clone)]
pub(crate) struct SubCard {
    /// The connection's id in the aggregator (for a single refresh).
    pub(crate) id: String,
    /// "<aggregator> · <label>" with the mapped provider (the aggregator's
    /// for an upstream Cuotai does not know).
    pub(crate) account: Account,
    /// The connection's current reading, as a report.
    pub(crate) report: Option<Report>,
    /// Its last `Ready` reading while the current one failed.
    pub(crate) last_good: Option<Report>,
    /// The aggregator's upstream slug.
    pub(crate) upstream: String,
    /// Off in the aggregator, or also monitored as a direct account.
    pub(crate) muted: bool,
}

/// The connection cards of the aggregator `account` from its current
/// report (or, while it failed, from its last good one), grouped by
/// upstream in order of first appearance. `last_good_sub` gives a
/// connection's preserved reading (`RefreshCycle::last_good_sub`).
pub(crate) fn sub_cards_of<'r>(
    account: &Account,
    report: Option<&'r Report>,
    last_good: Option<&'r Report>,
    last_good_sub: impl Fn(&str) -> Option<&'r cuotai_core::SubReport>,
) -> Vec<SubCard> {
    use cuotai_core::aggregators::{sub_account, sub_as_report};
    if !account.provider.is_aggregator() {
        return Vec::new();
    }
    let current = report.filter(|report| report.status == Status::Ready);
    let Some(source) = current.or(last_good) else {
        return Vec::new();
    };
    let mut upstreams: Vec<&str> = Vec::new();
    for sub in &source.sub_reports {
        if !upstreams.contains(&sub.upstream.as_str()) {
            upstreams.push(&sub.upstream);
        }
    }
    let mut cards = Vec::with_capacity(source.sub_reports.len());
    for upstream in upstreams {
        for sub in source
            .sub_reports
            .iter()
            .filter(|sub| sub.upstream == upstream)
        {
            let keyed = sub_account(account, sub);
            // PRD 0036 RF9.1: a duplicate says so ("also direct").
            let marker = if sub.duplicate_of.is_some() {
                tr("dashboard.sub.also_direct", " · also direct")
            } else {
                ""
            };
            let display = Account {
                name: format!("{} · {}{marker}", account.name, sub.label),
                provider: sub.provider.unwrap_or(account.provider),
                ..keyed
            };
            let preserved = if current.is_some() {
                last_good_sub(&sub.id)
            } else {
                (sub.status == Status::Ready).then_some(sub)
            };
            cards.push(SubCard {
                id: sub.id.clone(),
                report: current.map(|report| sub_as_report(report, sub)),
                last_good: preserved.map(|good| sub_as_report(source, good)),
                upstream: sub.upstream.clone(),
                muted: sub.enabled_upstream == Some(false) || sub.duplicate_of.is_some(),
                account: display,
            });
        }
    }
    cards
}

/// PRD 0036 RF9.1: the line an aggregator card shows instead of metrics:
/// "5/7 available · 9router 0.5.109 · Claude · 2 of 3 available; …".
fn aggregator_summary(report: &Report) -> Option<String> {
    use cuotai_core::aggregators::{instance_version, pools};
    if report.status != Status::Ready {
        return None;
    }
    let pools = pools(report);
    let available: usize = pools.iter().map(|pool| pool.available).sum();
    let total: usize = pools.iter().map(|pool| pool.total).sum();
    let mut parts = vec![trf(
        "dashboard.aggregator.available",
        "{available}/{total} available",
        &[("available", &available), ("total", &total)],
    )];
    if let Some(version) = instance_version(report) {
        let kind = cuotai_core::aggregators::shape(report.provider).map_or("9router", |s| s.kind());
        parts.push(format!("{kind} {version}"));
    }
    parts.extend(pools.iter().map(|pool| resolve_l10n(&pool.summary())));
    Some(parts.join(" · "))
}

/// One cell of the card grid: an account (its position in `visible`) or
/// one connection of the aggregator at that position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Account(usize),
    Sub(usize, usize),
}

/// PRD 0036 RF9.1: direct accounts first, in configuration order; then each
/// aggregator followed by its connections (unless folded). A filter that
/// does not match the aggregator itself keeps only the matching
/// connections.
fn card_slots(view: &View<'_>) -> Vec<Slot> {
    let aggregator = |index: usize| {
        view.accounts
            .get(index)
            .is_some_and(|account| account.provider.is_aggregator())
    };
    let mut slots: Vec<Slot> = view
        .visible
        .iter()
        .enumerate()
        .filter(|(_, index)| !aggregator(**index))
        .map(|(position, _)| Slot::Account(position))
        .collect();
    let needle = cuotai_core::text::fold(view.filter);
    for (position, &index) in view.visible.iter().enumerate() {
        if !aggregator(index) {
            continue;
        }
        slots.push(Slot::Account(position));
        if view.aggregators_collapsed {
            continue;
        }
        let account = &view.accounts[index];
        for sub in matching_cards(account, view.sub_cards.get(index), &needle) {
            slots.push(Slot::Sub(index, sub));
        }
    }
    slots
}

/// The indices of the connection cards of `account` the lowercase filter
/// `needle` keeps: all of them when the aggregator itself matches.
pub(crate) fn matching_cards(
    account: &Account,
    cards: Option<&Vec<SubCard>>,
    needle: &str,
) -> Vec<usize> {
    let parent_matches = needle.is_empty()
        || cuotai_core::text::fold(&account.name).contains(needle)
        || cuotai_core::text::fold(account.provider.label()).contains(needle);
    cards
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, card)| {
            parent_matches
                || cuotai_core::text::fold(&card.account.name).contains(needle)
                || cuotai_core::text::fold(&card.upstream).contains(needle)
        })
        .map(|(sub, _)| sub)
        .collect()
}

/// PRD 0036 RF2.3/RF2.4: the standing warnings of an aggregator instance
/// ("No password · Unencrypted"), `None` when it has none.
pub(crate) fn warning_chips(report: &Report) -> Option<String> {
    let warnings = cuotai_core::aggregators::ConnectionWarnings::of(report);
    if warnings.is_empty() {
        return None;
    }
    Some(
        warnings
            .chips()
            .iter()
            .map(resolve_l10n)
            .collect::<Vec<_>>()
            .join(" · "),
    )
}

/// A connection card: the account card of its reading, naming its
/// aggregator's usage access.
fn render_sub_card(
    frame: &mut Frame,
    area: Rect,
    view: &View<'_>,
    parent: &Account,
    card: &SubCard,
    focused: bool,
    now: DateTime<Utc>,
) {
    let accounts = std::slice::from_ref(&card.account);
    let reports = [card.report.clone()];
    let last_good = [card.last_good.as_ref()];
    let mini = View {
        accounts,
        reports: &reports,
        model_counts: &[],
        visible: &[],
        selected: None,
        last_good: &last_good,
        recent: &[],
        anim: None,
        history: None,
        sub_cards: &[],
        access_account: Some(parent),
        ..*view
    };
    render_card(frame, area, &mini, 0, focused, now);
    if card.muted {
        // "Off in 9router" or "also direct": dimmed, never counted.
        frame
            .buffer_mut()
            .set_style(area, Style::default().add_modifier(Modifier::DIM));
    }
}

/// PRD 0011 RF5: the status shown on the accounts of `provider`, if any.
fn incident_of<'a>(view: &View<'a>, provider: Provider) -> Option<&'a ProviderStatus> {
    view.incidents
        .iter()
        .find(|status| status.provider == provider)
}

/// Clickable action exposed by the footer hint line. Values mirror the
/// keyboard commands handled by `ui.rs`; a click on a footer cell is just a
/// different way to issue the same command, so footers never trigger anything
/// that the keyboard cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FooterAction {
    OpenDetails,
    CloseDetails,
    Add,
    Edit,
    /// SES-01: "Paste a new session" of the selected account.
    PasteSession,
    MoveUp,
    MoveDown,
    Delete,
    ToggleUnmeasured,
    /// PRD 0036 RF9.1: fold or unfold the aggregators' connections.
    ToggleAggregators,
    Filter,
    Models,
    Extensions,
    About,
    /// PRD 0026 RF6.1: the statistics screen.
    Stats,
    ClearFilter,
    RefreshSelected,
    Refresh,
    Quit,
}

/// Hit targets recorded while rendering the dashboard. The caller keeps the
/// returned value alongside the dashboard state and uses it to resolve the
/// next mouse event. `area` is the full frame rectangle the targets were
/// computed for; the caller MUST compare it against the current terminal area
/// before trusting the other fields — a resize between two `draw` calls would
/// otherwise make stale cells look clickable.
#[derive(Debug, Clone, Default)]
pub(crate) struct Targets {
    /// Frame area these targets were computed for.
    pub(crate) area: Rect,
    /// `(cell rect, position in `View::visible`)` for every card on the
    /// currently rendered page. Empty when the detail view is open or there
    /// are no accounts to draw.
    pub(crate) cards: Vec<(Rect, usize)>,
    /// Number of columns on the current page. Used by the caller to convert a
    /// wheel tick into a single visual row step when navigating cards.
    pub(crate) columns: usize,
    /// Content area of the detail view, if `View::details` was true.
    pub(crate) detail: Option<Rect>,
    /// Visible HTTPS source URL and its terminal-cell hitbox.
    pub(crate) source: Option<(Rect, String)>,
    /// Footer action hitboxes, in render order.
    pub(crate) footer: Vec<(Rect, FooterAction)>,
}

impl Targets {
    /// Position (inside `View::visible`) of the card covering the absolute
    /// terminal cell `(column, row)`, if any.
    pub(crate) fn card_at(&self, column: u16, row: u16) -> Option<usize> {
        self.cards
            .iter()
            .find(|(rect, _)| rect.contains(Position::new(column, row)))
            .map(|(_, position)| *position)
    }

    /// Footer action under the absolute terminal cell `(column, row)`, if any.
    pub(crate) fn footer_at(&self, column: u16, row: u16) -> Option<FooterAction> {
        self.footer
            .iter()
            .find(|(rect, _)| rect.contains(Position::new(column, row)))
            .map(|(_, action)| *action)
    }

    pub(crate) fn source_at(&self, column: u16, row: u16) -> Option<&str> {
        self.source.as_ref().and_then(|(rect, url)| {
            rect.contains(Position::new(column, row))
                .then_some(url.as_str())
        })
    }
}

pub(crate) fn draw(frame: &mut Frame, view: View) -> Targets {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
    let mut targets = Targets {
        area,
        ..Targets::default()
    };
    if area.width < NARROW_WIDTH || area.height < NARROW_HEIGHT {
        frame.render_widget(
            Paragraph::new(tr(
                "dashboard.too_small",
                "Cuotai: widen the terminal to at least 45 × 18. q quits.",
            ))
            .wrap(Wrap { trim: true }),
            area,
        );
        return targets;
    }
    let header = HEADER_HEIGHT + u16::from(view.banner.is_some());
    let footer = footer_rows(&view, usize::from(area.width));
    let areas = Layout::vertical([
        Constraint::Length(header),
        Constraint::Min(0),
        Constraint::Length(u16::try_from(footer.len()).unwrap_or(u16::MAX)),
    ])
    .split(area);
    render_header(frame, areas[0], &view);
    if view.details {
        render_detail(frame, areas[1], &view, &mut targets);
        targets.detail = Some(areas[1]);
    } else {
        render_cards(frame, areas[1], &view, &mut targets);
    }
    render_footer(frame, areas[2], footer, &mut targets);
    targets
}

/// Render the dashboard for one account/report pair into an off-screen
/// buffer and return it as text: the card's actual drawn output, used by the
/// extension acceptance test.
#[cfg(test)]
pub(crate) fn render_for_test(
    accounts: &[Account],
    reports: &[Option<Report>],
    model_counts: &[Option<usize>],
    visible: &[usize],
    now: DateTime<Utc>,
    locale: &'static str,
) -> String {
    struct LocaleGuard(Option<&'static str>);
    impl Drop for LocaleGuard {
        fn drop(&mut self) {
            crate::l10n::TEST_LOCALE.with(|slot| slot.set(self.0));
        }
    }
    let _locale = LocaleGuard(crate::l10n::TEST_LOCALE.with(|slot| slot.replace(Some(locale))));
    use ratatui::{backend::TestBackend, Terminal};
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let empty_history: &[Option<&Report>] = &[];
    let empty_recent: &[Vec<Vec<Point>>] = &[];
    let view = View {
        accounts,
        reports,
        model_counts,
        visible,
        selected: None,
        details: false,
        detail_scroll: 0,
        show_unmeasured: true,
        refreshing: false,
        location: "",
        filter: "",
        filtering: false,
        next_refresh: None,
        error: None,
        anim: None,
        last_good: empty_history,
        recent: empty_recent,
        refresh_seconds: 300,
        history: None,
        banner: None,
        incidents: &[],
        local_cost: None,
        sub_cards: &[],
        aggregators_collapsed: false,
        connection_focus: None,
        access_account: None,
        now,
    };
    terminal
        .draw(|frame| {
            draw(frame, view);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    buffer
        .content()
        .chunks(120)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_header(frame: &mut Frame, area: Rect, view: &View<'_>) {
    // The dashboard's header tracks the most recent fully-assembled
    // snapshot. When one or more slots are still pending (no fresh data
    // for that account yet) we surface that explicitly so the user is
    // never misled into thinking every account was just consulted.
    let any_report = view.reports.iter().any(Option::is_some);
    let all_reports = view.reports.iter().all(Option::is_some);
    let checked = match (any_report, all_reports) {
        (false, _) => tr("dashboard.header.not_checked", "not queried yet").to_string(),
        (true, false) => tr(
            "dashboard.header.partial",
            "partial update (some accounts pending)",
        )
        .to_string(),
        (true, true) => view
            .reports
            .iter()
            .filter_map(Option::as_ref)
            .map(|r| r.checked_at.format("%H:%M:%S UTC").to_string())
            .next_back()
            .unwrap_or_else(|| "—".into()),
    };
    let refresh_state = if view.refreshing {
        tr(
            "dashboard.header.refreshing",
            "Querying (previous data below)",
        )
        .to_string()
    } else {
        match view.next_refresh {
            Some(remaining) => {
                let seconds = remaining.as_secs();
                trf(
                    "dashboard.header.next_refresh",
                    "Update in {seconds}s",
                    &[("seconds", &seconds)],
                )
            }
            None => tr("dashboard.header.no_refresh", "Update: —").to_string(),
        }
    };
    let title = trf(
        "dashboard.header.title",
        " CUOTAI  |  {count} accounts  |  {checked}  |  {refresh} ",
        &[
            ("count", &view.accounts.len()),
            ("checked", &checked),
            ("refresh", &refresh_state),
        ],
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(CYAN))
        .title_style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD));
    let mut lines: Vec<Line> = Vec::new();
    if let Some(banner) = view.banner {
        lines.push(Line::styled(
            safe(banner),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ));
    }
    lines.extend(view.location.lines().map(|part| Line::from(safe(part))));
    if view.filtering || !view.filter.is_empty() {
        lines.push(Line::from(trf(
            "dashboard.filter",
            "Filter: {filter}{done}",
            &[
                ("filter", &safe(view.filter)),
                (
                    "done",
                    &if view.filtering {
                        tr("models.search.done", " | Enter finishes")
                    } else {
                        ""
                    },
                ),
            ],
        )));
    }
    if let Some(error) = view.error {
        lines.push(Line::styled(safe(error), Style::default().fg(RED)));
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// One styled footer span plus the action clicking it triggers, if any.
struct FooterSegment {
    text: String,
    style: Style,
    action: Option<FooterAction>,
}

fn footer_segments(view: &View<'_>) -> [Vec<FooterSegment>; 2] {
    let plain = |text: &str, action: Option<FooterAction>| FooterSegment {
        text: text.to_string(),
        style: Style::default(),
        action,
    };
    let toggle = if view.show_unmeasured {
        tr("dashboard.footer.hide_unmeasured", "u hide no data")
    } else {
        tr("dashboard.footer.show_unmeasured", "u show no data")
    };
    let count = if view.visible.is_empty() {
        tr("dashboard.footer.count.zero", "0 accounts").to_string()
    } else if view.visible.len() == 1 {
        tr("dashboard.footer.count.one", "1 account").to_string()
    } else if let Some(position) = view
        .selected
        .filter(|position| *position < view.visible.len())
    {
        // The selected card's place among the visible ones: with more
        // accounts than a page holds, the only sign of where the list is.
        trf(
            "dashboard.footer.count.position",
            "{position}/{count} accounts",
            &[
                ("position", &(position + 1)),
                ("count", &view.visible.len()),
            ],
        )
    } else {
        trf(
            "dashboard.footer.count.many",
            "{count} accounts",
            &[("count", &view.visible.len())],
        )
    };
    let navigation = vec![
        FooterSegment {
            text: " ↑↓/jk PgUp/PgDn ".into(),
            style: Style::default().fg(CYAN),
            action: None,
        },
        plain(tr("dashboard.footer.select", "select"), None),
        plain("  ", None),
        plain(
            tr("dashboard.footer.filter", "/ filter"),
            Some(FooterAction::Filter),
        ),
        plain("  ", None),
        plain(
            tr("dashboard.footer.clear", "c clear"),
            Some(FooterAction::ClearFilter),
        ),
        plain("  ", None),
        plain(
            tr("dashboard.footer.refresh_all", "r all"),
            Some(FooterAction::Refresh),
        ),
        plain(" · ", None),
        plain(
            tr("dashboard.footer.refresh_selected", "R selected"),
            Some(FooterAction::RefreshSelected),
        ),
        plain("  ", None),
        plain(
            tr("dashboard.footer.extensions", "x extensions"),
            Some(FooterAction::Extensions),
        ),
        plain("  ", None),
        plain(
            tr("dashboard.footer.stats", "S statistics"),
            Some(FooterAction::Stats),
        ),
        plain("  ", None),
        plain(
            tr("dashboard.footer.about", "? about"),
            Some(FooterAction::About),
        ),
        plain("  ", None),
        plain(
            tr("dashboard.footer.quit", "q quit"),
            Some(FooterAction::Quit),
        ),
    ];
    let separator = || FooterSegment {
        text: " • ".into(),
        style: Style::default(),
        action: None,
    };
    let mut actions = Vec::new();
    if view.details {
        actions.push(plain(tr("dashboard.footer.accounts", "←→ accounts"), None));
        actions.push(separator());
        actions.push(plain(tr("dashboard.footer.scroll", "↑↓/[ ] scroll"), None));
        actions.push(separator());
        actions.push(plain(
            tr("dashboard.footer.back", "Enter/Esc back"),
            Some(FooterAction::CloseDetails),
        ));
        actions.push(separator());
        actions.push(plain(
            tr("dashboard.footer.models", "m models"),
            Some(FooterAction::Models),
        ));
        actions.push(separator());
        actions.push(plain(tr("dashboard.footer.report", "b report"), None));
        actions.push(separator());
        actions.push(plain(tr("dashboard.footer.history", "1/2/3 history"), None));
    } else {
        actions.push(plain(details_hint(), Some(FooterAction::OpenDetails)));
    }
    actions.push(separator());
    actions.push(plain(
        tr("dashboard.footer.add", "a add"),
        Some(FooterAction::Add),
    ));
    actions.push(separator());
    actions.push(plain(
        tr("dashboard.footer.edit", "e edit"),
        Some(FooterAction::Edit),
    ));
    // SES-01: the selected account's vault session was rejected; the core
    // offers pasting a new one straight away.
    if selected_offers_new_paste(view) {
        actions.push(separator());
        actions.push(plain(
            tr("dashboard.footer.paste_session", "p paste a new session"),
            Some(FooterAction::PasteSession),
        ));
    }
    actions.push(separator());
    actions.push(plain(
        tr("dashboard.footer.move_up", "K move ↑"),
        Some(FooterAction::MoveUp),
    ));
    actions.push(separator());
    actions.push(plain(
        tr("dashboard.footer.move_down", "J move ↓"),
        Some(FooterAction::MoveDown),
    ));
    actions.push(separator());
    actions.push(plain(
        tr("dashboard.footer.delete", "d/Del delete"),
        Some(FooterAction::Delete),
    ));
    actions.push(separator());
    actions.push(plain(toggle, Some(FooterAction::ToggleUnmeasured)));
    if view
        .accounts
        .iter()
        .any(|account| account.provider.is_aggregator())
    {
        actions.push(separator());
        actions.push(plain(
            if view.aggregators_collapsed {
                tr("dashboard.footer.show_connections", "g show connections")
            } else {
                tr("dashboard.footer.fold_connections", "g fold connections")
            },
            Some(FooterAction::ToggleAggregators),
        ));
        actions.push(separator());
        actions.push(plain(
            tr("dashboard.footer.next_connection", "n connection"),
            None,
        ));
        actions.push(separator());
        actions.push(plain(
            tr("dashboard.footer.aggregator_filter", "f aggregator"),
            None,
        ));
    }
    actions.push(separator());
    actions.push(plain(&count, None));
    [navigation, actions]
}

/// SES-01: whether the selected card (or the open details) is an account
/// whose session the core asks to paste again.
fn selected_offers_new_paste(view: &View<'_>) -> bool {
    view.selected
        .and_then(|position| view.visible.get(position))
        .and_then(|&index| view.reports.get(index))
        .and_then(Option::as_ref)
        .is_some_and(cuotai_core::user_session::offers_new_paste)
}

/// Whether a segment only pads or separates its neighbours; one never opens
/// a flowed row.
fn is_footer_padding(segment: &FooterSegment) -> bool {
    segment.action.is_none() && matches!(segment.text.trim(), "" | "•" | "·")
}

/// The footer's segments flowed into rows of at most `width` cells: each
/// group (navigation, actions) starts a row, a segment that does not fit
/// moves whole to the next row, and the keys beyond `FOOTER_MAX_ROWS` are
/// dropped at a segment boundary rather than clipped mid-word.
fn footer_rows(view: &View<'_>, width: usize) -> Vec<Vec<FooterSegment>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<FooterSegment>> = Vec::new();
    for group in footer_segments(view) {
        let mut row: Vec<FooterSegment> = Vec::new();
        let mut used = 0;
        for segment in group {
            let needed = cells(&segment.text);
            if used + needed > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                used = 0;
                if is_footer_padding(&segment) {
                    continue;
                }
            }
            used += needed;
            row.push(segment);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    rows.truncate(FOOTER_MAX_ROWS);
    // A cut flow never ends in a dangling separator.
    if let Some(last) = rows.last_mut() {
        while last.last().is_some_and(is_footer_padding) {
            last.pop();
        }
    }
    while rows.len() < FOOTER_MIN_ROWS {
        rows.push(Vec::new());
    }
    rows
}

fn render_footer(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<Vec<FooterSegment>>,
    targets: &mut Targets,
) {
    for (row, segments) in rows.into_iter().enumerate() {
        let Ok(row) = u16::try_from(row) else {
            break;
        };
        if row >= area.height {
            break;
        }
        let line_area = Rect {
            y: area.y + row,
            height: 1,
            ..area
        };
        let mut spans = Vec::with_capacity(segments.len());
        let mut offset = 0u16;
        for segment in segments {
            let span = Span::styled(segment.text, segment.style);
            let width = span.width() as u16;
            if let Some(action) = segment.action {
                let hitbox = Rect {
                    x: line_area.x + offset.min(line_area.width),
                    y: line_area.y,
                    width: width.min(line_area.width.saturating_sub(offset)),
                    height: 1,
                };
                if hitbox.width > 0 {
                    targets.footer.push((hitbox, action));
                }
            }
            offset = offset.saturating_add(width);
            spans.push(span);
        }
        // Single-line layout: no wrap, so hitbox offsets above are exact.
        frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
    }
}

fn render_cards(frame: &mut Frame, area: Rect, view: &View<'_>, targets: &mut Targets) {
    if view.visible.is_empty() {
        let message = if view.accounts.is_empty() {
            tr(
                "dashboard.empty.no_accounts",
                "No accounts. a opens the wizard; c clears the filter.",
            )
        } else if !view.filter.is_empty() || view.filtering {
            tr(
                "dashboard.empty.no_match",
                "No results for the current filter. c clears the filter.",
            )
        } else if !view.show_unmeasured {
            tr(
                "dashboard.empty.all_unmeasured",
                "Every visible account has no data. u shows accounts without data.",
            )
        } else {
            tr("dashboard.empty.nothing", "No account to show.")
        };
        frame.render_widget(
            Paragraph::new(message).wrap(Wrap { trim: true }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(tr("dashboard.cards.title", " Accounts ")),
            ),
            area,
        );
        return;
    }

    let columns = if area.width >= WIDE_COLUMNS_WIDTH {
        2
    } else {
        1
    };
    let column_width = (area.width / columns as u16).max(1);
    let card_height = MIN_CARD_HEIGHT;
    let rows_per_page = (area.height / card_height).max(1);
    let page_capacity = (columns as u16 * rows_per_page).max(1) as usize;
    let slots = card_slots(view);
    let selected_slot = view
        .selected
        .and_then(|position| {
            slots
                .iter()
                .position(|slot| *slot == Slot::Account(position))
        })
        .unwrap_or(0);
    let selected_page = selected_slot / page_capacity;
    let page_start = selected_page * page_capacity;
    let page_end = (page_start + page_capacity).min(slots.len());
    let page = &slots[page_start..page_end];

    if page.is_empty() {
        return;
    }

    targets.columns = columns;
    let now = view.now;
    let total_rows = page.len().div_ceil(columns);
    let row_constraints: Vec<Constraint> = (0..total_rows)
        .map(|_| Constraint::Length(card_height))
        .collect();
    let row_chunks = Layout::vertical(row_constraints).split(area);
    let column_constraints: Vec<Constraint> = (0..columns)
        .map(|_| Constraint::Length(column_width))
        .collect();
    for (row_index, row_area) in row_chunks.iter().enumerate() {
        let cell_chunks = Layout::horizontal(column_constraints.as_slice()).split(*row_area);
        for (column_index, cell_area) in cell_chunks.iter().enumerate() {
            let global_index = row_index * columns + column_index;
            let Some(&slot) = page.get(global_index) else {
                continue;
            };
            match slot {
                Slot::Account(visible_position) => {
                    let account_index = view.visible[visible_position];
                    let is_selected = view.selected == Some(visible_position);
                    render_card(frame, *cell_area, view, account_index, is_selected, now);
                    targets.cards.push((*cell_area, visible_position));
                }
                Slot::Sub(account_index, sub) => {
                    let card = &view.sub_cards[account_index][sub];
                    render_sub_card(
                        frame,
                        *cell_area,
                        view,
                        &view.accounts[account_index],
                        card,
                        view.connection_focus == Some((account_index, sub)),
                        now,
                    );
                    // A click selects its aggregator.
                    if let Some(position) = view.visible.iter().position(|i| *i == account_index) {
                        targets.cards.push((*cell_area, position));
                    }
                }
            }
        }
    }
}

fn render_card(
    frame: &mut Frame,
    area: Rect,
    view: &View<'_>,
    account_index: usize,
    selected: bool,
    now: DateTime<Utc>,
) {
    let account = &view.accounts[account_index];
    let report = view.reports.get(account_index).and_then(Option::as_ref);
    // A failed query with a preserved reading shows the old metrics,
    // labelled stale with their original age; the class stays unknown.
    let stale = view.last_good.get(account_index).copied().flatten();
    let mut rows: Vec<(usize, MetricRow)> = stale
        .or(report)
        .map(|report| {
            // The preserved reading is old: no pace from today's clock.
            let clock = pace_clock(view, account_index, report, stale.is_some(), now);
            let recent = recent_of(view, account_index);
            let row = |(index, metric)| metric_row(metric, clock, points(recent, index));
            report
                .metrics
                .iter()
                .enumerate()
                .map(row)
                .enumerate()
                .collect()
        })
        .unwrap_or_default();
    // PRD 0009 RF3.1: the core's binding metric leads the card; the others
    // keep the adapter order.
    if let Some(limit) = report.and_then(binding) {
        rows[..=limit.index].rotate_right(1);
    }
    let class = classification(report);
    // PRD 0005 RF4.2: with nothing to draw, the core's explanation (the
    // reason of an Unavailable account, a changed undocumented interface)
    // takes the place of the generic failure and may use every free row.
    let access = access::describe_reading(view.access_account.unwrap_or(account), report);
    let explanation = report
        .and_then(|report| access.explain(report))
        .map(|note| resolve_l10n(&note));
    // PRD 0016 RF4.1: an account whose query uses a web session names the
    // `UserSession` level on the border and the core's warning in the
    // card's free rows (in full in the details).
    let session_notice = cuotai_core::user_session::notice(account);
    let first_note = if rows.is_empty() {
        explanation
            .clone()
            .or_else(|| report.and_then(aggregator_summary))
            .or_else(|| report.and_then(|report| report.notes.first().cloned()))
    } else {
        None
    };
    let hidden_metrics = rows.len().saturating_sub(METRICS_VISIBLE_IN_CARD);
    let hint_rows = if hidden_metrics > 0 { 1 } else { 0 };
    let note_rows = match (&explanation, rows.is_empty()) {
        (Some(text), true) => {
            let free = area.height.saturating_sub(3 + hint_rows as u16);
            let width = area.width.saturating_sub(2).max(20);
            textwrap_simple(text, usize::from(width))
                .len()
                .min(usize::from(free))
        }
        _ => note_row_count(first_note.as_deref(), area.width),
    };

    let border_style = if selected {
        Style::default().fg(CYAN)
    } else {
        Style::default().fg(MUTED)
    };
    let title_style = if selected {
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(FG).add_modifier(Modifier::BOLD)
    };
    let title = match view.anim.and_then(|anim| anim.spinner(account_index)) {
        Some(spinner) => format!(" {spinner} {} ", safe(&account.display_name())),
        None => format!(" {} ", safe(&account.display_name())),
    };
    // PRD 0024 RF7.5: the best account of a group says so on its border,
    // while the title still fits.
    let title = match best_chip_of(view, account_index) {
        Some(chip) if cells(&title) + cells(&chip) + 4 <= usize::from(area.width) => {
            format!("{title}· {} ", safe(&chip))
        }
        _ => title,
    };
    // PRD 0036 RF2.3/RF2.4: the aggregator's standing warnings, while they fit.
    let title = match report.and_then(warning_chips) {
        Some(chips) if cells(&title) + cells(&chips) + 4 <= usize::from(area.width) => {
            format!("{title}· {} ", safe(&chips))
        }
        _ => title,
    };
    // PRD 0011 RF5: the provider's incident badge on the top border, to the
    // right of the name and never over it.
    let badge = incident_of(view, account.provider)
        .and_then(|status| incident_badge(status, tui_locale(), view.now))
        .and_then(|badge| {
            let room = usize::from(area.width.saturating_sub(3))
                .saturating_sub(Span::raw(title.as_str()).width());
            badge_title(&badge, room)
        });
    // PRD 0016 RF3.2: the known expiry of the web session the query used,
    // right of the access level on the bottom border.
    let expiry = report
        .and_then(|report| cuotai_core::user_session::report_expiry(report, now))
        .map(|expiry| session_expiry_title(&expiry));
    let expiry_width = expiry.as_ref().map_or(0, |line| {
        u16::try_from(line.width())
            .unwrap_or(u16::MAX)
            .saturating_add(1)
    });
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title_style(title_style)
        .title(title);
    if let Some(notice) = &session_notice {
        block = block.title_bottom(session_title(
            &notice.level,
            area.width.saturating_sub(expiry_width),
        ));
    }
    if let Some(expiry) = expiry {
        block = block.title_bottom(expiry.right_aligned());
    }
    if let Some(badge) = badge {
        block = block.title(badge);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Layout: provider/status/class line, then up to 2 metrics (each 2 rows),
    // optional wrapped note, then a trailing hint when more metrics exist.
    let mut constraints = vec![Constraint::Length(1)];
    let visible_metrics = rows.len().min(METRICS_VISIBLE_IN_CARD);
    // PRD 0023 RF8.1: the "extra" line takes a free row under the status
    // line, only while extra usage bills and the card has room for it.
    let extra_text = report
        .filter(|_| stale.is_none())
        .and_then(|report| extra_usage_text(report, tui_locale()));
    let needed = 1 + 2 * visible_metrics + note_rows + hint_rows;
    let extra_rows = usize::from(extra_text.is_some() && usize::from(inner.height) > needed);
    if extra_rows > 0 {
        constraints.push(Constraint::Length(1));
    }
    for _ in 0..visible_metrics {
        constraints.push(Constraint::Length(1)); // values line
        constraints.push(Constraint::Length(1)); // gauge
    }
    for _ in 0..note_rows {
        constraints.push(Constraint::Length(1));
    }
    for _ in 0..hint_rows {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0));
    let chunks = Layout::vertical(constraints).split(inner);

    let mut chunk_index = 0;
    let separator = || Span::styled(" · ", Style::default().fg(MUTED));
    let mut status_spans = vec![
        Span::styled(
            crate::ui::provider_label(account.provider),
            Style::default().fg(MUTED),
        ),
        separator(),
        match report {
            Some(report) => Span::styled(
                crate::ui::status_label(report.status),
                Style::default().fg(status_color(report.status)),
            ),
            None => Span::styled(
                tr("dashboard.card.waiting", "Waiting"),
                Style::default().fg(MUTED),
            ),
        },
    ];
    let suffix = match stale {
        Some(stale) => Some(Span::styled(
            trf(
                "dashboard.card.stale",
                "(stale) {age}",
                &[("age", &age_text(stale.checked_at, view.now))],
            ),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        )),
        None => {
            status_spans.push(separator());
            status_spans.push(Span::styled(
                class_label(class),
                Style::default()
                    .fg(class_color(class))
                    .add_modifier(Modifier::BOLD),
            ));
            report.map(|report| {
                Span::styled(
                    format!(
                        "{}{}",
                        age_text(report.checked_at, view.now),
                        data_as_of_suffix(report)
                    ),
                    Style::default().fg(MUTED),
                )
            })
        }
    };
    if let Some(count) = view.model_counts.get(account_index).copied().flatten() {
        let count_spans = vec![
            separator(),
            Span::styled(model_count_label(count), Style::default().fg(MUTED)),
        ];
        let suffix_width = suffix
            .as_ref()
            .map(|span| Line::from(vec![separator(), span.clone()]).width())
            .unwrap_or(0);
        if Line::from(status_spans.clone()).width()
            + Line::from(count_spans.clone()).width()
            + suffix_width
            <= usize::from(inner.width)
        {
            status_spans.splice(3..3, count_spans);
        }
    }
    let prefix = Line::from(status_spans);
    if let Some(suffix) = suffix {
        let suffix = Line::from(vec![separator(), suffix]);
        let suffix_width = u16::try_from(suffix.width())
            .unwrap_or(u16::MAX)
            .min(inner.width);
        let prefix_width = u16::try_from(prefix.width())
            .unwrap_or(u16::MAX)
            .min(inner.width.saturating_sub(suffix_width));
        let status_chunks = Layout::horizontal([
            Constraint::Length(prefix_width),
            Constraint::Length(suffix_width),
            Constraint::Min(0),
        ])
        .split(chunks[chunk_index]);
        frame.render_widget(Paragraph::new(prefix), status_chunks[0]);
        frame.render_widget(Paragraph::new(suffix), status_chunks[1]);
    } else {
        frame.render_widget(Paragraph::new(prefix), chunks[chunk_index]);
    }
    chunk_index += 1;
    if extra_rows > 0 {
        if let Some(text) = &extra_text {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    safe(text).into_owned(),
                    Style::default().fg(PINK).add_modifier(Modifier::BOLD),
                )),
                chunks[chunk_index],
            );
            chunk_index += 1;
        }
    }

    let motion_key = account.state_key();
    for (index, metric) in rows.iter().take(METRICS_VISIBLE_IN_CARD) {
        // A preserved reading stays still: no bar transition.
        let motion = view
            .anim
            .filter(|_| stale.is_none())
            .map(|anim| (anim, motion_key.as_str(), *index));
        render_metric_block(frame, &chunks, &mut chunk_index, metric, now, motion);
    }
    if note_rows > 0 {
        if let Some(note) = first_note.as_deref() {
            let note_chunks = &chunks[chunk_index..chunk_index + note_rows];
            render_note_lines(frame, note_chunks, note);
            chunk_index += note_rows;
        }
    }
    if hint_rows > 0 {
        let hint_chunks = &chunks[chunk_index..chunk_index + hint_rows];
        let hint = build_hint(hidden_metrics);
        frame.render_widget(
            Paragraph::new(hint).wrap(Wrap { trim: true }),
            hint_chunks[0],
        );
    }
    if let (Some(notice), Some(free)) = (&session_notice, chunks.last()) {
        let lines = clipped_lines(
            &resolve_l10n(&notice.warning),
            usize::from(free.width),
            usize::from(free.height),
        );
        let lines: Vec<Line> = lines
            .into_iter()
            .map(|line| Line::styled(line, Style::default().fg(MUTED)))
            .collect();
        frame.render_widget(Paragraph::new(lines), *free);
    }
}

/// PRD 0023 RF8: the core's one-line summary of paid extra usage while it
/// bills (`Active` or `CapReached`), "Limite atingido · no extra $6.25" when
/// the account is also at its limit; `None` otherwise.
pub(crate) fn extra_usage_text(report: &Report, locale: &str) -> Option<String> {
    let state = overage::assess(report).filter(|state| state.state.is_billing())?;
    let blocks = scope_blocks(report);
    let text = if classification(Some(report)) == AccountClass::Exhausted {
        state.summary_with_limit_hit()
    } else if !blocks.is_empty() {
        // PRD 0023 RF10.3: an exhausted model limit marks only its scope.
        state.summary_with_scope_hit(&blocks)
    } else {
        state.text()
    };
    Some(text.resolve(locale))
}

/// The details lines of the "USO EXTRA" section (PRD 0023 RF8.1): the
/// state, the cap and cycle end when known, and the provider-units caveat.
fn extra_usage_lines(report: &Report, locale: &str) -> Vec<Line<'static>> {
    let Some(state) = overage::assess(report) else {
        return Vec::new();
    };
    let heading = Style::default().fg(PURPLE).add_modifier(Modifier::BOLD);
    let mut parts = vec![state.text().resolve(locale)];
    if let Some(cap) = state.cap_amount() {
        let cap = match cap {
            cuotai_core::text::ParamValue::Text(text) => text,
            cuotai_core::text::ParamValue::L10n(text) => text.resolve(locale),
        };
        parts.push(trf("dashboard.extra.cap", "cap {cap}", &[("cap", &cap)]));
    }
    if let Some(end) = state.cycle_ends_at {
        parts.push(trf(
            "dashboard.extra.cycle_ends",
            "cycle ends {at}",
            &[("at", &local_time(end))],
        ));
    }
    let mut lines = vec![Line::from(vec![
        Span::styled(tr("dashboard.extra.heading", "  EXTRA USAGE  "), heading),
        Span::styled(
            safe(&parts.join(" · ")).into_owned(),
            Style::default().fg(PINK),
        ),
    ])];
    if let Some(caveat) = state.units_caveat() {
        lines.push(Line::styled(
            format!("             {}", safe(&caveat.resolve(locale))),
            Style::default().fg(MUTED),
        ));
    }
    lines
}

/// `text` wrapped to `width` in at most `rows` lines, the last one ending in
/// "…" when the text did not fit.
fn clipped_lines(text: &str, width: usize, rows: usize) -> Vec<String> {
    if rows == 0 || width == 0 {
        return Vec::new();
    }
    let mut lines = textwrap_simple(&safe(text), width);
    if lines.len() > rows {
        lines.truncate(rows);
        let last = &mut lines[rows - 1];
        let (head, _) = split_at_cells(last, width.saturating_sub(1));
        *last = format!("{head}…");
    }
    lines
}

fn class_color(class: AccountClass) -> Color {
    match class {
        AccountClass::Available => GREEN,
        AccountClass::Exhausted => RED,
        AccountClass::Unmeasured => MUTED,
    }
}

/// PRD 0016 RF4.1: a web session's `UserSession` level on the card's bottom
/// border, muted and cut to the card so it never competes with the quota.
/// The integration's access level stays out of the UI: it is documented in
/// `docs/providers.md`.
fn session_title(session: &cuotai_core::text::Localized, width: u16) -> Line<'static> {
    let text = resolve_card_l10n(session);
    let room = usize::from(width.saturating_sub(4));
    let text = ellipsize(&text, room);
    Line::from(Span::styled(
        format!(" {text} "),
        Style::default().fg(MUTED).remove_modifier(Modifier::BOLD),
    ))
}

/// "Sessão expira em 12 dias", yellow under three days.
fn session_expiry_title(expiry: &cuotai_core::user_session::SessionExpiry) -> Line<'static> {
    let text = resolve_l10n(&expiry.label);
    let color = if expiry.soon { YELLOW } else { MUTED };
    Line::from(Span::styled(
        format!(" {} ", text.trim_end_matches('.')),
        Style::default().fg(color).remove_modifier(Modifier::BOLD),
    ))
}

/// PRD 0011 RF5: how a provider status reads on a card and in `--once`,
/// the desktop's rule. Only an incident or maintenance (marked as the last
/// reading when stale) or a page the latest query could not read is
/// flagged; `Operational` and a plain `Unknown` (never read as healthy)
/// show nothing outside the details.
pub(crate) struct IncidentBadge {
    pub(crate) glyph: &'static str,
    pub(crate) color: Color,
    /// The core's state label (or "page unavailable"), plus the stale mark.
    pub(crate) label: String,
    /// The active incident's title, without terminal controls.
    pub(crate) title: Option<String>,
    /// The incident page, or the status page without one.
    pub(crate) url: String,
}

/// Shape and color of each state; the shape carries it, color repeats it.
fn incident_look(state: IncidentState) -> (&'static str, Color) {
    match state {
        IncidentState::Operational => ("✓", GREEN),
        IncidentState::Degraded => ("!", YELLOW),
        IncidentState::PartialOutage => ("▲", RED),
        IncidentState::MajorOutage => ("✕", RED),
        IncidentState::Maintenance => ("i", CYAN),
        IncidentState::Unknown => ("?", MUTED),
    }
}

pub(crate) fn incident_badge(
    status: &ProviderStatus,
    locale: &str,
    now: DateTime<Utc>,
) -> Option<IncidentBadge> {
    let notice = !matches!(
        status.state,
        IncidentState::Operational | IncidentState::Unknown
    );
    if !notice && !status.stale && !status.expired {
        return None;
    }
    let url = status
        .incident
        .as_ref()
        .map_or_else(|| status.page_url.clone(), |incident| incident.url.clone());
    let checked = status.read_at.map(|read_at| read_text(read_at, now));
    if status.expired {
        let mut label = status.state.label().resolve(locale);
        if let Some(checked) = checked {
            label.push_str(&format!(" · {checked}"));
        }
        return Some(IncidentBadge {
            glyph: "?",
            color: MUTED,
            label,
            title: None,
            url,
        });
    }
    if !notice {
        let mut label = tr("dashboard.incident.unavailable", "Status page unavailable").to_owned();
        if let Some(checked) = checked {
            label.push_str(&format!(" · {checked}"));
        }
        return Some(IncidentBadge {
            glyph: "?",
            color: MUTED,
            label,
            title: None,
            url,
        });
    }
    let (glyph, color) = incident_look(status.state);
    let mut label = status.state.label().resolve(locale);
    if status.stale {
        label.push_str(tr("dashboard.incident.last_reading", " · last reading"));
        if let Some(checked) = checked {
            label.push_str(&format!(" · {checked}"));
        }
    }
    Some(IncidentBadge {
        glyph,
        color,
        label,
        title: status
            .incident
            .as_ref()
            .map(|incident| safe(&incident.title).into_owned()),
        url,
    })
}

/// The card's badge in at most `room` cells: the whole label, a cut one or
/// only the shape.
fn badge_title(badge: &IncidentBadge, room: usize) -> Option<Line<'static>> {
    let full = format!(" {} {} ", badge.glyph, badge.label);
    let text = if Span::raw(full.as_str()).width() <= room {
        full
    } else if room >= 6 {
        let cut: String = badge.label.chars().take(room - 5).collect();
        format!(" {} {}… ", badge.glyph, cut.trim_end())
    } else if room >= 3 {
        format!(" {} ", badge.glyph)
    } else {
        return None;
    };
    Some(
        Line::from(Span::styled(
            text,
            Style::default()
                .fg(badge.color)
                .add_modifier(Modifier::BOLD),
        ))
        .right_aligned(),
    )
}

/// PRD 0011 RF5: the details' full provider status (operational and
/// unknown too): state and incident, start, reading time and page.
fn incident_lines(status: &ProviderStatus, now: DateTime<Utc>) -> Vec<Line<'static>> {
    let (glyph, color) = incident_look(status.state);
    let mut heading = vec![
        Span::styled(
            tr("dashboard.incident.heading", "  PROVIDER  "),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{glyph} {}", resolve_l10n(&status.state.label())),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(incident) = &status.incident {
        heading.push(Span::styled(
            format!(" · {}", safe(&incident.title)),
            Style::default().fg(FG),
        ));
    }
    let mut facts = Vec::new();
    if let Some(start) = status.incident.as_ref().and_then(|i| i.started_at) {
        facts.push(trf(
            "dashboard.incident.since",
            "since {at}",
            &[("at", &local_time(start))],
        ));
    }
    match (status.read_at, status.stale) {
        (Some(read), false) => facts.push(read_text(read, now)),
        (Some(read), true) => facts.push(trf(
            "dashboard.incident.read_stale",
            "read {age} · the last status query failed; this is the previous reading",
            &[("age", &age_text(read, now))],
        )),
        (None, false) => facts.push(tr("dashboard.incident.not_read", "not read yet").into()),
        (None, true) => facts.push(
            tr(
                "dashboard.incident.failed_unread",
                "the status query failed; page not read yet",
            )
            .into(),
        ),
    }
    let url = status
        .incident
        .as_ref()
        .map_or(status.page_url.as_str(), |incident| incident.url.as_str());
    vec![
        Line::from(heading),
        Line::styled(
            format!("  {}", facts.join(" · ")),
            Style::default().fg(if status.stale { YELLOW } else { MUTED }),
        ),
        Line::styled(format!("  {}", safe(url)), Style::default().fg(CYAN)),
    ]
}

/// "read 3 min ago" for a status page reading.
fn read_text(read_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    trf(
        "dashboard.incident.read",
        "read {age}",
        &[("age", &age_text(read_at, now))],
    )
}

/// "Enter details".
fn details_hint() -> &'static str {
    tr("dashboard.footer.details", "Enter details")
}

fn build_hint(hidden_metrics: usize) -> Line<'static> {
    if hidden_metrics > 0 {
        let more = if hidden_metrics == 1 {
            tr("dashboard.hint.more_one", "+1 metric • ").to_string()
        } else {
            trf(
                "dashboard.hint.more_many",
                "+{count} metrics • ",
                &[("count", &hidden_metrics)],
            )
        };
        let mut spans = vec![Span::raw(more)];
        spans.push(Span::styled(
            details_hint(),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ));
        Line::from(spans)
    } else {
        Line::from(Span::styled(
            details_hint(),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ))
    }
}

fn note_row_count(note: Option<&str>, _width: u16) -> usize {
    match note {
        Some(text) if !text.is_empty() => {
            // Single wrapped line is enough for an inline error hint inside
            // the card — the detail view carries the full note verbatim.
            if text.trim().is_empty() {
                0
            } else {
                1
            }
        }
        _ => 0,
    }
}

/// The terminal cells `text` occupies (CJK and emoji take two, combining
/// marks none), as ratatui will draw it.
pub(crate) fn cells(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// `text` split where its first `width` cells end: a character that would
/// cross the boundary starts the tail. The head is never empty while the
/// text is not, so a caller consuming the text always makes progress.
fn split_at_cells(text: &str, width: usize) -> (&str, &str) {
    let mut used = 0;
    for (offset, character) in text.char_indices() {
        let cell_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if offset > 0 && used + cell_width > width {
            return text.split_at(offset);
        }
        used += cell_width;
    }
    (text, "")
}

/// `text` cut to `room` cells, ending in "…" when something was cut.
fn ellipsize(text: &str, room: usize) -> String {
    if cells(text) <= room {
        return text.to_string();
    }
    let (head, _) = split_at_cells(text, room.saturating_sub(1));
    let head = if room == 0 { "" } else { head.trim_end() };
    format!("{head}…")
}

/// `text` wrapped to `width` cells on spaces; a word wider than a line (CJK
/// text, a URL) breaks at character boundaries instead of being clipped.
fn textwrap_simple(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_cells = 0;
    for word in text.split_whitespace() {
        let mut word = word;
        let mut word_cells = cells(word);
        loop {
            if current.is_empty() {
                if word_cells <= width {
                    current.push_str(word);
                    current_cells = word_cells;
                    break;
                }
                let (head, tail) = split_at_cells(word, width);
                lines.push(head.to_string());
                word = tail;
                word_cells = cells(tail);
                if word.is_empty() {
                    break;
                }
            } else if current_cells + 1 + word_cells <= width {
                current.push(' ');
                current.push_str(word);
                current_cells += 1 + word_cells;
                break;
            } else {
                lines.push(std::mem::take(&mut current));
                current_cells = 0;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(text.to_string());
    }
    lines
}

fn render_note_lines(frame: &mut Frame, chunks: &[Rect], note: &str) {
    let width = chunks
        .first()
        .map(|rect| rect.width.max(20) as usize)
        .unwrap_or(40);
    let lines = textwrap_simple(note, width);
    for (chunk, line) in chunks.iter().zip(
        lines
            .iter()
            .map(String::as_str)
            .chain(std::iter::repeat("")),
    ) {
        let styled = Line::from(Span::styled(safe(line), Style::default().fg(YELLOW)));
        frame.render_widget(Paragraph::new(styled).wrap(Wrap { trim: true }), *chunk);
    }
}

fn render_metric_block(
    frame: &mut Frame,
    chunks: &[Rect],
    chunk_index: &mut usize,
    metric: &MetricRow,
    now: DateTime<Utc>,
    motion: Option<(&Anim, &str, usize)>,
) {
    let fill = |ratio: f64| motion.map_or(ratio, |(anim, name, i)| anim.bar(name, i, ratio));
    let unicode = motion.is_some();
    let text_area = chunks[*chunk_index];
    let gauge_area = chunks[*chunk_index + 1];
    *chunk_index += 2;

    // PRD 0007 RF4: a discreet stage icon before the label on the dashboard.
    let mut text_spans: Vec<Span> = metric.pace.iter().map(pace_icon).collect();
    text_spans.push(Span::styled(
        format!("{}: ", safe(&metric.label)),
        Style::default().fg(FG).add_modifier(Modifier::BOLD),
    ));
    text_spans.push(Span::raw(safe(&metric.text)));
    if let Some(period) = &metric.period {
        text_spans.push(Span::styled(
            format!("  · {}", safe(period)),
            Style::default().fg(MUTED),
        ));
    }
    // PRD 0024 RF6.2: the core's pace chip beside the stage icon when the
    // card is wide enough to keep the metric text readable.
    if let Some(pace) = metric
        .pace
        .as_ref()
        .filter(|_| text_area.width >= CHIP_MIN_WIDTH)
    {
        let chip = pace.chip(pace_unit()).text.resolve(tui_locale());
        text_spans.push(Span::styled(
            format!("  {}", safe(&chip)),
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(text_spans)).wrap(Wrap { trim: true }),
        text_area,
    );

    match metric.state {
        MetricState::Unmeasured => {
            let line = Line::from(vec![
                Span::styled("— ", Style::default().fg(MUTED)),
                Span::styled(
                    tr("dashboard.metric.no_data", "no data"),
                    Style::default().fg(MUTED),
                ),
            ]);
            frame.render_widget(Paragraph::new(line), gauge_area);
        }
        MetricState::Exhausted { no_cap } => {
            if no_cap {
                let line = Line::from(Span::styled(
                    tr("dashboard.metric.balance_exhausted", "balance exhausted"),
                    Style::default().fg(RED).add_modifier(Modifier::BOLD),
                ));
                frame.render_widget(Paragraph::new(line), gauge_area);
            } else {
                render_gauge(
                    frame,
                    gauge_area,
                    fill(0.0),
                    Some(RED),
                    metric,
                    now,
                    unicode,
                );
            }
        }
        MetricState::Available { ratio, uncapped } => {
            if uncapped {
                let line = Line::from(Span::styled(no_limit(), Style::default().fg(MUTED)));
                frame.render_widget(Paragraph::new(line), gauge_area);
            } else {
                let ratio = fill(ratio);
                let color = metric_state_color(metric.state).unwrap_or(YELLOW);
                let color = motion.map_or(color, |(anim, ..)| anim.color(ratio, color));
                render_gauge(frame, gauge_area, ratio, Some(color), metric, now, unicode);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_gauge(
    frame: &mut Frame,
    area: Rect,
    ratio: f64,
    color: Option<Color>,
    metric: &MetricRow,
    now: DateTime<Utc>,
    unicode: bool,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let bar_color = color.unwrap_or(MUTED);
    let has_reset = metric.resets_at.is_some();
    let reset_width = if has_reset { 16 } else { 0 };
    let bar_min = 8u16;
    let chunks = if has_reset {
        Layout::horizontal([Constraint::Min(bar_min), Constraint::Length(reset_width)]).split(area)
    } else {
        Layout::horizontal([Constraint::Min(bar_min)]).split(area)
    };
    let label_text = match metric.state {
        MetricState::Unmeasured => "—".to_string(),
        MetricState::Exhausted { .. } => tr("dashboard.metric.exhausted", "exhausted").to_string(),
        MetricState::Available { uncapped: true, .. } => no_limit().to_string(),
        MetricState::Available { .. } => {
            format!("{:>3.0}%", (ratio * 100.0).round())
        }
    };
    let style = Style::default().fg(bar_color).add_modifier(Modifier::BOLD);
    let gauge = Gauge::default()
        .ratio(ratio.clamp(0.0, 1.0))
        .use_unicode(unicode)
        .gauge_style(style)
        .label(Span::styled(
            label_text,
            Style::default().bg(bar_color).fg(BG),
        ));
    frame.render_widget(gauge, chunks[0]);
    if has_reset {
        frame.render_widget(
            Paragraph::new(render_reset(metric.resets_at.unwrap(), now)),
            chunks[1],
        );
    }
}

/// "limit not reported".
fn no_limit() -> &'static str {
    tr("dashboard.metric.no_limit", "limit not reported")
}

fn render_detail(frame: &mut Frame, area: Rect, view: &View<'_>, targets: &mut Targets) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PURPLE))
        .title(tr("dashboard.detail.title", " Details · missing ≠ zero "));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let Some(selected_position) = view.selected else {
        frame.render_widget(
            Paragraph::new(tr("dashboard.detail.none", "No account selected."))
                .wrap(Wrap { trim: true })
                .scroll((view.detail_scroll, 0)),
            inner,
        );
        return;
    };
    let Some(&account_index) = view.visible.get(selected_position) else {
        frame.render_widget(
            Paragraph::new(tr("dashboard.detail.invalid", "Invalid selection."))
                .wrap(Wrap { trim: true })
                .scroll((view.detail_scroll, 0)),
            inner,
        );
        return;
    };
    let account = &view.accounts[account_index];
    let sections = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(inner);
    let previous = selected_position
        .checked_sub(1)
        .and_then(|position| view.visible.get(position));
    let next = view.visible.get(selected_position + 1);
    let neighbor = |index: Option<&usize>, arrow: &str| match index {
        Some(&index) => format!("{arrow} {}", safe(&view.accounts[index].name)),
        None => format!("{arrow} —"),
    };
    let heading = vec![
        Line::from(vec![
            Span::styled(
                tr("dashboard.detail.account", " ACCOUNT  "),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                safe(&account.display_name()).into_owned(),
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  {}", crate::ui::provider_label(account.provider)),
                Style::default().fg(MUTED),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(neighbor(previous, "←"), Style::default().fg(CYAN)),
            Span::styled("    ·    ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{}/{}", selected_position + 1, view.visible.len()),
                Style::default().fg(PURPLE),
            ),
            Span::styled("    ·    ", Style::default().fg(MUTED)),
            Span::styled(neighbor(next, "→"), Style::default().fg(CYAN)),
        ]),
    ];
    frame.render_widget(Paragraph::new(heading), sections[0]);
    let body = sections[1];
    let now = view.now;
    let mut lines: Vec<Line> = Vec::new();
    let Some(report) = view.reports.get(account_index).and_then(Option::as_ref) else {
        lines.push(Line::from(Span::styled(
            tr(
                "dashboard.detail.no_report",
                "No updated data. r queries; e configures this account.",
            ),
            Style::default().fg(MUTED),
        )));
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .scroll((view.detail_scroll, 0)),
            body,
        );
        return;
    };

    lines.push(Line::from(vec![
        Span::styled(
            tr("dashboard.detail.status", "  STATUS  "),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            crate::ui::status_label(report.status),
            Style::default()
                .fg(status_color(report.status))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            trf(
                "dashboard.detail.checked",
                "   ·   queried {age} ({utc} · {local} local){as_of}",
                &[
                    ("age", &age_text(report.checked_at, now)),
                    ("utc", &report.checked_at.format("%H:%M:%S UTC")),
                    (
                        "local",
                        &report.checked_at.with_timezone(&Local).format("%H:%M:%S"),
                    ),
                    ("as_of", &data_as_of_suffix(report)),
                ],
            ),
            Style::default().fg(MUTED),
        ),
    ]));
    // PRD 0010 RF5: a failed query with a preserved reading lists the old
    // metrics under an explicit stale line with their original age; the
    // current report's notes below still carry the cause of the failure.
    let stale = view.last_good.get(account_index).copied().flatten();
    if let Some(stale) = stale {
        lines.push(Line::from(vec![
            Span::styled(
                tr("dashboard.detail.reading", "  READING "),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                trf(
                    "dashboard.detail.stale",
                    "(stale) last valid {age} ({utc} · {local} local)",
                    &[
                        ("age", &age_text(stale.checked_at, now)),
                        ("utc", &stale.checked_at.format("%H:%M:%S UTC")),
                        (
                            "local",
                            &stale.checked_at.with_timezone(&Local).format("%H:%M:%S"),
                        ),
                    ],
                ),
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let limit = binding(report);
    if let Some(limit) = &limit {
        lines.push(Line::from(vec![
            Span::styled(
                tr("dashboard.detail.limit", "  LIMIT  "),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                safe(&resolve_l10n(&limit.summary(local_time))).into_owned(),
                Style::default().fg(PINK).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    // PRD 0023 RF10.3: a model at its limit is named with its return; the
    // account keeps its own state.
    for block in scope_blocks(report) {
        lines.push(Line::from(vec![
            Span::styled(
                tr("dashboard.detail.model", "  MODEL  "),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                safe(&resolve_l10n(&block.summary(local_time))).into_owned(),
                Style::default().fg(PINK),
            ),
        ]));
    }
    if stale.is_none() {
        lines.extend(extra_usage_lines(report, tui_locale()));
    }
    // PRD 0005 RF4: how this account's usage is read and, when the
    // descriptor explains the failure, that explanation leading the quotas
    // in place of the generic "no metrics" line; the adapter's notes stay
    // listed below.
    let access = access::describe_reading(account, Some(report));
    // PRD 0016 RF4.1: the web session's level and warning, in full.
    if let Some(notice) = cuotai_core::user_session::notice(account) {
        lines.push(Line::from(vec![
            Span::styled(
                tr("dashboard.detail.session", "  SESSION  "),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                resolve_l10n(&notice.level),
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {}", safe(&resolve_l10n(&notice.warning))),
                Style::default().fg(YELLOW),
            ),
        ]));
    }
    if let Some(status) = incident_of(view, account.provider) {
        lines.extend(incident_lines(status, now));
    }
    // PRD 0024 RF7.5: the best account of the group, or what it has left.
    {
        let rows: Vec<MetricRow> = stale
            .unwrap_or(report)
            .metrics
            .iter()
            .enumerate()
            .map(|(index, metric)| {
                let clock = pace_clock(view, account_index, report, stale.is_some(), now);
                metric_row(metric, clock, points(recent_of(view, account_index), index))
            })
            .collect();
        let exhausted = classification(Some(report)) == AccountClass::Exhausted;
        if let Some(sentence) = advice_sentence(view, account_index, &rows, exhausted) {
            lines.push(Line::from(vec![
                Span::styled(
                    tr("dashboard.detail.group", "  GROUP   "),
                    Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(safe(&sentence).into_owned(), Style::default().fg(GREEN)),
            ]));
        }
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        tr("dashboard.detail.quotas", "  QUOTAS AND USAGE"),
        Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
    ));
    let explanation = access.explain(report);
    if let Some(explanation) = &explanation {
        lines.push(Line::from(Span::styled(
            safe(&resolve_l10n(explanation)).into_owned(),
            Style::default().fg(YELLOW),
        )));
    }
    // PRD 0036 RF2.3/RF2.4: the instance's standing warnings.
    if let Some(chips) = warning_chips(report) {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ {}", safe(&chips)),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        )));
    }
    // PRD 0006 RF6.1: the span picker, then one history line per metric.
    let detail_history = view
        .history
        .filter(|detail| detail.account == account_index);
    if let Some(detail) = detail_history {
        lines.push(history_heading(detail));
    }

    let shown = stale.unwrap_or(report);
    let clock = pace_clock(view, account_index, shown, stale.is_some(), now);
    if shown.metrics.is_empty() {
        if explanation.is_none() {
            lines.push(Line::from(Span::styled(
                tr(
                    "dashboard.detail.no_metrics",
                    "Provider without metrics exposed in this query.",
                ),
                Style::default().fg(MUTED),
            )));
        }
    } else {
        let motion_key = account.state_key();
        for (index, metric) in shown.metrics.iter().enumerate() {
            let recent = points(recent_of(view, account_index), index);
            let row = metric_row(metric, clock, recent);
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}", safe(&row.label)),
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                row.period
                    .as_ref()
                    .map(|period| {
                        Span::styled(
                            format!("   ·   {}", safe(period)),
                            Style::default().fg(MUTED),
                        )
                    })
                    .unwrap_or_default(),
                if limit.as_ref().is_some_and(|limit| limit.index == index) {
                    Span::styled(
                        tr("dashboard.detail.binding", "   ◆ binding"),
                        Style::default().fg(PINK).add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::default()
                },
            ]));
            lines.push(Line::from(format!("  {}", safe(&row.text))));
            let motion = view
                .anim
                .filter(|_| stale.is_none())
                .map(|anim| (anim, motion_key.as_str(), index));
            lines.push(line_bar(&row, body.width.max(20), now, motion));
            // PRD 0007 RF4: one pace line per metric, labelled a linear
            // estimate; it wraps at small widths.
            if let Some(pace) = &row.pace {
                // PRD 0033 RF4.2: the line names its metric unless the
                // "limitante" mark beside the heading already does.
                let is_limit = limit.as_ref().is_some_and(|limit| limit.index == index);
                let label = if is_limit { "" } else { row.label.as_str() };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    pace_icon(pace),
                    Span::styled(
                        pace::labelled(&safe(label), &pace_text(pace, tui_locale(), local_time)),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        trf(
                            "dashboard.detail.pace_expected",
                            "   ·   expected {percent}% used · linear estimate",
                            &[(
                                "percent",
                                &format!("{:.0}", pace.expected_used_fraction * 100.0),
                            )],
                        ),
                        Style::default().fg(MUTED),
                    ),
                ]));
            }
            // A storage failure is shown once, on the heading.
            if let Some((detail, Ok(recorded))) = detail_history.map(|d| (d, &d.metrics)) {
                let key = history::metric_key(metric);
                let recorded = recorded.iter().find(|m| m.key == key);
                lines.push(history_line(recorded, detail, body.width));
            }
        }
    }

    if !report.notes.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled(
            tr("dashboard.detail.notes", "  NOTES"),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ));
    }
    // RF8: prefer the structured sidecar so the active locale (CUOTAI_LANG,
    // default pt-BR) is respected; fall back to the legacy plain Portuguese
    // vector when a note is somehow missing from the sidecar.
    let mut note_texts: Vec<String> = Vec::with_capacity(report.notes.len());
    for (idx, fallback_text) in report.notes.iter().enumerate() {
        note_texts.push(match report.notes_l10n.get(idx) {
            Some(note) => resolve_l10n(note),
            None => fallback_text.clone(),
        });
    }
    for text in &note_texts {
        lines.push(Line::from(Span::styled(
            safe(text),
            Style::default().fg(YELLOW),
        )));
    }
    // PRD 0015 RF4.2: this computer's local usage of the tool the account is
    // used through, the same section on every account of that tool.
    if let Some(panel) = view.local_cost {
        if let Some(tool) = panel.summary.for_provider(account.provider) {
            lines.extend(local_cost_lines(panel, tool));
        }
    }
    let source = (!report.source.is_empty()).then_some(safe(&report.source));
    let link = source.as_deref().filter(|source| {
        source
            .strip_prefix("https://")
            .and_then(|rest| rest.split_once('/').map(|(host, _)| host).or(Some(rest)))
            .is_some_and(|host| {
                !host.is_empty()
                    && host
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                    && source.bytes().all(|byte| byte.is_ascii_graphic())
            })
    });
    if let Some(source) = source.as_ref().filter(|_| link.is_none()) {
        lines.push(Line::default());
        lines.push(Line::styled(
            format!("{}{source}", source_prefix()),
            Style::default().fg(MUTED),
        ));
    }
    let content_height = body.height.saturating_sub(u16::from(link.is_some()));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((view.detail_scroll, 0)),
        Rect {
            height: content_height,
            ..body
        },
    );
    if let Some(source) = link.filter(|_| body.height > 0) {
        let row = Rect {
            y: body.y + content_height,
            height: 1,
            ..body
        };
        let prefix = source_prefix();
        frame.render_widget(
            Paragraph::new(format!("{prefix}{source}")).style(Style::default().fg(CYAN)),
            row,
        );
        let offset = Span::raw(prefix).width() as u16;
        if row.width > offset {
            targets.source = Some((
                Rect {
                    x: row.x + offset,
                    width: u16::try_from(cells(source))
                        .unwrap_or(u16::MAX)
                        .min(row.width - offset),
                    ..row
                },
                source.to_owned(),
            ));
        }
    }
}

/// "Source: " before the report's source.
fn source_prefix() -> &'static str {
    tr("dashboard.detail.source", "Source: ")
}

/// PRD 0015 RF4.2: the account details' local usage section: the core's
/// rows (why there are no complete numbers, today, yesterday and the
/// span, by model and by day), the notice that it is an estimate and an
/// ignored `pricing.json`; a source that is off explains how to turn it on.
fn local_cost_lines(panel: &crate::cost::CostPanel, tool: &SourceCost) -> Vec<Line<'static>> {
    let summary = &panel.summary;
    let text = |l10n: &cuotai_core::text::Localized| safe(&resolve_l10n(l10n)).into_owned();
    let mut lines = vec![
        Line::default(),
        Line::styled(
            format!("  {}", text(&cost::heading(tool.source)).to_uppercase()),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
    ];
    for (row, line) in crate::cost::source_rows(tool, &summary.span) {
        use crate::cost::Row;
        let (indent, style) = match row {
            Row::Note => ("  ", Style::default().fg(YELLOW)),
            Row::Amount => ("  ", Style::default().fg(FG)),
            Row::Heading => ("  ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
            Row::Item => ("    ", Style::default().fg(MUTED)),
        };
        lines.push(Line::styled(format!("{indent}{}", text(&line)), style));
    }
    if let Some(usage) = &tool.usage {
        // PRD 0030 RF6.2: the period's total at the typed rate, under it.
        if let (Some(display), Some(total)) = (&panel.display, usage.period.total) {
            lines.push(Line::styled(
                format!("  {}", text(&display.text(total, summary.span.last_day))),
                Style::default().fg(MUTED),
            ));
        }
        // PRD 0030 RF1: the plan price next to the estimate; nothing
        // without a plan.
        for value in panel.plans.iter().filter(|v| v.source == tool.source) {
            lines.push(Line::styled(
                format!("  {}", text(&value.text)),
                Style::default().fg(FG),
            ));
        }
    }
    let tail = match &tool.usage {
        None => vec![cost::enable_hint()],
        Some(_) => summary
            .pricing_error
            .iter()
            .chain(&summary.prices_age)
            .chain(&summary.unpriced)
            .cloned()
            .chain([summary.notice.clone()])
            .collect(),
    };
    lines.extend(
        tail.iter()
            .map(|l10n| Line::styled(format!("  {}", text(l10n)), Style::default().fg(MUTED))),
    );
    lines
}

fn line_bar(
    metric: &MetricRow,
    width: u16,
    now: DateTime<Utc>,
    motion: Option<(&Anim, &str, usize)>,
) -> Line<'static> {
    let ratio_color = metric_state_color(metric.state);
    let fill = |ratio: f64| motion.map_or(ratio, |(anim, name, i)| anim.bar(name, i, ratio));
    let reset_label = metric
        .resets_at
        .map(|reset| format!("  {}", format_reset_line(reset, now)))
        .unwrap_or_default();
    let width = usize::from(width)
        .saturating_sub(2 + cells(&reset_label) + 16)
        .max(1);
    let bar_of = |ratio: f64| match motion {
        Some(_) => eighth_bar(ratio, width),
        None => {
            let filled = ((ratio * width as f64).round() as usize).min(width);
            "█".repeat(filled) + &"░".repeat(width - filled)
        }
    };
    let (bar, suffix) = match metric.state {
        MetricState::Unmeasured => (
            "—".to_string(),
            format!(" {}", tr("dashboard.metric.no_data", "no data")),
        ),
        MetricState::Exhausted { no_cap } => (
            if no_cap {
                String::new()
            } else {
                bar_of(fill(0.0))
            },
            if no_cap {
                format!(
                    " {}",
                    tr("dashboard.metric.balance_exhausted", "balance exhausted")
                )
            } else {
                format!(" {}", tr("dashboard.metric.exhausted", "exhausted"))
            },
        ),
        MetricState::Available { ratio, uncapped } => {
            if uncapped {
                (String::new(), format!(" {}", no_limit()))
            } else {
                let ratio = fill(ratio);
                let suffix = trf(
                    "dashboard.metric.remaining",
                    " {percent}% left",
                    &[("percent", &format!("{:>3.0}", (ratio * 100.0).round()))],
                );
                (bar_of(ratio), suffix)
            }
        }
    };
    let mut color = ratio_color.unwrap_or(MUTED);
    if let (
        Some((anim, ..)),
        MetricState::Available {
            ratio,
            uncapped: false,
        },
    ) = (motion, metric.state)
    {
        color = anim.color(fill(ratio), color);
    }
    let mut spans = vec![Span::styled("  ", Style::default().fg(color))];
    spans.push(Span::styled(bar, Style::default().fg(color)));
    if !reset_label.is_empty() {
        spans.push(Span::styled(reset_label, Style::default().fg(CYAN)));
    }
    spans.push(Span::styled(suffix, Style::default().fg(color)));
    Line::from(spans)
}

fn format_reset_line(reset: DateTime<Utc>, now: DateTime<Utc>) -> String {
    match reset_seconds(reset, now) {
        Some(total) => format_reset_remaining(total),
        None => format!("{}{}", renews(), reset_text(reset, now)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gauge_colours_follow_the_core_level_thresholds() {
        let colour = |ratio| {
            metric_state_color(MetricState::Available {
                ratio,
                uncapped: false,
            })
        };
        assert_eq!(colour(0.70), Some(GREEN));
        assert_eq!(colour(0.69), Some(YELLOW));
        assert_eq!(colour(0.30), Some(YELLOW));
        assert_eq!(colour(0.29), Some(RED));
        assert_eq!(colour(0.55), Some(YELLOW), "was green at the old 50% cut");
    }

    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{MetricKind, MetricWindow, Provider};

    fn metric(
        label: &str,
        used: Option<f64>,
        limit: Option<f64>,
        remaining: Option<f64>,
    ) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used,
            limit,
            remaining,
            unit: "req".into(),
            kind: MetricKind::Quota,
            window: None,
            period: None,
            resets_at: None,
        }
    }

    fn report(status: Status, metrics: Vec<Metric>) -> Report {
        Report {
            account_id: None,
            diagnostic: None,
            account: "acc".into(),
            provider: Provider::OpenAiApi,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    #[test]
    fn missing_report_is_unmeasured() {
        assert_eq!(classification(None), AccountClass::Unmeasured);
    }

    /// PRD 0017 RF5.13: an extension account renders as a normal card in
    /// each shipped locale — the provider label, the `Extension` access
    /// label and the metric text come from the core's own report, not from
    /// a fixture string.
    #[test]
    fn screens_in_english_spanish_and_italian_carry_no_portuguese() {
        // PRD 0034 RF2: the dashboard (cards, header, footer), an adapter's
        // Portuguese metric texts, the age, the about screen and the
        // vault audit, rendered outside pt-BR.
        let account = Account {
            id: None,
            name: "work".into(),
            provider: Provider::AlibabaCloud,
            ..Account::default()
        };
        let report = Report {
            account_id: None,
            diagnostic: None,
            account: "work".into(),
            provider: Provider::AlibabaCloud,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status: Status::Ready,
            metrics: vec![Metric {
                billing: Default::default(),
                scope: None,
                label: "Créditos do plano".into(),
                used: Some(1.0),
                limit: Some(2.0),
                remaining: Some(1.0),
                unit: "créditos".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Monthly),
                period: Some("ciclo de cobrança".into()),
                resets_at: None,
            }],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: "x".into(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        };
        for locale in ["en-US", "es", "it"] {
            let screen = crate::dashboard::render_for_test(
                std::slice::from_ref(&account),
                &[Some(report.clone())],
                &[],
                &[0],
                Utc.with_ymd_and_hms(2026, 1, 1, 0, 3, 0).unwrap(),
                locale,
            );
            let markers: Vec<_> = cuotai_core::text::portuguese_markers(&screen)
                .into_iter()
                .filter(|m| !(locale == "es" && *m == "está"))
                .collect();
            assert!(markers.is_empty(), "{locale}: {markers:?}\n{screen}");
            let texts = crate::l10n::with_locale(locale, || {
                let mut about = crate::about_view::AboutView::default();
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
                terminal.draw(|frame| about.draw(frame)).unwrap();
                let about: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                format!(
                    "{about}\n{}\n{}",
                    crate::ui::age_text(
                        report.checked_at,
                        report.checked_at + chrono::TimeDelta::minutes(3)
                    ),
                    crate::ui::metric_text(&report.metrics[0])
                )
            });
            let markers: Vec<_> = cuotai_core::text::portuguese_markers(&texts)
                .into_iter()
                .filter(|m| !(locale == "es" && *m == "está"))
                .collect();
            assert!(markers.is_empty(), "{locale}: {markers:?}\n{texts}");
        }
        // pt-BR keeps the adapter's own words.
        let screen = crate::dashboard::render_for_test(
            &[account],
            &[Some(report)],
            &[],
            &[0],
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 3, 0).unwrap(),
            "pt-BR",
        );
        assert!(screen.contains("Créditos do plano"), "{screen}");
        assert!(screen.contains("Alibaba Cloud — faturamento"), "{screen}");
    }

    #[test]
    fn an_extension_account_renders_a_normal_card_in_every_locale() {
        for locale in ["pt-BR", "en-US", "es", "it"] {
            let account = Account {
                id: None,
                name: "quota".into(),
                provider: Provider::Extension,
                params: std::collections::BTreeMap::from([(
                    "extension_id".to_owned(),
                    "quota".to_owned(),
                )]),
                refresh_seconds: Some(300),
                ..Account::default()
            };
            let report = Report {
                account_id: None,
                diagnostic: None,
                account: "quota".into(),
                provider: Provider::Extension,
                checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                status: Status::Ready,
                metrics: vec![Metric {
                    billing: Default::default(),
                    scope: None,
                    label: "Monthly".into(),
                    used: Some(1.0),
                    limit: Some(2.0),
                    remaining: Some(1.0),
                    unit: "requests".into(),
                    kind: MetricKind::Quota,
                    window: Some(MetricWindow::Monthly),
                    period: None,
                    resets_at: None,
                }],
                notes: Vec::new(),
                notes_l10n: Vec::new(),
                source: "extension:quota".into(),
                session_expires_at: None,
                sub_reports: Vec::new(),
                as_of: None,
                overage_hint: None,
            };
            let expected_metric =
                crate::l10n::with_locale(locale, || metric_text(&report.metrics[0]));
            let screen = crate::dashboard::render_for_test(
                &[account],
                &[Some(report)],
                &[],
                &[0],
                Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap(),
                locale,
            );
            assert!(
                screen.contains(Provider::Extension.label()),
                "{locale}: the card names the provider:\n{screen}"
            );
            for label in ACCESS_LABELS {
                assert!(
                    !screen.contains(label),
                    "{locale}: no access label `{label}` on the card:\n{screen}"
                );
            }
            assert!(
                screen.contains(&expected_metric),
                "{locale}: the card renders the metric text `{expected_metric}`:\n{screen}"
            );
        }
    }

    #[test]
    fn a_passed_reset_reads_renova_agora_once() {
        let at = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let line = |now| render_reset(at, now).to_string();
        assert_eq!(line(at - chrono::Duration::seconds(47)), "renova 47s");
        assert_eq!(line(at), "renova agora");
        assert_eq!(line(at + chrono::Duration::minutes(5)), "renova agora");
        assert_eq!(format_reset_line(at, at), "renova agora");
        assert_eq!(
            format_reset_line(at, at - chrono::Duration::seconds(47)),
            "47s"
        );
    }

    #[test]
    fn non_ready_status_is_unmeasured() {
        let report = report(
            Status::AuthenticationError,
            vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn limit_only_metric_is_unmeasured() {
        let report = report(Status::Ready, vec![metric("m", None, Some(10.0), None)]);
        assert_eq!(classification(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn used_zero_limit_zero_is_unmeasured() {
        let report = report(Status::Ready, vec![metric("m", Some(0.0), Some(0.0), None)]);
        assert_eq!(classification(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn remaining_only_with_zero_cap_is_available() {
        let report = report(
            Status::Ready,
            vec![metric("balance", Some(0.0), Some(0.0), Some(5.0))],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Available);
    }

    #[test]
    fn remaining_only_with_zero_cap_debt_is_exhausted() {
        let report = report(
            Status::Ready,
            vec![metric("balance", Some(0.0), Some(0.0), Some(-1.0))],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn fully_consumed_is_exhausted() {
        let report = report(
            Status::Ready,
            vec![metric("m", Some(10.0), Some(10.0), Some(0.0))],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn negative_debt_is_exhausted() {
        let report = report(
            Status::Ready,
            vec![metric("balance", None, None, Some(-1.5))],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn nan_used_is_not_measured() {
        let report = report(
            Status::Ready,
            vec![metric("m", Some(f64::NAN), Some(10.0), None)],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn nan_remaining_is_not_measured() {
        let report = report(
            Status::Ready,
            vec![metric("balance", None, None, Some(f64::NAN))],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn used_without_limit_is_available() {
        let report = report(
            Status::Ready,
            vec![metric("balance", Some(3.5), None, None)],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Available);
    }

    #[test]
    fn mixed_metrics_one_exhausted_means_exhausted() {
        let report = report(
            Status::Ready,
            vec![
                metric("ok", Some(1.0), Some(10.0), Some(9.0)),
                metric("drained", Some(5.0), Some(5.0), Some(0.0)),
            ],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn hide_unmeasured_drops_unmeasured_group() {
        let accounts = vec![
            Account {
                id: None,
                name: "alpha".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "beta".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "gamma".into(),
                provider: Provider::Cursor,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
        ];
        let ready = report(
            Status::Ready,
            vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
        );
        let errored = report(Status::Error, vec![]);
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![ready.clone(), errored, ready],
        };
        let indices = visible_indices(&accounts, &reports_of(&snapshot), "", false);
        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn filter_substring_is_case_insensitive() {
        let accounts = vec![
            Account {
                id: None,
                name: "OpenAI Work".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "Anthropic".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
        ];
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(2.0), Some(1.0))],
                ),
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(2.0), Some(1.0))],
                ),
            ],
        };
        let indices = visible_indices(&accounts, &reports_of(&snapshot), "open", true);
        assert_eq!(indices, vec![0]);
    }

    #[test]
    fn empty_filter_returns_all_in_original_order() {
        let accounts = vec![
            Account {
                id: None,
                name: "one".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "two".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "three".into(),
                provider: Provider::Cursor,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
        ];
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(2.0), Some(1.0))],
                ),
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(2.0), Some(1.0))],
                ),
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(2.0), Some(1.0))],
                ),
            ],
        };
        let indices = visible_indices(&accounts, &reports_of(&snapshot), "", true);
        assert_eq!(indices, vec![0, 1, 2]);
    }
    #[test]
    fn empty_optional_credit_balance_does_not_hide_available_subscription_quota() {
        let report = report(
            Status::Ready,
            vec![
                metric("Sessão", Some(10.0), Some(100.0), Some(90.0)),
                metric("Créditos extras", None, None, Some(0.0)),
            ],
        );
        assert_eq!(classification(Some(&report)), AccountClass::Available);
    }

    #[test]
    fn visible_order_is_stable_when_classification_changes() {
        // Configuration order is the only thing the user can rely on across
        // refreshes, since classification depends on the report status. The
        // order MUST stay [0, 1, 2, 3] even though the second account flips
        // between exhausted and unmeasured.
        let mut accounts = vec![
            Account {
                id: None,
                name: "alpha".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "beta".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "gamma".into(),
                provider: Provider::Cursor,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "delta".into(),
                provider: Provider::Copilot,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
        ];
        let base = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
                ),
                report(
                    Status::Ready,
                    vec![metric("m", Some(5.0), Some(5.0), Some(0.0))],
                ),
                report(Status::Unsupported, vec![]),
                report(Status::AuthenticationError, vec![]),
            ],
        };
        let indices = visible_indices(&accounts, &reports_of(&base), "", true);
        assert_eq!(indices, vec![0, 1, 2, 3]);

        // Reclassify beta (drain it) — order must NOT change.
        accounts[1].name = "beta-now-drained".into();
        let mut reports = base.reports.clone();
        reports[1] = report(Status::AuthenticationError, vec![]);
        let reshot = Snapshot {
            started_at: base.started_at,
            finished_at: base.finished_at,
            reports,
        };
        let indices_after = visible_indices(&accounts, &reports_of(&reshot), "", true);
        assert_eq!(indices_after, vec![0, 1, 2, 3]);
    }

    // --- Hit-target regression tests (mouse support) -----------------------

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn three_accounts() -> Vec<Account> {
        vec![
            Account {
                id: None,
                name: "alpha".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "beta".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
            Account {
                id: None,
                name: "gamma".into(),
                provider: Provider::Cursor,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            },
        ]
    }

    fn ready_snapshot() -> Snapshot {
        Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
                ),
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
                ),
                report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
                ),
            ],
        }
    }

    /// Convert a snapshot fixture into the per-slot `Vec<Option<Report>>`
    /// expected by `View::reports` and `visible_indices`. Tests that
    /// previously wrote `reports: &reports_of(&snapshot)` use this helper to
    /// keep the legacy fixture builders.
    fn reports_of(snapshot: &Snapshot) -> Vec<Option<Report>> {
        snapshot.reports.iter().cloned().map(Some).collect()
    }

    #[test]
    fn wide_layout_records_two_columns_and_card_positions() {
        let accounts = three_accounts();
        let snapshot = ready_snapshot();
        let visible = visible_indices(&accounts, &reports_of(&snapshot), "", true);
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut targets = Targets::default();
        terminal
            .draw(|frame| {
                targets = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: false,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "/tmp/config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: Some(Duration::from_secs(60)),
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();

        assert_eq!(targets.columns, 2);
        assert_eq!(targets.cards.len(), 3);
        let (first, pos0) = targets.cards[0];
        let (second, pos1) = targets.cards[1];
        let (third, pos2) = targets.cards[2];
        assert_eq!((pos0, pos1, pos2), (0, 1, 2));
        assert!(first.y >= HEADER_HEIGHT);
        assert_eq!(second.y, first.y);
        assert!(second.x > first.x);
        assert_eq!(third.x, first.x);
        assert!(third.y > first.y);
        // Hit-test helpers agree with the recorded rects.
        assert_eq!(targets.card_at(first.x, first.y), Some(0));
        assert_eq!(
            targets.card_at(second.x + second.width - 1, second.y + second.height - 1),
            Some(1),
        );
        assert_eq!(targets.card_at(third.x + 1, third.y + 1), Some(2));
        // Header cells are not cards.
        assert_eq!(targets.card_at(0, 0), None);
    }

    /// A direct OpenAI account and a 9router instance with a Claude
    /// connection (`work`) and an unmapped `glm` one that failed.
    fn aggregator_fixture() -> (Vec<Account>, Vec<Option<Report>>) {
        aggregator_fixture_with(Vec::new())
    }

    fn aggregator_fixture_with(
        notes: Vec<cuotai_core::text::Localized>,
    ) -> (Vec<Account>, Vec<Option<Report>>) {
        let direct = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            ..Account::default()
        };
        let router = Account {
            name: "home".into(),
            provider: Provider::NineRouter,
            ..Account::default()
        };
        let checked_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let sub = |id: &str, upstream: &str, provider, status, metrics| cuotai_core::SubReport {
            id: id.into(),
            upstream: upstream.into(),
            provider,
            label: id.into(),
            status,
            metrics,
            notes_l10n: Vec::new(),
            checked_at,
            as_of: None,
            enabled_upstream: Some(true),
            duplicate_of: None,
            plan: None,
            via: None,
            identity: None,
        };
        let instance = Report {
            account: "home".into(),
            provider: Provider::NineRouter,
            sub_reports: vec![
                sub(
                    "work",
                    "claude",
                    Some(Provider::Claude),
                    Status::Ready,
                    vec![metric("weekly (7d)", Some(40.0), Some(100.0), None)],
                ),
                sub("zhipu", "glm", None, Status::Error, Vec::new()),
            ],
            notes_l10n: notes,
            ..report(Status::Ready, Vec::new())
        };
        let mut plain = report(
            Status::Ready,
            vec![metric("x", Some(1.0), Some(10.0), None)],
        );
        plain.account = "OpenAI".into();
        (vec![router, direct], vec![Some(instance), Some(plain)])
    }

    #[test]
    fn a_duplicate_connection_is_marked_also_direct_and_muted() {
        let (accounts, mut reports) = aggregator_fixture();
        reports[0].as_mut().unwrap().sub_reports[0].duplicate_of = Some("claude:work".into());
        let cards = sub_cards_of(&accounts[0], reports[0].as_ref(), None, |_| None);
        assert!(cards[0].muted);
        assert!(cards[0].account.name.ends_with("também direta"));
        assert!(!cards[1].account.name.contains("também direta"));
    }

    fn draw_aggregators(collapsed: bool, filter: &str, width: u16) -> (String, Targets) {
        let (accounts, reports) = aggregator_fixture();
        draw_aggregators_of(accounts, reports, collapsed, filter, width, None)
    }

    fn draw_aggregators_of(
        accounts: Vec<Account>,
        reports: Vec<Option<Report>>,
        collapsed: bool,
        filter: &str,
        width: u16,
        focus: Option<(usize, usize)>,
    ) -> (String, Targets) {
        let visible = visible_indices(&accounts, &reports, filter, true);
        let sub_cards: Vec<Vec<SubCard>> = accounts
            .iter()
            .enumerate()
            .map(|(i, account)| sub_cards_of(account, reports[i].as_ref(), None, |_| None))
            .collect();
        let mut terminal = Terminal::new(TestBackend::new(width, 60)).unwrap();
        let mut targets = Targets::default();
        terminal
            .draw(|frame| {
                targets = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports,
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: false,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "/tmp/config.toml",
                        filter,
                        filtering: false,
                        next_refresh: None,
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &sub_cards,
                        aggregators_collapsed: collapsed,
                        connection_focus: focus,
                        access_account: None,
                        now: Utc.with_ymd_and_hms(2026, 1, 1, 0, 5, 0).unwrap(),
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, targets)
    }

    fn exposure_notes() -> Vec<cuotai_core::text::Localized> {
        ["note.nine_router.exposed", "note.nine_router.private_http"]
            .into_iter()
            .map(|id| cuotai_core::text::Localized::fixed(id, "x"))
            .collect()
    }

    /// PRD 0036 RF2.3/RF2.4: the card names the standing warnings.
    #[test]
    fn aggregator_card_shows_the_exposure_and_private_http_chips() {
        let (accounts, reports) = aggregator_fixture();
        let (plain, _) = draw_aggregators_of(accounts, reports, false, "", 120, None);
        assert!(!plain.contains("No password") && !plain.contains("Unencrypted"));
        let (accounts, reports) = aggregator_fixture_with(exposure_notes());
        let (text, _) = draw_aggregators_of(accounts, reports, false, "", 120, None);
        let chips = warning_chips(reports_ref(&aggregator_fixture_with(exposure_notes()).1));
        assert!(text.contains(chips.as_deref().unwrap()), "{text}");
        assert!(chips.unwrap().contains(" · "));
    }

    fn reports_ref(reports: &[Option<Report>]) -> &Report {
        reports[0].as_ref().unwrap()
    }

    #[test]
    fn warning_chips_are_absent_without_the_notes() {
        let (_, reports) = aggregator_fixture();
        assert_eq!(warning_chips(reports_ref(&reports)), None);
    }

    /// PRD 0036 RF4.4: the focused connection card is drawn selected, the
    /// filter narrows which cards can take the focus.
    #[test]
    fn focused_connection_card_is_highlighted_and_the_filter_narrows_cards() {
        let (accounts, reports) = aggregator_fixture();
        let cards: Vec<Vec<SubCard>> = accounts
            .iter()
            .enumerate()
            .map(|(i, a)| sub_cards_of(a, reports[i].as_ref(), None, |_| None))
            .collect();
        assert_eq!(
            cards[0].iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["work", "zhipu"]
        );
        assert_eq!(
            matching_cards(&accounts[0], Some(&cards[0]), ""),
            vec![0, 1]
        );
        assert_eq!(
            matching_cards(&accounts[0], Some(&cards[0]), "zhipu"),
            vec![1]
        );
        let (plain, _) =
            draw_aggregators_of(accounts.clone(), reports.clone(), false, "", 120, None);
        let (focused, _) = draw_aggregators_of(accounts, reports, false, "", 120, Some((0, 1)));
        // The text is identical; only the border colour changes.
        assert_eq!(plain, focused);
    }

    /// PRD 0036 RF9.1: the aggregator comes after the direct accounts with
    /// its pool summary, then one card per connection grouped by upstream;
    /// `g` folds them; a filter on a connection keeps its aggregator.
    #[test]
    fn aggregators_render_as_a_section_after_direct_accounts() {
        let (text, targets) = draw_aggregators(false, "", 120);
        let direct = text.find("OpenAI").expect("direct card");
        let header = text.find(" home ").expect("aggregator card");
        let work = text.find("home · work").expect("connection card");
        let zhipu = text.find("home · zhipu").expect("failed connection card");
        assert!(direct < header && header < work && work < zhipu, "{text}");
        assert!(text.contains("1/2 disponíveis"), "{text}");
        assert!(text.contains("Claude"), "{text}");
        // Connection cards are click targets of their aggregator (position 0
        // in `visible`, since the aggregator is first in the configuration).
        assert_eq!(targets.cards.len(), 4);
        assert_eq!(
            targets
                .cards
                .iter()
                .filter(|(_, position)| *position == 0)
                .count(),
            3
        );
        // Folded: only the two accounts.
        let (folded, targets) = draw_aggregators(true, "", 120);
        assert!(!folded.contains("home · work"), "{folded}");
        assert_eq!(targets.cards.len(), 2);
        // A filter on a connection keeps the aggregator and that connection.
        let (filtered, _) = draw_aggregators(false, "zhipu", 120);
        assert!(filtered.contains("home · zhipu"), "{filtered}");
        assert!(!filtered.contains("home · work"), "{filtered}");
        assert!(!filtered.contains("OpenAI"), "{filtered}");
        // Narrow terminal: one column, still in order.
        let (narrow, targets) = draw_aggregators(false, "", 60);
        assert_eq!(targets.columns, 1);
        assert!(narrow.find("OpenAI").unwrap() < narrow.find(" home ").unwrap());
    }

    #[test]
    fn detail_view_records_detail_area_and_close_action() {
        let accounts = three_accounts();
        let snapshot = ready_snapshot();
        let visible = visible_indices(&accounts, &reports_of(&snapshot), "", true);
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut targets = Targets::default();
        terminal
            .draw(|frame| {
                targets = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: true,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "/tmp/config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: Some(Duration::from_secs(60)),
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();

        assert!(targets.detail.is_some());
        assert!(targets.cards.is_empty());
        assert!(targets.columns == 0);
        // OpenDetails is only offered in cards mode.
        assert!(targets
            .footer
            .iter()
            .all(|(_, action)| *action != FooterAction::OpenDetails));
    }
    #[test]
    fn detail_source_click_targets_visible_url() {
        let accounts = three_accounts();
        let mut snapshot = ready_snapshot();
        snapshot.reports[0].source =
            "https://home.qwencloud.com/billing/subscription/token-plan-individual".into();
        snapshot.reports[0].metrics.clear();
        snapshot.reports[0].notes.clear();
        let visible = visible_indices(&accounts, &reports_of(&snapshot), "", true);
        for width in [120, 45] {
            let backend = TestBackend::new(width, 40);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut targets = Targets::default();
            terminal
                .draw(|frame| {
                    targets = draw(
                        frame,
                        View {
                            accounts: &accounts,
                            reports: &reports_of(&snapshot),
                            model_counts: &[],
                            visible: &visible,
                            selected: Some(0),
                            details: true,
                            detail_scroll: 0,
                            show_unmeasured: true,
                            refreshing: false,
                            location: "config.toml",
                            filter: "",
                            filtering: false,
                            next_refresh: None,
                            error: None,
                            anim: None,
                            last_good: &[],
                            recent: &[],
                            refresh_seconds: 300,
                            history: None,
                            banner: None,
                            incidents: &[],
                            local_cost: None,
                            sub_cards: &[],
                            aggregators_collapsed: false,
                            connection_focus: None,
                            access_account: None,
                            now: Utc::now(),
                        },
                    );
                })
                .unwrap();
            let (rect, url) = targets.source.as_ref().expect("clickable source");
            assert_eq!(url, &snapshot.reports[0].source);
            assert_eq!(targets.source_at(rect.x + 2, rect.y), Some(url.as_str()));
            assert_eq!(targets.source_at(rect.x, rect.y.saturating_sub(1)), None);
            assert_eq!(rect.width, (url.len() as u16).min(width - 9));
        }
    }

    #[test]
    fn composite_source_remains_scrollable_not_clickable() {
        let accounts = three_accounts();
        let mut snapshot = ready_snapshot();
        snapshot.reports[0].source = "https://one.example + https://two.example".into();
        let visible = visible_indices(&accounts, &reports_of(&snapshot), "", true);
        let mut terminal = Terminal::new(TestBackend::new(45, 25)).unwrap();
        let mut targets = Targets::default();
        terminal
            .draw(|frame| {
                targets = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: true,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: None,
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();
        assert!(targets.source.is_none());
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "F"));
    }

    #[test]
    fn detail_heading_tracks_adjacent_visible_accounts() {
        let accounts = three_accounts();
        let snapshot = ready_snapshot();
        let visible = vec![0, 2]; // beta is filtered out.
        let mut terminal = Terminal::new(TestBackend::new(100, 26)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(1),
                        details: true,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "config.toml",
                        filter: "a",
                        filtering: false,
                        next_refresh: None,
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("← alpha"));
        assert!(text.contains("→ —"));
        assert!(text.contains("2/2"));
        assert!(!text.contains("← beta"));
    }

    #[test]
    fn header_shows_countdown_when_idle_and_querying_state_when_active() {
        let accounts = three_accounts();
        let snapshot = ready_snapshot();
        let visible = visible_indices(&accounts, &reports_of(&snapshot), "", true);

        // Idle, plenty of time left: header shows the countdown in seconds.
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: false,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "/tmp/config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: Some(Duration::from_secs(42)),
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let header_text: String = (0..buffer.area.width)
            .map(|col| buffer[(col, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            header_text.contains("42s"),
            "expected countdown in header, got: {header_text:?}"
        );

        // Idle, zero seconds left: header still shows the countdown value
        // (zero is a real, observable state).
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: false,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "/tmp/config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: Some(Duration::from_secs(0)),
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let header_text: String = (0..buffer.area.width)
            .map(|col| buffer[(col, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            header_text.contains("0s"),
            "expected zero countdown in header, got: {header_text:?}"
        );

        // Active query: header shows the querying hint, not a countdown.
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = draw(
                    frame,
                    View {
                        accounts: &accounts,
                        reports: &reports_of(&snapshot),
                        model_counts: &[],
                        visible: &visible,
                        selected: Some(0),
                        details: false,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: true,
                        location: "/tmp/config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: None,
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now: Utc::now(),
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let header_text: String = (0..buffer.area.width)
            .map(|col| buffer[(col, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            header_text.contains("Consultando"),
            "expected querying hint in header, got: {header_text:?}"
        );
    }

    #[test]
    fn reset_banner_takes_its_own_header_line_without_hiding_the_location() {
        let accounts = three_accounts();
        let snapshot = ready_snapshot();
        let reports = reports_of(&snapshot);
        let visible = visible_indices(&accounts, &reports, "", true);
        let render = |banner: Option<&str>| {
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut targets = Targets::default();
            terminal
                .draw(|frame| {
                    targets = draw(
                        frame,
                        View {
                            accounts: &accounts,
                            reports: &reports,
                            model_counts: &[],
                            visible: &visible,
                            selected: Some(0),
                            details: false,
                            detail_scroll: 0,
                            show_unmeasured: true,
                            refreshing: false,
                            location: "/tmp/config.toml (--config)\nAviso: o desktop usa outro",
                            filter: "",
                            filtering: false,
                            next_refresh: None,
                            error: None,
                            anim: None,
                            last_good: &[],
                            recent: &[],
                            refresh_seconds: 300,
                            history: None,
                            banner,
                            incidents: &[],
                            local_cost: None,
                            sub_cards: &[],
                            aggregators_collapsed: false,
                            connection_focus: None,
                            access_account: None,
                            now: Utc::now(),
                        },
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let rows: Vec<String> = (0..6)
                .map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect())
                .collect();
            (rows, targets.cards[0].0.y)
        };

        let (rows, first_card) = render(Some("14:30 · Claude: cota 5h renovada (Pessoal)"));
        assert!(rows[1].contains("14:30 · Claude: cota 5h renovada (Pessoal)"));
        assert!(rows[2].contains("/tmp/config.toml"));
        assert!(rows[3].contains("Aviso: o desktop usa outro"));
        assert_eq!(first_card, HEADER_HEIGHT + 1);

        let (rows, first_card) = render(None);
        assert!(rows[1].contains("/tmp/config.toml"));
        assert!(rows[2].contains("Aviso: o desktop usa outro"));
        assert_eq!(first_card, HEADER_HEIGHT);
    }

    #[test]
    fn card_gauge_renders_intermediate_then_final_value_after_refresh() {
        let accounts = three_accounts();
        let mut anim = Anim::new(false).with_tween();
        let start = anim.now;
        let render = |anim: &Anim, remaining: f64| {
            let reports: Vec<Option<Report>> = (0..3)
                .map(|_| {
                    Some(report(
                        Status::Ready,
                        vec![metric("m", None, Some(100.0), Some(remaining))],
                    ))
                })
                .collect();
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        View {
                            accounts: &accounts,
                            reports: &reports,
                            model_counts: &[],
                            visible: &[0, 1, 2],
                            selected: Some(0),
                            details: false,
                            detail_scroll: 0,
                            show_unmeasured: true,
                            refreshing: false,
                            location: "config.toml",
                            filter: "",
                            filtering: false,
                            next_refresh: None,
                            error: None,
                            anim: Some(anim),
                            last_good: &[],
                            recent: &[],
                            refresh_seconds: 300,
                            history: None,
                            banner: None,
                            incidents: &[],
                            local_cost: None,
                            sub_cards: &[],
                            aggregators_collapsed: false,
                            connection_focus: None,
                            access_account: None,
                            now: Utc::now(),
                        },
                    );
                })
                .unwrap();
            (reports, terminal.backend().buffer().clone())
        };
        let percent = |buffer: &ratatui::buffer::Buffer| {
            let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
            let at = text.find('%').expect("gauge label");
            text[at - 3..at].trim().parse::<u32>().unwrap()
        };
        let (old, _) = render(&anim, 80.0);
        anim.observe(&accounts, &old);
        let (new, _) = render(&anim, 20.0);
        anim.observe(&accounts, &new);
        anim.track([false; 3].into_iter(), start);
        assert_eq!(percent(&render(&anim, 20.0).1), 80, "t=0 still shows old");
        anim.track([false; 3].into_iter(), start + Duration::from_millis(100));
        let mid = percent(&render(&anim, 20.0).1);
        assert!(mid > 20 && mid < 80, "intermediate frame: {mid}%");
        anim.track([false; 3].into_iter(), start + Duration::from_millis(300));
        assert_eq!(percent(&render(&anim, 20.0).1), 20, "final value");
        assert!(!anim.active());
    }

    /// 5 h free, a daily window and the week exhausted (third, beyond the
    /// two metrics a card shows).
    fn blocked_by_week() -> Vec<Option<Report>> {
        let window = |label: &str, used: f64, window: MetricWindow| Metric {
            window: Some(window),
            resets_at: Some(Utc.with_ymd_and_hms(2026, 1, 3, 14, 30, 0).unwrap()),
            ..metric(label, Some(used), Some(100.0), None)
        };
        let mut reports = reports_of(&ready_snapshot());
        reports[0] = Some(report(
            Status::Ready,
            vec![
                window("Sessao", 0.0, MetricWindow::Window5h),
                window("Diaria", 10.0, MetricWindow::Daily),
                window("Semana", 100.0, MetricWindow::Weekly),
            ],
        ));
        reports
    }

    fn render_rows(reports: &[Option<Report>], details: bool) -> Vec<String> {
        render_at(reports, &[], &[], details, at_min(0))
    }

    /// The fixture reports' `checked_at` plus `minutes`.
    fn at_min(minutes: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minutes)
    }

    fn render_at(
        reports: &[Option<Report>],
        last_good: &[Option<&Report>],
        model_counts: &[Option<usize>],
        details: bool,
        now: DateTime<Utc>,
    ) -> Vec<String> {
        render_sized(reports, last_good, model_counts, details, now, (120, 40))
    }

    fn render_sized(
        reports: &[Option<Report>],
        last_good: &[Option<&Report>],
        model_counts: &[Option<usize>],
        details: bool,
        now: DateTime<Utc>,
        (width, height): (u16, u16),
    ) -> Vec<String> {
        render_accounts(
            &three_accounts(),
            reports,
            last_good,
            model_counts,
            (details, 0),
            now,
            (width, height),
        )
    }

    /// `(details, selected)`: the details view and the selected position.
    fn render_accounts(
        accounts: &[Account],
        reports: &[Option<Report>],
        last_good: &[Option<&Report>],
        model_counts: &[Option<usize>],
        (details, selected): (bool, usize),
        now: DateTime<Utc>,
        (width, height): (u16, u16),
    ) -> Vec<String> {
        render_view(
            accounts,
            reports,
            last_good,
            model_counts,
            (details, selected),
            now,
            (width, height),
            None,
            &[],
            None,
        )
    }

    /// [`render_accounts`] with the details' history (PRD 0006 RF6.1).
    #[allow(clippy::too_many_arguments)]
    fn render_view(
        accounts: &[Account],
        reports: &[Option<Report>],
        last_good: &[Option<&Report>],
        model_counts: &[Option<usize>],
        (details, selected): (bool, usize),
        now: DateTime<Utc>,
        (width, height): (u16, u16),
        history: Option<&DetailHistory>,
        incidents: &[ProviderStatus],
        local_cost: Option<&cuotai_core::cost::CostSummary>,
    ) -> Vec<String> {
        let buffer = render_view_buffer(
            accounts,
            reports,
            last_good,
            model_counts,
            (details, selected),
            now,
            (width, height),
            history,
            incidents,
            local_cost,
        );
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    /// [`render_view`]'s drawn buffer, colors included.
    #[allow(clippy::too_many_arguments)]
    fn render_view_buffer(
        accounts: &[Account],
        reports: &[Option<Report>],
        last_good: &[Option<&Report>],
        model_counts: &[Option<usize>],
        (details, selected): (bool, usize),
        now: DateTime<Utc>,
        (width, height): (u16, u16),
        history: Option<&DetailHistory>,
        incidents: &[ProviderStatus],
        local_cost: Option<&cuotai_core::cost::CostSummary>,
    ) -> ratatui::buffer::Buffer {
        let panel = local_cost.map(|summary| crate::cost::CostPanel::of(summary.clone()));
        let local_cost = panel.as_ref();
        let visible = visible_indices(accounts, reports, "", true);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    View {
                        accounts,
                        reports,
                        model_counts,
                        visible: &visible,
                        selected: Some(selected),
                        details,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: None,
                        error: None,
                        anim: None,
                        last_good,
                        recent: &[],
                        refresh_seconds: 300,
                        history,
                        banner: None,
                        incidents,
                        local_cost,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now,
                    },
                );
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The README and `docs/tui.md` screenshots: six placeholder accounts
    /// (no real names, e-mails or tokens) in English, 120 x 34 cells.
    fn screenshot_fixture() -> (Vec<Account>, Vec<Option<Report>>) {
        let account = |name: &str, provider: Provider| Account {
            id: None,
            name: name.into(),
            provider,
            credential_env: None,
            session: None,
            oauth: None,
            params: Default::default(),
            refresh_seconds: None,
            group: None,
        };
        let accounts = vec![
            account("Personal", Provider::Claude),
            account("Work", Provider::ChatGpt),
            account("Editor", Provider::Cursor),
            account("Team", Provider::Copilot),
            account("API", Provider::OpenAiApi),
            account("Research", Provider::Groq),
        ];
        let quota =
            |label: &str, used: f64, limit: f64, unit: &str, window, resets_in: i64| Metric {
                window: Some(window),
                resets_at: Some(at_min(resets_in)),
                unit: unit.into(),
                ..metric(label, Some(used), Some(limit), None)
            };
        let ready = |provider: Provider, metrics: Vec<Metric>| Report {
            provider,
            ..report(Status::Ready, metrics)
        };
        let reports = vec![
            Some(ready(
                Provider::Claude,
                vec![
                    quota(
                        "Session (5h)",
                        31.0,
                        100.0,
                        "%",
                        MetricWindow::Window5h,
                        185,
                    ),
                    quota(
                        "Weekly",
                        64.0,
                        100.0,
                        "%",
                        MetricWindow::Weekly,
                        3 * 1440 + 183,
                    ),
                ],
            )),
            Some(ready(
                Provider::ChatGpt,
                vec![
                    quota(
                        "Session (5h)",
                        12.0,
                        100.0,
                        "%",
                        MetricWindow::Window5h,
                        240,
                    ),
                    quota(
                        "Weekly",
                        41.0,
                        100.0,
                        "%",
                        MetricWindow::Weekly,
                        5 * 1440 + 60,
                    ),
                ],
            )),
            Some(ready(
                Provider::Cursor,
                vec![quota(
                    "Requests",
                    500.0,
                    500.0,
                    "req",
                    MetricWindow::Monthly,
                    19 * 60 + 59,
                )],
            )),
            Some(ready(
                Provider::Copilot,
                vec![quota(
                    "Premium requests",
                    120.0,
                    300.0,
                    "req",
                    MetricWindow::Monthly,
                    12 * 1440,
                )],
            )),
            Some(ready(
                Provider::OpenAiApi,
                vec![Metric {
                    kind: MetricKind::Spend,
                    unit: "USD".into(),
                    ..metric("Spend this month", Some(42.7), None, None)
                }],
            )),
            Some(ready(
                Provider::Groq,
                vec![quota(
                    "Daily requests",
                    9650.0,
                    10000.0,
                    "req",
                    MetricWindow::Daily,
                    329,
                )],
            )),
        ];
        (accounts, reports)
    }

    /// Writes `tui-dashboard.svg` and `tui-details.svg` to
    /// `$CUOTAI_TUI_SCREENSHOTS`; see `capture.rs` for the PNG step.
    #[test]
    #[ignore = "writes the documentation screenshots; run with CUOTAI_TUI_SCREENSHOTS=<dir>"]
    fn tui_documentation_screenshots() {
        let dir = std::path::PathBuf::from(
            std::env::var_os("CUOTAI_TUI_SCREENSHOTS").expect("CUOTAI_TUI_SCREENSHOTS"),
        );
        let (accounts, reports) = screenshot_fixture();
        for (name, details) in [("tui-dashboard", false), ("tui-details", true)] {
            let buffer = crate::l10n::with_locale("en-US", || {
                render_view_buffer(
                    &accounts,
                    &reports,
                    &[],
                    &[],
                    (details, 0),
                    at_min(0) + chrono::Duration::seconds(7),
                    (120, 34),
                    None,
                    &[],
                    None,
                )
            });
            std::fs::write(
                dir.join(format!("{name}.svg")),
                crate::capture::svg(&buffer),
            )
            .unwrap();
        }
    }

    /// The screenshot fixture renders in English, with placeholder names only.
    #[test]
    fn the_screenshot_fixture_is_english_and_placeholder_only() {
        let (accounts, reports) = screenshot_fixture();
        let text = crate::l10n::with_locale("en-US", || {
            render_accounts(
                &accounts,
                &reports,
                &[],
                &[],
                (false, 0),
                at_min(0),
                (120, 34),
            )
        })
        .join("\n");
        assert!(
            text.contains("Personal") && text.contains("Research"),
            "{text}"
        );
        for portuguese in ["contas", "renova", "Atualizado", "esgotado", "detalhes"] {
            assert!(!text.contains(portuguese), "{portuguese}:\n{text}");
        }
        assert!(!text.contains('@'), "no e-mail in a screenshot");
    }

    #[test]
    fn card_leads_with_binding_metric_and_keeps_adapter_order_after_it() {
        let rows = render_rows(&blocked_by_week(), false).join("\n");
        let week = rows.find("Semana:").expect("binding metric on the card");
        let session = rows.find("Sessao:").expect("first other metric stays");
        assert!(week < session);
        assert!(
            !rows.contains("Diaria:"),
            "third metric folds into the hint"
        );
        assert!(rows.contains("+1 métrica"));
    }

    /// An account with a free 5 h window and an extra-usage meter: active
    /// in money, or in provider units when `units`.
    fn on_extra(units: bool, spent: f64) -> Vec<Option<Report>> {
        let mut extra = Metric {
            window: Some(MetricWindow::Monthly),
            limit: Some(50.0),
            unit: if units { "unid. provedor" } else { "USD" }.into(),
            kind: if units {
                cuotai_core::MetricKind::Quota
            } else {
                cuotai_core::MetricKind::Spend
            },
            ..metric("Extra", Some(spent), Some(50.0), None)
        }
        .overage();
        extra.resets_at = Some(Utc.with_ymd_and_hms(2026, 1, 20, 0, 0, 0).unwrap());
        let mut reports = reports_of(&ready_snapshot());
        reports[0] = Some(report(
            Status::Ready,
            vec![metric("Sessao", Some(10.0), Some(100.0), None), extra],
        ));
        reports
    }

    /// PRD 0023 RF8.1: the card shows the core summary under the status
    /// line while extra usage bills, and the details a "USO EXTRA" section
    /// with the cap, the cycle end and the provider-units caveat.
    #[test]
    fn extra_usage_has_a_card_line_and_a_details_section() {
        let card = render_rows(&on_extra(false, 6.25), false).join(
            "
",
        );
        assert!(card.contains("No extra · $6.25"), "{card}");
        let details = render_rows(&on_extra(true, 12.5), true).join(
            "
",
        );
        assert!(details.contains("USO EXTRA"), "{details}");
        assert!(details.contains("No extra · 12.5 unid. provedor"));
        assert!(details.contains("teto 50 unid. provedor"));
        assert!(details.contains("não em dinheiro"));
        // Nothing billed yet: no card line.
        let armed = render_rows(&on_extra(false, 0.0), false).join(
            "
",
        );
        assert!(!armed.contains("No extra"));
        // Without extra usage data the section is absent.
        let plain = render_rows(&blocked_by_week(), true).join(
            "
",
        );
        assert!(!plain.contains("USO EXTRA"));
    }

    /// The summary combines with an exhausted account: "Limit hit · on extra".
    #[test]
    fn extra_usage_text_combines_with_the_limit_hit() {
        let mut reports = on_extra(false, 6.25);
        let report = reports[0].as_mut().unwrap();
        assert_eq!(
            extra_usage_text(report, "en-US").as_deref(),
            Some("On extra · $6.25")
        );
        report.metrics[0].used = Some(100.0);
        assert_eq!(
            extra_usage_text(report, "en-US").as_deref(),
            Some("Limit hit · on extra $6.25")
        );
    }

    /// PRD 0023 RF10.3: a model at its limit names itself in the summary
    /// while extra usage bills; the account keeps its own state.
    #[test]
    fn extra_usage_text_names_a_model_at_its_limit() {
        let mut reports = on_extra(false, 6.25);
        let report = reports[0].as_mut().unwrap();
        let mut opus = metric("Opus", Some(100.0), Some(100.0), None);
        opus.scope = Some(cuotai_core::Scope::model("opus", "Opus"));
        report.metrics.insert(1, opus);
        assert_eq!(
            extra_usage_text(report, "en-US").as_deref(),
            Some("Opus limit hit · on extra $6.25")
        );
    }

    #[test]
    fn card_without_binding_keeps_adapter_order() {
        let mut reports = blocked_by_week();
        if let Some(report) = reports[0].as_mut() {
            report.status = Status::Error;
        }
        let rows = render_rows(&reports, false).join("\n");
        assert!(rows.find("Sessao:").unwrap() < rows.find("Diaria:").unwrap());
        assert!(!rows.contains("Semana:"));
    }

    #[test]
    fn details_mark_only_the_binding_metric_and_keep_every_metric() {
        let rows = render_rows(&blocked_by_week(), true);
        let marked: Vec<_> = rows
            .iter()
            .filter(|row| row.contains("◆ limitante"))
            .collect();
        assert_eq!(marked.len(), 1);
        assert!(marked[0].contains("Semana"));
        let text = rows.join("\n");
        assert!(text.contains("Sessao") && text.contains("Diaria"));
        assert!(text.contains("LIMITE  bloqueada até"));
        assert!(text.contains("pela janela semanal"));
    }

    /// PRD 0007 RF4 at `at_min(0)`: 5 h window 3 h in at 40% (20 points in
    /// reserve), week 138 h of 168 h in at 88% (ahead, runs out 11h 10m before
    /// the reset), a daily window opened 5 min ago and an `other` window.
    fn paced() -> Vec<Option<Report>> {
        let window = |label: &str, used: f64, window: MetricWindow, resets_in: i64| Metric {
            window: Some(window),
            resets_at: Some(at_min(resets_in)),
            ..metric(label, Some(used), Some(100.0), None)
        };
        let mut reports = reports_of(&ready_snapshot());
        reports[0] = Some(report(
            Status::Ready,
            vec![
                window("Sessao", 40.0, MetricWindow::Window5h, 120),
                window("Semana", 88.0, MetricWindow::Weekly, 30 * 60),
                window("Diaria", 1.0, MetricWindow::Daily, 24 * 60 - 5),
                window("Outra", 50.0, MetricWindow::Other, 60),
            ],
        ));
        reports
    }

    /// PRD 0024 RF7.5: the card of the best account in a group says so on
    /// its border and the details of an exhausted member say what the best
    /// one has left; ungrouped accounts of different providers get nothing.
    #[test]
    fn the_best_account_of_a_group_is_marked_and_advised() {
        let mut accounts = three_accounts();
        for account in &mut accounts[..2] {
            account.group = Some("coding".into());
        }
        let mut reports = reports_of(&ready_snapshot());
        let used = |label: &str, used: f64| {
            report(
                Status::Ready,
                vec![metric(label, Some(used), Some(100.0), None)],
            )
        };
        reports[0] = Some(used("Sessao", 100.0));
        reports[1] = Some(used("Sessao", 40.0));
        let cards = render_accounts(
            &accounts,
            &reports,
            &[],
            &[],
            (false, 0),
            at_min(0),
            (200, 30),
        )
        .join("\n");
        assert_eq!(cards.matches("Melhor agora").count(), 1, "{cards}");
        let details = render_accounts(
            &accounts,
            &reports,
            &[],
            &[],
            (true, 0),
            at_min(0),
            (200, 40),
        )
        .join("\n");
        assert!(
            details.contains("alpha esgotou · beta tem 60% sobrando"),
            "{details}"
        );
        let best = render_accounts(
            &accounts,
            &reports,
            &[],
            &[],
            (true, 1),
            at_min(0),
            (200, 40),
        )
        .join("\n");
        assert!(
            best.contains("GRUPO") && best.contains("Melhor agora"),
            "{best}"
        );
        // Without a shared group the two providers are never compared.
        let ungrouped = three_accounts();
        let plain = render_accounts(
            &ungrouped,
            &reports,
            &[],
            &[],
            (false, 0),
            at_min(0),
            (200, 30),
        )
        .join("\n");
        assert!(!plain.contains("Melhor agora"), "{plain}");
    }

    /// PRD 0035 RF6.3/RF6.7: under both ambiguous widths, every locale's
    /// cards and header at 80 columns keep their borders in place: after the
    /// frame's glyph pass no ambiguous-width shape or border is left, so a
    /// CJK terminal draws each row in exactly the 80 cells laid out.
    #[test]
    fn every_locale_keeps_its_columns_under_both_ambiguous_widths() {
        use crate::glyphs::{fallback, Glyphs};
        use unicode_width::UnicodeWidthChar;
        for row in cuotai_core::text::LOCALES {
            for mode in [Glyphs::Unicode, Glyphs::AmbiguousWide] {
                let rows = crate::l10n::with_locale(row.code, || {
                    render_sized(&paced(), &[], &[], false, at_min(0), (80, 24))
                });
                for line in rows {
                    let drawn: String = line
                        .chars()
                        .map(|c| fallback(c, mode).unwrap_or(c))
                        .collect();
                    if mode == Glyphs::AmbiguousWide {
                        let misdrawn = drawn
                            .chars()
                            .find(|c| !c.is_alphanumeric() && c.width_cjk() != c.width());
                        assert_eq!(misdrawn, None, "{}: {drawn}", row.code);
                    }
                }
            }
        }
    }

    #[test]
    fn cards_prefix_eligible_metrics_with_the_pace_stage_icon() {
        let rows = render_rows(&paced(), false).join("\n");
        // The binding week leads; the 5 h window follows.
        assert!(rows.contains("↑ Semana:") && rows.contains("↓ Sessao:"));
        // Still at a narrow width, without losing the label.
        let narrow = render_sized(&paced(), &[], &[], false, at_min(0), (45, 18)).join("\n");
        assert!(narrow.contains("↑ Semana:"), "{narrow}");
        // PRD 0024 RF6.2: the core's chip follows the label when the card is wide
        // enough, never at the narrow width.
        let wide = render_sized(&paced(), &[], &[], false, at_min(0), (200, 30)).join("\n");
        assert!(wide.contains("20 pts de reserva"), "{wide}");
        assert!(!narrow.contains("pts de reserva"), "{narrow}");
        // A preserved reading is old: no pace for it.
        let (reports, mut good) = failed_after_success();
        good.metrics = paced()[0].clone().unwrap().metrics;
        let rows = render_at(&reports, &[Some(&good), None, None], &[], false, at_min(42));
        let rows = rows.join("\n");
        assert!(rows.contains("Semana:") && !rows.contains("↑ Semana:"));
        assert!(rows.contains("Sessao:") && !rows.contains("↓ Sessao:"));
    }

    #[test]
    fn details_show_one_pace_line_per_eligible_metric_only() {
        let text = render_rows(&paced(), true).join("\n");
        assert!(text.contains(
            "↓ Sessao: 20 pontos de reserva · dura até a renovação   ·   esperado 60% usado · estimativa linear"
        ));
        let ahead = format!(
            "↑ 6 pontos acima do ritmo · acaba às {}, 11h 10m antes da renovação",
            local_time(at_min(0) + chrono::Duration::seconds(67_745))
        );
        // PRD 0033 RF4.2: the 5-hour line names its metric; the binding
        // week (marked "limitante") does not repeat its own label.
        assert!(text.contains(&ahead), "{text}");
        assert!(!text.contains("Semana: 6 pontos"), "{text}");
        assert!(text.contains("esperado 82% usado"));
        // Freshly opened and `other` windows invent nothing.
        assert_eq!(text.matches("estimativa linear").count(), 2);
        // Narrow details wrap the line instead of cutting the stage.
        let narrow = render_sized(&paced(), &[], &[], true, at_min(0), (45, 40)).join("\n");
        assert!(narrow.contains("20 pontos de reserva"), "{narrow}");
    }

    /// PRD 0024 FIX-11: the pace is read at the reading's `checked_at` and
    /// disappears once the reading is no longer fresh (300 s interval).
    #[test]
    fn pace_follows_the_reading_time_and_hides_once_not_fresh() {
        // 7 minutes later the reading is still fresh: the stage is the one
        // at `checked_at` (20 points), not at the wall clock (22 points).
        let fresh = render_at(&paced(), &[], &[], true, at_min(7)).join("\n");
        assert!(fresh.contains("↓ Sessao: 20 pontos de reserva"), "{fresh}");
        assert!(fresh.contains("esperado 60% usado"), "{fresh}");
        let card = render_at(&paced(), &[], &[], false, at_min(7)).join("\n");
        assert!(card.contains("↓ Sessao:"), "{card}");
        // Past 1.5× the interval it is aging: the age only, no pace.
        for minutes in [8, 20] {
            let text = render_at(&paced(), &[], &[], true, at_min(minutes)).join("\n");
            assert!(!text.contains("estimativa linear"), "{text}");
            let card = render_at(&paced(), &[], &[], false, at_min(minutes)).join("\n");
            assert!(
                card.contains("Sessao:") && !card.contains("↓ Sessao:"),
                "{card}"
            );
        }
    }

    /// PRD 0010 RF5.1: a failed query whose cycle preserved the last valid
    /// reading; `last_good` mirrors `RefreshCycle::last_good_report`.
    fn failed_after_success() -> (Vec<Option<Report>>, Report) {
        let mut reports = reports_of(&ready_snapshot());
        let good = report(
            Status::Ready,
            vec![metric("Antiga", Some(3.0), Some(10.0), Some(7.0))],
        );
        let mut failed = report(Status::Error, Vec::new());
        failed.checked_at = at_min(40);
        failed.notes = vec!["Falha de rede".into()];
        reports[0] = Some(failed);
        (reports, good)
    }

    #[test]
    fn cards_show_relative_age_from_the_injected_clock() {
        let rows = render_at(
            &reports_of(&ready_snapshot()),
            &[],
            &[Some(2), None, None],
            false,
            at_min(3),
        )
        .join("\n");
        assert_eq!(rows.matches("há 3 min").count(), 3);
        let first_card = rows
            .lines()
            .find(|line| line.contains("OpenAI API"))
            .expect("first card status line");
        assert!(first_card.contains("Disponível · há 3 min"));
        assert!(!rows.contains("desatualizada"));
    }

    #[test]
    fn failed_card_with_preserved_reading_is_marked_stale_with_original_age() {
        let (reports, good) = failed_after_success();
        let rows =
            render_at(&reports, &[Some(&good), None, None], &[], false, at_min(42)).join("\n");
        // The old metrics stay on screen with their own age (42 min), not
        // the failed query's (2 min), and the class is not claimed.
        assert!(rows.contains("(desatualizada) há 42 min"));
        assert!(rows.contains("Antiga:"));
        assert!(!rows.contains("há 2 min"));
        assert_eq!(rows.matches("desatualizada").count(), 1);

        // Without a preserved reading the failed card has no metrics and
        // no stale mark; the next success removes the mark the same way.
        let rows = render_at(&reports, &[], &[], false, at_min(42)).join("\n");
        assert!(!rows.contains("desatualizada") && !rows.contains("Antiga:"));
    }

    #[test]
    fn a_reading_restored_from_the_history_shows_stale_while_waiting() {
        // PRD 0010 RF2.3: just opened, no query answered yet; the cycle
        // offers the last recorded reading of the first account.
        let (_, good) = failed_after_success();
        let rows = render_at(
            &[None, None, None],
            &[Some(&good), None, None],
            &[],
            false,
            at_min(90),
        )
        .join("\n");
        let card = rows
            .lines()
            .find(|line| line.contains("desatualizada"))
            .expect("stale status line");
        assert!(
            card.contains("Aguardando") && card.contains("(desatualizada) há 1 h"),
            "{card}"
        );
        assert!(rows.contains("Antiga:"));
        assert_eq!(rows.matches("Aguardando").count(), 3);
    }

    #[test]
    fn details_show_age_local_time_and_the_stale_reading_with_the_cause() {
        let (reports, good) = failed_after_success();
        let text =
            render_at(&reports, &[Some(&good), None, None], &[], true, at_min(42)).join("\n");
        let local = |at: DateTime<Utc>| at.with_timezone(&Local).format("%H:%M:%S").to_string();
        assert!(text.contains(&format!(
            "consultado há 2 min (00:40:00 UTC · {} local)",
            local(at_min(40))
        )));
        assert!(text.contains(&format!(
            "(desatualizada) última válida há 42 min (00:00:00 UTC · {} local)",
            local(at_min(0))
        )));
        assert!(text.contains("Antiga") && text.contains("Falha de rede"));
    }

    /// PRD 0005 RF4: an Unavailable account (Poolside, `Unsupported`), an
    /// Undocumented one whose response broke the schema (Zed, `Error` with
    /// `query.schema`) and an Official one whose failure is not explained.
    fn access_fixture() -> (Vec<Account>, Vec<Option<Report>>) {
        let account = |name: &str, provider| Account {
            id: None,
            name: name.into(),
            provider,
            credential_env: None,
            session: None,
            oauth: None,
            params: Default::default(),
            refresh_seconds: None,
            group: None,
        };
        let failed = |provider, status, id: &'static str, text: &str| Report {
            provider,
            notes: vec![text.into()],
            notes_l10n: vec![cuotai_core::text::Localized::fixed(id, text)],
            ..report(status, Vec::new())
        };
        let accounts = vec![
            account("pool", Provider::Poolside),
            account("zed", Provider::Zed),
            account("openai", Provider::OpenAiApi),
        ];
        let reports = vec![
            Some(failed(
                Provider::Poolside,
                Status::Unsupported,
                "query.inference.poolside_no_public_usage",
                "Poolside não publica endpoint de uso/saldo por chave.",
            )),
            Some(failed(
                Provider::Zed,
                Status::Error,
                "query.schema",
                "Resposta incompatível com a interface de consulta; nenhum valor foi estimado.",
            )),
            Some(failed(
                Provider::OpenAiApi,
                Status::Error,
                "query.transport",
                "Falha de rede ou tempo limite na consulta.",
            )),
        ];
        (accounts, reports)
    }

    /// The access levels in the four locales (core catalogs and Portuguese
    /// fallbacks): none is rendered by the dashboard.
    const ACCESS_LABELS: [&str; 15] = [
        "Oficial",
        "Não documentada",
        "Indisponível",
        "Derivada",
        "Official",
        "Undocumented",
        "Unavailable",
        "Derived",
        "No documentada",
        "No disponible",
        "Ufficiale",
        "Non documentato",
        "Non disponibile",
        "Derivato",
        "Oficial ",
    ];

    #[test]
    fn dashboard_renders_no_access_label_in_any_locale() {
        let (accounts, mut reports) = access_fixture();
        // `Unsupported` reads "Indisponível" as a report status; an error
        // keeps the access reason without that unrelated word.
        if let Some(report) = reports[0].as_mut() {
            report.status = Status::Error;
        }
        for locale in ["pt-BR", "en-US", "es", "it"] {
            for selected in 0..accounts.len() {
                for details in [false, true] {
                    let previous = crate::l10n::TEST_LOCALE.with(|slot| slot.replace(Some(locale)));
                    let screen = render_accounts(
                        &accounts,
                        &reports,
                        &[],
                        &[],
                        (details, selected),
                        at_min(0),
                        (120, 40),
                    )
                    .join("\n");
                    crate::l10n::TEST_LOCALE.with(|slot| slot.set(previous));
                    for label in ACCESS_LABELS {
                        assert!(!screen.contains(label), "{locale}: `{label}`:\n{screen}");
                    }
                }
            }
        }
    }

    #[test]
    fn cards_explain_unavailable_or_changed_interfaces() {
        let (accounts, reports) = access_fixture();
        let rows = render_accounts(
            &accounts,
            &reports,
            &[],
            &[],
            (false, 0),
            at_min(0),
            (120, 40),
        )
        .join("\n");
        // No access level or summary on the cards: that lives in the docs.
        assert!(!rows.contains("Chave de inferência do Poolside."));
        // RF4.2: the reason replaces the adapter's generic note.
        assert!(rows.contains("O endpoint de uso conhecido exige um token"));
        assert!(!rows.contains("Poolside não publica"));
        // A broken schema on an undocumented interface says it may have changed.
        assert!(rows.contains("A resposta não segue a interface interna"));
        assert!(!rows.contains("Resposta incompatível"));
        // A failure the descriptor does not explain keeps its own note.
        assert!(rows.contains("Falha de rede ou tempo limite"));
    }

    #[test]
    fn details_show_the_explanation_with_the_adapter_notes() {
        let (accounts, reports) = access_fixture();
        let details = |selected| {
            render_accounts(
                &accounts,
                &reports,
                &[],
                &[],
                (true, selected),
                at_min(0),
                (120, 40),
            )
            .join("\n")
        };
        let pool = details(0);
        assert!(!pool.contains("ACESSO"));
        assert!(pool.contains("O endpoint de uso conhecido exige um token"));
        assert!(!pool.contains("Provedor sem métricas expostas"));
        assert!(pool.contains("Poolside não publica"), "adapter note kept");
        let zed = details(1);
        assert!(zed.contains("pode ter mudado sem aviso"));
        assert!(zed.contains("github.com/decolua/9router"), "{zed}");
        let openai = details(2);
        assert!(openai.contains("Provedor sem métricas expostas"));
    }

    /// PRD 0006 RF6.1: the first account's "Sessao" recorded at the start,
    /// middle and end of the last 24 hours (0, 90 and 40 of 100); "Diaria"
    /// and "Semana" have no samples in the span.
    fn session_history(range: HistoryRange) -> DetailHistory {
        let session = blocked_by_week()[0].as_ref().unwrap().metrics[0].clone();
        let end = at_min(0);
        let point = |hours: i64, used: f64| Point {
            taken_at: end - chrono::Duration::hours(hours),
            used,
        };
        DetailHistory {
            account: 0,
            range,
            start: range.since(end),
            end,
            metrics: Ok(vec![MetricHistory {
                key: history::metric_key(&session),
                label: session.label.clone(),
                unit: session.unit.clone(),
                kind: session.kind,
                window: session.window,
                billing: cuotai_core::Billing::Included,
                scoped: false,
                limit: Some(100.0),
                points: vec![point(24, 0.0), point(12, 90.0), point(0, 40.0)],
                windows: Vec::new(),
            }]),
        }
    }

    fn render_history(detail: &DetailHistory, width: u16) -> String {
        render_view(
            &three_accounts(),
            &blocked_by_week(),
            &[],
            &[],
            (true, 0),
            at_min(0),
            (width, 60),
            Some(detail),
            &[],
            None,
        )
        .join("\n")
    }

    #[test]
    fn details_draw_a_sparkline_per_metric_over_the_chosen_span() {
        let text = render_history(&session_history(HistoryRange::Day), 120);
        assert!(
            text.contains("HISTÓRICO  [1 24 h]  2 7 d   3 30 d"),
            "{text}"
        );
        // 48 half-hour buckets: the first reading, the peak in the middle
        // and the last one, with blanks (never the lowest bar) between.
        let line = format!(
            "24 h  ▁{}▇{}▄  mín 0 · máx 90 · último 40 req",
            " ".repeat(23),
            " ".repeat(22)
        );
        assert!(text.contains(&line), "{text}");
        // A metric without samples in the span says so, once per metric.
        assert_eq!(
            text.matches("histórico 24 h: sem medições registradas")
                .count(),
            2
        );
        // The 7-day span is the one highlighted; its first six days are
        // blank, kept in place behind the span label.
        let week = render_history(&session_history(HistoryRange::Week), 120);
        assert!(
            week.contains("HISTÓRICO   1 24 h  [2 7 d]  3 30 d"),
            "{week}"
        );
        let line = format!(
            "7 d  {}▁  ▇  ▄  mín 0 · máx 90 · último 40 req",
            " ".repeat(41)
        );
        assert!(week.contains(&line), "{week}");
        assert!(week.contains("histórico 7 d: sem medições registradas"));
    }

    #[test]
    fn a_narrow_detail_degrades_the_sparkline_to_minimum_maximum_and_last() {
        let text = render_history(&session_history(HistoryRange::Day), 45);
        assert!(
            !text.contains(['▁', '▂', '▃', '▄', '▅', '▆', '▇']),
            "{text}"
        );
        assert!(text.contains("histórico 24 h: mín 0 · máx 90"), "{text}");
        assert!(text.contains("último 40"), "{text}");
    }

    #[test]
    fn a_history_failure_shows_once_and_another_accounts_history_never() {
        let failed = DetailHistory {
            metrics: Err("Falha ao ler ou gravar o histórico local de medições.".into()),
            ..session_history(HistoryRange::Day)
        };
        let text = render_history(&failed, 120);
        assert!(text.contains("Falha ao ler ou gravar o histórico local"));
        assert!(!text.contains("histórico 24 h"), "{text}");
        let other = DetailHistory {
            account: 1,
            ..session_history(HistoryRange::Day)
        };
        let text = render_history(&other, 120);
        assert!(!text.contains("HISTÓRICO") && !text.contains("histórico 24 h"));
    }

    /// PRD 0011 RF5 fixture: alpha's provider in a partial outage, beta's
    /// operational and gamma's page unreadable since the start.
    fn statuses() -> Vec<ProviderStatus> {
        let status = |provider, state, stale: bool| ProviderStatus {
            provider,
            state,
            incident: None,
            page_url: "https://status.example.com/".into(),
            read_at: (!stale).then(|| at_min(-2)),
            stale,
            expired: false,
        };
        vec![
            ProviderStatus {
                incident: Some(cuotai_core::incidents::Incident {
                    title: "Elevated API errors".into(),
                    started_at: None,
                    url: "https://status.example.com/incidents/x1".into(),
                }),
                ..status(Provider::OpenAiApi, IncidentState::PartialOutage, false)
            },
            status(Provider::AnthropicApi, IncidentState::Operational, false),
            status(Provider::Cursor, IncidentState::Unknown, true),
        ]
    }

    fn render_status(
        accounts: &[Account],
        (details, selected): (bool, usize),
        incidents: &[ProviderStatus],
        size: (u16, u16),
    ) -> Vec<String> {
        let reports: Vec<Option<Report>> = accounts
            .iter()
            .map(|_| {
                Some(report(
                    Status::Ready,
                    vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
                ))
            })
            .collect();
        render_view(
            accounts,
            &reports,
            &[],
            &[],
            (details, selected),
            at_min(0),
            size,
            None,
            incidents,
            None,
        )
    }

    /// RF5.1 text snapshot: the badge sits on each card's top border, right
    /// of the name, only for an incident or an unreadable page; status
    /// queries off draw the cards exactly as before.
    #[test]
    fn cards_carry_the_incident_badge_snapshot() {
        let rows = render_status(&three_accounts(), (false, 0), &statuses(), (60, 40));
        let borders: Vec<&str> = [4, 12, 20].iter().map(|&row| rows[row].as_str()).collect();
        assert_eq!(
            borders.join("\n"),
            "\
┌ alpha ──────────────────────────── ▲ Interrupção parcial ┐
┌ beta ────────────────────────────────────────────────────┐
┌ gamma ────────────────── ? Página de status indisponível ┐",
        );
        // The card body is untouched by the badge.
        assert_eq!(
            rows[5],
            "│OpenAI API · Atualizado · Disponível · agora              │"
        );
        let off = render_status(&three_accounts(), (false, 0), &[], (60, 40)).join("\n");
        assert!(!off.contains("Interrupção") && !off.contains("indisponível"));
        assert!(!off.contains('▲'));
    }

    #[test]
    fn only_incidents_and_unreadable_pages_flag_a_card() {
        let text = render_status(&three_accounts(), (false, 0), &statuses(), (60, 40)).join("\n");
        assert_eq!(text.matches("▲ Interrupção parcial").count(), 1);
        // Operational shows nothing on the card; a failed read says so,
        // never "operational".
        assert!(!text.contains("Operacional"), "{text}");
        assert!(text.contains("? Página de status indisponível"), "{text}");

        // A stale incident names the original check's age.
        let mut stale = statuses();
        stale[0].stale = true;
        let text = render_status(&three_accounts(), (false, 0), &stale, (100, 30)).join("\n");
        assert!(
            text.contains("▲ Interrupção parcial · última leitura · lido há 2 min"),
            "{text}"
        );

        // Once the core expires the retained reading, the card says unknown,
        // keeps the check age, and no longer presents the old incident.
        let mut expired = statuses();
        expired[0].state = IncidentState::Unknown;
        expired[0].incident = None;
        expired[0].read_at = Some(at_min(-31));
        expired[0].expired = true;
        let text = render_status(&three_accounts(), (false, 0), &expired, (100, 30)).join("\n");
        assert!(
            text.contains("? Status desconhecido · lido há 31 min"),
            "{text}"
        );
    }

    #[test]
    fn a_narrow_card_cuts_the_badge_and_keeps_the_name() {
        let mut accounts = three_accounts();
        accounts[0].name = "conta-pessoal-de-trabalho".into();
        let rows = render_status(&accounts, (false, 0), &statuses(), (45, 30));
        let top = &rows[4];
        assert!(top.starts_with("┌ conta-pessoal-de-trabalho "), "{top}");
        assert!(top.contains("─ ▲ Interrupçã… ┐"), "{top}");
        assert_eq!(Span::raw(top.as_str()).width(), 45);
    }

    #[test]
    fn details_list_the_providers_full_status() {
        let at = |selected| {
            render_status(&three_accounts(), (true, selected), &statuses(), (100, 40)).join("\n")
        };
        let alpha = at(0);
        assert!(
            alpha.contains("PROVEDOR  ▲ Interrupção parcial · Elevated API errors"),
            "{alpha}"
        );
        assert!(alpha.contains("lido há 2 min"), "{alpha}");
        assert!(alpha.contains("https://status.example.com/incidents/x1"));
        // Operational and unknown are spelled out in the details.
        let beta = at(1);
        assert!(beta.contains("PROVEDOR  ✓ Operacional"), "{beta}");
        assert!(beta.contains("https://status.example.com/"));
        let gamma = at(2);
        assert!(gamma.contains("PROVEDOR  ? Status desconhecido"), "{gamma}");
        assert!(
            gamma.contains("a consulta de status falhou; página ainda não lida"),
            "{gamma}"
        );
        // A failed read after a valid one keeps it, said to be the previous.
        let mut stale = statuses();
        stale[0].stale = true;
        let alpha = render_status(&three_accounts(), (true, 0), &stale, (100, 40)).join("\n");
        assert!(
            alpha.contains("lido há 2 min · a última consulta de status falhou"),
            "{alpha}"
        );
        // Off: no status block at all.
        let off = render_status(&three_accounts(), (true, 0), &[], (100, 40)).join("\n");
        assert!(!off.contains("PROVEDOR"), "{off}");
    }

    /// PRD 0015 RF4.2: the details of every account of a source's tool
    /// carry its local usage (today, yesterday, the span, by model and by
    /// day, the estimate notice); another provider's account never does,
    /// and a source that is off says so and how to turn it on.
    #[test]
    fn details_of_the_tools_accounts_show_the_local_usage() {
        use cuotai_core::cost::{
            summarize, DailyUsage, Period, Prices, ScanReport, SourceId, SourceScan, SourceState,
            Span, SummaryInput, Tokens,
        };
        let account = |name: &str, provider| Account {
            id: None,
            name: name.into(),
            provider,
            ..Account::default()
        };
        let accounts = [
            account("max", Provider::Claude),
            account("api", Provider::AnthropicApi),
        ];
        let reports: Vec<Option<Report>> = accounts
            .iter()
            .map(|_| Some(report(Status::Ready, Vec::new())))
            .collect();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let usage = [DailyUsage {
            source: SourceId::ClaudeCode,
            day: today,
            model: "claude-mystery-9".into(),
            tokens: Tokens {
                input: 12_000,
                ..Tokens::default()
            },
            requests: 3,
            sessions: 1,
        }];
        let scan = SourceScan {
            source: SourceId::ClaudeCode,
            state: SourceState::Ready,
            roots: Vec::new(),
            files: 1,
            read: 1,
            restarted: 0,
            unreadable: 0,
            foreign: 0,
            lines: 3,
            requests: 3,
            duplicates: 0,
            invalid: 0,
            expired: 0,
            complete: true,
        };
        let last = ScanReport {
            scanned_at: at_min(0),
            sources: vec![scan],
            complete: true,
        };
        let summary = |enabled: &[SourceId]| {
            summarize(&SummaryInput {
                today,
                span: Span::Period(Period::Last7Days),
                enabled,
                usage: &usage,
                prices: &Prices::embedded(),
                pricing_error: None,
                last: Some(&last),
            })
        };
        let on = summary(&[SourceId::ClaudeCode]);
        let render = |selected, cost| {
            render_view(
                &accounts,
                &reports,
                &[],
                &[],
                (true, selected),
                at_min(0),
                (110, 50),
                None,
                &[],
                Some(cost),
            )
            .join("\n")
        };
        let max = render(0, &on);
        assert!(
            max.contains("USO LOCAL DO CLAUDE CODE NESTE COMPUTADOR"),
            "{max}"
        );
        assert!(
            max.contains("Hoje: 12K tokens · custo desconhecido"),
            "{max}"
        );
        assert!(max.contains("Ontem: sem uso"), "{max}");
        assert!(max.contains("Por modelo (Últimos 7 dias)"), "{max}");
        assert!(max.contains("claude-mystery-9: 12K tokens"), "{max}");
        assert!(max.contains("2026-01-01: 12K tokens"), "{max}");
        assert!(max.contains("não é uma cobrança"), "{max}");
        let api = render(1, &on);
        assert!(!api.contains("USO LOCAL"), "{api}");

        let off = render(0, &summary(&[]));
        assert!(
            off.contains("Claude Code: desligado; nada é lido."),
            "{off}"
        );
        assert!(off.contains("cuotai --cost-enable"), "{off}");
        assert!(!off.contains("Hoje:"), "{off}");
    }

    /// PRD 0030 RF1/RF6: the details of the tool's accounts show the plan
    /// price next to the estimate and the period total at the typed rate;
    /// without either, nothing is added.
    #[test]
    fn details_show_the_plan_line_and_the_amount_at_the_typed_rate() {
        use cuotai_core::cost::{
            plan_values, summarize, DailyUsage, DisplayCurrency, Period, PlanInput, PlanSetting,
            Prices, SourceId, Span, SummaryInput, Tokens,
        };
        let account = Account {
            id: None,
            name: "max".into(),
            provider: Provider::Claude,
            ..Account::default()
        };
        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let usage = [DailyUsage {
            source: SourceId::ClaudeCode,
            day: today,
            model: "claude-opus-5-5".into(),
            tokens: Tokens {
                input: 1_000_000,
                ..Tokens::default()
            },
            requests: 3,
            sessions: 1,
        }];
        let prices = Prices::embedded();
        let enabled = [SourceId::ClaudeCode];
        let summary = summarize(&SummaryInput {
            today,
            span: Span::Period(Period::Last7Days),
            enabled: &enabled,
            usage: &usage,
            prices: &prices,
            pricing_error: None,
            last: None,
        });
        let plans = std::collections::BTreeMap::from([(
            account.state_key(),
            PlanSetting::new(20.0, "USD", None).unwrap(),
        )]);
        let display = DisplayCurrency::new("BRL", 5.0, today).unwrap();
        let accounts = [account.clone()];
        let values = plan_values(&PlanInput {
            today,
            plans: &plans,
            accounts: &accounts,
            enabled: &enabled,
            usage: &usage,
            prices: &prices,
            display: Some(&display),
        });
        assert_eq!(values.len(), 1);
        let panel = crate::cost::CostPanel {
            summary,
            plans: values,
            display: Some(display),
        };
        let tool = panel.summary.for_provider(Provider::Claude).unwrap();
        let shown: String = local_cost_lines(&panel, tool)
            .iter()
            .map(|line| line.to_string() + "\n")
            .collect();
        assert!(shown.contains("BRL 20.00"), "{shown}");
        assert!(shown.contains("no seu câmbio"), "{shown}");
        assert!(shown.contains("plano USD 20.00"), "{shown}");
        let plain = crate::cost::CostPanel::of(panel.summary.clone());
        let plain: String = local_cost_lines(&plain, tool)
            .iter()
            .map(|line| line.to_string() + "\n")
            .collect();
        assert!(
            !plain.contains("plano") && !plain.contains("câmbio"),
            "{plain}"
        );
    }

    // SES-01: a rejected vault session puts "Paste a new session" in the
    // footer of its card and of its details; any other state does not.
    #[test]
    fn a_rejected_session_offers_a_new_paste_on_its_card_and_details() {
        let account = Account {
            id: None,
            name: "MiMo".into(),
            provider: Provider::XiaomiMimo,
            ..Default::default()
        };
        let rejected = |id: &'static str| {
            let mut report = report(Status::AuthenticationError, Vec::new());
            report.provider = Provider::XiaomiMimo;
            report.notes_l10n = vec![cuotai_core::text::Localized::fixed(id, "x")];
            report.notes = vec!["x".into()];
            report
        };
        let footer = |report: Report, details: bool| {
            render_view(
                std::slice::from_ref(&account),
                &[Some(report)],
                &[],
                &[],
                (details, 0),
                Utc::now(),
                (260, 40),
                None,
                &[],
                None,
            )
            .join("\n")
        };
        for details in [false, true] {
            let text = footer(rejected("user_session.rejected"), details);
            assert!(text.contains("p colar nova sessão"), "{text}");
            let text = footer(rejected("query.transport"), details);
            assert!(!text.contains("colar nova sessão"), "{text}");
        }
        let mut ready = rejected("user_session.rejected");
        ready.status = Status::Ready;
        assert!(!footer(ready, false).contains("colar nova sessão"));
    }

    /// Fifty accounts with five metrics and two notes each: the synthetic
    /// load the performance review measures frames against.
    fn fifty_accounts() -> (Vec<Account>, Vec<Option<Report>>) {
        let providers = [
            Provider::OpenAiApi,
            Provider::AnthropicApi,
            Provider::Cursor,
            Provider::Copilot,
            Provider::Groq,
        ];
        let accounts: Vec<Account> = (0..50)
            .map(|i| Account {
                id: None,
                name: format!("Conta de teste número {i:02} — ação"),
                provider: providers[i % providers.len()],
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
                group: None,
            })
            .collect();
        let reports = accounts
            .iter()
            .map(|account| {
                let mut r = report(
                    Status::Ready,
                    (0..5)
                        .map(|m| {
                            let mut metric = metric(
                                &format!("quota {m}"),
                                Some(f64::from(m) * 10.0 + 1.0),
                                Some(100.0),
                                Some(99.0 - f64::from(m) * 10.0),
                            );
                            metric.resets_at =
                                Some(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap());
                            metric
                        })
                        .collect(),
                );
                r.account = account.name.clone();
                r.provider = account.provider;
                r.notes = vec!["first note".into(), "second note with more words".into()];
                Some(r)
            })
            .collect();
        (accounts, reports)
    }

    /// Render `accounts`/`reports` at `width`x`height` and return the rows.
    fn rows_at(
        width: u16,
        height: u16,
        accounts: &[Account],
        reports: &[Option<Report>],
        selected: Option<usize>,
        details: bool,
    ) -> Vec<String> {
        let visible = visible_indices(accounts, reports, "", true);
        let model_counts = vec![None; accounts.len()];
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    View {
                        accounts,
                        reports,
                        model_counts: &model_counts,
                        visible: &visible,
                        selected,
                        details,
                        detail_scroll: 0,
                        show_unmeasured: true,
                        refreshing: false,
                        location: "~/.config/cuotai/config.toml",
                        filter: "",
                        filtering: false,
                        next_refresh: Some(std::time::Duration::from_secs(120)),
                        error: None,
                        anim: None,
                        last_good: &[],
                        recent: &[],
                        refresh_seconds: 300,
                        history: None,
                        banner: None,
                        incidents: &[],
                        local_cost: None,
                        sub_cards: &[],
                        aggregators_collapsed: false,
                        connection_focus: None,
                        access_account: None,
                        now,
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect()
    }

    /// At 80 columns the footer flows onto extra rows at segment boundaries,
    /// so the quit, about and toggle keys stay visible; at 200 columns the
    /// two rows suffice and the cards keep the room.
    #[test]
    fn narrow_footer_flows_whole_keys_onto_extra_rows() {
        let (accounts, reports) = fifty_accounts();
        let rows = rows_at(80, 24, &accounts, &reports, Some(0), false);
        let footer: Vec<&str> = rows.iter().rev().take(4).map(String::as_str).collect();
        let text = footer.join("\n");
        for key in [
            "? sobre",
            "q sair",
            "u ocultar sem dados",
            "50 contas",
            "x extensões",
        ] {
            assert!(text.contains(key), "{key} missing from the footer:\n{text}");
        }
        for row in &footer {
            assert!(
                !row.trim_end().ends_with(" x ext"),
                "row cut mid-segment: {row:?}"
            );
        }
        let wide = rows_at(200, 60, &accounts, &reports, Some(6), false);
        assert!(
            wide[58].contains("q sair"),
            "two rows suffice at 200 columns"
        );
        // The selected card's place among fifty: the only sign of where the
        // list stands once the accounts span several pages.
        assert!(wide[59].contains("7/50 contas"), "{}", wide[59]);
        assert!(wide[57].trim().is_empty() || wide[57].ends_with('┘'));
        let unselected = rows_at(200, 60, &accounts, &reports, None, false);
        assert!(unselected[59].contains(" 50 contas"), "{}", unselected[59]);
    }

    #[test]
    fn textwrap_measures_cells_not_bytes() {
        // Nine characters, fourteen bytes: fits a ten-cell line.
        assert_eq!(textwrap_simple("ação açãos", 10), vec!["ação açãos"]);
        // Fifteen bytes, five characters, ten cells: each half is two
        // double-width characters.
        assert_eq!(textwrap_simple("日本語認証", 4), vec!["日本", "語認", "証"]);
        // An overlong URL breaks instead of being clipped by the widget.
        assert_eq!(
            textwrap_simple("see https://example.com/a/b", 12),
            vec!["see", "https://exam", "ple.com/a/b"]
        );
        assert_eq!(textwrap_simple("", 8), vec![""]);
    }

    #[test]
    fn ellipsize_cuts_on_cells_and_keeps_short_text() {
        assert_eq!(ellipsize("ação", 4), "ação");
        assert_eq!(ellipsize("ação feita", 6), "ação…");
        assert_eq!(ellipsize("日本語", 4), "日…");
        assert_eq!(ellipsize("日本語", 0), "…");
        assert_eq!(
            clipped_lines("認証に失敗しました", 6, 2),
            vec!["認証に", "失敗…"]
        );
    }

    /// Wide (CJK, emoji) account names and notes keep the card borders
    /// aligned at 80 columns: every bordered row still ends in its border.
    #[test]
    fn wide_characters_keep_card_borders_aligned_at_80_columns() {
        let (mut accounts, mut reports) = fifty_accounts();
        accounts.truncate(2);
        reports.truncate(2);
        accounts[0].name = "日本語のアカウント名がとても長い場合のテスト用の名前です".into();
        accounts[1].name = "한국어 계정 🚀 이름".into();
        let failed = report(Status::AuthenticationError, Vec::new());
        let mut failed = failed;
        failed.notes = vec![
            "認証に失敗しました。トークンの有効期限が切れている可能性があります。設定を確認してください。もう一度ログインしてください。".into(),
        ];
        reports[1] = Some(failed);
        let rows = rows_at(80, 24, &accounts, &reports, Some(0), false);
        if std::env::var_os("CUOTAI_BENCH_DUMP").is_some() {
            for row in &rows {
                println!("{row}");
            }
        }
        for row in &rows {
            if row.starts_with('│') {
                assert!(row.ends_with('│'), "right border lost: {row:?}");
            }
            if row.starts_with('┌') || row.starts_with('└') {
                assert!(
                    row.ends_with('┐') || row.ends_with('┘'),
                    "corner lost: {row:?}"
                );
            }
        }
    }

    /// Frame-time benchmark: `cargo test --release -p cuotai-tui
    /// bench_frame_time -- --ignored --nocapture`. Prints the mean time of
    /// `draw` per frame at 80x24 and 200x60, and the cost of cloning the
    /// fifty reports once (what the event loop used to do per frame).
    #[test]
    #[ignore = "benchmark; run by hand with --release and --nocapture"]
    fn bench_frame_time_fifty_accounts() {
        let (accounts, reports) = fifty_accounts();
        let visible = visible_indices(&accounts, &reports, "", true);
        let model_counts = vec![None; accounts.len()];
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        for (width, height) in [(80u16, 24u16), (200, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let frames = 300;
            let started = std::time::Instant::now();
            for _ in 0..frames {
                terminal
                    .draw(|frame| {
                        draw(
                            frame,
                            View {
                                accounts: &accounts,
                                reports: &reports,
                                model_counts: &model_counts,
                                visible: &visible,
                                selected: Some(7),
                                details: false,
                                detail_scroll: 0,
                                show_unmeasured: true,
                                refreshing: false,
                                location: "~/.config/cuotai/config.toml",
                                filter: "",
                                filtering: false,
                                next_refresh: Some(std::time::Duration::from_secs(120)),
                                error: None,
                                anim: None,
                                last_good: &[],
                                recent: &[],
                                refresh_seconds: 300,
                                history: None,
                                banner: None,
                                incidents: &[],
                                local_cost: None,
                                sub_cards: &[],
                                aggregators_collapsed: false,
                                connection_focus: None,
                                access_account: None,
                                now,
                            },
                        );
                    })
                    .unwrap();
            }
            let per_frame = started.elapsed() / frames;
            println!("draw {width}x{height}: {per_frame:?} per frame");
            if std::env::var_os("CUOTAI_BENCH_DUMP").is_some() {
                let buffer = terminal.backend().buffer();
                for row in buffer.content().chunks(usize::from(width)) {
                    println!(
                        "{}",
                        row.iter().map(|cell| cell.symbol()).collect::<String>()
                    );
                }
            }
        }
        let started = std::time::Instant::now();
        let calls = 100_000;
        for _ in 0..calls {
            std::hint::black_box(tr("dashboard.footer.select", "select"));
        }
        println!("tr(): {:?} per call", started.elapsed() / calls);
        let started = std::time::Instant::now();
        let rounds = 1000;
        for _ in 0..rounds {
            let cloned: Vec<Option<Report>> = reports.clone();
            std::hint::black_box(cloned);
        }
        println!("clone of fifty reports: {:?}", started.elapsed() / rounds);
    }
}
