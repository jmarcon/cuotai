//! PRD 0042 RF2–RF4: the local cost by session, by 5-hour block and by
//! project, and the live block, from the index's per-request rows.
//!
//! Everything here is pure ([`sessions`], [`blocks`], [`projects`],
//! [`live`]); [`Scanner`] feeds it from the index, the preferences and the
//! prices and reads no log. A block is computed, never read from the
//! provider: it starts at the hour of the first request after the previous
//! block ended (as `ccusage` aligns Claude Code's windows) and lasts the
//! source's block length. The provider's own window is attached beside it
//! for display ([`ProviderWindow`]) and never merged. Projections follow
//! PRD 0024's honesty rules: no burn rate from fewer than 3 requests or
//! less than 3 minutes, no projection before 20% of the block has passed.

use super::estimate::{estimate, Estimate, Usd};
use super::index::{DailyUsage, ProjectRow, Request};
use super::prices::{Prices, PricingFile};
use super::view::{compact_tokens, Span, SpanView};
use super::{CostError, DayZone, Scanner, SourceId};
use crate::model::{MetricWindow, Report, Status};
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Duration, DurationRound, Local, TimeDelta, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// RF3.3: the burn rate looks at this many minutes before now.
pub const BURN_WINDOW_MINUTES: i64 = 10;
/// RF3.3: fewer requests than this in the window give no burn rate.
const BURN_MIN_REQUESTS: usize = 3;
/// RF3.3: a window shorter than this (block younger) gives no burn rate.
const BURN_MIN_MINUTES: i64 = 3;
/// RF3.3: no projection before this share of the block has passed.
const PROJECTION_MIN_ELAPSED: f64 = 0.2;
/// The characters of a digest shown and typed (`--cost-label`).
const SHORT: usize = 8;

/// RF5.1: the dimension `cuotai --cost --by` and the desktop tabs show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Breakdown {
    #[default]
    Day,
    Model,
    Session,
    Block,
    Project,
}

impl Breakdown {
    pub const ALL: [Breakdown; 5] = [
        Breakdown::Day,
        Breakdown::Model,
        Breakdown::Session,
        Breakdown::Block,
        Breakdown::Project,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Model => "model",
            Self::Session => "session",
            Self::Block => "block",
            Self::Project => "project",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|by| by.slug() == slug)
    }

    /// The tab's name (`cost.by.<slug>`).
    pub fn label(self) -> Localized {
        let text = match self {
            Self::Day => "Days",
            Self::Model => "Models",
            Self::Session => "Sessions",
            Self::Block => "Blocks",
            Self::Project => "Projects",
        };
        Localized::fixed(format!("cost.by.{}", self.slug()), text)
    }
}

/// `name` is not a breakdown.
pub fn unknown_breakdown(name: &str) -> Localized {
    let known = Breakdown::ALL.map(Breakdown::slug).join(", ");
    Localized::with_params(
        "cost.error.unknown_breakdown",
        params([("name", name.into()), ("known", known.as_str().into())]),
        "Unknown breakdown: {name}. Breakdowns: {known}.",
    )
}

/// The estimate of some requests (grouped by day and model).
fn estimate_of<'a>(requests: impl IntoIterator<Item = &'a Request>, prices: &Prices) -> Estimate {
    let mut rows: BTreeMap<(SourceId, chrono::NaiveDate, &str), DailyUsage> = BTreeMap::new();
    for request in requests {
        let row = rows
            .entry((request.source, request.day, request.model.as_str()))
            .or_insert_with(|| DailyUsage {
                source: request.source,
                day: request.day,
                model: request.model.clone(),
                tokens: Default::default(),
                requests: 0,
                sessions: 0,
            });
        row.tokens.add(&request.tokens);
        row.requests += 1;
    }
    let rows: Vec<DailyUsage> = rows.into_values().collect();
    estimate(&rows, prices)
}

fn short(digest: &str) -> String {
    digest.chars().take(SHORT).collect()
}

fn minutes(delta: TimeDelta) -> u64 {
    u64::try_from(delta.num_minutes()).unwrap_or(0)
}

/// `2h 38m` (or `12m`).
pub fn duration_text(minutes: u64) -> Localized {
    if minutes < 60 {
        return Localized::with_param("cost.duration.minutes", "m", minutes, "{m}m");
    }
    Localized::with_params(
        "cost.duration.hours",
        params([("h", (minutes / 60).into()), ("m", (minutes % 60).into())]),
        "{h}h {m}m",
    )
}

