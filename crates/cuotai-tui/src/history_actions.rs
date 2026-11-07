//! PRD 0052 RF5.1 — the TUI half of the history export and erase parity:
//! from an account's details, `X` exports the recorded history (this
//! account or every account, CSV or JSON, to a path the person types) and
//! `D` erases it after the localized confirmation word. Every storage rule
//! lives in the core: the file is written by
//! [`HistoryStore::export_to_file`] (the same writer `--history-export`
//! uses) and the erasure is [`HistoryStore::erase_account`] /
//! [`HistoryStore::erase_all`], exactly as `--history-erase` calls them.

use std::ops::Bound;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use cuotai_core::history::{ExportFormat, HistoryStore};
use cuotai_core::Account;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::l10n::{tr, trf};
use crate::palette;

const BG: Color = palette::BG;
const FG: Color = palette::FG;
const MUTED: Color = palette::MUTED;
const ACCENT: Color = palette::ACCENT;
const GOOD: Color = palette::GOOD;
const BAD: Color = palette::BAD;

/// Which action the screen performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Export,
    Erase,
}

/// What the dashboard does after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    Stay,
    Close,
    /// The history was erased: the dashboard re-reads its history views.
    Erased,
}

/// The focused export field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Scope,
    Format,
    Path,
}

pub(crate) struct HistoryActions {
    mode: Mode,
    /// The account the details showed: its index and display name.
    account: Option<(usize, String)>,
    /// Every account instead of the selected one.
    all: bool,
    format: ExportFormat,
    path: String,
    focus: Focus,
    typed: String,
    /// The error of the last attempt (the screen stays open).
    error: Option<String>,
    /// The result; any key then closes.
    done: Option<String>,
}

impl HistoryActions {
    /// `account`: the selected account's index and display name; without
    /// one the scope is every account.
    pub(crate) fn new(mode: Mode, account: Option<(usize, String)>) -> Self {
        let all = account.is_none();
        let mut view = Self {
            mode,
            account,
            all,
            format: ExportFormat::Csv,
            path: String::new(),
            focus: Focus::Path,
            typed: String::new(),
            error: None,
            done: None,
        };
        view.path = view.default_path();
        view
    }

    /// `cuotai-history-<account|all>-<date>.<ext>` in the current directory.
    fn default_path(&self) -> String {
        let scope = match (&self.account, self.all) {
            (Some((_, name)), false) => slug(name),
            _ => "all".to_owned(),
        };
        let date = chrono::Local::now().format("%Y%m%d");
        let name = format!("cuotai-history-{scope}-{date}.{}", self.format.extension());
        std::env::current_dir()
            .map(|dir| dir.join(&name))
            .unwrap_or_else(|_| PathBuf::from(&name))
            .display()
            .to_string()
    }

    fn toggle_scope(&mut self) {
        if self.account.is_none() {
            return;
        }
        let was_default = self.path == self.default_path();
        self.all = !self.all;
        if was_default {
            self.path = self.default_path();
        }
    }

    fn toggle_format(&mut self) {
        let old = self.format;
        self.format = match old {
            ExportFormat::Csv => ExportFormat::Json,
            ExportFormat::Json => ExportFormat::Csv,
        };
        let suffix = format!(".{}", old.extension());
        if let Some(stem) = self.path.strip_suffix(&suffix) {
            self.path = format!("{stem}.{}", self.format.extension());
        }
    }

    /// One key. `store`: the history, `None` when it is unavailable;
    /// `accounts`: the configuration's accounts, in order.
    pub(crate) fn key(
        &mut self,
        code: KeyCode,
        store: Option<&HistoryStore>,
        accounts: &[Account],
    ) -> Reply {
        if self.done.is_some() {
            return Reply::Close;
        }
        if code == KeyCode::Esc {
            return Reply::Close;
        }
        match self.mode {
            Mode::Export => self.export_key(code, store, accounts),
            Mode::Erase => self.erase_key(code, store, accounts),
        }
    }

