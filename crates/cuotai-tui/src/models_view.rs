//! Models section (PRD 0002 RF5): catalog state of one configured account,
//! local search and manual sync. Opened from account details or the wizard;
//! model tasks run in background and never touch quota refresh.

use crate::palette;
use crate::ui::{resolve_l10n, safe};
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use cuotai_core::{
    models::{self, ModelCatalog, ModelError, ModelInfo, ModelState},
    text::{status_l10n, Localized},
    Account, Monitor, Status,
};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::sync::Arc;
use tokio::task::JoinHandle;

const BG: Color = palette::BG;
const FG: Color = palette::FG;
const MUTED: Color = palette::MUTED;
const CYAN: Color = palette::ACCENT;
const GREEN: Color = palette::GOOD;
const YELLOW: Color = palette::WARN;
const RED: Color = palette::BAD;

type Outcome = Result<ModelState, ModelError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    Stay,
    Close,
    Sync,
}

pub(crate) struct ModelsView {
    index: usize,
    /// Identity the view was opened for; results for any other account at
    /// `index` (after edit/remove/move/reload) are discarded.
    account: Account,
    state: Outcome,
    query: String,
    searching: bool,
    scroll: u16,
    job: Option<JoinHandle<Outcome>>,
    footer: Vec<(Rect, KeyCode)>,
}

/// Only an account with nothing recorded gets the automatic first query.
fn first_load(state: &Outcome) -> bool {
    matches!(state, Ok(ModelState::NotFetched))
}

fn catalog(state: &Outcome) -> Option<&ModelCatalog> {
    match state {
        Ok(ModelState::Ready(catalog)) => Some(catalog),
        Ok(
            ModelState::Failed { previous, .. }
            | ModelState::Fetching { previous, .. }
            | ModelState::Interrupted { previous, .. },
        ) => previous.as_ref(),
        _ => None,
    }
}

/// Case-insensitive local match on id or name; never uses the network.
fn filtered<'a>(models: &'a [ModelInfo], query: &str) -> Vec<&'a ModelInfo> {
    let query = query.to_lowercase();
    models
        .iter()
        .filter(|m| {
            m.id.to_lowercase().contains(&query)
                || m.name
                    .as_ref()
                    .is_some_and(|n| n.to_lowercase().contains(&query))
        })
        .collect()
}

impl ModelsView {
    pub(crate) fn open(monitor: &Arc<Monitor>, index: usize, account: Account) -> Self {
        let mut view = Self::new(index, account, monitor.model_state(index));
        if first_load(&view.state) {
            view.spawn(monitor, true);
        }
        view
    }

    fn new(index: usize, account: Account, state: Outcome) -> Self {
        Self {
            index,
            account,
            state,
            query: String::new(),
            searching: false,
            scroll: 0,
            job: None,
            footer: Vec::new(),
        }
    }

