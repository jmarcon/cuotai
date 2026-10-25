//! PRD 0026 RF6: usage statistics, the reset timeline and the event journal
//! as text, for the `S` screen of the dashboard and for the read-only
//! `--stats`, `--timeline` and `--events-log` flags.
//!
//! Everything here reads the history store and the last readings; no
//! provider is queried. The figures come from `cuotai_core::stats`,
//! `cuotai_core::resets::timeline` and the history's journal; this module
//! only words and lays them out.

use crate::l10n::{tr, tr_in, trf_in};
use crate::palette;
use crate::ui::{local_time, safe};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeDelta, Utc};
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use cuotai_core::{
    history::{HistoryError, HistoryStore, JournalEntry},
    resets::{timeline_of_config, Lane, ShareLevel, Timeline, TimelineRange},
    stats::{self, Coverage, StatsRange, StatsSummary},
    text, Config, Report,
};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

const BAR_CELLS: usize = 20;
/// Entries of the journal on the screen and the default of `--events-log`.
const EVENT_LIMIT: usize = 20;
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

const BG: Color = palette::BG;
const FG: Color = palette::FG;
const MUTED: Color = palette::MUTED;
const CYAN: Color = palette::ACCENT;
const YELLOW: Color = palette::WARN;
const RED: Color = palette::BAD;

/// How a row is drawn on the screen (plain on the command line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Normal,
    Heading,
    Muted,
    Bad,
}

pub(crate) type Row = (Tone, String);

/// Everything the three views show, read once.
pub(crate) struct Data {
    pub stats: StatsSummary,
    pub timeline: Timeline,
    pub events: Vec<JournalEntry>,
}

/// Read the statistics, timeline and journal of `config` from `store` and
/// the `reports` already held (the cycle's, or the last recorded ones).
pub(crate) fn load(
    store: &HistoryStore,
    config: &Config,
    reports: &[Option<Report>],
    days: StatsRange,
    range: TimelineRange,
    now: DateTime<Utc>,
) -> Result<Data, HistoryError> {
    let stats = stats::summarize(store, &config.accounts, days, now, &Local)?;
    let timeline = timeline_of(config, reports, range, now);
    let since = now - TimeDelta::days(i64::from(days.days()));
    let events = store.journal(since..=now, &config.accounts, EVENT_LIMIT)?;
    Ok(Data {
        stats,
        timeline,
        events,
    })
}

/// The timeline of the held readings; a reading is stale when it is older
/// than its account's interval allows (PRD 0010).
pub(crate) fn timeline_of(
    config: &Config,
    reports: &[Option<Report>],
    range: TimelineRange,
    now: DateTime<Utc>,
) -> Timeline {
    timeline_of_config(config, reports, range, now)
}

fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

fn bar(share: f64) -> String {
    let filled = ((share.clamp(0.0, 1.0) * BAR_CELLS as f64).round() as usize).min(BAR_CELLS);
    format!("{}{}", "█".repeat(filled), "░".repeat(BAR_CELLS - filled))
}

fn weekday(date: NaiveDate, locale: &str) -> &'static str {
    match date.weekday().num_days_from_monday() {
        0 => tr_in(locale, "stats.weekday.mon", "Mon"),
        1 => tr_in(locale, "stats.weekday.tue", "Tue"),
        2 => tr_in(locale, "stats.weekday.wed", "Wed"),
        3 => tr_in(locale, "stats.weekday.thu", "Thu"),
        4 => tr_in(locale, "stats.weekday.fri", "Fri"),
        5 => tr_in(locale, "stats.weekday.sat", "Sat"),
        _ => tr_in(locale, "stats.weekday.sun", "Sun"),
    }
}

fn day_label(date: NaiveDate, today: bool, locale: &str) -> String {
    let label = format!("{} {}", weekday(date, locale), date.format("%d/%m"));
    if today {
        trf_in(
            locale,
            "stats.day.today",
            "{label} today",
            &[("label", &label)],
        )
    } else {
        label
    }
}

