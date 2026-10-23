//! "Novidades" screen and `cuotai --changelog [VERSION]` (PRD 0019
//! RF7.7): the changelog parsed by `cuotai_core::changelog` from the file
//! embedded in the binary, so both surfaces work offline and never query a
//! provider.
//!
//! The category labels follow the interface language (`CUOTAI_LANG`,
//! default pt-BR) while the entries stay in English, the changelog's
//! language — which the screen and the command both state.
//!
//! The internal builds 0.1.0–0.79.0 were never published (PRD 0039 D11):
//! the changelog summarizes them in one pre-release history, shown last,
//! and `--changelog 0.5.0` (or `0.5.x`) prints that history with a note
//! instead of failing.

use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use cuotai_core::changelog::{self, Changelog, PreRelease, Section};
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

/// What's new: the whole history of `CHANGELOG.md`, newest first and the
/// pre-release history last, with the current version marked. Opened from
/// the "Sobre" screen.
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

/// Heading of the pre-release history, marked when this binary is one of
/// the internal builds it summarizes.
fn pre_release_line(history: &PreRelease) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            history.title.clone(),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if history.covers(CURRENT) {
                "  (esta versão)"
            } else {
                ""
            },
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
    if let Some(history) = &changelog.pre_release {
        lines.push(pre_release_line(history));
        lines.extend(section_lines(&history.section, locale));
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
/// A version (or series) of the internal builds prints the pre-release
/// history, preceded by a note that says so. Exit 0; a version the
/// changelog does not know is a local error (1).
pub(crate) fn print(version: Option<&str>, locale: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let changelog = changelog::entries();
    let selected = match version {
        None => Selection {
            releases: changelog.releases.iter().collect(),
            pre_release: changelog.pre_release.as_ref(),
        },
        Some(wanted) => {
            let selected = matching(changelog, wanted);
            if selected.is_empty() {
                return Err(unknown_version(wanted, locale).into());
            }
            selected
        }
    };
    print!("{}", render_text(version, &selected, locale));
    Ok(0)
}

/// The text `--changelog` prints for `selected`.
fn render_text(version: Option<&str>, selected: &Selection<'_>, locale: &str) -> String {
    let mut out = format!("Cuotai {CURRENT}\n\n");
    out.push_str(&resolve(&changelog_entries_language_l10n(), locale));
    out.push('\n');
    if let Some(version) = version {
        out.push_str(&format!("\nVERSION: {version}\n"));
        if let Some(history) = selected.pre_release {
            out.push_str(&format!(
                "\n{}\n",
                pre_release_note(version, history, locale)
            ));
        }
    }
    for release in &selected.releases {
        out.push_str(&format!("\n{} - {}\n", release.version, release.date));
        section_text(&release.section, locale, &mut out);
    }
    if let Some(history) = selected.pre_release {
        out.push_str(&format!("\n{}\n", history.title));
        section_text(&history.section, locale, &mut out);
    }
    out
}

fn section_text(section: &Section, locale: &str, out: &mut String) {
    for paragraph in &section.paragraphs {
        out.push_str(&format!("\n{paragraph}\n"));
    }
    for block in &section.categories {
        out.push_str(&format!(
            "\n{}\n",
            resolve(&changelog_category_l10n(block.category), locale)
        ));
        for entry in &block.entries {
            out.push_str(&format!("- {}\n", entry.text));
        }
    }
}

/// What a `--changelog` argument selects: released sections, and the
/// pre-release history when the argument names one of its internal builds.
#[derive(Debug)]
struct Selection<'a> {
    releases: Vec<&'a changelog::Release>,
    pre_release: Option<&'a PreRelease>,
}

impl Selection<'_> {
    fn is_empty(&self) -> bool {
        self.releases.is_empty() && self.pre_release.is_none()
    }
}

/// The sections a `--changelog` argument selects: an exact version, or
/// every version of a minor series ("0.68", "0.68.x"). A version inside the
/// pre-release range, or a series that overlaps it, selects the history.
fn matching<'a>(changelog: &'a Changelog, wanted: &str) -> Selection<'a> {
    let series = wanted
        .strip_suffix(".x")
        .map(|prefix| format!("{prefix}."))
        .unwrap_or_else(|| wanted.to_string());
    let series = if series.split('.').count() == 2 {
        format!("{series}.")
    } else {
        series
    };
    let releases = changelog
        .releases
        .iter()
        .filter(|release| {
            release.version == wanted
                || (series.ends_with('.') && release.version.starts_with(&series))
        })
        .collect();
    let pre_release = changelog.pre_release.as_ref().filter(|history| {
        history.covers(wanted)
            || (series.ends_with('.') && series_overlaps(&series, &history.first, &history.last))
    });
    Selection {
        releases,
        pre_release,
    }
}

