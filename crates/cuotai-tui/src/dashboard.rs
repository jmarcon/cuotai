//! Cards-style quota dashboard.
//!
//! Behavioral classification of a report (`Available`/`Exhausted`/`Unmeasured`)
//! and per-metric rendering state live in `cuotai_core::metrics` so other
//! consumers (CLI, web) can reuse the same semantics. This module only owns
//! colors, labels, layout, and the unmeasured toggle. No credentials are read
//! or stored here.

use crate::ui::{metric_text, resolve_l10n, safe};
use chrono::{DateTime, Utc};
use cuotai_core::metrics::{classify_metric, classify_report, AccountClass, MetricState};
#[cfg(test)]
use cuotai_core::Snapshot;
use cuotai_core::{Account, Metric, Report, Status};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};
use std::time::Duration;

const MIN_CARD_HEIGHT: u16 = 8;
const NARROW_WIDTH: u16 = 45;
const NARROW_HEIGHT: u16 = 18;
const WIDE_COLUMNS_WIDTH: u16 = 110;
const HEADER_HEIGHT: u16 = 4;
const FOOTER_HEIGHT: u16 = 3;
const METRICS_VISIBLE_IN_CARD: usize = 2;

// Dracula palette: background, foreground, selection, muted, and status accents.
const BG: Color = Color::Rgb(40, 42, 54);
const FG: Color = Color::Rgb(248, 248, 242);
const MUTED: Color = Color::Rgb(98, 114, 164);
const PURPLE: Color = Color::Rgb(189, 147, 249);
const CYAN: Color = Color::Rgb(139, 233, 253);
const GREEN: Color = Color::Rgb(80, 250, 123);
const YELLOW: Color = Color::Rgb(241, 250, 140);
const RED: Color = Color::Rgb(255, 85, 85);
const PINK: Color = Color::Rgb(255, 121, 198);

/// Localized label for an account class. Behavioral classification lives in
/// `cuotai_core::metrics`; this function only owns the Portuguese labels the
/// dashboard prints.
pub(crate) fn class_label(class: AccountClass) -> &'static str {
    match class {
        AccountClass::Available => "Disponível",
        AccountClass::Exhausted => "Esgotado",
        AccountClass::Unmeasured => "Sem dados",
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
    let needle = filter.to_lowercase();
    accounts
        .iter()
        .enumerate()
        .filter(|(_, account)| {
            needle.is_empty()
                || account.name.to_lowercase().contains(&needle)
                || account.provider.label().to_lowercase().contains(&needle)
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
            } else if ratio >= 0.5 {
                Some(GREEN)
            } else if ratio >= 0.2 {
                Some(YELLOW)
            } else {
                Some(RED)
            }
        }
    }
}

/// Build a short, locale-neutral countdown ("3d 4h", "5h 02m", "12m 03s",
/// "47s"). Once `resets_at` is at or before now, return the renewal hint
/// without inventing a cycle.
fn reset_text(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> &'static str {
    if resets_at <= now {
        return "renova agora";
    }
    "renova em breve"
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
        Span::styled("renova ", Style::default().fg(MUTED)),
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
}

fn metric_row(metric: &Metric) -> MetricRow {
    MetricRow {
        label: metric.label.clone(),
        text: metric_text(metric),
        state: classify_metric(metric),
        period: metric.period.clone(),
        resets_at: metric.resets_at,
    }
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
    MoveUp,
    MoveDown,
    Delete,
    ToggleUnmeasured,
    Filter,
    Models,
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
            Paragraph::new("Cuotai: amplie o terminal para pelo menos 45 × 18. q sai.")
                .wrap(Wrap { trim: true }),
            area,
        );
        return targets;
    }
    let areas = Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(FOOTER_HEIGHT),
    ])
    .split(area);
    render_header(frame, areas[0], &view);
    if view.details {
        render_detail(frame, areas[1], &view, &mut targets);
        targets.detail = Some(areas[1]);
    } else {
        render_cards(frame, areas[1], &view, &mut targets);
    }
    render_footer(frame, areas[2], &view, &mut targets);
    targets
}