fn local_text(zone: DayZone, at: DateTime<Utc>, format: &str) -> String {
    match zone {
        DayZone::Local => crate::instant::format_local(at, &Local, format),
        DayZone::Fixed(offset) => crate::instant::format_local(at, &offset, format),
    }
}

/// RF2.1: one agent conversation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionSummary {
    pub source: SourceId,
    /// The anonymous session digest (the tool's identifier is never kept).
    pub session: String,
    /// Its first characters, as `--cost-label session` accepts them.
    pub short: String,
    /// RF2.2: the person's label.
    pub label: Option<String>,
    /// "Session · 2026-10-25 14:02–16:40 · 2h 38m", or the label.
    pub name: Localized,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
    pub duration_minutes: u64,
    /// The project fingerprint, `None` for "No project".
    pub project: Option<String>,
    /// Tokens by kind, requests, cost and the models used.
    pub estimate: Estimate,
}

/// RF2.1: the sessions of `requests`, newest activity first.
pub fn sessions(
    requests: &[Request],
    labels: &BTreeMap<String, String>,
    prices: &Prices,
    zone: DayZone,
) -> Vec<SessionSummary> {
    let mut groups: BTreeMap<(SourceId, &str), Vec<&Request>> = BTreeMap::new();
    for request in requests {
        groups
            .entry((request.source, request.session.as_str()))
            .or_default()
            .push(request);
    }
    let mut out: Vec<SessionSummary> = groups
        .into_iter()
        .map(|((source, session), rows)| {
            let first = rows.iter().map(|r| r.at).min().unwrap_or_default();
            let last = rows.iter().map(|r| r.at).max().unwrap_or_default();
            // The project the session spent most requests in.
            let mut by_project: BTreeMap<Option<&str>, usize> = BTreeMap::new();
            for row in &rows {
                *by_project.entry(row.project.as_deref()).or_default() += 1;
            }
            let project = by_project
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .and_then(|(project, _)| project.map(str::to_owned));
            let label = labels.get(session).cloned();
            let duration_minutes = minutes(last - first);
            let name = match &label {
                Some(label) => Localized::with_param(
                    "cost.session.labelled",
                    "label",
                    label.as_str(),
                    "{label}",
                ),
                None => Localized::with_params(
                    "cost.session.name",
                    params([
                        ("start", local_text(zone, first, "%Y-%m-%d %H:%M").into()),
                        ("end", local_text(zone, last, "%H:%M").into()),
                        (
                            "duration",
                            ParamValue::l10n(duration_text(duration_minutes)),
                        ),
                    ]),
                    "Session · {start}–{end} · {duration}",
                ),
            };
            SessionSummary {
                source,
                short: short(session),
                session: session.to_owned(),
                label,
                name,
                first,
                last,
                duration_minutes,
                project,
                estimate: estimate_of(rows.iter().copied(), prices),
            }
        })
        .collect();
    out.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.session.cmp(&b.session)));
    out
}

/// RF3.2: the provider's own window, shown beside a local block and never
/// merged with it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ProviderWindow {
    pub resets_at: DateTime<Utc>,
    /// `None` when the reading has no percentage.
    pub used_pct: Option<f64>,
}

impl ProviderWindow {
    /// The 5-hour window of the first ready account of `source`'s providers
    /// in `reports` that carries a reset time.
    pub fn from_reports(reports: &[Report], source: SourceId) -> Option<Self> {
        reports
            .iter()
            .filter(|r| source.providers().contains(&r.provider) && r.status == Status::Ready)
            .flat_map(|r| r.metrics.iter())
            .filter(|m| m.window == Some(MetricWindow::Window5h))
            .find_map(|m| {
                let used_pct = if m.unit == "%" {
                    m.used
                } else {
                    match (m.used, m.limit) {
                        (Some(used), Some(limit)) if limit > 0.0 => Some(used / limit * 100.0),
                        _ => None,
                    }
                };
                m.resets_at.map(|resets_at| Self {
                    resets_at,
                    used_pct: used_pct.filter(|pct| pct.is_finite()),
                })
            })
    }
}

