use chrono::Local;
use crossterm::event::KeyCode;
use cuotai_core::extensions::management::{ExtensionItem, ExtensionState, SecretItem};
use cuotai_core::text::Localized;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::ui::{resolve_l10n, safe};

const BG: Color = Color::Rgb(40, 42, 54);
const FG: Color = Color::Rgb(248, 248, 242);
const CYAN: Color = Color::Rgb(139, 233, 253);
const GREEN: Color = Color::Rgb(80, 250, 123);
const YELLOW: Color = Color::Rgb(241, 250, 140);
const RED: Color = Color::Rgb(255, 85, 85);
const MUTED: Color = Color::Rgb(98, 114, 164);

pub(crate) enum Action {
    Stay,
    Close,
    Disclose(String),
    /// The core's opaque candidate for the disclosure just reviewed.
    Approve(cuotai_core::extensions::management::Candidate),
    Disable(String),
    Test(String),
    Remove(usize),
    /// PRD 0028 RF7.4: ask core for the extension's declared secrets.
    Secrets(String),
    /// PRD 0028 RF7.1: use (`on`) or stop using the override extension `id`
    /// for the account at `index`.
    SetOverride {
        id: String,
        index: usize,
        on: bool,
    },
    /// Keep one secret's value in the vault through core.
    SaveSecret {
        id: String,
        name: String,
        value: String,
    },
}

/// PRD 0028 RF7.4: the secret being typed. The value is never drawn, only
/// its length as dots, and is dropped as soon as core has it.
struct SecretPrompt {
    id: String,
    items: Vec<SecretItem>,
    index: usize,
    value: String,
}

pub(crate) struct ExtensionsView {
    items: Vec<ExtensionItem>,
    selected: usize,
    disclosure: Option<Disclosure>,
    /// First shown line of the disclosure; the footer says how to move.
    scroll: u16,
    message: Option<(String, bool)>,
    secret: Option<SecretPrompt>,
    /// PRD 0028 RF7.1: the extension whose override targets are listed.
    overriding: Option<String>,
}

/// The pending disclosure: the lines to review and the core's opaque
/// candidate, which only becomes an approval after an explicit confirmation.
struct Disclosure {
    lines: Vec<Localized>,
    candidate: cuotai_core::extensions::management::Candidate,
}

impl ExtensionsView {
    pub(crate) fn new(items: Vec<ExtensionItem>) -> Self {
        Self {
            items,
            selected: 0,
            disclosure: None,
            scroll: 0,
            message: None,
            secret: None,
            overriding: None,
        }
    }

    /// RF7.4: start asking for the secrets core listed for `id`.
    pub(crate) fn ask_secrets(&mut self, id: String, items: Vec<SecretItem>) {
        self.disclosure = None;
        if items.is_empty() {
            self.message = Some(("Esta extensão não declara segredos.".into(), false));
            return;
        }
        self.message = None;
        self.secret = Some(SecretPrompt {
            id,
            items,
            index: 0,
            value: String::new(),
        });
    }

    /// The secret just saved: move to the next one, or finish.
    pub(crate) fn secret_saved(&mut self, message: String) {
        if let Some(prompt) = &mut self.secret {
            prompt.index += 1;
            prompt.value.clear();
            if prompt.index >= prompt.items.len() {
                self.secret = None;
            }
        }
        self.message = Some((message, false));
    }

    pub(crate) fn replace(&mut self, items: Vec<ExtensionItem>) {
        self.items = items;
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }

    /// The view with `id` selected, for a frontend that just opened it for
    /// one specific extension (editing that account's row).
    pub(crate) fn focused(mut self, id: Option<&str>) -> Self {
        if let Some(id) = id {
            if let Some(position) = self.items.iter().position(|item| item.id == id) {
                self.selected = position;
            }
        }
        self
    }