    fn spawn(&mut self, monitor: &Arc<Monitor>, first: bool) {
        let monitor = Arc::clone(monitor);
        let index = self.index;
        self.job = Some(tokio::spawn(async move {
            if first {
                monitor.load_models(index).await
            } else {
                monitor.sync_models(index).await
            }
        }));
    }
    /// Manual sync; the core coalesces with a claim already in flight.
    pub(crate) fn sync(&mut self, monitor: &Arc<Monitor>) {
        if self.job.is_none() {
            self.spawn(monitor, false);
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Applies a finished task. `false` means the account at `index` is no
    /// longer the one this view opened for, so the caller must close it.
    pub(crate) async fn poll(&mut self, accounts: &[Account]) -> bool {
        if self.job.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(Ok(result)) = match self.job.take() {
                Some(job) => Some(job.await),
                None => None,
            } {
                return self.finish(result, accounts);
            }
        }
        self.current(accounts)
    }

    fn current(&self, accounts: &[Account]) -> bool {
        accounts.get(self.index) == Some(&self.account)
    }

    fn finish(&mut self, result: Outcome, accounts: &[Account]) -> bool {
        if !self.current(accounts) {
            return false;
        }
        self.state = result;
        true
    }

    pub(crate) fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Reply {
        if self.searching {
            match code {
                _ if modifiers.contains(KeyModifiers::CONTROL) => {}
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.query.pop();
                    self.scroll = 0;
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.scroll = 0;
                }
                _ => {}
            }
            return Reply::Stay;
        }
        match code {
            KeyCode::Esc => return Reply::Close,
            KeyCode::Char('s') => return Reply::Sync,
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Char('c') => {
                self.query.clear();
                self.scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char(']') => {
                self.scroll = self.scroll.saturating_add(1)
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('[') => {
                self.scroll = self.scroll.saturating_sub(1)
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home => self.scroll = 0,
            // Clamped to the last page by `draw`.
            KeyCode::End => self.scroll = u16::MAX,
            _ => {}
        }
        Reply::Stay
    }

    /// Wheel scrolls, right click goes back, left click on a footer action
    /// is the same as its key (as in the dashboard).
    pub(crate) fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> Reply {
        match kind {
            MouseEventKind::ScrollDown => self.key(KeyCode::Down, KeyModifiers::NONE),
            MouseEventKind::ScrollUp => self.key(KeyCode::Up, KeyModifiers::NONE),
            MouseEventKind::Down(MouseButton::Right) => Reply::Close,
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = self
                    .footer
                    .iter()
                    .find(|(rect, _)| rect.contains(Position::new(column, row)))
                    .map(|(_, code)| *code);
                match hit {
                    Some(code) => {
                        self.searching = false;
                        self.key(code, KeyModifiers::NONE)
                    }
                    None => Reply::Stay,
                }
            }
            _ => Reply::Stay,
        }
    }

    fn info_lines(&self) -> Vec<Line<'static>> {
        let at = |t: &DateTime<Utc>| t.format("%Y-%m-%d %H:%M UTC").to_string();
        let text = |l: &Localized| safe(&resolve_l10n(l)).into_owned();
        let (status, detail, color) = match &self.state {
            _ if self.job.is_some() => ("Consultando modelos…".to_string(), None, YELLOW),
            Ok(ModelState::NotFetched) => (
                "Ainda não consultado".to_string(),
                Some("s Sincronizar modelos consulta a conta.".to_string()),
                MUTED,
            ),
            Ok(ModelState::Ready(_)) => (text(&status_l10n(Status::Ready)), None, GREEN),
            Ok(ModelState::Failed {
                status,
                error,
                at: t,
                ..
            }) => (
                format!("{} · {}", text(&status_l10n(*status)), at(t)),
                Some(text(error)),
                RED,
            ),
            Ok(ModelState::Fetching { note, since, .. }) => {
                (format!("… {}", at(since)), Some(text(note)), YELLOW)
            }
            Ok(ModelState::Interrupted { note, at: t, .. }) => {
                (format!("⚠ {}", at(t)), Some(text(note)), YELLOW)
            }
            Ok(ModelState::Unsupported { reason }) => (
                text(&status_l10n(Status::Unsupported)),
                Some(text(reason)),
                YELLOW,
            ),
            Err(error) => (
                text(&status_l10n(Status::Error)),
                Some(text(&error.l10n())),
                RED,
            ),
        };
        let mut lines = vec![Line::from(vec![
            Span::styled("Status: ", Style::default().fg(MUTED)),
            Span::styled(status, Style::default().fg(color)),
        ])];
        if let Some(detail) = detail {
            lines.push(Line::styled(detail, Style::default().fg(color)));
        }
        let catalog = catalog(&self.state);
        let source = catalog
            .map(|c| c.source.clone())
            .or_else(|| models::support(&self.account).ok());
        if let Some(source) = source {
            lines.push(Line::from(format!("Fonte: {}", text(&source))));
        }
        if let Some(catalog) = catalog {
            lines.push(Line::from(format!(
                "{} de {} modelos · consultado em {}",
                filtered(&catalog.models, &self.query).len(),
                catalog.models.len(),
                at(&catalog.fetched_at)
            )));
        }
        if self.searching || !self.query.is_empty() {
            lines.push(Line::styled(
                format!(
                    "Busca: {}{}",
                    safe(&self.query),
                    if self.searching {
                        " | Enter conclui"
                    } else {
                        ""
                    }
                ),
                Style::default().fg(CYAN),
            ));
        }
        lines
    }

    fn list_lines(&self) -> Vec<Line<'static>> {
        let Some(catalog) = catalog(&self.state) else {
            return vec![Line::styled(
                "Sem catálogo salvo para esta conta.",
                Style::default().fg(MUTED),
            )];
        };
        if catalog.models.is_empty() {
            return vec![Line::styled(
                "A conta não retornou nenhum modelo.",
                Style::default().fg(MUTED),
            )];
        }
        let shown = filtered(&catalog.models, &self.query);
        if shown.is_empty() {
            return vec![Line::styled(
                "Nenhum modelo corresponde à busca.",
                Style::default().fg(MUTED),
            )];
        }
        shown
            .into_iter()
            .map(|m| {
                let mut spans = vec![Span::styled(
                    safe(&m.id).into_owned(),
                    Style::default().fg(FG).add_modifier(Modifier::BOLD),
                )];
                if let Some(name) = m.name.as_deref().filter(|n| *n != m.id) {
                    spans.push(Span::styled(
                        format!("  {}", safe(name)),
                        Style::default().fg(MUTED),
                    ));
                }
                Line::from(spans)
            })
            .collect()
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
        let info = Paragraph::new(self.info_lines()).wrap(Wrap { trim: true });
        let info_height = (info.line_count(area.width.saturating_sub(2)) as u16)
            .saturating_add(2)
            .min(area.height / 2);
        let areas = Layout::vertical([
            Constraint::Length(info_height),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
        let title = format!(
            " MODELOS · {} · {} ",
            safe(&self.account.display_name()),
            self.account.provider.label()
        );
        frame.render_widget(
            info.block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(CYAN))
                    .title(title),
            ),
            areas[0],
        );
        let list = self.list_lines();
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(areas[1]);
        let max = list.len().saturating_sub(usize::from(inner.height));
        self.scroll = self.scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
        frame.render_widget(
            Paragraph::new(list).block(block).scroll((self.scroll, 0)),
            areas[1],
        );
        self.footer.clear();
        let mut spans = Vec::new();
        let mut x = areas[2].x;
        for (label, code) in [
            (" / buscar", Some(KeyCode::Char('/'))),
            ("  c limpar", Some(KeyCode::Char('c'))),
            ("  s Sincronizar modelos", Some(KeyCode::Char('s'))),
            ("  ↑↓/PgUp/PgDn rolar", None),
            ("  Esc voltar", Some(KeyCode::Esc)),
        ] {
            let span = Span::styled(label, Style::default().fg(YELLOW));
            let width = span.width() as u16;
            if let Some(code) = code {
                let rect = Rect::new(x, areas[2].y, width, 1).intersection(areas[2]);
                self.footer.push((rect, code));
            }
            x = x.saturating_add(width);
            spans.push(span);
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), areas[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::Provider;

    fn account(name: &str) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider: Provider::Groq,
            ..Default::default()
        }
    }