/// Whether the minor series `"X.Y."` has a version in `first..=last`. Only
/// exactly `"X.Y."` is a series: `"0.79.1."` (from `0.79.1.x`) is not.
fn series_overlaps(series: &str, first: &str, last: &str) -> bool {
    let minor = |text: &str| -> Option<(u64, u64)> {
        let mut parts = text.split('.');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    };
    let exact_series = series.strip_suffix('.').and_then(|prefix| {
        let (major, minor) = prefix.split_once('.')?;
        Some((major.parse().ok()?, minor.parse().ok()?))
    });
    match (exact_series, minor(first), minor(last)) {
        (Some(wanted), Some(first), Some(last)) => first <= wanted && wanted <= last,
        _ => false,
    }
}

/// The note above the pre-release history when `--changelog` names an
/// internal build (`changelog.pre_release_note` in the core catalogs).
fn pre_release_note(wanted: &str, history: &PreRelease, locale: &str) -> String {
    let range = history.range();
    let fallback = format!(
        "A versão \"{wanted}\" foi uma build interna e nunca foi publicada. Suas mudanças estão resumidas no histórico de pré-lançamento ({range})."
    );
    cuotai_core::text::Localized::with_params(
        "changelog.pre_release_note",
        cuotai_core::text::params([("wanted", wanted.into()), ("range", range.into())]),
        fallback,
    )
    .resolve(locale)
}