    fn export_key(
        &mut self,
        code: KeyCode,
        store: Option<&HistoryStore>,
        accounts: &[Account],
    ) -> Reply {
        match (code, self.focus) {
            (KeyCode::Tab | KeyCode::Down, _) => {
                self.focus = match self.focus {
                    Focus::Scope => Focus::Format,
                    Focus::Format => Focus::Path,
                    Focus::Path => Focus::Scope,
                };
            }
            (KeyCode::BackTab | KeyCode::Up, _) => {
                self.focus = match self.focus {
                    Focus::Scope => Focus::Path,
                    Focus::Format => Focus::Scope,
                    Focus::Path => Focus::Format,
                };
            }
            (KeyCode::Left | KeyCode::Right | KeyCode::Char(' '), Focus::Scope) => {
                self.toggle_scope()
            }
            (KeyCode::Left | KeyCode::Right | KeyCode::Char(' '), Focus::Format) => {
                self.toggle_format()
            }
            (KeyCode::Char(c), Focus::Path) if !c.is_control() => self.path.push(c),
            (KeyCode::Backspace, Focus::Path) => {
                self.path.pop();
            }
            (KeyCode::Enter, _) => self.export(store, accounts),
            _ => {}
        }
        Reply::Stay
    }

    fn selected<'a>(&self, accounts: &'a [Account]) -> Vec<&'a Account> {
        match (&self.account, self.all) {
            (Some((index, _)), false) => accounts.get(*index).into_iter().collect(),
            _ => accounts.iter().collect(),
        }
    }

    fn export(&mut self, store: Option<&HistoryStore>, accounts: &[Account]) {
        let Some(store) = store else {
            self.error = Some(unavailable());
            return;
        };
        let typed = self.path.trim();
        if typed.is_empty() {
            self.error = Some(
                tr(
                    "history_actions.path_empty",
                    "Type the path of the file to write.",
                )
                .to_owned(),
            );
            return;
        }
        let path = resolve(typed);
        let selected: Vec<Account> = self.selected(accounts).into_iter().cloned().collect();
        match store.export_to_file(&selected, Bound::Unbounded, self.format, &path) {
            Ok(rows) => {
                self.error = None;
                self.done = Some(crate::data_cli::exported_text(rows, &shown(&path)));
            }
            Err(error) => {
                self.error = Some(error.l10n().resolve(crate::ui::tui_locale()));
            }
        }
    }

    fn erase_key(
        &mut self,
        code: KeyCode,
        store: Option<&HistoryStore>,
        accounts: &[Account],
    ) -> Reply {
        match code {
            KeyCode::Tab | KeyCode::BackTab => self.toggle_scope(),
            KeyCode::Char(c) if !c.is_control() => self.typed.push(c),
            KeyCode::Backspace => {
                self.typed.pop();
            }
            KeyCode::Enter => return self.erase(store, accounts),
            _ => {}
        }
        Reply::Stay
    }

    fn erase(&mut self, store: Option<&HistoryStore>, accounts: &[Account]) -> Reply {
        if self.typed.trim() != crate::data_cli::word() {
            self.typed.clear();
            self.error = Some(
                tr(
                    "history_actions.word_mismatch",
                    "The word does not match; nothing was erased.",
                )
                .to_owned(),
            );
            return Reply::Stay;
        }
        let Some(store) = store else {
            self.error = Some(unavailable());
            return Reply::Stay;
        };
        let result = match (&self.account, self.all) {
            (Some((index, _)), false) => match accounts.get(*index) {
                Some(account) => store.erase_account(account),
                None => {
                    self.error = Some(unavailable());
                    return Reply::Stay;
                }
            },
            _ => store.erase_all(),
        };
        match result {
            Ok(removed) => {
                self.error = None;
                self.done = Some(crate::data_cli::erased_text(removed));
                Reply::Erased
            }
            Err(error) => {
                self.error = Some(error.l10n().resolve(crate::ui::tui_locale()));
                Reply::Stay
            }
        }
    }

    pub(crate) fn mouse(&mut self, kind: MouseEventKind) -> Reply {
        match kind {
            MouseEventKind::Down(MouseButton::Right) => Reply::Close,
            _ => Reply::Stay,
        }
    }

    fn scope_text(&self) -> String {
        match (&self.account, self.all) {
            (Some((_, name)), false) => trf(
                "history_actions.scope_account",
                "This account: {name}",
                &[("name", &crate::ui::safe(name))],
            ),
            _ => tr("history_actions.scope_all", "Every account").to_owned(),
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let label = Style::default().fg(MUTED);
        let value = Style::default().fg(FG);
        let focused = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
        let mut lines = Vec::new();
        match self.mode {
            Mode::Export => {
                let row = |focus: Focus, name: &str, text: String, cursor: bool| {
                    let on = self.focus == focus;
                    let mut spans = vec![
                        Span::styled(if on { "› " } else { "  " }, focused),
                        Span::styled(format!("{name}: "), label),
                        Span::styled(text, if on { focused } else { value }),
                    ];
                    if cursor && on {
                        spans.push(Span::styled("▏", focused));
                    }
                    Line::from(spans)
                };
                lines.push(row(
                    Focus::Scope,
                    tr("history_actions.scope", "Scope"),
                    self.scope_text(),
                    false,
                ));
                lines.push(row(
                    Focus::Format,
                    tr("history_actions.format", "Format"),
                    self.format.extension().to_uppercase(),
                    false,
                ));
                lines.push(row(
                    Focus::Path,
                    tr("history_actions.path", "File"),
                    self.path.clone(),
                    true,
                ));
            }
            Mode::Erase => {
                let notice = match (&self.account, self.all) {
                    (Some((_, name)), false) => {
                        crate::data_cli::erase_notice(Some(&crate::ui::safe(name)))
                    }
                    _ => crate::data_cli::erase_notice(None),
                };
                lines.push(Line::styled(notice, value));
                lines.push(Line::styled(
                    tr(
                        "history_actions.erase_hint",
                        "Erased samples cannot be recovered.",
                    )
                    .to_owned(),
                    label,
                ));
                lines.push(Line::default());
                lines.push(Line::from(vec![
                    Span::styled(
                        trf(
                            "cli.erase_prompt",
                            "Type {word} to confirm: ",
                            &[("word", &crate::data_cli::word())],
                        ),
                        label,
                    ),
                    Span::styled(self.typed.clone(), focused),
                    Span::styled("▏", focused),
                ]));
            }
        }
        if let Some(error) = &self.error {
            lines.push(Line::default());
            lines.push(Line::styled(error.clone(), Style::default().fg(BAD)));
        }
        if let Some(done) = &self.done {
            lines.push(Line::default());
            lines.push(Line::styled(done.clone(), Style::default().fg(GOOD)));
        }
        lines
    }

    pub(crate) fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
        let areas = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        let title = match self.mode {
            Mode::Export => tr("history_actions.export_title", " EXPORT HISTORY "),
            Mode::Erase => tr("history_actions.erase_title", " ERASE HISTORY "),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT))
            .title(title);
        frame.render_widget(
            Paragraph::new(self.lines())
                .wrap(Wrap { trim: false })
                .block(block),
            areas[0],
        );
        let footer = if self.done.is_some() {
            tr("history_actions.done_footer", "Any key to go back")
        } else {
            match self.mode {
                Mode::Export => tr(
                    "history_actions.export_footer",
                    "Tab next field · ←→ change · Enter export · Esc back",
                ),
                Mode::Erase => tr(
                    "history_actions.erase_footer",
                    "Tab this account or all · Enter erase · Esc back",
                ),
            }
        };
        frame.render_widget(
            Paragraph::new(footer).style(Style::default().fg(MUTED)),
            areas[1],
        );
    }
}