    fn ready(models: &[(&str, Option<&str>)]) -> Outcome {
        Ok(ModelState::Ready(ModelCatalog {
            models: models
                .iter()
                .map(|(id, name)| ModelInfo {
                    id: id.to_string(),
                    name: name.map(str::to_string),
                    description: None,
                })
                .collect(),
            source: Localized::fixed("models.source.groq", "groq"),
            fetched_at: Utc::now(),
        }))
    }

    #[test]
    fn search_matches_id_or_name_case_insensitively() {
        let Ok(ModelState::Ready(catalog)) = ready(&[
            ("llama-3", Some("Meta Llama")),
            ("gemma", None),
            ("x1", Some("Whisper")),
        ]) else {
            unreachable!()
        };
        let ids = |q| -> Vec<String> {
            filtered(&catalog.models, q)
                .iter()
                .map(|m| m.id.clone())
                .collect()
        };
        assert_eq!(ids("LLAMA"), ["llama-3"]);
        assert_eq!(ids("whis"), ["x1"]);
        assert_eq!(ids(""), ["llama-3", "gemma", "x1"]);
        assert!(ids("zzz").is_empty());
    }

    #[test]
    fn only_not_fetched_triggers_first_load() {
        assert!(first_load(&Ok(ModelState::NotFetched)));
        assert!(!first_load(&ready(&[])));
        assert!(!first_load(&Ok(ModelState::Unsupported {
            reason: Localized::fixed("models.unsupported", "x"),
        })));
        assert!(!first_load(&Ok(ModelState::Interrupted {
            note: Localized::fixed("models.interrupted", "x"),
            at: Utc::now(),
            previous: None,
        })));
        assert!(!first_load(&Err(ModelError::NoCache)));
    }

    #[test]
    fn result_for_changed_identity_is_discarded() {
        let accounts = vec![account("a"), account("b")];
        let mut view = ModelsView::new(0, accounts[0].clone(), Ok(ModelState::NotFetched));
        assert!(view.finish(ready(&[("m", None)]), &accounts));
        assert!(catalog(&view.state).is_some());

        // Moved/removed: another account now sits at index 0.
        let moved = vec![account("b"), account("a")];
        let mut view = ModelsView::new(0, accounts[0].clone(), Ok(ModelState::NotFetched));
        assert!(!view.finish(ready(&[("m", None)]), &moved));
        assert_eq!(view.state, Ok(ModelState::NotFetched));

        // Edited in place (same name, new credential) is another identity.
        let mut edited = accounts.clone();
        edited[0].credential_env = Some("OTHER".into());
        assert!(!view.finish(ready(&[("m", None)]), &edited));
        assert!(!view.finish(ready(&[("m", None)]), &[]));
    }

    #[test]
    fn s_syncs_outside_search_and_types_inside_it() {
        let mut view = ModelsView::new(0, account("a"), ready(&[]));
        assert_eq!(
            view.key(KeyCode::Char('s'), KeyModifiers::NONE),
            Reply::Sync
        );
        assert_eq!(
            view.key(KeyCode::Char('/'), KeyModifiers::NONE),
            Reply::Stay
        );
        for c in "sonnet".chars() {
            assert_eq!(view.key(KeyCode::Char(c), KeyModifiers::NONE), Reply::Stay);
        }
        assert_eq!(view.query, "sonnet");
        // Esc ends the search first; only a second Esc leaves Models.
        assert_eq!(view.key(KeyCode::Esc, KeyModifiers::NONE), Reply::Stay);
        assert_eq!(view.key(KeyCode::Esc, KeyModifiers::NONE), Reply::Close);
        assert_eq!(
            view.key(KeyCode::Char('c'), KeyModifiers::NONE),
            Reply::Stay
        );
        assert!(view.query.is_empty());
    }
}