/// The versions `--changelog` knows, for the error message. The ids live
/// in the core catalogs (`changelog.unknown_version`); they are namespaced
/// like every other core text so no other catalog can collide with them.
fn unknown_version(wanted: &str, locale: &str) -> String {
    let changelog = changelog::entries();
    let known = changelog
        .releases
        .iter()
        .map(|release| release.version.clone())
        .chain(changelog.pre_release.iter().map(PreRelease::range))
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
        render_with(&mut view, locale, width, height)
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

    /// Scrolls until `needle` is visible (released sections never change, so
    /// a historical entry is a stable target) and returns whether it showed.
    fn scroll_until(view: &mut ChangelogView, locale: &str, needle: &str) -> bool {
        for _ in 0..400 {
            if render_with(view, locale, 200, 30).contains(needle) {
                return true;
            }
            view.key(KeyCode::PageDown);
        }
        false
    }

    /// Every section of the embedded file, newest first: released ones, then
    /// the pre-release history.
    fn sections() -> Vec<&'static Section> {
        let changelog = changelog::entries();
        changelog
            .releases
            .iter()
            .map(|release| &release.section)
            .chain(changelog.pre_release.iter().map(|history| &history.section))
            .collect()
    }

    /// The heading the screen shows for every section, newest first.
    fn headings() -> Vec<String> {
        let changelog = changelog::entries();
        changelog
            .releases
            .iter()
            .map(|release| format!("{} - {}", release.version, release.date))
            .chain(changelog.pre_release.iter().map(|h| h.title.clone()))
            .collect()
    }

    #[test]
    fn screen_shows_the_current_version_and_the_localized_categories() {
        let screen = render("pt-BR", 100, 30);
        assert!(screen.contains("NOVIDADES"), "{screen}");
        assert!(screen.contains(CURRENT), "{screen}");
        // The running version is marked: its own section or the pre-release
        // history that covers it.
        let mut view = ChangelogView::default();
        assert!(scroll_until(&mut view, "pt-BR", "esta versão"));
        let mut view = ChangelogView::default();
        assert!(scroll_until(&mut view, "pt-BR", "Adicionado"));
        // The entries stay in English, whatever the interface language is:
        // a released entry keeps its English text under the pt-BR interface.
        let first_entry: String = sections()
            .iter()
            .flat_map(|section| section.categories.iter())
            .flat_map(|block| block.entries.iter())
            .next()
            .expect("an entry")
            .text
            .chars()
            .take(40)
            .collect();
        let mut view = ChangelogView::default();
        assert!(
            scroll_until(&mut view, "pt-BR", &first_entry),
            "the released English entries stay reachable: {first_entry}"
        );
    }

    #[test]
    fn category_labels_follow_the_interface_language() {
        // A Security category exists in the history.
        assert!(sections().iter().any(|section| section
            .categories
            .iter()
            .any(|block| block.category == changelog::Category::Security)));
        let mut view = ChangelogView::default();
        assert!(scroll_until(&mut view, "es", "Seguridad"));
        let mut view = ChangelogView::default();
        assert!(scroll_until(&mut view, "en-US", "Security"));
    }

    #[test]
    fn every_section_is_reachable_by_scrolling() {
        let wanted = headings();
        let mut view = ChangelogView::default();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..400 {
            let screen = render_with(&mut view, "pt-BR", 200, 30);
            for heading in &wanted {
                if screen.contains(heading.as_str()) {
                    seen.insert(heading.clone());
                }
            }
            view.key(KeyCode::PageDown);
        }
        let missing: Vec<_> = wanted.iter().filter(|h| !seen.contains(*h)).collect();
        assert!(missing.is_empty(), "not shown: {missing:?}");
    }

    #[test]
    fn end_clamps_and_home_returns() {
        view_end_shows_the_oldest_section();
        let mut view = ChangelogView::default();
        view.key(KeyCode::End);
        view.key(KeyCode::Home);
        assert!(render_with(&mut view, "pt-BR", 120, 30).contains(CURRENT));
    }

    /// The oldest section is the last one in the file: End reaches it.
    fn view_end_shows_the_oldest_section() {
        let mut view = ChangelogView::default();
        view.key(KeyCode::End);
        // A tall screen, so End shows the whole oldest heading and its tail.
        let end = render_with(&mut view, "pt-BR", 200, 400);
        let oldest = headings().pop().expect("a section");
        assert!(end.contains(&oldest), "{oldest}");
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

    const WITH_HISTORY: &str = "# C\n\n## [Unreleased]\n\n## [0.81.0] - 2026-11-02\n\n### Fixed\n\n- Newer fix.\n\n## [0.80.1] - 2026-10-30\n\n### Fixed\n\n- A fix.\n\n## [0.80.0] - 2026-10-24\n\n### Added\n\n- First release.\n\n## Pre-release history (0.1.0–0.79.0, internal builds, never published)\n\nSummary.\n\n### Added\n\n- The dashboard.\n\n[Unreleased]: https://example/compare/v0.81.0...HEAD\n";

    #[test]
    fn a_version_argument_selects_exact_series_and_the_pre_release_history() {
        let changelog = changelog::parse(WITH_HISTORY).unwrap();
        let exact = matching(&changelog, "0.80.1");
        assert_eq!(exact.releases.len(), 1);
        assert!(exact.pre_release.is_none());
        let series = matching(&changelog, "0.80.x");
        assert_eq!(series.releases.len(), 2);
        assert!(series
            .releases
            .iter()
            .all(|r| r.version.starts_with("0.80.")));
        assert!(series.pre_release.is_none());
        assert_eq!(matching(&changelog, "0.80").releases.len(), 2);
        // An internal build: no section of its own, the history instead.
        for wanted in [
            "0.5.0", "0.1.0", "0.79.0", "0.14.3", "0.5.x", "0.5", "0.79", "0.0.x",
        ] {
            let selection = matching(&changelog, wanted);
            assert!(selection.releases.is_empty(), "{wanted}");
            if wanted == "0.0.x" {
                assert!(selection.is_empty(), "{wanted}");
            } else {
                assert!(selection.pre_release.is_some(), "{wanted}");
            }
        }
        // Outside every range.
        // Extra components are not a minor series, even inside the range.
        for wanted in [
            "9.9.9",
            "0.79.1",
            "0.82.x",
            "1.0",
            "x",
            "0.79.1.x",
            "0.79.1.",
            "0.5.garbage.x",
            "0.5.0.x",
        ] {
            assert!(matching(&changelog, wanted).is_empty(), "{wanted}");
        }
        // The text names the history and explains why.
        let text = render_text(Some("0.5.0"), &matching(&changelog, "0.5.0"), "en-US");
        assert!(text.contains("VERSION: 0.5.0"), "{text}");
        assert!(
            text.contains("\"0.5.0\" was an internal build and was never published"),
            "{text}"
        );
        assert!(text.contains("(0.1.0–0.79.0)"), "{text}");
        assert!(
            text.contains("Pre-release history (0.1.0–0.79.0, internal builds, never published)"),
            "{text}"
        );
        assert!(text.contains("- The dashboard."), "{text}");
        assert!(!text.contains("First release."), "{text}");
        let pt = render_text(Some("0.5.0"), &matching(&changelog, "0.5.0"), "pt-BR");
        assert!(pt.contains("build interna"), "{pt}");
        // A released version prints no note.
        let text = render_text(Some("0.80.0"), &matching(&changelog, "0.80.0"), "en-US");
        assert!(!text.contains("internal build"), "{text}");
        assert!(text.contains("0.80.0 - 2026-10-24"), "{text}");
        // Without an argument, everything, the history last.
        let all = Selection {
            releases: changelog.releases.iter().collect(),
            pre_release: changelog.pre_release.as_ref(),
        };
        let text = render_text(None, &all, "en-US");
        let newest = text.find("0.81.0 - 2026-11-02").unwrap();
        let history = text.find("Pre-release history (").unwrap();
        assert!(newest < history, "{text}");
        assert!(!text.contains("internal build and"), "{text}");
    }

    #[test]
    fn the_embedded_changelog_answers_every_known_version() {
        let changelog = changelog::entries();
        // The running version is always printable: its own section or the
        // pre-release history.
        assert!(!matching(changelog, CURRENT).is_empty());
        if let Some(history) = &changelog.pre_release {
            assert!(matching(changelog, &history.first).pre_release.is_some());
        }
        assert!(matching(changelog, "9.9.9").is_empty());
        let error = unknown_version("9.9.9", "en-US");
        assert!(error.contains("9.9.9"), "{error}");
        let range_or_version = changelog
            .pre_release
            .as_ref()
            .map(PreRelease::range)
            .or_else(|| changelog.releases.first().map(|r| r.version.clone()))
            .unwrap();
        assert!(error.contains(&range_or_version), "{error}");
    }
}