fn render_header(frame: &mut Frame, area: Rect, view: &View<'_>) {
    // The dashboard's header tracks the most recent fully-assembled
    // snapshot. When one or more slots are still pending (no fresh data
    // for that account yet) we surface that explicitly so the user is
    // never misled into thinking every account was just consulted.
    let any_report = view.reports.iter().any(Option::is_some);
    let all_reports = view.reports.iter().all(Option::is_some);
    let checked = match (any_report, all_reports) {
        (false, _) => "ainda não consultado".to_string(),
        (true, false) => "atualização parcial (algumas contas pendentes)".to_string(),
        (true, true) => view
            .reports
            .iter()
            .filter_map(Option::as_ref)
            .map(|r| r.checked_at.format("%H:%M:%S UTC").to_string())
            .next_back()
            .unwrap_or_else(|| "—".into()),
    };
    let refresh_state = if view.refreshing {
        "Consultando (dados anteriores abaixo)".to_string()
    } else {
        match view.next_refresh {
            Some(remaining) => {
                let seconds = remaining.as_secs();
                format!("Atualização em {}s", seconds)
            }
            None => "Atualização: —".to_string(),
        }
    };
    let title = format!(
        " CUOTAI  |  {} contas  |  {}  |  {} ",
        view.accounts.len(),
        checked,
        refresh_state,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(CYAN))
        .title_style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD));
    let mut lines: Vec<Line> = Vec::new();
    lines.extend(view.location.lines().map(|part| Line::from(safe(part))));
    if view.filtering || !view.filter.is_empty() {
        lines.push(Line::from(format!(
            "Filtro: {}{}",
            safe(view.filter),
            if view.filtering {
                " | Enter conclui"
            } else {
                ""
            }
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
        "u ocultar sem dados"
    } else {
        "u mostrar sem dados"
    };
    let count = if view.visible.is_empty() {
        "0 contas".to_string()
    } else if view.visible.len() == 1 {
        "1 conta".to_string()
    } else {
        format!("{} contas", view.visible.len())
    };
    let navigation = vec![
        FooterSegment {
            text: " ↑↓/jk PgUp/PgDn ".into(),
            style: Style::default().fg(CYAN),
            action: None,
        },
        plain("selecionar", None),
        plain("  ", None),
        plain("/ filtrar", Some(FooterAction::Filter)),
        plain("  ", None),
        plain("c limpar", Some(FooterAction::ClearFilter)),
        plain("  ", None),
        plain("r todas", Some(FooterAction::Refresh)),
        plain(" · ", None),
        plain("R selecionada", Some(FooterAction::RefreshSelected)),
        plain("  ", None),
        plain("q sair", Some(FooterAction::Quit)),
    ];
    let separator = || FooterSegment {
        text: " • ".into(),
        style: Style::default(),
        action: None,
    };
    let mut actions = Vec::new();
    if view.details {
        actions.push(plain("←→ contas", None));
        actions.push(separator());
        actions.push(plain("↑↓/[ ] rolar", None));
        actions.push(separator());
        actions.push(plain("Enter/Esc voltar", Some(FooterAction::CloseDetails)));
        actions.push(separator());
        actions.push(plain("m modelos", Some(FooterAction::Models)));
    } else {
        actions.push(plain("Enter detalhes", Some(FooterAction::OpenDetails)));
    }
    actions.push(separator());
    actions.push(plain("a adicionar", Some(FooterAction::Add)));
    actions.push(separator());
    actions.push(plain("e editar", Some(FooterAction::Edit)));
    actions.push(separator());
    actions.push(plain("K mover ↑", Some(FooterAction::MoveUp)));
    actions.push(separator());
    actions.push(plain("J mover ↓", Some(FooterAction::MoveDown)));
    actions.push(separator());
    actions.push(plain("d/Del excluir", Some(FooterAction::Delete)));
    actions.push(separator());
    actions.push(plain(toggle, Some(FooterAction::ToggleUnmeasured)));
    actions.push(separator());
    actions.push(plain(&count, None));
    [navigation, actions]
}

fn render_footer(frame: &mut Frame, area: Rect, view: &View<'_>, targets: &mut Targets) {
    for (row, segments) in footer_segments(view).into_iter().enumerate() {
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
            "Nenhuma conta. a abre o wizard; c limpa o filtro."
        } else if !view.filter.is_empty() || view.filtering {
            "Sem resultados para o filtro atual. c limpa o filtro."
        } else if !view.show_unmeasured {
            "Todas as contas visíveis estão sem dados. u mostra contas sem dados."
        } else {
            "Nenhuma conta para exibir."
        };
        frame.render_widget(
            Paragraph::new(message)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title(" Contas ")),
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
    let selected_page = view
        .selected
        .map(|position| position / page_capacity)
        .unwrap_or(0);
    let page_start = selected_page * page_capacity;
    let page_end = (page_start + page_capacity).min(view.visible.len());
    let page = &view.visible[page_start..page_end];

    if page.is_empty() {
        return;
    }

    targets.columns = columns;
    let now = Utc::now();
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
            let Some(&account_index) = page.get(global_index) else {
                continue;
            };
            let visible_position = page_start + global_index;
            let is_selected = view.selected == Some(visible_position);
            render_card(frame, *cell_area, view, account_index, is_selected, now);
            targets.cards.push((*cell_area, visible_position));
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
    let rows: Vec<MetricRow> = report
        .map(|report| report.metrics.iter().map(metric_row).collect())
        .unwrap_or_default();
    let class = classification(report);
    let first_note = if rows.is_empty() {
        report.and_then(|report| report.notes.first().cloned())
    } else {
        None
    };
    let hidden_metrics = rows.len().saturating_sub(METRICS_VISIBLE_IN_CARD);
    let hint_rows = if hidden_metrics > 0 { 1 } else { 0 };
    let note_rows = note_row_count(first_note.as_deref(), area.width);

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
    let title = format!(" {} ", safe(&account.name));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title_style(title_style)
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Layout: provider/status/class line, then up to 2 metrics (each 2 rows),
    // optional wrapped note, then a trailing hint when more metrics exist.
    let mut constraints = vec![Constraint::Length(1)];
    let visible_metrics = rows.len().min(METRICS_VISIBLE_IN_CARD);
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
    let provider_status_line = match report {
        Some(report) => Line::from(vec![
            Span::styled(account.provider.label(), Style::default().fg(MUTED)),
            Span::styled("  · ", Style::default().fg(MUTED)),
            Span::styled(
                report.status.label(),
                Style::default().fg(status_color(report.status)),
            ),
            Span::styled("  · ", Style::default().fg(MUTED)),
            Span::styled(
                class_label(class),
                Style::default()
                    .fg(class_color(class))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        None => Line::from(vec![
            Span::styled(account.provider.label(), Style::default().fg(MUTED)),
            Span::styled("  · ", Style::default().fg(MUTED)),
            Span::styled("Aguardando", Style::default().fg(MUTED)),
            Span::styled("  · ", Style::default().fg(MUTED)),
            Span::styled(
                class_label(class),
                Style::default()
                    .fg(class_color(class))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    };
    frame.render_widget(Paragraph::new(provider_status_line), chunks[chunk_index]);
    chunk_index += 1;

    for metric in rows.iter().take(METRICS_VISIBLE_IN_CARD) {
        render_metric_block(frame, &chunks, &mut chunk_index, metric, now);
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
}

fn class_color(class: AccountClass) -> Color {
    match class {
        AccountClass::Available => GREEN,
        AccountClass::Exhausted => RED,
        AccountClass::Unmeasured => MUTED,
    }
}

fn build_hint(hidden_metrics: usize) -> Line<'static> {
    if hidden_metrics > 0 {
        let mut spans = vec![Span::raw(format!(
            "+{hidden_metrics} métric{} • ",
            if hidden_metrics == 1 { "a" } else { "as" }
        ))];
        spans.push(Span::styled(
            "Enter detalhes",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ));
        Line::from(spans)
    } else {
        Line::from(Span::styled(
            "Enter detalhes",
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

fn textwrap_simple(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
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
) {
    let text_area = chunks[*chunk_index];
    let gauge_area = chunks[*chunk_index + 1];
    *chunk_index += 2;

    let mut text_spans = vec![Span::styled(
        format!("{}: ", safe(&metric.label)),
        Style::default().fg(FG).add_modifier(Modifier::BOLD),
    )];
    text_spans.push(Span::raw(safe(&metric.text)));
    if let Some(period) = &metric.period {
        text_spans.push(Span::styled(
            format!("  · {}", safe(period)),
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
                Span::styled("sem dados", Style::default().fg(MUTED)),
            ]);
            frame.render_widget(Paragraph::new(line), gauge_area);
        }
        MetricState::Exhausted { no_cap } => {
            if no_cap {
                let line = Line::from(Span::styled(
                    "saldo esgotado",
                    Style::default().fg(RED).add_modifier(Modifier::BOLD),
                ));
                frame.render_widget(Paragraph::new(line), gauge_area);
            } else {
                render_gauge(frame, gauge_area, 0.0, Some(RED), metric, now);
            }
        }
        MetricState::Available { ratio, uncapped } => {
            if uncapped {
                let line = Line::from(Span::styled(
                    "limite não informado",
                    Style::default().fg(MUTED),
                ));
                frame.render_widget(Paragraph::new(line), gauge_area);
            } else {
                let color = metric_state_color(metric.state).unwrap_or(YELLOW);
                render_gauge(frame, gauge_area, ratio, Some(color), metric, now);
            }
        }
    }
}

fn render_gauge(
    frame: &mut Frame,
    area: Rect,
    ratio: f64,
    color: Option<Color>,
    metric: &MetricRow,
    now: DateTime<Utc>,
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
        MetricState::Exhausted { .. } => "esgotado".to_string(),
        MetricState::Available { uncapped: true, .. } => "limite não informado".to_string(),
        MetricState::Available { ratio, .. } => {
            format!("{:>3.0}%", (ratio * 100.0).round())
        }
    };
    let style = Style::default().fg(bar_color).add_modifier(Modifier::BOLD);
    let gauge = Gauge::default()
        .ratio(ratio.clamp(0.0, 1.0))
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

fn render_detail(frame: &mut Frame, area: Rect, view: &View<'_>, targets: &mut Targets) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PURPLE))
        .title(" Detalhes · ausente ≠ zero ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let Some(selected_position) = view.selected else {
        frame.render_widget(
            Paragraph::new("Nenhuma conta selecionada.")
                .wrap(Wrap { trim: true })
                .scroll((view.detail_scroll, 0)),
            inner,
        );
        return;
    };
    let Some(&account_index) = view.visible.get(selected_position) else {
        frame.render_widget(
            Paragraph::new("Seleção inválida.")
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
                " CONTA  ",
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                safe(&account.name),
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  {}", account.provider.label()),
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
    let now = Utc::now();
    let mut lines: Vec<Line> = Vec::new();
    let Some(report) = view.reports.get(account_index).and_then(Option::as_ref) else {
        lines.push(Line::from(Span::styled(
            "Nenhum dado atualizado. r consulta; e configura esta conta.",
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
            "  STATUS  ",
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            report.status.label(),
            Style::default()
                .fg(status_color(report.status))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "   ·   consultado {}",
                report.checked_at.format("%H:%M:%S UTC")
            ),
            Style::default().fg(MUTED),
        ),
    ]));
    lines.push(Line::default());
    lines.push(Line::styled(
        "  QUOTAS E CONSUMO",
        Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
    ));

    if report.metrics.is_empty() {
        lines.push(Line::from(Span::styled(
            "Provedor sem métricas expostas nesta consulta.",
            Style::default().fg(MUTED),
        )));
    } else {
        for metric in &report.metrics {
            let row = metric_row(metric);
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
            ]));
            lines.push(Line::from(format!("  {}", safe(&row.text))));
            lines.push(line_bar(&row, body.width.max(20), now));
        }
    }

    if !report.notes.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled(
            "  OBSERVAÇÕES",
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
            format!("Fonte: {source}"),
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
        frame.render_widget(
            Paragraph::new(format!("Fonte: {source}")).style(Style::default().fg(CYAN)),
            row,
        );
        if row.width > 7 {
            targets.source = Some((
                Rect {
                    x: row.x + 7,
                    width: (source.len() as u16).min(row.width - 7),
                    ..row
                },
                source.to_owned(),
            ));
        }
    }
}

fn line_bar(metric: &MetricRow, width: u16, now: DateTime<Utc>) -> Line<'static> {
    let ratio_color = metric_state_color(metric.state);
    let reset_label = metric
        .resets_at
        .map(|reset| format!("  {}", format_reset_line(reset, now)))
        .unwrap_or_default();
    let width = usize::from(width)
        .saturating_sub(2 + reset_label.chars().count() + 16)
        .max(1);
    let (bar, suffix) = match metric.state {
        MetricState::Unmeasured => ("—".to_string(), " sem dados".to_string()),
        MetricState::Exhausted { no_cap } => (
            if no_cap {
                String::new()
            } else {
                "░".repeat(width)
            },
            if no_cap {
                " saldo esgotado".to_string()
            } else {
                " esgotado".to_string()
            },
        ),
        MetricState::Available { ratio, uncapped } => {
            if uncapped {
                (String::new(), " limite não informado".to_string())
            } else {
                let filled = ((ratio * width as f64).round() as usize).min(width);
                let bar = "█".repeat(filled) + &"░".repeat(width - filled);
                let suffix = format!(" {:>3.0}% restante", (ratio * 100.0).round());
                (bar, suffix)
            }
        }
    };
    let color = ratio_color.unwrap_or(MUTED);
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
        None => reset_text(reset, now).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{MetricKind, Provider};

    fn metric(
        label: &str,
        used: Option<f64>,
        limit: Option<f64>,
        remaining: Option<f64>,
    ) -> Metric {
        Metric {
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
            account: "acc".into(),
            provider: Provider::OpenAiApi,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    #[test]
    fn missing_report_is_unmeasured() {
        assert_eq!(classification(None), AccountClass::Unmeasured);
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
                name: "alpha".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "beta".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "gamma".into(),
                provider: Provider::Cursor,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
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
                name: "OpenAI Work".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "Anthropic".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
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
                name: "one".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "two".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "three".into(),
                provider: Provider::Cursor,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
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
                name: "alpha".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "beta".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "gamma".into(),
                provider: Provider::Cursor,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "delta".into(),
                provider: Provider::Copilot,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
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
                name: "alpha".into(),
                provider: Provider::OpenAiApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "beta".into(),
                provider: Provider::AnthropicApi,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            },
            Account {
                name: "gamma".into(),
                provider: Provider::Cursor,
                credential_env: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
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
}