/// RF3.2: one block of one source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockSummary {
    pub source: SourceId,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// `now` is inside the block.
    pub active: bool,
    pub last_request: DateTime<Utc>,
    pub sessions: u64,
    pub projects: u64,
    pub estimate: Estimate,
    /// The provider's window, when an account of the source reported one;
    /// shown beside, never merged.
    pub provider_window: Option<ProviderWindow>,
}

impl BlockSummary {
    /// "2026-10-25 14:00–19:00" in the computer's time.
    pub fn window_text(&self, zone: DayZone) -> String {
        format!(
            "{}–{}",
            local_text(zone, self.start, "%Y-%m-%d %H:%M"),
            local_text(zone, self.end, "%H:%M")
        )
    }
}

/// RF3.1: the blocks of `requests` (any order, any sources), newest first,
/// with `hours` per source; the block containing `now` is active.
pub fn blocks(
    requests: &[Request],
    hours: &dyn Fn(SourceId) -> u8,
    now: DateTime<Utc>,
    prices: &Prices,
) -> Vec<BlockSummary> {
    let mut out = Vec::new();
    for source in SourceId::ALL {
        let mut rows: Vec<&Request> = requests.iter().filter(|r| r.source == source).collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by_key(|r| r.at);
        let length = Duration::hours(i64::from(hours(source).max(1)));
        let mut current: Vec<&Request> = Vec::new();
        let mut start = DateTime::<Utc>::default();
        let mut close = |start: DateTime<Utc>, rows: &mut Vec<&Request>| {
            if rows.is_empty() {
                return;
            }
            let end = start + length;
            out.push(BlockSummary {
                source,
                start,
                end,
                active: start <= now && now < end,
                last_request: rows.last().map(|r| r.at).unwrap_or(start),
                sessions: rows
                    .iter()
                    .map(|r| r.session.as_str())
                    .collect::<BTreeSet<_>>()
                    .len() as u64,
                projects: rows
                    .iter()
                    .filter_map(|r| r.project.as_deref())
                    .collect::<BTreeSet<_>>()
                    .len() as u64,
                estimate: estimate_of(rows.iter().copied(), prices),
                provider_window: None,
            });
            rows.clear();
        };
        for row in rows {
            if current.is_empty() || row.at >= start + length {
                close(start, &mut current);
                start = row.at.duration_trunc(TimeDelta::hours(1)).unwrap_or(row.at);
            }
            current.push(row);
        }
        close(start, &mut current);
    }
    out.sort_by(|a, b| b.start.cmp(&a.start).then(a.source.cmp(&b.source)));
    out
}

/// RF3.3: the active block of one source with its rate and projection.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveBlock {
    pub block: BlockSummary,
    pub elapsed_minutes: u64,
    pub left_minutes: u64,
    /// Tokens per minute over the last [`BURN_WINDOW_MINUTES`]; `None`
    /// under 3 requests or 3 minutes.
    pub burn_rate_tokens_per_min: Option<f64>,
    /// The block's cost at the current rate by its end; `None` without a
    /// burn rate, before 20% of the block, or when nothing is priced.
    pub projected_block_cost: Option<Usd>,
    /// "This block ≈ $4.10 · 38K tok/min"; `None` without a burn rate.
    pub line: Option<Localized>,
    /// "At this rate ≈ $6.40 by 18:53"; `None` without a projection.
    pub projection: Option<Localized>,
}

