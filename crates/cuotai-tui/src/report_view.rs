//! PRD 0028 RF2.4: the "Report a problem" screen opened from an account's
//! details. It shows the secret-free draft built by `cuotai_core::support`
//! and the link; nothing is sent and nothing leaves the process.

use crate::palette;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

const BG: Color = palette::BG;
const FG: Color = palette::FG;
const MUTED: Color = palette::MUTED;
const CYAN: Color = palette::ACCENT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    Stay,
    Close,
}

pub(crate) struct ReportView {
    text: String,
    scroll: u16,
}

impl ReportView {
    pub(crate) fn new(text: String) -> Self {
        Self { text, scroll: 0 }
    }

    pub(crate) fn key(&mut self, code: KeyCode) -> Reply {
        match code {
            KeyCode::Esc | KeyCode::Char('q' | 'b') | KeyCode::Enter => return Reply::Close,
            KeyCode::Down | KeyCode::Char('j' | ']') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k' | '[') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = u16::MAX,
            _ => {}
        }
        Reply::Stay
    }

    pub(crate) fn mouse(&mut self, kind: MouseEventKind) -> Reply {
        match kind {
            MouseEventKind::ScrollDown => self.key(KeyCode::Down),
            MouseEventKind::ScrollUp => self.key(KeyCode::Up),
            MouseEventKind::Down(MouseButton::Right) => Reply::Close,
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
            .title(" REPORTAR PROBLEMA ");
        let inner = block.inner(areas[0]);
        let body = Paragraph::new(self.text.as_str()).wrap(Wrap { trim: false });
        let total = body.line_count(inner.width);
        let max = total.saturating_sub(usize::from(inner.height));
        self.scroll = self.scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
        frame.render_widget(body.block(block).scroll((self.scroll, 0)), areas[0]);
        frame.render_widget(
            Paragraph::new("Nada é enviado pelo Cuotai · ↑↓ rolar · Esc voltar")
                .style(Style::default().fg(MUTED)),
            areas[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_scroll_and_close() {
        let mut view = ReportView::new("x".into());
        assert_eq!(view.key(KeyCode::Down), Reply::Stay);
        assert_eq!(view.scroll, 1);
        assert_eq!(view.key(KeyCode::Up), Reply::Stay);
        assert_eq!(view.scroll, 0);
        assert_eq!(view.key(KeyCode::Esc), Reply::Close);
        assert_eq!(
            view.mouse(MouseEventKind::Down(MouseButton::Right)),
            Reply::Close
        );
    }
}