fn unavailable() -> String {
    tr(
        "history_actions.unavailable",
        "The local history is unavailable; there is nothing to export or erase.",
    )
    .to_owned()
}

/// A file-name-safe form of an account name.
fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "account".to_owned()
    } else {
        slug
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `~/…` expands to the home directory; a relative path is taken from the
/// current directory.
fn resolve(typed: &str) -> PathBuf {
    let path = match typed.strip_prefix("~/").or(typed.strip_prefix("~\\")) {
        Some(rest) => home().map_or_else(|| PathBuf::from(typed), |home| home.join(rest)),
        None => PathBuf::from(typed),
    };
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(&path))
            .unwrap_or(path)
    }
}

/// The written path as shown: the home prefix abbreviated.
fn shown(path: &Path) -> String {
    cuotai_core::diagnostics::redact::home_abbreviated(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::config_file::ConfigIdentity;
    use cuotai_core::{Metric, MetricKind, MetricWindow};
    use ratatui::{backend::TestBackend, Terminal};

    const CONFIG: &str = r#"
[[accounts]]
name = "Work"
provider = "groq"
credential_env = "GROQ_KEY_WORK"
[[accounts]]
name = "Home"
provider = "groq"
credential_env = "GROQ_KEY_HOME"
"#;

    fn accounts() -> Vec<Account> {
        cuotai_core::Config::from_toml(CONFIG).unwrap().accounts
    }

    /// A history with one recent sample of `Work`.
    fn store(dir: &Path) -> HistoryStore {
        let store = HistoryStore::new(
            dir.join("history.sqlite3"),
            &ConfigIdentity::Explicit("history-actions".into()),
        );
        let metric = Metric {
            label: "5h".into(),
            used: Some(40.0),
            limit: Some(100.0),
            remaining: Some(60.0),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at: Some(chrono::Utc::now() + chrono::TimeDelta::hours(2)),
            billing: Default::default(),
            scope: None,
            derived_from: None,
        };
        let taken = chrono::Utc::now() - chrono::TimeDelta::minutes(30);
        store.record(&accounts()[0], taken, &[metric]).unwrap();
        store
    }

    fn render(view: &HistoryActions) -> String {
        let mut terminal = Terminal::new(TestBackend::new(90, 14)).expect("terminal");
        terminal.draw(|frame| view.draw(frame)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn type_text(view: &mut HistoryActions, text: &str, store: Option<&HistoryStore>) {
        for c in text.chars() {
            view.key(KeyCode::Char(c), store, &accounts());
        }
    }

    #[test]
    fn the_export_prompt_shows_scope_format_and_path() {
        crate::l10n::with_locale("en-US", || {
            let mut view = HistoryActions::new(Mode::Export, Some((0, "Work".into())));
            let text = render(&view);
            assert!(text.contains("EXPORT HISTORY"), "{text}");
            assert!(text.contains("Scope: This account: Work"), "{text}");
            assert!(text.contains("Format: CSV"), "{text}");
            assert!(text.contains("cuotai-history-work-"), "{text}");
            assert!(text.contains("Enter export"), "{text}");
            // Scope and format toggle; the default path follows both.
            view.key(KeyCode::Tab, None, &accounts());
            view.key(KeyCode::Right, None, &accounts());
            view.key(KeyCode::Tab, None, &accounts());
            view.key(KeyCode::Right, None, &accounts());
            let text = render(&view);
            assert!(text.contains("Scope: Every account"), "{text}");
            assert!(text.contains("Format: JSON"), "{text}");
            assert!(view.path.contains("cuotai-history-all-"), "{}", view.path);
            assert!(view.path.ends_with(".json"), "{}", view.path);
        });
    }

    #[test]
    fn export_writes_the_chosen_format_through_the_core_writer() {
        let dir = tempfile::tempdir().expect("dir");
        let store = store(dir.path());
        for (format, expect) in [(false, "account,"), (true, "{\"version\":1")] {
            let mut view = HistoryActions::new(Mode::Export, Some((0, "Work".into())));
            if format {
                view.focus = Focus::Format;
                view.key(KeyCode::Right, Some(&store), &accounts());
            }
            let out = dir.path().join(format!("out.{}", view.format.extension()));
            view.path = out.display().to_string();
            assert_eq!(
                view.key(KeyCode::Enter, Some(&store), &accounts()),
                Reply::Stay
            );
            assert!(view.error.is_none(), "{:?}", view.error);
            let written = std::fs::read_to_string(&out).expect("file");
            assert!(written.starts_with(expect), "{written}");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600);
            }
            let done = view.done.clone().expect("result");
            // Rows and the written path (the CLI's own result text).
            assert!(done.starts_with("1 "), "{done}");
            assert!(done.contains(&shown(&out)), "{done}");
            // Any key closes the result.
            assert_eq!(view.key(KeyCode::Char('x'), None, &[]), Reply::Close);
        }
    }

    #[test]
    fn an_empty_path_or_no_history_is_an_error_not_a_panic() {
        crate::l10n::with_locale("en-US", || {
            let mut view = HistoryActions::new(Mode::Export, None);
            view.key(KeyCode::Enter, None, &accounts());
            assert!(view.error.as_deref().unwrap().contains("unavailable"));
            let dir = tempfile::tempdir().expect("dir");
            let store = store(dir.path());
            view.path.clear();
            view.key(KeyCode::Enter, Some(&store), &accounts());
            assert!(view.error.as_deref().unwrap().contains("Type the path"));
            assert!(view.done.is_none());
        });
    }

    #[test]
    fn the_erase_confirmation_lists_the_scope_and_asks_for_the_word() {
        crate::l10n::with_locale("en-US", || {
            let mut view = HistoryActions::new(Mode::Erase, Some((1, "Home".into())));
            let text = render(&view);
            assert!(text.contains("ERASE HISTORY"), "{text}");
            assert!(
                text.contains("The history of Home will be erased."),
                "{text}"
            );
            assert!(text.contains("Type ERASE to confirm:"), "{text}");
            view.key(KeyCode::Tab, None, &accounts());
            let text = render(&view);
            assert!(
                text.contains("The history of every account on this device will be erased."),
                "{text}"
            );
        });
    }

    #[test]
    fn a_wrong_word_refuses_and_esc_cancels() {
        crate::l10n::with_locale("en-US", || {
            let dir = tempfile::tempdir().expect("dir");
            let store = store(dir.path());
            let mut view = HistoryActions::new(Mode::Erase, Some((0, "Work".into())));
            type_text(&mut view, "erase", Some(&store));
            assert_eq!(
                view.key(KeyCode::Enter, Some(&store), &accounts()),
                Reply::Stay
            );
            assert!(view.error.as_deref().unwrap().contains("does not match"));
            assert!(view.done.is_none());
            assert!(view.typed.is_empty());
            assert_eq!(
                view.key(KeyCode::Esc, Some(&store), &accounts()),
                Reply::Close
            );
        });
    }

    #[test]
    fn the_exact_word_erases_and_reports_the_result() {
        crate::l10n::with_locale("en-US", || {
            let dir = tempfile::tempdir().expect("dir");
            let store = store(dir.path());
            for (account, removed) in [(Some((0, "Work".to_owned())), 1), (None, 0)] {
                let mut view = HistoryActions::new(Mode::Erase, account);
                type_text(&mut view, "ERASE", Some(&store));
                assert_eq!(
                    view.key(KeyCode::Enter, Some(&store), &accounts()),
                    Reply::Erased
                );
                let text = render(&view);
                assert!(
                    text.contains(&format!("{removed} history samples erased.")),
                    "{text}"
                );
                assert!(text.contains("Any key to go back"), "{text}");
            }
        });
    }

    #[test]
    fn the_word_is_localized() {
        crate::l10n::with_locale("pt-BR", || {
            let mut view = HistoryActions::new(Mode::Erase, None);
            let dir = tempfile::tempdir().expect("dir");
            let store = store(dir.path());
            type_text(&mut view, "ERASE", Some(&store));
            assert_eq!(
                view.key(KeyCode::Enter, Some(&store), &accounts()),
                Reply::Stay
            );
            type_text(&mut view, "APAGAR", Some(&store));
            assert_eq!(
                view.key(KeyCode::Enter, Some(&store), &accounts()),
                Reply::Erased
            );
        });
    }

    #[test]
    fn paths_expand_home_and_slugs_are_file_safe() {
        assert_eq!(slug("Work / Team@x"), "work---team-x");
        assert_eq!(slug("çã"), "account");
        assert!(resolve("relative.csv").is_absolute());
        if let Some(home) = home() {
            assert_eq!(resolve("~/a.csv"), home.join("a.csv"));
        }
    }
}