/// Mini bars of the daily peaks of one account; a day without data is a
/// gap (a space), never the lowest bar.
fn sparkline(days: &[stats::DayPeak]) -> String {
    days.iter()
        .map(|day| {
            day.share.map_or(' ', |share| {
                SPARK[((share.clamp(0.0, 1.0) * 7.0).round() as usize).min(7)]
            })
        })
        .collect()
}

/// RF1/RF6.1: the figures of `summary` as rows.
pub(crate) fn stats_rows(summary: &StatsSummary, locale: &str) -> Vec<Row> {
    let mut rows = vec![(
        Tone::Heading,
        trf_in(
            locale,
            "stats.heading",
            "Statistics · last {days} days",
            &[("days", &summary.range.days())],
        ),
    )];
    if summary.coverage == Coverage::NotEnough {
        rows.push((
            Tone::Muted,
            format!("— {}", StatsSummary::not_enough().resolve(locale)),
        ));
    }
    let base = match summary.coverage {
        Coverage::Full => days_with_data(summary.range.days(), locale),
        Coverage::Partial {
            days_with_data: days,
        } => days_with_data(days, locale),
        Coverage::NotEnough => String::new(),
    };
    rows.push((
        Tone::Normal,
        trf_in(
            locale,
            "stats.hits",
            "Limits reached: {hits}",
            &[("hits", &summary.hits)],
        ),
    ));
    rows.push((
        Tone::Normal,
        match (summary.average_peak, summary.coverage) {
            (Some(average), c) if c != Coverage::NotEnough => trf_in(
                locale,
                "stats.average_peak",
                "Average peak of the tightest limit: {peak} ({base})",
                &[("peak", &percent(average)), ("base", &base)],
            ),
            _ => tr_in(
                locale,
                "stats.average_peak.none",
                "Average peak of the tightest limit: —",
            )
            .to_owned(),
        },
    ));
    rows.push((
        Tone::Normal,
        match &summary.extra_spent {
            Some(spent) => trf_in(
                locale,
                "stats.extra_spent",
                "Extra spending: {amounts}",
                &[(
                    "amounts",
                    &spent
                        .iter()
                        .map(|e| {
                            format!("{:.2} {}", e.amount, safe(&crate::ui::unit_text(&e.unit)))
                        })
                        .collect::<Vec<_>>()
                        .join(" · "),
                )],
            ),
            None => tr_in(locale, "stats.extra_spent.none", "Extra spending: —").to_owned(),
        },
    ));
    if summary.unknown_limits > 0 {
        rows.push((
            Tone::Muted,
            trf_in(
                locale,
                "stats.unknown_limits",
                "{count} limit(s) without a known ceiling were left out of the averages.",
                &[("count", &summary.unknown_limits)],
            ),
        ));
    }
    rows.push((Tone::Normal, String::new()));
    rows.push((
        Tone::Heading,
        tr_in(locale, "stats.daily", "Usage peak per day").to_owned(),
    ));
    for day in &summary.daily {
        let label = day_label(day.day, day.is_today, locale);
        let (tone, text) = match day.share {
            Some(share) => (
                if share >= 1.0 {
                    Tone::Bad
                } else {
                    Tone::Normal
                },
                format!("{label:<16} {} {}", bar(share), percent(share)),
            ),
            None => (
                Tone::Muted,
                format!("{label:<16} {}", tr_in(locale, "stats.no_data", "no data")),
            ),
        };
        rows.push((tone, text));
    }
    rows.push((Tone::Normal, String::new()));
    rows.push((
        Tone::Heading,
        tr_in(locale, "stats.per_account", "Per account").to_owned(),
    ));
    if summary.accounts.is_empty() {
        rows.push((
            Tone::Muted,
            tr_in(
                locale,
                "stats.no_accounts",
                "No account in the configuration.",
            )
            .to_owned(),
        ));
    }
    for account in &summary.accounts {
        let average = account.average_peak.map_or("—".to_owned(), percent);
        rows.push((
            if account.hits > 0 {
                Tone::Bad
            } else {
                Tone::Normal
            },
            format!(
                "{:<24} {}",
                safe(&account.name),
                trf_in(
                    locale,
                    "stats.account_line",
                    "average {average} · {hits} limit(s) reached · {spark}",
                    &[
                        ("average", &format!("{average:>4}")),
                        ("hits", &account.hits),
                        ("spark", &sparkline(&account.days)),
                    ],
                )
            ),
        ));
    }
    rows.push((Tone::Normal, String::new()));
    rows.push((Tone::Muted, summary.history_kept().resolve(locale)));
    rows
}