    pub(crate) fn show_disclosure(
        &mut self,
        lines: Vec<Localized>,
        candidate: cuotai_core::extensions::management::Candidate,
    ) {
        self.message = None;
        self.scroll = 0;
        self.disclosure = Some(Disclosure { lines, candidate });
    }

    pub(crate) fn set_message(&mut self, message: String, error: bool) {
        self.disclosure = None;
        self.secret = None;
        self.message = Some((message, error));
    }

    pub(crate) fn key(&mut self, code: KeyCode) -> Action {
        if let Some(prompt) = &mut self.secret {
            return match code {
                KeyCode::Esc => {
                    self.secret = None;
                    Action::Stay
                }
                KeyCode::Backspace => {
                    prompt.value.pop();
                    Action::Stay
                }
                KeyCode::Enter if !prompt.value.trim().is_empty() => Action::SaveSecret {
                    id: prompt.id.clone(),
                    name: prompt.items[prompt.index].name.clone(),
                    value: std::mem::take(&mut prompt.value),
                },
                KeyCode::Char(c) if !c.is_control() => {
                    prompt.value.push(c);
                    Action::Stay
                }
                _ => Action::Stay,
            };
        }
        if let Some(id) = &self.overriding {
            let id = id.clone();
            return match code {
                KeyCode::Char(c @ '1'..='9') => {
                    let position = c as usize - '1' as usize;
                    let target = self
                        .items
                        .iter()
                        .find(|item| item.id == id)
                        .and_then(|item| item.override_targets.get(position));
                    match target {
                        Some(target) => Action::SetOverride {
                            id,
                            index: target.index,
                            on: !target.active,
                        },
                        None => Action::Stay,
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.overriding = None;
                    Action::Stay
                }
                _ => Action::Stay,
            };
        }
        if let Some(pending) = &self.disclosure {
            return match code {
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.disclosure = None;
                    Action::Stay
                }
                // Scrolling never approves: RF5.6 requires the whole
                // disclosure to be readable, so approval is a separate key.
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll = self.scroll.saturating_add(1);
                    Action::Stay
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll = self.scroll.saturating_sub(1);
                    Action::Stay
                }
                KeyCode::PageDown => {
                    self.scroll = self.scroll.saturating_add(10);
                    Action::Stay
                }
                KeyCode::PageUp => {
                    self.scroll = self.scroll.saturating_sub(10);
                    Action::Stay
                }
                KeyCode::Home => {
                    self.scroll = 0;
                    Action::Stay
                }
                KeyCode::Char('s' | 'S') => {
                    let candidate = pending.candidate.clone();
                    self.disclosure = None;
                    Action::Approve(candidate)
                }
                _ => Action::Stay,
            };
        }
        match code {
            KeyCode::Esc | KeyCode::Char('q') => Action::Close,
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.items.len().saturating_sub(1));
                Action::Stay
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                Action::Stay
            }
            // What the user may do is core's answer (RF5.10): the view only routes it.
            KeyCode::Char('e' | 'E') => self
                .selected_item()
                .filter(|item| item.actions.enable)
                .map(|item| Action::Disclose(item.id.clone()))
                .unwrap_or(Action::Stay),
            KeyCode::Char('d' | 'D') => self
                .selected_item()
                .filter(|item| item.actions.disable)
                .map(|item| Action::Disable(item.id.clone()))
                .unwrap_or(Action::Stay),
            KeyCode::Char('t' | 'T') => self
                .selected_item()
                .filter(|item| item.actions.test)
                .map(|item| Action::Test(item.id.clone()))
                .unwrap_or(Action::Stay),
            // RF7.1: the accounts an enabled override extension can replace.
            KeyCode::Char('o' | 'O') => {
                if let Some(item) = self
                    .selected_item()
                    .filter(|item| !item.override_targets.is_empty())
                {
                    self.overriding = Some(item.id.clone());
                    self.message = None;
                }
                Action::Stay
            }
            // RF7.4: secrets are entered only for an approved extension.
            KeyCode::Char('v' | 'V') => self
                .selected_item()
                .filter(|item| item.actions.test)
                .map(|item| Action::Secrets(item.id.clone()))
                .unwrap_or(Action::Stay),
            KeyCode::Delete | KeyCode::Char('x' | 'X') => self
                .selected_item()
                .filter(|item| item.actions.remove)
                .and_then(|item| item.account.as_ref())
                .map(|account| Action::Remove(account.index))
                .unwrap_or(Action::Stay),
            _ => Action::Stay,
        }
    }

    fn selected_item(&self) -> Option<&ExtensionItem> {
        self.items.get(self.selected)
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        frame.render_widget(
            Block::default().style(Style::default().bg(BG).fg(FG)),
            frame.area(),
        );
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(frame.area());
        frame.render_widget(
            Paragraph::new(
                "Extensões executáveis locais · regras e aprovação fornecidas pelo core",
            )
            .block(Block::default().borders(Borders::ALL).title(" Extensões "))
            .wrap(Wrap { trim: true }),
            areas[0],
        );

        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(areas[1]);
        let inner_height = inner.height;
        let overriding = self
            .overriding
            .as_ref()
            .and_then(|id| self.items.iter().find(|item| &item.id == id));
        let lines = if let Some(item) = overriding {
            let mut lines = vec![
                Line::styled(
                    format!(
                        "Substituição pela extensão {}: escolha as contas",
                        safe(&item.name)
                    ),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Line::styled(
                    "A extensão recebe só as próprias variáveis e segredos, nunca as credenciais da conta.",
                    Style::default().fg(MUTED),
                ),
                Line::default(),
            ];
            for (position, target) in item.override_targets.iter().enumerate().take(9) {
                lines.push(Line::from(format!(
                    "  {}  [{}] {}",
                    position + 1,
                    if target.active { "x" } else { " " },
                    safe(&target.name)
                )));
            }
            lines
        } else if let Some(prompt) = &self.secret {
            let item = &prompt.items[prompt.index];
            vec![
                Line::styled(
                    format!(
                        "Segredo {} de {}: {} (variável {})",
                        prompt.index + 1,
                        prompt.items.len(),
                        safe(&item.name),
                        safe(&item.env)
                    ),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Line::from(safe(&item.description).into_owned()),
                Line::default(),
                Line::from(format!(
                    "Valor: {}",
                    "•".repeat(prompt.value.chars().count())
                )),
                Line::default(),
                Line::styled(
                    if item.stored {
                        "Já há um valor no cofre; Enter o substitui."
                    } else {
                        "O valor fica só no cofre do sistema e só chega ao ambiente da extensão."
                    },
                    Style::default().fg(MUTED),
                ),
            ]
        } else if let Some(pending) = &self.disclosure {
            let mut lines = vec![
                Line::styled(
                    "Revise todos os itens antes de aprovar:",
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Line::default(),
            ];
            for note in &pending.lines {
                lines.push(Line::from(safe(&resolve_l10n(note)).into_owned()));
                lines.push(Line::default());
            }
            lines
        } else if self.items.is_empty() {
            vec![Line::styled(
                "Nenhuma extensão descoberta.",
                Style::default().fg(MUTED),
            )]
        } else {
            let mut lines: Vec<Line> = Vec::new();
            // Visual row of each extension's header, measured after wrapping
            // at the real width, so the viewport stays correct on a narrow
            // terminal (a long name wraps onto several rows).
            let mut rows: Vec<(usize, u16)> = Vec::new();
            let mut visual = 0u16;
            for (index, item) in self.items.iter().enumerate() {
                let selected = index == self.selected;
                let color = match item.state {
                    ExtensionState::Enabled => GREEN,
                    ExtensionState::Changed | ExtensionState::Missing => YELLOW,
                    ExtensionState::Disabled => MUTED,
                    ExtensionState::Invalid => RED,
                };
                rows.push((index, visual));
                // Wrapped height of one line: ratatui's `Line` has no line count, so the
                // same wrap the paragraph uses is measured through it.
                let height = |line: &Line| -> u16 {
                    Paragraph::new(line.clone())
                        .wrap(Wrap { trim: false })
                        .line_count(inner.width)
                        .min(usize::from(u16::MAX)) as u16
                };
                let header = Line::from(vec![
                    Span::styled(
                        if selected { "▶ " } else { "  " },
                        Style::default().fg(CYAN),
                    ),
                    Span::styled(
                        safe(&item.name).into_owned(),
                        Style::default().add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                    ),
                    Span::styled(
                        format!("  [{}]", safe(&item.id)),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        format!("  {}", resolve_l10n(&item.state_label)),
                        Style::default().fg(color),
                    ),
                ]);
                visual = visual.saturating_add(height(&header));
                lines.push(header);
                if selected {
                    let mut detail: Vec<Line> = Vec::new();
                    if let Some(description) = &item.description {
                        detail.push(Line::styled(
                            format!("    {}", safe(description)),
                            Style::default().fg(MUTED),
                        ));
                    }
                    let last = item
                        .last_run
                        .map(|at| {
                            at.with_timezone(&Local)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_else(|| "nunca".into());
                    detail.push(Line::styled(
                        format!("    Última execução: {last}"),
                        Style::default().fg(MUTED),
                    ));
                    if let Some(error) = &item.error {
                        detail.push(Line::styled(
                            format!("    {}", safe(&resolve_l10n(error))),
                            Style::default().fg(RED),
                        ));
                    }
                    for line in detail {
                        visual = visual.saturating_add(height(&line));
                        lines.push(line);
                    }
                }
            }
            // Keep the selected row visible with its detail lines.
            let height = inner_height;
            let max = visual.saturating_sub(height);
            let selected_row = rows
                .iter()
                .find(|(index, _)| *index == self.selected)
                .map(|(_, offset)| *offset)
                .unwrap_or_default();
            self.scroll = selected_row
                .saturating_sub(height.saturating_div(3))
                .min(max);
            lines
        };
        let total = Paragraph::new(lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(inner.width);
        let mut paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block);
        if self.disclosure.is_some() {
            // Wrap-aware: the longest scroll is the line count minus the
            // visible rows, so a wrapped disclosure can reach its end.
            let max = total
                .saturating_sub(usize::from(inner.height))
                .min(usize::from(u16::MAX)) as u16;
            self.scroll = self.scroll.min(max);
        }
        paragraph = paragraph.scroll((self.scroll, 0));
        frame.render_widget(paragraph, areas[1]);
        let footer = if overriding.is_some() {
            "1–9 liga/desliga a substituição da conta · Esc volta"
        } else if self.secret.is_some() {
            "digite o valor (oculto) · Enter guarda no cofre · Backspace apaga · Esc cancela"
        } else if self.disclosure.is_some() {
            "↑↓/jk PgUp/PgDn rolam (sem aprovar) · s aprova e ativa · n/Esc cancela · risco residual permanece"
        } else {
            "↑↓ seleciona · e ativa · d desativa · t testa · v segredos · o substitui · x/Delete remove a conta · Esc volta"
        };
        let mut footer_lines = vec![Line::styled(footer, Style::default().fg(YELLOW))];
        if let Some((message, error)) = &self.message {
            footer_lines.push(Line::styled(
                safe(message).into_owned(),
                Style::default().fg(if *error { RED } else { GREEN }),
            ));
        }
        frame.render_widget(
            Paragraph::new(footer_lines).wrap(Wrap { trim: true }),
            areas[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::extensions::management::{AccountIdentity, ExtensionActions, ExtensionItem};
    use cuotai_core::text::Localized;
    use ratatui::{backend::TestBackend, Terminal};

    /// One row per state core reports, with the actions core allows.
    fn item(state: ExtensionState, account: bool, approved: bool) -> ExtensionItem {
        let name = match state {
            ExtensionState::Enabled => "quota",
            ExtensionState::Disabled => "pendente",
            ExtensionState::Changed => "alterada",
            ExtensionState::Missing => "ausente",
            ExtensionState::Invalid => "invalida",
        };
        ExtensionItem {
            id: name.to_owned(),
            name: name.to_owned(),
            version: "1".into(),
            description: Some(format!("descricao de {name}")),
            state,
            state_label: state.label(),
            approved,
            actions: ExtensionActions {
                enable: state.can_enable() && (state != ExtensionState::Enabled || !account),
                disable: approved,
                test: state == ExtensionState::Enabled,
                remove: account,
            },
            last_run: None,
            error: Some(Localized::fixed("extensions.run.failed", "falhou")),
            catalog: None,
            account: account.then(|| AccountIdentity {
                index: 0,
                name: name.to_owned(),
                key: format!("extension:{name}:{name}"),
            }),
            overrides: None,
            override_targets: Vec::new(),
        }
    }

    fn items() -> Vec<ExtensionItem> {
        vec![
            item(ExtensionState::Disabled, false, false),
            item(ExtensionState::Enabled, true, true),
            item(ExtensionState::Changed, true, true),
            item(ExtensionState::Missing, true, true),
            item(ExtensionState::Invalid, false, false),
        ]
    }

    fn render(view: &mut ExtensionsView, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| view.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// RF5.9: every row core reports is shown with its state label, and the
    /// selected one brings its detail lines (last run, error).
    #[test]
    fn every_state_core_reports_is_rendered_with_its_label() {
        let mut view = ExtensionsView::new(items());
        let screen = render(&mut view, 100, 24);
        for state in [
            ExtensionState::Disabled,
            ExtensionState::Enabled,
            ExtensionState::Changed,
            ExtensionState::Missing,
            ExtensionState::Invalid,
        ] {
            assert!(
                screen.contains(&state.label().resolve("pt-BR")),
                "{state:?} listed:\n{screen}"
            );
        }
        assert!(screen.contains("descricao de pendente"), "{screen}");
        assert!(screen.contains("Última execução"), "{screen}");
        assert!(screen.contains("falhou"), "the last error shows: {screen}");
    }

    /// RF5.9: the disclosure is only approvable after it was shown, and
    /// scrolling it never approves.
    #[test]
    fn the_disclosure_is_shown_before_any_approval_is_possible() {
        let mut view = ExtensionsView::new(items());
        assert!(matches!(view.key(KeyCode::Char('s')), Action::Stay));
        assert!(matches!(view.key(KeyCode::Char('e')), Action::Disclose(_)));
        assert!(matches!(view.key(KeyCode::Char('s')), Action::Stay));
        assert!(matches!(view.key(KeyCode::Char('d')), Action::Stay));

        // A stale-but-approved row offers the revocation, not the test.
        view.selected = 2;
        assert!(matches!(view.key(KeyCode::Char('d')), Action::Disable(_)));
        assert!(matches!(view.key(KeyCode::Char('t')), Action::Stay));

        // An invalid row offers nothing but the removal of nothing.
        view.selected = 4;
        assert!(matches!(view.key(KeyCode::Char('e')), Action::Stay));
        assert!(matches!(view.key(KeyCode::Char('d')), Action::Stay));

        // The disclosure: scrolling never approves, `s` does.
        let lines = vec![Localized::fixed(
            "extensions.consent.risk",
            "risco residual",
        )];
        view.show_disclosure(lines, candidate());
        let screen = render(&mut view, 100, 24);
        assert!(screen.contains("risco residual"), "{screen}");
        assert!(
            screen.contains("s aprova"),
            "the controls are visible:\n{screen}"
        );
        for key in [KeyCode::Down, KeyCode::Char('j'), KeyCode::PageDown] {
            assert!(
                matches!(view.key(key), Action::Stay),
                "scrolling never approves"
            );
        }
        assert!(matches!(view.key(KeyCode::Char('s')), Action::Approve(_)));
    }

    fn candidate() -> cuotai_core::extensions::management::Candidate {
        // The view only carries the handle around: any token the core issued has
        // this shape, and the test never approves anything.
        serde_json::from_value(serde_json::json!({ "token": "test-token" }))
            .expect("a candidate is an opaque token")
    }

    /// RF5.10: with many rows on a narrow terminal the viewport follows the
    /// selection, so the actions never act on an invisible row.
    #[test]
    fn a_secret_is_typed_hidden_and_handed_to_core_once() {
        let mut view = ExtensionsView::new(vec![item(ExtensionState::Enabled, true, true)]);
        assert!(matches!(view.key(KeyCode::Char('v')), Action::Secrets(_)));
        view.ask_secrets(
            "x".into(),
            vec![SecretItem {
                name: "token".into(),
                env: "X_TOKEN".into(),
                description: "Proxy token".into(),
                stored: false,
            }],
        );
        assert!(matches!(view.key(KeyCode::Enter), Action::Stay), "empty");
        for c in "seed42".chars() {
            assert!(matches!(view.key(KeyCode::Char(c)), Action::Stay));
        }
        let drawn = render(&mut view, 80, 20);
        assert!(!drawn.contains("seed42"), "the value is never drawn");
        assert!(drawn.contains("X_TOKEN"));
        match view.key(KeyCode::Enter) {
            Action::SaveSecret { name, value, .. } => {
                assert_eq!((name.as_str(), value.as_str()), ("token", "seed42"));
            }
            _ => panic!("Enter hands the value to core"),
        }
        view.secret_saved("ok".into());
        assert!(matches!(view.key(KeyCode::Char('q')), Action::Close));
    }

    #[test]
    fn an_override_toggles_the_numbered_account() {
        let mut row = item(ExtensionState::Enabled, false, true);
        row.overrides = Some(cuotai_core::Provider::Claude);
        row.override_targets = vec![cuotai_core::extensions::management::OverrideTarget {
            index: 3,
            name: "Work".into(),
            active: false,
        }];
        let mut view = ExtensionsView::new(vec![row]);
        assert!(matches!(view.key(KeyCode::Char('o')), Action::Stay));
        assert!(render(&mut view, 80, 20).contains("[ ] Work"));
        assert!(matches!(view.key(KeyCode::Char('2')), Action::Stay));
        assert!(matches!(
            view.key(KeyCode::Char('1')),
            Action::SetOverride {
                index: 3,
                on: true,
                ..
            }
        ));
        assert!(matches!(view.key(KeyCode::Esc), Action::Stay));
        assert!(matches!(view.key(KeyCode::Char('q')), Action::Close));
    }

    #[test]
    fn the_viewport_follows_the_selection_on_a_narrow_terminal() {
        let rows: Vec<ExtensionItem> = (0..40)
            .map(|index| {
                let mut row = item(ExtensionState::Disabled, false, false);
                row.id = format!("ext-{index:02}");
                // A name long enough to wrap several times at 40 columns.
                row.name = format!("extensao-de-nome-bem-comprido-{index:02}");
                row.description = Some(format!("descricao bem comprido {index}"));
                row
            })
            .collect();
        let mut view = ExtensionsView::new(rows);
        for _ in 0..35 {
            assert!(matches!(view.key(KeyCode::Down), Action::Stay));
        }
        let screen = render(&mut view, 40, 12);
        assert!(
            screen.contains("ext-35"),
            "the selected row is on screen:\n{screen}"
        );
        assert!(
            !screen.contains("ext-00"),
            "and it scrolled instead of showing the top:\n{screen}"
        );
    }
}