/// RF3.3: `block` (the active one) at `now`, from its `requests`.
pub fn live(
    block: &BlockSummary,
    requests: &[Request],
    now: DateTime<Utc>,
    prices: &Prices,
    zone: DayZone,
) -> LiveBlock {
    let elapsed = (now - block.start).max(TimeDelta::zero());
    let left = (block.end - now).max(TimeDelta::zero());
    let window_start = block
        .start
        .max(now - Duration::minutes(BURN_WINDOW_MINUTES));
    let recent: Vec<&Request> = requests
        .iter()
        .filter(|r| r.source == block.source && r.at >= window_start && r.at <= now)
        .collect();
    let window_minutes = (now - window_start).num_seconds() as f64 / 60.0;
    let measured = recent.len() >= BURN_MIN_REQUESTS
        && (now - window_start) >= Duration::minutes(BURN_MIN_MINUTES);
    let burn = measured.then(|| {
        let tokens: u64 = recent.iter().map(|r| r.tokens.total()).sum();
        tokens as f64 / window_minutes
    });
    let length = (block.end - block.start).num_seconds().max(1) as f64;
    let share = elapsed.num_seconds() as f64 / length;
    let projected = match (burn, block.estimate.total) {
        (Some(_), Some(so_far)) if share >= PROJECTION_MIN_ELAPSED => {
            let recent_cost = estimate_of(recent.iter().copied(), prices).total;
            recent_cost.map(|cost| {
                let per_minute = cost.micros() as f64 / window_minutes;
                let more = (per_minute * left.num_seconds() as f64 / 60.0).round();
                so_far + Usd::from_micros(more.max(0.0) as u64)
            })
        }
        _ => None,
    };
    let rate_text = burn.map(|rate| compact_tokens(rate.round() as u64));
    let line = rate_text.as_ref().map(|rate| match block.estimate.total {
        Some(cost) => Localized::with_params(
            "cost.live.line",
            params([
                ("cost", cost.to_string().into()),
                ("rate", rate.as_str().into()),
            ]),
            "This block ≈ {cost} · {rate} tok/min",
        ),
        None => Localized::with_params(
            "cost.live.line_unknown",
            params([
                (
                    "tokens",
                    compact_tokens(block.estimate.tokens.total()).into(),
                ),
                ("rate", rate.as_str().into()),
            ]),
            "This block: {tokens} tokens, unknown cost · {rate} tok/min",
        ),
    });
    let projection = projected.map(|cost| {
        Localized::with_params(
            "cost.live.projection",
            params([
                ("cost", cost.to_string().into()),
                ("at", local_text(zone, block.end, "%H:%M").into()),
            ]),
            "At this rate ≈ {cost} by {at}",
        )
    });
    LiveBlock {
        block: block.clone(),
        elapsed_minutes: minutes(elapsed),
        left_minutes: minutes(left),
        burn_rate_tokens_per_min: burn,
        projected_block_cost: projected,
        line,
        projection,
    }
}

/// RF4.1: one project over a span.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectSummary {
    /// The fingerprint; `None` for the requests without a project.
    pub project: Option<String>,
    pub short: Option<String>,
    pub label: Option<String>,
    /// RF4.3: the last folder name, only while the person keeps them.
    pub folder_hint: Option<String>,
    /// The label, "Project ab12cd34" or "No project".
    pub name: Localized,
    pub hidden: bool,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
    pub sessions: u64,
    pub estimate: Estimate,
}

/// The name of a project: its label or the digest prefix.
pub fn project_name(project: Option<&str>, label: Option<&str>) -> Localized {
    match (project, label) {
        (_, Some(label)) => {
            Localized::with_param("cost.project.labelled", "label", label, "{label}")
        }
        (Some(project), None) => Localized::with_param(
            "cost.project.name",
            "short",
            short(project).as_str(),
            "Project {short}",
        ),
        (None, None) => Localized::fixed("cost.project.none", "No project"),
    }
}

/// RF4.1: the projects of `requests`, costliest first (hidden ones
/// included and flagged).
pub fn projects(
    requests: &[Request],
    known: &[ProjectRow],
    prices: &Prices,
) -> Vec<ProjectSummary> {
    let known: BTreeMap<&str, &ProjectRow> =
        known.iter().map(|p| (p.project.as_str(), p)).collect();
    let mut groups: BTreeMap<Option<&str>, Vec<&Request>> = BTreeMap::new();
    for request in requests {
        groups
            .entry(request.project.as_deref())
            .or_default()
            .push(request);
    }
    let mut out: Vec<ProjectSummary> = groups
        .into_iter()
        .map(|(project, rows)| {
            let row = project.and_then(|p| known.get(p).copied());
            let label = row.and_then(|r| r.label.clone());
            ProjectSummary {
                project: project.map(str::to_owned),
                short: project.map(short),
                name: project_name(project, label.as_deref()),
                label,
                folder_hint: row.and_then(|r| r.folder_hint.clone()),
                hidden: row.is_some_and(|r| r.hidden),
                first: rows.iter().map(|r| r.at).min().unwrap_or_default(),
                last: rows.iter().map(|r| r.at).max().unwrap_or_default(),
                sessions: rows
                    .iter()
                    .map(|r| (r.source, r.session.as_str()))
                    .collect::<BTreeSet<_>>()
                    .len() as u64,
                estimate: estimate_of(rows.iter().copied(), prices),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.estimate
            .total
            .cmp(&a.estimate.total)
            .then(b.estimate.tokens.total().cmp(&a.estimate.tokens.total()))
            .then(a.project.cmp(&b.project))
    });
    out
}

/// RF2/RF5: one page of sessions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionPage {
    pub notice: Localized,
    pub span: SpanView,
    /// Every session of the span (not only the page).
    pub count: usize,
    pub total: Estimate,
    pub sessions: Vec<SessionSummary>,
}