fn lane_text(lane: &Lane, now: DateTime<Utc>, locale: &str) -> String {
    let left = if lane.exhausted {
        tr_in(locale, "stats.lane.exhausted", "exhausted").to_owned()
    } else {
        trf_in(
            locale,
            "stats.lane.remaining",
            "{share} left",
            &[("share", &percent(lane.remaining_share))],
        )
    };
    let stale = if lane.stale {
        tr_in(locale, "stats.lane.stale", " · old reading")
    } else {
        ""
    };
    match lane.returns_at {
        Some(at) => format!(
            "{} {} · {} · {} · {left}{stale}",
            local_time(at),
            text::countdown(at - now).resolve(locale),
            safe(&lane.name),
            safe(&crate::ui::metric_label_text(&lane.label)),
        ),
        None => format!(
            "{} · {} · {} · {left}{stale}",
            unknown_return(locale),
            safe(&lane.name),
            safe(&crate::ui::metric_label_text(&lane.label))
        ),
    }
}

fn unknown_return(locale: &str) -> &'static str {
    tr_in(locale, "stats.unknown_return", "Unknown return")
}

fn days_with_data(days: impl std::fmt::Display, locale: &str) -> String {
    trf_in(
        locale,
        "stats.days_with_data",
        "{days} days with data",
        &[("days", &days)],
    )
}

/// RF2/RF6.1: "19:30 in 1h 35m · Claude Work · 0% left", earliest first.
pub(crate) fn timeline_rows(timeline: &Timeline, locale: &str) -> Vec<Row> {
    let span = match timeline.range {
        TimelineRange::Hours24 => "24 h",
        TimelineRange::Days7 => tr_in(locale, "stats.span.7_days", "7 days"),
    };
    let mut rows = vec![(
        Tone::Heading,
        trf_in(
            locale,
            "stats.timeline.heading",
            "Limits return · next {span}",
            &[("span", &span)],
        ),
    )];
    if timeline.lanes.is_empty() {
        rows.push((
            Tone::Muted,
            tr_in(locale, "stats.timeline.empty", "No return in this period.").to_owned(),
        ));
    }
    for lane in &timeline.lanes {
        let tone = match lane.level {
            ShareLevel::Bad => Tone::Bad,
            _ => Tone::Normal,
        };
        rows.push((tone, lane_text(lane, timeline.now, locale)));
    }
    if !timeline.later.is_empty() {
        rows.push((
            Tone::Heading,
            tr_in(locale, "stats.timeline.later", "Later").to_owned(),
        ));
        for lane in &timeline.later {
            rows.push((Tone::Normal, lane_text(lane, timeline.now, locale)));
        }
    }
    if !timeline.unknown.is_empty() {
        rows.push((Tone::Heading, unknown_return(locale).to_owned()));
        for lane in &timeline.unknown {
            rows.push((Tone::Muted, lane_text(lane, timeline.now, locale)));
        }
    }
    rows
}

/// "Recent events": `HH:MM` or date and time, account, summary.
pub(crate) fn event_rows(events: &[JournalEntry], locale: &str) -> Vec<Row> {
    let mut rows = vec![(
        Tone::Heading,
        tr_in(locale, "stats.events.heading", "Recent events").to_owned(),
    )];
    if events.is_empty() {
        rows.push((
            Tone::Muted,
            tr_in(locale, "stats.events.empty", "No event recorded.").to_owned(),
        ));
    }
    for entry in events {
        let who = match (entry.account.as_deref(), entry.parent.as_deref()) {
            (Some(name), _) => safe(name).into_owned(),
            // PRD 0036 RF7.3: a connection is named by its aggregator.
            (None, Some(parent)) => format!("{} › 9router", safe(parent)),
            (None, None) => cuotai_core::history::removed_account().resolve(locale),
        };
        let tone = match entry.kind {
            cuotai_core::events::EventKind::QuotaExhausted
            | cuotai_core::events::EventKind::AccountBlocked
            | cuotai_core::events::EventKind::RefreshFailed
            | cuotai_core::events::EventKind::AuthRequired => Tone::Bad,
            _ => Tone::Normal,
        };
        rows.push((
            tone,
            format!(
                "{} · {who} · {}",
                local_time(entry.occurred_at),
                safe(&entry.summary.resolve(locale))
            ),
        ));
    }
    rows
}

