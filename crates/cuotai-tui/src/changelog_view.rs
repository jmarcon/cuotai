//! "Novidades" screen and `cuotai --changelog [VERSION]` (PRD 0019
//! RF7.7): the changelog parsed by `cuotai_core::changelog` from the file
//! embedded in the binary, so both surfaces work offline and never query a
//! provider.
//!
//! The category labels follow the interface language (`CUOTAI_LANG`,
//! default pt-BR) while the entries stay in English, the changelog's
//! language — which the screen and the command both state.

use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use cuotai_core::changelog::{self, Changelog, Section};
use cuotai_core::text::{
    changelog_category_l10n, changelog_entries_language_l10n, changelog_title_l10n, Localized,
};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

const BG: Color = Color::Rgb(40, 42, 54);
const FG: Color = Color::Rgb(248, 248, 242);
const MUTED: Color = Color::Rgb(98, 114, 164);
const CYAN: Color = Color::Rgb(139, 233, 253);
const YELLOW: Color = Color::Rgb(241, 250, 140);

/// The version this binary carries; the screen marks its section.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    Stay,
    Close,
}

/// What's new: the whole history of `CHANGELOG.md`, newest first, with the
/// current version marked. Opened from the "Sobre" screen.
#[derive(Default)]
pub(crate) struct ChangelogView {
    scroll: u16,
    footer: Vec<(Rect, KeyCode)>,
}

fn resolve(note: &Localized, locale: &str) -> String {
    note.resolve(locale)
}

/// Heading of one released version: `0.68.2 - 2026-10-21`, plus the marker
/// of the running version.
fn version_line(version: &str, date: &str) -> Line<'static> {
    let current = version == CURRENT;
    Line::from(vec![
        Span::styled(
            format!("{version} - {date}"),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if current { "  (esta versão)" } else { "" },
            Style::default().fg(YELLOW),
        ),
    ])
}

/// One section: its category labels (localized) over the entries
/// (English, untouched).
fn section_lines(section: &Section, locale: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for paragraph in &section.paragraphs {
        lines.push(Line::from(paragraph.clone()));
        lines.push(Line::from(""));
    }
    for block in &section.categories {
        lines.push(Line::styled(
            resolve(&changelog_category_l10n(block.category), locale),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ));
        for entry in &block.entries {
            lines.push(Line::styled(
                format!("- {}", entry.text),
                Style::default().fg(FG),
            ));
        }
        lines.push(Line::from(""));
    }
    lines
}

/// The whole screen body: identity, the language note, then every version.
pub(crate) fn lines(locale: &str) -> Vec<Line<'static>> {
    let changelog = changelog::entries();
    let mut lines = vec![
        Line::styled(
            format!("Cuotai {CURRENT}"),
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            resolve(&changelog_entries_language_l10n(), locale),
            Style::default().fg(MUTED),
        ),
        Line::from(""),
    ];
    if !changelog.unreleased.is_empty() {
        lines.push(Line::styled(
            "Unreleased",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ));
        lines.extend(section_lines(&changelog.unreleased, locale));
    }
    for release in &changelog.releases {
        lines.push(version_line(&release.version, &release.date));
        lines.extend(section_lines(&release.section, locale));
    }
    lines
}

impl ChangelogView {
    pub(crate) fn key(&mut self, code: KeyCode) -> Reply {
        match code {
            KeyCode::Esc | KeyCode::Char('q' | '?') | KeyCode::Enter => return Reply::Close,
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

    pub(crate) fn draw(&mut self, frame: &mut Frame, locale: &str) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
        let areas = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CYAN))
            .title(format!(
                " {} ",
                resolve(&changelog_title_l10n(), locale).to_uppercase()
            ));
        let inner = block.inner(areas[0]);
        let body = Paragraph::new(lines(locale)).wrap(ratatui::widgets::Wrap { trim: false });
        let total = body.line_count(inner.width);
        let max = total.saturating_sub(usize::from(inner.height));
        self.scroll = self.scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
        frame.render_widget(body.block(block).scroll((self.scroll, 0)), areas[0]);

        self.footer.clear();
        let mut spans = Vec::new();
        let mut x = areas[1].x;
        for (label, code) in [
            (" ↑↓/PgUp/PgDn rolar", None),
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

// ---------------------------------------------------------------------------
// `cuotai --changelog [VERSION]`
// ---------------------------------------------------------------------------

/// `cuotai --changelog [VERSION]`: the same history as the screen, in text.
/// `VERSION` is a version ("0.68.2") or a minor series ("0.68.x", "0.68").
/// Exit 0; an unknown version is a local error (1).
pub(crate) fn print(version: Option<&str>, locale: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let changelog = changelog::entries();
    let selected: Vec<&changelog::Release> = match version {
        None => changelog.releases.iter().collect(),
        Some(wanted) => {
            let selected = matching(changelog, wanted);
            if selected.is_empty() {
                return Err(unknown_version(wanted, locale).into());
            }
            selected
        }
    };
    let mut out = format!("Cuotai {CURRENT}\n\n");
    out.push_str(&resolve(&changelog_entries_language_l10n(), locale));
    out.push('\n');
    if let Some(version) = version {
        out.push_str(&format!("\nVERSION: {version}\n"));
    }
    for release in selected {
        out.push_str(&format!("\n{} - {}\n", release.version, release.date));
        for paragraph in &release.section.paragraphs {
            out.push_str(&format!("\n{paragraph}\n"));
        }
        for block in &release.section.categories {
            out.push_str(&format!(
                "\n{}\n",
                resolve(&changelog_category_l10n(block.category), locale)
            ));
            for entry in &block.entries {
                out.push_str(&format!("- {}\n", entry.text));
            }
        }
    }
    print!("{out}");
    Ok(0)
}

/// The released sections a `--changelog` argument selects: an exact
/// version, or every version of a minor series ("0.68", "0.68.x").
fn matching<'a>(changelog: &'a Changelog, wanted: &str) -> Vec<&'a changelog::Release> {
    let series = wanted
        .strip_suffix(".x")
        .map(|prefix| format!("{prefix}."))
        .unwrap_or_else(|| wanted.to_string());
    let series = if series.split('.').count() == 2 {
        format!("{series}.")
    } else {
        series
    };
    changelog
        .releases
        .iter()
        .filter(|release| {
            release.version == wanted
                || (series.ends_with('.') && release.version.starts_with(&series))
        })
        .collect()
}

