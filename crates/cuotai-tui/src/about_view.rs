//! "Sobre" screen: version, license and repository of Cuotai plus the
//! open-source libraries it relies on (list owned by `cuotai_core::about`).
//! Read-only and local: it never touches the network or the configuration.

use crate::palette;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use cuotai_core::about::{self, Component, Library, Role};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

const BG: Color = palette::BG;
const FG: Color = palette::FG;
const MUTED: Color = palette::MUTED;
const CYAN: Color = palette::ACCENT;
const YELLOW: Color = palette::WARN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    Stay,
    Close,
    /// PRD 0019 RF7.7: open the "Novidades" screen from About.
    Changelog,
}

#[derive(Default)]
pub(crate) struct AboutView {
    scroll: u16,
    footer: Vec<(Rect, KeyCode)>,
}

fn component(component: Component) -> &'static str {
    match component {
        Component::Core => "núcleo",
        Component::Tui => "terminal",
        Component::Desktop => "desktop",
    }
}

fn section(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Runtime => ("Bibliotecas", "Compiladas nos aplicativos."),
        Role::Build => (
            "Ferramentas de compilação",
            "Usadas só ao compilar o aplicativo.",
        ),
        Role::Test => ("Testes", "Usadas só nos testes automatizados."),
        Role::Asset => ("Recursos visuais", "Ícones distribuídos com o desktop."),
        Role::Theme => ("Temas", "Paletas abertas seguidas pelos temas embutidos."),
        Role::Reference => (
            "Projetos de referência",
            "Lógica e contratos adaptados para Rust; o código original não é executado.",
        ),
    }
}

fn entry(library: &Library) -> [Line<'static>; 2] {
    let used = library
        .used_by
        .iter()
        .map(|c| component(*c))
        .collect::<Vec<_>>()
        .join(", ");
    [
        Line::from(vec![
            Span::styled(
                library.name.to_string(),
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} · {}", library.license, used),
                Style::default().fg(MUTED),
            ),
        ]),
        Line::styled(
            format!("  {}", library.repository),
            Style::default().fg(CYAN),
        ),
    ]
}

/// Whole screen body: identity first, then one block per role.
fn lines() -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            format!(
                "{} {}",
                about::PROJECT_NAME,
                cuotai_core::build_info::display_version()
            ),
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("Licença {}", about::PROJECT_LICENSE)),
        Line::styled(about::PROJECT_REPOSITORY, Style::default().fg(CYAN)),
        Line::from(""),
        Line::styled(
            "O Cuotai é construído sobre projetos de código aberto mantidos por suas comunidades. \
             Obrigado a quem os desenvolve. Os logotipos são marcas dos respectivos titulares.",
            Style::default().fg(MUTED),
        ),
    ];
    let brazilian = about::from_brazil();
    if !brazilian.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "Agradecimento especial: projetos criados por brasileiros",
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::styled(
            brazilian
                .iter()
                .map(|l| l.name)
                .collect::<Vec<_>>()
                .join(" · "),
            Style::default().fg(FG),
        ));
    }
    for role in [
        Role::Runtime,
        Role::Build,
        Role::Test,
        Role::Asset,
        Role::Theme,
        Role::Reference,
    ] {
        let libraries = about::libraries_with(role);
        if libraries.is_empty() {
            continue;
        }
        let (title, note) = section(role);
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{title} ({})", libraries.len()),
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {note}"), Style::default().fg(MUTED)),
        ]));
        for library in libraries {
            lines.extend(entry(library));
        }
    }
    lines
}

impl AboutView {
    pub(crate) fn key(&mut self, code: KeyCode) -> Reply {
        match code {
            KeyCode::Esc | KeyCode::Char('q' | '?') | KeyCode::Enter => return Reply::Close,
            // PRD 0019 RF7.7: What's new (the same history as
            // `cuotai --changelog`), offline.
            KeyCode::Char('n') => return Reply::Changelog,
            KeyCode::Down | KeyCode::Char('j' | ']') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k' | '[') => self.scroll = self.scroll.saturating_sub(1),
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
    /// is the same as its key.
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

    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
        let areas = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CYAN))
            .title(" SOBRE ");
        let inner = block.inner(areas[0]);
        let body = Paragraph::new(lines()).wrap(ratatui::widgets::Wrap { trim: false });
        let total = body.line_count(inner.width);
        let max = total.saturating_sub(usize::from(inner.height));
        self.scroll = self.scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
        frame.render_widget(body.block(block).scroll((self.scroll, 0)), areas[0]);

        self.footer.clear();
        let mut spans = Vec::new();
        let mut x = areas[1].x;
        for (label, code) in [
            (" ↑↓/PgUp/PgDn rolar", None),
            ("  n novidades", Some(KeyCode::Char('n'))),
            ("  Esc voltar", Some(KeyCode::Esc)),
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
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render(view: &mut AboutView, width: u16, height: u16) -> String {
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

    #[test]
    fn first_page_shows_identity_and_first_libraries() {
        let screen = render(&mut AboutView::default(), 100, 30);
        assert!(screen.contains("SOBRE"));
        assert!(screen.contains(about::PROJECT_REPOSITORY));
        assert!(screen.contains("Bibliotecas ("));
        assert!(screen.contains("aes-gcm"));
        assert!(screen.contains("https://github.com/RustCrypto/AEADs"));
    }

    #[test]
    fn every_listed_library_is_reachable_by_scrolling() {
        let mut view = AboutView::default();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let screen = render(&mut view, 100, 20);
            for library in about::libraries() {
                if screen.contains(library.name) && screen.contains(library.repository) {
                    seen.insert(library.name);
                }
            }
            view.key(KeyCode::PageDown);
        }
        for library in about::libraries() {
            assert!(seen.contains(library.name), "{} not shown", library.name);
        }
    }

    #[test]
    fn end_clamps_and_home_returns() {
        let mut view = AboutView::default();
        view.key(KeyCode::End);
        let end = render(&mut view, 100, 20);
        assert!(end.contains("Qwen Code"));
        assert!(view.scroll < u16::MAX);
        view.key(KeyCode::Home);
        assert!(render(&mut view, 100, 20).contains(about::PROJECT_REPOSITORY));
    }

    #[test]
    fn closes_with_esc_q_question_mark_and_right_click() {
        for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?')] {
            assert_eq!(AboutView::default().key(code), Reply::Close);
        }
        assert_eq!(
            AboutView::default().mouse(MouseEventKind::Down(MouseButton::Right), 0, 0),
            Reply::Close
        );
        assert_eq!(AboutView::default().key(KeyCode::Char('x')), Reply::Stay);
    }

    #[test]
    fn footer_click_opens_whats_new_and_back() {
        let mut view = AboutView::default();
        render(&mut view, 100, 20);
        // PRD 0019 RF7.7: `n novidades` is the first clickable action.
        let rect = view.footer[0].0;
        assert_eq!(
            view.mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y),
            Reply::Changelog
        );
        assert_eq!(view.key(KeyCode::Char('n')), Reply::Changelog);
        let back = view.footer[1].0;
        assert_eq!(
            view.mouse(MouseEventKind::Down(MouseButton::Left), back.x, back.y),
            Reply::Close
        );
    }

    #[test]
    fn narrow_terminal_does_not_panic() {
        let mut view = AboutView::default();
        render(&mut view, 20, 6);
        view.key(KeyCode::End);
        render(&mut view, 20, 6);
    }
}