fn plain(rows: &[Row]) -> String {
    rows.iter().map(|(_, text)| format!("{text}\n")).collect()
}

// ---------- the command-line flags ----------

pub(crate) fn stats_text(summary: &StatsSummary, locale: &str) -> String {
    plain(&stats_rows(summary, locale))
}

pub(crate) fn timeline_text(timeline: &Timeline, locale: &str) -> String {
    plain(&timeline_rows(timeline, locale))
}

pub(crate) fn events_text(events: &[JournalEntry], locale: &str) -> String {
    plain(&event_rows(events, locale))
}

pub(crate) fn events_json(events: &[JournalEntry], locale: &str) -> serde_json::Value {
    serde_json::json!({
        "events": events.iter().map(|entry| serde_json::json!({
            "occurred_at": entry.occurred_at,
            "event": entry.kind.name(),
            "account": entry.account,
            "parent": entry.parent,
            "metric": entry.metric,
            "threshold": entry.threshold,
            "used_ratio": entry.used_ratio,
            "resets_at": entry.resets_at,
            "category": entry.category.map(|category| category.name()),
            "summary": entry.summary.resolve(locale),
        })).collect::<Vec<_>>(),
    })
}

/// `--stats --days`: only 7 or 30 are statistics ranges.
pub(crate) fn stats_range(days: Option<u32>) -> Result<StatsRange, String> {
    match days {
        None | Some(7) => Ok(StatsRange::Days7),
        Some(30) => Ok(StatsRange::Days30),
        Some(_) => Err(tr("stats.flag.days", "--days accepts 7 or 30 with --stats.").to_owned()),
    }
}

/// `--timeline --range`.
pub(crate) fn timeline_range(range: Option<&str>) -> Result<TimelineRange, String> {
    match range {
        None | Some("24h") => Ok(TimelineRange::Hours24),
        Some("7d") => Ok(TimelineRange::Days7),
        Some(_) => Err(tr("stats.flag.range", "--range accepts 24h or 7d.").to_owned()),
    }
}

/// `--events-log --days N`: how far back the journal is read.
pub(crate) fn event_days(days: Option<u32>) -> Result<u32, String> {
    match days {
        None => Ok(7),
        Some(days @ 1..=365) => Ok(days),
        Some(_) => Err(tr(
            "stats.flag.event_days",
            "--days goes from 1 to 365 with --events-log.",
        )
        .to_owned()),
    }
}

/// `--events-log`: the journal of the last `days` days, newest first.
pub(crate) fn read_events(
    store: &HistoryStore,
    config: &Config,
    days: u32,
    now: DateTime<Utc>,
) -> Result<Vec<JournalEntry>, HistoryError> {
    store.journal(
        now - TimeDelta::days(i64::from(days))..=now,
        &config.accounts,
        usize::MAX,
    )
}

// ---------- the `S` screen ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    Stay,
    Close,
}

/// The statistics screen: figures, timeline and recent events, one scroll.
pub(crate) struct StatsView {
    store: HistoryStore,
    config: Config,
    reports: Vec<Option<Report>>,
    days: StatsRange,
    range: TimelineRange,
    data: Result<Data, HistoryError>,
    scroll: u16,
    footer: Vec<(Rect, KeyCode)>,
}

impl StatsView {
    pub(crate) fn open(store: HistoryStore, config: &Config, reports: Vec<Option<Report>>) -> Self {
        let mut view = Self {
            store,
            config: config.clone(),
            reports,
            days: StatsRange::Days7,
            range: TimelineRange::Hours24,
            data: Err(HistoryError::Storage),
            scroll: 0,
            footer: Vec::new(),
        };
        view.reload();
        view
    }