/// The versions `--changelog` knows, for the error message. The ids live
/// in the core catalogs (`changelog.unknown_version`); they are namespaced
/// like every other core text so no other catalog can collide with them.
fn unknown_version(wanted: &str, locale: &str) -> String {
    let known = changelog::entries()
        .releases
        .iter()
        .map(|release| release.version.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let fallback =
        format!("A versão \"{wanted}\" não está no changelog; versões conhecidas: {known}.");
    cuotai_core::text::Localized::with_params(
        "changelog.unknown_version",
        cuotai_core::text::params([("wanted", wanted.into()), ("known", known.into())]),
        fallback,
    )
    .resolve(locale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render(locale: &str, width: u16, height: u16) -> String {
        let mut view = ChangelogView::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| view.draw(frame, locale)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    /// Scrolls until `needle` is visible (released sections never change, so
    /// a historical entry is a stable target) and returns whether it showed.
    fn scroll_until(view: &mut ChangelogView, locale: &str, needle: &str) -> bool {
        for _ in 0..400 {
            if render_with(view, locale, 120, 30).contains(needle) {
                return true;
            }
            view.key(KeyCode::PageDown);
        }
        false
    }

    #[test]
    fn screen_shows_the_current_version_and_the_localized_categories() {
        let screen = render("pt-BR", 100, 30);
        assert!(screen.contains("NOVIDADES"), "{screen}");
        assert!(screen.contains(CURRENT), "{screen}");
        assert!(
            screen.contains("Segurança")
                || screen.contains("Adicionado")
                || screen.contains("Corrigido")
        );
        assert!(screen.contains("esta versão"), "{screen}");
        // The entries stay in English, whatever the interface language is:
        // a released entry keeps its English text under the pt-BR interface.
        let mut view = ChangelogView::default();
        assert!(
            scroll_until(&mut view, "pt-BR", "OAuth client secrets"),
            "the released English entries stay reachable"
        );
    }

    #[test]
    fn category_labels_follow_the_interface_language() {
        // A Security category exists in the released history (0.68.x).
        let mut view = ChangelogView::default();
        assert!(scroll_until(&mut view, "es", "Seguridad"));
        let mut view = ChangelogView::default();
        assert!(scroll_until(&mut view, "en-US", "Security"));
    }

    #[test]
    fn every_version_is_reachable_by_scrolling() {
        let mut view = ChangelogView::default();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..400 {
            let screen = render_with(&mut view, "pt-BR", 120, 30);
            for release in &changelog::entries().releases {
                let heading = format!("{} - {}", release.version, release.date);
                if screen.lines().any(|line| line.contains(&heading)) {
                    seen.insert(release.version.clone());
                }
            }
            view.key(KeyCode::PageDown);
        }
        let missing: Vec<_> = changelog::entries()
            .releases
            .iter()
            .filter(|release| !seen.contains(&release.version))
            .map(|release| release.version.clone())
            .collect();
        assert!(missing.is_empty(), "not shown: {missing:?}");
    }

    fn render_with(view: &mut ChangelogView, locale: &str, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| view.draw(frame, locale)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn end_clamps_and_home_returns() {
        let mut view = ChangelogView::default();
        view.key(KeyCode::End);
        let end = render_with(&mut view, "pt-BR", 120, 30);
        // The oldest section is the last one in the file.
        assert!(end.contains("0.1.0 - 2026-10-01"), "{end}");
        view.key(KeyCode::Home);
        assert!(render_with(&mut view, "pt-BR", 120, 30).contains(CURRENT));
    }

    #[test]
    fn keys_close_and_scroll() {
        for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?')] {
            assert_eq!(ChangelogView::default().key(code), Reply::Close);
        }
        let mut view = ChangelogView::default();
        view.key(KeyCode::PageDown);
        assert!(view.scroll > 0);
        view.key(KeyCode::Home);
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn a_version_argument_selects_exact_and_series() {
        let changelog = changelog::entries();
        let exact = matching(changelog, CURRENT);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].version, CURRENT);
        let series = matching(changelog, "0.68.x");
        assert!(series.len() >= 2, "{series:?}");
        assert!(series.iter().all(|r| r.version.starts_with("0.68.")));
        let two = matching(changelog, "0.68");
        assert_eq!(two.len(), series.len());
        assert!(matching(changelog, "9.9.9").is_empty());
        let error = unknown_version("9.9.9", "pt-BR");
        assert!(
            error.contains("9.9.9") && error.contains("0.68.2"),
            "{error}"
        );
    }
}