/// RF3/RF5: the blocks of a span.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockPage {
    pub notice: Localized,
    pub span: SpanView,
    pub total: Estimate,
    pub blocks: Vec<BlockSummary>,
}

/// RF4/RF5: the projects of a span.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectPage {
    pub notice: Localized,
    pub span: SpanView,
    /// Over the projects that are not hidden.
    pub total: Estimate,
    pub hidden: usize,
    pub projects: Vec<ProjectSummary>,
}

impl Scanner {
    /// The span's bounds at `now` and the requests of the sources that are
    /// on, from `extra` before its first day.
    fn span_requests(
        &self,
        source: Option<SourceId>,
        span: Span,
        now: DateTime<Utc>,
        extra: TimeDelta,
    ) -> Result<(SpanView, Vec<Request>, DateTime<Utc>), CostError> {
        let settings = self.settings().load()?;
        let today = self.zone().day(now);
        let first = span.first_day(today);
        let from = match first {
            Some(day) => self.zone().start_of(day),
            None => DateTime::<Utc>::default(),
        };
        let view = SpanView {
            period: match span {
                Span::Period(period) => Some(period),
                Span::Days(_) => None,
            },
            days: match span {
                Span::Days(days) => Some(days),
                Span::Period(_) => None,
            },
            label: span.label(),
            first_day: first,
            last_day: today,
        };
        let enabled = settings.enabled_sources(self.env());
        if enabled.is_empty() || source.is_some_and(|s| !enabled.contains(&s)) {
            return Ok((view, Vec::new(), from));
        }
        let requests = self
            .index()
            .requests(source, from - extra, now + TimeDelta::seconds(1))?
            .into_iter()
            .filter(|r| enabled.contains(&r.source))
            .collect();
        Ok((view, requests, from))
    }