    fn reload(&mut self) {
        self.data = load(
            &self.store,
            &self.config,
            &self.reports,
            self.days,
            self.range,
            Utc::now(),
        );
        self.scroll = 0;
    }

    pub(crate) fn key(&mut self, code: KeyCode) -> Reply {
        match code {
            KeyCode::Esc | KeyCode::Char('q' | 'S') | KeyCode::Enter => return Reply::Close,
            KeyCode::Char('d') => {
                self.days = match self.days {
                    StatsRange::Days7 => StatsRange::Days30,
                    StatsRange::Days30 => StatsRange::Days7,
                };
                self.reload();
            }
            KeyCode::Char('t') => {
                self.range = match self.range {
                    TimelineRange::Hours24 => TimelineRange::Days7,
                    TimelineRange::Days7 => TimelineRange::Hours24,
                };
                self.reload();
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = u16::MAX,
            _ => {}
        }
        Reply::Stay
    }

    pub(crate) fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> Reply {
        match kind {
            MouseEventKind::ScrollDown => self.key(KeyCode::Down),
            MouseEventKind::ScrollUp => self.key(KeyCode::Up),
            MouseEventKind::Down(MouseButton::Right) => Reply::Close,
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = self
                    .footer
                    .iter()
                    .find(|(rect, _)| rect.contains(Position::new(column, row)))
                    .map(|(_, code)| *code);
                hit.map_or(Reply::Stay, |code| self.key(code))
            }
            _ => Reply::Stay,
        }
    }

    fn lines(&self, locale: &str) -> Vec<Line<'static>> {
        let style = |tone| match tone {
            Tone::Normal => Style::default().fg(FG),
            Tone::Heading => Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            Tone::Muted => Style::default().fg(MUTED),
            Tone::Bad => Style::default().fg(RED),
        };
        let rows = match &self.data {
            Ok(data) => {
                let mut rows = stats_rows(&data.stats, locale);
                rows.push((Tone::Normal, String::new()));
                rows.extend(timeline_rows(&data.timeline, locale));
                rows.push((Tone::Normal, String::new()));
                rows.extend(event_rows(&data.events, locale));
                rows
            }
            Err(error) => vec![(Tone::Bad, error.l10n().resolve(locale))],
        };
        rows.into_iter()
            .map(|(tone, text)| Line::styled(text, style(tone)))
            .collect()
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame, locale: &str) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
        let areas = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CYAN))
            .title(tr_in(locale, "stats.title", " STATISTICS "));
        let inner = block.inner(areas[0]);
        let body = Paragraph::new(self.lines(locale)).wrap(Wrap { trim: false });
        let total = body.line_count(inner.width);
        let max = total.saturating_sub(usize::from(inner.height));
        self.scroll = self.scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
        frame.render_widget(body.block(block).scroll((self.scroll, 0)), areas[0]);

        self.footer.clear();
        let mut spans = Vec::new();
        let mut x = areas[1].x;
        let days = match self.days {
            StatsRange::Days7 => tr_in(locale, "stats.footer.30_days", "d 30 days"),
            StatsRange::Days30 => tr_in(locale, "stats.footer.7_days", "d 7 days"),
        };
        let range = match self.range {
            TimelineRange::Hours24 => tr_in(locale, "stats.footer.t_7_days", "t 7 days"),
            TimelineRange::Days7 => "t 24 h",
        };
        for (label, code) in [
            (
                tr_in(locale, "stats.footer.scroll", " ↑↓ scroll").to_owned(),
                None,
            ),
            (format!("  {days}"), Some(KeyCode::Char('d'))),
            (format!("  {range}"), Some(KeyCode::Char('t'))),
            (
                tr_in(locale, "about.footer.back", "  Esc back").to_owned(),
                Some(KeyCode::Esc),
            ),
        ] {
            let span = Span::styled(label, Style::default().fg(YELLOW));
            let width = span.width() as u16;
            if let Some(code) = code {
                let rect = Rect::new(x, areas[1].y, width, 1).intersection(areas[1]);
                self.footer.push((rect, code));
            }
            x = x.saturating_add(width);
            spans.push(span);
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), areas[1]);
    }
}

#[cfg(test)]
#[path = "stats_view_tests.rs"]
mod tests;