    /// RF2.1: the sessions active in `span`, newest first; `limit` (`None`:
    /// all) after `offset`.
    pub fn sessions(
        &self,
        pricing: &PricingFile,
        span: Span,
        now: DateTime<Utc>,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<SessionPage, CostError> {
        let (view, requests, _) = self.span_requests(None, span, now, TimeDelta::zero())?;
        let (prices, _) = Prices::with_user_file(pricing);
        let labels = self.index().session_labels()?;
        let all = sessions(&requests, &labels, &prices, self.zone());
        Ok(SessionPage {
            notice: super::estimate_notice(),
            span: view,
            count: all.len(),
            total: estimate_of(&requests, &prices),
            sessions: all
                .into_iter()
                .skip(offset)
                .take(limit.unwrap_or(usize::MAX))
                .collect(),
        })
    }

    /// RF3.1/RF3.2: the blocks that end inside `span` (only `source` when
    /// given), newest first, with the provider windows of `reports`
    /// attached to each source's active block.
    pub fn blocks(
        &self,
        pricing: &PricingFile,
        source: Option<SourceId>,
        span: Span,
        now: DateTime<Utc>,
        reports: &[Report],
    ) -> Result<BlockPage, CostError> {
        let settings = self.settings().load()?;
        let longest = TimeDelta::hours(i64::from(super::BLOCK_HOURS_MAX));
        let (view, requests, from) = self.span_requests(source, span, now, longest)?;
        let (prices, _) = Prices::with_user_file(pricing);
        let mut all = blocks(&requests, &|s| settings.block_hours(s), now, &prices);
        all.retain(|block| block.end > from);
        attach_windows(&mut all, reports);
        let total = estimate_of(requests.iter().filter(|r| r.at >= from), &prices);
        Ok(BlockPage {
            notice: super::estimate_notice(),
            span: view,
            total,
            blocks: all,
        })
    }

    /// RF4.1: the projects with requests in `span`, costliest first.
    pub fn projects(
        &self,
        pricing: &PricingFile,
        span: Span,
        now: DateTime<Utc>,
    ) -> Result<ProjectPage, CostError> {
        let (view, requests, _) = self.span_requests(None, span, now, TimeDelta::zero())?;
        let (prices, _) = Prices::with_user_file(pricing);
        let known = self.index().projects()?;
        let all = projects(&requests, &known, &prices);
        let hidden: BTreeSet<&str> = known
            .iter()
            .filter(|p| p.hidden)
            .map(|p| p.project.as_str())
            .collect();
        let total = estimate_of(
            requests
                .iter()
                .filter(|r| r.project.as_deref().is_none_or(|p| !hidden.contains(p))),
            &prices,
        );
        Ok(ProjectPage {
            notice: super::estimate_notice(),
            span: view,
            total,
            hidden: all.iter().filter(|p| p.hidden).count(),
            projects: all,
        })
    }

    /// RF3.3: the active block of each source that is on (only `source`
    /// when given) at `now`, with the provider windows of `reports`.
    pub fn live(
        &self,
        pricing: &PricingFile,
        source: Option<SourceId>,
        now: DateTime<Utc>,
        reports: &[Report],
    ) -> Result<Vec<LiveBlock>, CostError> {
        let settings = self.settings().load()?;
        let enabled = settings.enabled_sources(self.env());
        let longest = TimeDelta::hours(i64::from(super::BLOCK_HOURS_MAX));
        let mut out = Vec::new();
        let (prices, _) = Prices::with_user_file(pricing);
        for each in SourceId::ALL {
            if source.is_some_and(|s| s != each) || !enabled.contains(&each) {
                continue;
            }
            // Two block lengths back reach the start of the active block.
            let requests = self.index().requests(
                Some(each),
                now - longest * 2,
                now + TimeDelta::seconds(1),
            )?;
            let mut found = blocks(&requests, &|s| settings.block_hours(s), now, &prices);
            attach_windows(&mut found, reports);
            if let Some(active) = found.iter().find(|b| b.active) {
                out.push(live(active, &requests, now, &prices, self.zone()));
            }
        }
        Ok(out)
    }
}

/// RF3.2: the provider window of each source on its active block.
fn attach_windows(blocks: &mut [BlockSummary], reports: &[Report]) {
    for block in blocks.iter_mut().filter(|b| b.active) {
        block.provider_window = ProviderWindow::from_reports(reports, block.source);
    }
}

/// "Local estimate" beside "provider window" (RF3.2 wording).
pub fn provider_window_text(window: &ProviderWindow, zone: DayZone) -> Localized {
    let at = local_text(zone, window.resets_at, "%H:%M");
    match window.used_pct {
        Some(pct) => Localized::with_params(
            "cost.block.provider_window",
            params([("pct", format!("{pct:.0}").into()), ("at", at.into())]),
            "Provider window: {pct}% used, resets at {at}",
        ),
        None => Localized::with_param(
            "cost.block.provider_window_reset",
            "at",
            at,
            "Provider window: resets at {at}",
        ),
    }
}

/// "Local estimate", the label of a computed block.
pub fn local_estimate_text() -> Localized {
    Localized::fixed("cost.block.local_estimate", "Local estimate")
}

/// RF5.1: the heading of one breakdown over a span: "Sessions (Last 7 days)".
pub fn by_heading(by: Breakdown, span: &SpanView) -> Localized {
    Localized::with_params(
        "cost.by.heading",
        params([
            ("by", ParamValue::l10n(by.label())),
            ("period", ParamValue::l10n(span.label.clone())),
        ]),
        "{by} ({period})",
    )
}

/// RF5.1: "12 more; --all lists every one."
pub fn more_text(count: usize) -> Localized {
    Localized::with_param(
        "cost.by.more",
        "count",
        count as u64,
        "{count} more; --all lists every one.",
    )
}

/// RF5.1: nothing in the span for this breakdown.
pub fn by_empty_text() -> Localized {
    Localized::fixed(
        "cost.by.empty",
        "No usage in the logs read for this period.",
    )
}

/// RF3.2: one block for a list: "Claude Code · 2026-10-25 14:00–19:00 ·
/// active · 2 session(s), 1 project(s)".
pub fn block_line(block: &BlockSummary, zone: DayZone) -> Localized {
    let state = if block.active {
        Localized::fixed("cost.block.active", "active")
    } else {
        Localized::fixed("cost.block.closed", "closed")
    };
    Localized::with_params(
        "cost.block.line",
        params([
            ("source", block.source.label().into()),
            ("window", block.window_text(zone).into()),
            ("state", ParamValue::l10n(state)),
            ("sessions", block.sessions.into()),
            ("projects", block.projects.into()),
        ]),
        "{source} · {window} · {state} · {sessions} session(s), {projects} project(s)",
    )
}

/// RF5.2: the active block's window and time: "Claude Code · local
/// estimate 2026-10-25 14:00–19:00 · 1h 20m elapsed, 3h 40m left".
pub fn live_window_text(live: &LiveBlock, zone: DayZone) -> Localized {
    Localized::with_params(
        "cost.live.window",
        params([
            ("source", live.block.source.label().into()),
            ("window", live.block.window_text(zone).into()),
            (
                "elapsed",
                ParamValue::l10n(duration_text(live.elapsed_minutes)),
            ),
            ("left", ParamValue::l10n(duration_text(live.left_minutes))),
        ]),
        "{source} · local estimate {window} · {elapsed} elapsed, {left} left",
    )
}

/// RF3.3: why there is no burn rate yet.
pub fn live_rate_missing_text() -> Localized {
    Localized::with_params(
        "cost.live.rate_missing",
        params([
            ("requests", (BURN_MIN_REQUESTS as u64).into()),
            ("minutes", (BURN_WINDOW_MINUTES as u64).into()),
        ]),
        "Burn rate: not enough recent requests yet ({requests} in the last {minutes} minutes).",
    )
}

/// RF5.2: `source` has no active block.
pub fn live_none_text(source: SourceId) -> Localized {
    Localized::with_param(
        "cost.live.none",
        "source",
        source.label(),
        "{source}: no active block.",
    )
}

/// "hidden", beside a hidden project.
pub fn hidden_text() -> Localized {
    Localized::fixed("cost.project.hidden", "hidden")
}

/// RF2.2/RF4.2: the label of `digest` was saved (`true`) or removed.
pub fn label_saved_text(digest: &str, saved: bool) -> Localized {
    let short = short(digest);
    if saved {
        Localized::with_param(
            "cost.label.saved",
            "short",
            short.as_str(),
            "Label of {short} saved in the local cost index.",
        )
    } else {
        Localized::with_param(
            "cost.label.removed",
            "short",
            short.as_str(),
            "Label of {short} removed.",
        )
    }
}

/// RF4.1: the project `digest` was hidden (`true`) or shown again.
pub fn hidden_saved_text(digest: &str, hidden: bool) -> Localized {
    let short = short(digest);
    if hidden {
        Localized::with_param(
            "cost.project.hidden_saved",
            "short",
            short.as_str(),
            "Project {short} hidden: listed, but left out of the totals.",
        )
    } else {
        Localized::with_param(
            "cost.project.shown_saved",
            "short",
            short.as_str(),
            "Project {short} counted in the totals again.",
        )
    }
}

/// RF3.1: the block length of `source` was saved.
pub fn block_hours_text(source: SourceId, hours: u8) -> Localized {
    Localized::with_params(
        "cost.block.hours_saved",
        params([
            ("source", source.label().into()),
            ("hours", u64::from(hours).into()),
        ]),
        "{source}: blocks of {hours} hour(s).",
    )
}

/// RF4.3: the folder-name hint of `source` was turned on, or off with
/// `removed` names deleted.
pub fn folder_hint_text(source: SourceId, on: bool, removed: usize) -> Localized {
    if on {
        Localized::with_param(
            "cost.folder_hint.on",
            "source",
            source.label(),
            "{source}: the last folder name of each project is kept on this computer to prefill its label.",
        )
    } else {
        Localized::with_params(
            "cost.folder_hint.off",
            params([
                ("source", source.label().into()),
                ("removed", removed.into()),
            ]),
            "{source}: folder names are no longer kept; {removed} deleted.",
        )
    }
}

#[cfg(test)]
#[path = "breakdown_tests.rs"]
mod tests;
