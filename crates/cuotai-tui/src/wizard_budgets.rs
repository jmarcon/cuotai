//! PRD 0041 RF5.2: the wizard's **Budgets and floors** step.
//!
//! The rows, their choices, texts, parsing and validation come from the
//! core (`budget::editor`, `budget::problems`, `budget::describe`); this
//! module only keeps the transient draft (which row, which field, the
//! typed filter of the metric picker) and draws it.
//!
//! Keys: in the list, ↑↓ pick a row, `a` adds a budget, `f` adds a floor,
//! `e` or → edits the row, `x` or Delete removes it, Enter goes on. While a
//! row is edited, ↑↓ move between its fields, ←→ change a choice, typing
//! fills a text field (on the metric, it filters the choices), and Enter
//! or Esc returns to the list.

use crate::l10n::{tr, trf};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use cuotai_core::budget::editor::{self, BudgetRow, FloorRow, PERIODS};
use cuotai_core::budget::{self, Budget, Floor, MAX_BUDGETS, MAX_FLOORS};
use cuotai_core::text::Localized;
use cuotai_core::{Account, Provider};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The fields of a budget row, in order.
const BUDGET_FIELDS: usize = 7;
/// The fields of a floor row, in order.
const FLOOR_FIELDS: usize = 4;
/// Longest typed text of a field (the core rejects longer ones).
const TEXT_MAX: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Budget(usize),
    Floor(usize),
}

#[derive(Debug, Default)]
pub(super) struct BudgetStep {
    budgets: Vec<BudgetRow>,
    floors: Vec<FloorRow>,
    /// The highlighted row: budgets first, then floors.
    selected: usize,
    /// `Some(field)` while the highlighted row is edited.
    editing: Option<usize>,
    /// The typed filter of the metric picker (RF5.2 type-to-filter).
    filter: String,
    /// F-07: the metrics of the edited account's last good reading, so a
    /// budget in another unit than its metric is refused; `None` for a new
    /// account or before any reading.
    reading: Option<Vec<cuotai_core::Metric>>,
    /// F-10: the rows added in this session whose amount was not typed yet;
    /// their empty-amount problem stays hidden in the list (Enter still
    /// stops on it), budgets first, then floors.
    fresh: Vec<bool>,
}

pub(super) struct Colors {
    pub prompt: Color,
    pub accent: Color,
    pub dim: Color,
    pub bad: Color,
}

impl BudgetStep {
    /// The rows of `account` (an edit of the same provider), else none.
    pub(super) fn of(account: Option<&Account>) -> Self {
        let mut step = Self::default();
        if let Some(account) = account {
            let locale = crate::ui::tui_locale();
            step.budgets = account
                .budgets
                .iter()
                .map(|budget| BudgetRow::from_budget(budget, locale))
                .collect();
            step.floors = account
                .floors
                .iter()
                .map(|floor| FloorRow::from_floor(floor, locale))
                .collect();
            step.fresh = vec![false; step.rows()];
        }
        step
    }

    /// F-07: check the rows against `metrics`, the edited account's last
    /// good reading.
    pub(super) fn with_reading(mut self, metrics: Option<Vec<cuotai_core::Metric>>) -> Self {
        self.reading = metrics;
        self
    }

    /// The core's check of the rows for `provider` (RF1.2, F-05, F-07,
    /// F-10, F-11), amounts read in the TUI's language.
    fn check(&self, provider: Provider) -> editor::RowCheck {
        editor::check_rows(
            provider,
            &self.budgets,
            &self.floors,
            crate::ui::tui_locale(),
            self.reading.as_deref(),
        )
    }

    /// The budgets the draft saves.
    pub(super) fn budgets(&self) -> Vec<Budget> {
        let locale = crate::ui::tui_locale();
        self.budgets
            .iter()
            .map(|row| row.to_budget(locale))
            .collect()
    }

    /// The floors the draft saves.
    pub(super) fn floors(&self) -> Vec<Floor> {
        let locale = crate::ui::tui_locale();
        self.floors.iter().map(|row| row.to_floor(locale)).collect()
    }

    /// F-10: whether row `index` (budgets first) shows `problem`: an empty
    /// amount of a row added in this session waits until it is typed.
    fn shows(&self, index: usize, problem: &editor::RowProblem) -> bool {
        let empty = match self.row(index) {
            Some(Row::Budget(i)) => self.budgets[i].amount.trim().is_empty(),
            Some(Row::Floor(i)) => self.floors[i].amount.trim().is_empty(),
            None => false,
        };
        !(empty
            && problem.rule == budget::BudgetRule::Amount
            && problem.field.ends_with(".amount")
            && self.fresh.get(index).copied().unwrap_or(false))
    }

    /// The first problem of row `index` of `list` the list shows.
    fn row_note(
        &self,
        problems: &[editor::RowProblem],
        list: &str,
        index: usize,
        row: usize,
    ) -> Option<Localized> {
        let shown: Vec<editor::RowProblem> = problems
            .iter()
            .filter(|problem| self.shows(row, problem))
            .cloned()
            .collect();
        editor::row_problem(&shown, list, index)
    }

    /// The first problem of the rows for `provider` (RF1.2), localized.
    pub(super) fn problem(&self, provider: Provider) -> Option<Localized> {
        self.check(provider)
            .problems
            .into_iter()
            .next()
            .map(|problem| problem.note)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.budgets.is_empty() && self.floors.is_empty()
    }

    fn rows(&self) -> usize {
        self.budgets.len() + self.floors.len()
    }

    fn row(&self, index: usize) -> Option<Row> {
        if index < self.budgets.len() {
            Some(Row::Budget(index))
        } else if index < self.rows() {
            Some(Row::Floor(index - self.budgets.len()))
        } else {
            None
        }
    }

    fn fields(&self) -> usize {
        match self.row(self.selected) {
            Some(Row::Budget(_)) => BUDGET_FIELDS,
            _ => FLOOR_FIELDS,
        }
    }

    /// `true` while a row is edited: Esc then returns to the list instead
    /// of cancelling the wizard.
    pub(super) fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// The key, when the step uses it (`Some(())`); `None` leaves it to the
    /// wizard (Enter, Shift+Tab and Esc in the list).
    pub(super) fn handle_key(&mut self, key: KeyEvent, provider: Provider) -> Option<()> {
        match self.editing {
            None => self.list_key(key, provider),
            Some(field) => {
                self.edit_key(key, field, provider);
                Some(())
            }
        }
    }

    fn list_key(&mut self, key: KeyEvent, provider: Provider) -> Option<()> {
        let plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;
        match key.code {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.rows().saturating_sub(1));
            }
            KeyCode::Char('a') | KeyCode::Char('A') if plain => {
                if self.budgets.len() < MAX_BUDGETS {
                    let mut row = BudgetRow::new(provider);
                    // F-11: the unit of the metric in the last reading.
                    if let Some(unit) = self.reading.as_deref().and_then(|metrics| {
                        editor::metric_unit(&row.to_budget(crate::ui::tui_locale()), metrics)
                    }) {
                        row.unit = unit;
                    }
                    self.budgets.push(row);
                    self.fresh.insert(self.budgets.len() - 1, true);
                    self.selected = self.budgets.len() - 1;
                    self.start_edit();
                }
            }
            KeyCode::Char('f') | KeyCode::Char('F') if plain => {
                if self.floors.len() < MAX_FLOORS {
                    let mut row = FloorRow::default();
                    if let Some(first) = editor::floor_targets(provider).first() {
                        row.target = *first;
                    }
                    self.floors.push(row);
                    self.fresh.push(true);
                    self.selected = self.rows() - 1;
                    self.start_edit();
                }
            }
            KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Right if plain => {
                if self.row(self.selected).is_some() {
                    self.start_edit();
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete if plain => {
                match self.row(self.selected) {
                    Some(Row::Budget(i)) => {
                        self.budgets.remove(i);
                    }
                    Some(Row::Floor(i)) => {
                        self.floors.remove(i);
                    }
                    None => {}
                }
                if self.selected < self.fresh.len() {
                    self.fresh.remove(self.selected);
                }
                self.selected = self.selected.min(self.rows().saturating_sub(1));
            }
            KeyCode::Char(_) => {}
            _ => return None,
        }
        Some(())
    }

    fn start_edit(&mut self) {
        self.editing = Some(0);
        self.filter.clear();
    }

    fn edit_key(&mut self, key: KeyEvent, field: usize, provider: Provider) {
        let last = self.fields() - 1;
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                self.editing = None;
                self.filter.clear();
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.editing = Some(field.saturating_sub(1));
                self.filter.clear();
            }
            KeyCode::Down | KeyCode::Tab => {
                self.editing = Some((field + 1).min(last));
                self.filter.clear();
            }
            KeyCode::Left => self.cycle(field, -1, provider),
            KeyCode::Right => self.cycle(field, 1, provider),
            KeyCode::Backspace => {
                if field == 0 {
                    self.filter.pop();
                    self.apply_filter(provider);
                } else if let Some(text) = self.text_mut(field) {
                    text.pop();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if field == 0 {
                    self.filter.clear();
                } else if let Some(text) = self.text_mut(field) {
                    text.clear();
                }
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                if field == 0 {
                    if self.filter.chars().count() < TEXT_MAX {
                        self.filter.push(c);
                        self.apply_filter(provider);
                    }
                } else if let Some(text) = self.text_mut(field) {
                    if text.chars().count() < TEXT_MAX {
                        text.push(c);
                    }
                    if field == 1 {
                        if let Some(fresh) = self.fresh.get_mut(self.selected) {
                            *fresh = false;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The typed filter picks the first matching metric choice.
    fn apply_filter(&mut self, provider: Provider) {
        let locale = crate::ui::tui_locale();
        match self.row(self.selected) {
            Some(Row::Budget(i)) => {
                if let Some(first) = editor::targets(provider, &self.filter, locale).first() {
                    self.budgets[i].target = *first;
                }
            }
            Some(Row::Floor(i)) => {
                let query = cuotai_core::text::fold(self.filter.trim());
                if let Some(first) = editor::floor_targets(provider).into_iter().find(|target| {
                    cuotai_core::text::fold(&target.text().resolve(locale)).contains(&query)
                }) {
                    self.floors[i].target = first;
                }
            }
            None => {}
        }
    }

    fn text_mut(&mut self, field: usize) -> Option<&mut String> {
        match self.row(self.selected)? {
            Row::Budget(i) => {
                let row = &mut self.budgets[i];
                match field {
                    1 => Some(&mut row.amount),
                    2 => Some(&mut row.unit),
                    5 => Some(&mut row.filter),
                    6 => Some(&mut row.label),
                    _ => None,
                }
            }
            Row::Floor(i) => {
                let row = &mut self.floors[i];
                match field {
                    1 => Some(&mut row.amount),
                    2 => Some(&mut row.unit),
                    3 => Some(&mut row.filter),
                    _ => None,
                }
            }
        }
    }

    fn cycle(&mut self, field: usize, delta: isize, provider: Provider) {
        let locale = crate::ui::tui_locale();
        let step = |len: usize, at: usize| -> usize {
            (at as isize + delta).rem_euclid(len.max(1) as isize) as usize
        };
        match self.row(self.selected) {
            Some(Row::Budget(i)) => {
                let row = &mut self.budgets[i];
                match field {
                    0 => {
                        let choices = editor::targets(provider, &self.filter, locale);
                        let at = choices.iter().position(|t| *t == row.target).unwrap_or(0);
                        if let Some(next) = choices.get(step(choices.len(), at)) {
                            row.target = *next;
                        }
                    }
                    3 => {
                        let at = PERIODS.iter().position(|p| *p == row.period).unwrap_or(0);
                        row.period = PERIODS[step(PERIODS.len(), at)];
                        row.start = None;
                    }
                    4 => {
                        let choices = editor::starts(row.period);
                        let at = choices.iter().position(|s| *s == row.start).unwrap_or(0);
                        row.start = choices[step(choices.len(), at)];
                    }
                    _ => {}
                }
            }
            Some(Row::Floor(i)) if field == 0 => {
                let row = &mut self.floors[i];
                let choices = editor::floor_targets(provider);
                let at = choices.iter().position(|t| *t == row.target).unwrap_or(0);
                if let Some(next) = choices.get(step(choices.len(), at)) {
                    row.target = *next;
                }
            }
            _ => {}
        }
    }

    /// The step's body.
    pub(super) fn lines(&self, provider: Provider, colors: &Colors) -> Vec<Line<'static>> {
        let locale = crate::ui::tui_locale();
        let prompt = Style::default().fg(colors.prompt);
        let accent = Style::default().fg(colors.accent);
        let dim = Style::default().fg(colors.dim);
        let bad = Style::default().fg(colors.bad);
        let check = self.check(provider);
        let problems = check.problems;
        let mut lines = vec![
            Line::from(Span::styled(
                tr(
                    "wizard.budgets.prompt",
                    "Budgets and floors (optional): your own limits on spend, usage or credits, and the balance level that warns you.",
                ),
                prompt,
            )),
            Line::default(),
            Line::from(Span::styled(
                tr("wizard.budgets.budgets", "Budgets"),
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ];
        if self.budgets.is_empty() {
            lines.push(Line::from(Span::styled(
                tr("wizard.budgets.none", "  none"),
                dim,
            )));
        }
        let marker = |index: usize| if index == self.selected { "> " } else { "  " };
        for (i, row) in self.budgets.iter().enumerate() {
            let sentence = check
                .sentences
                .get(i)
                .map(|sentence| sentence.resolve(locale))
                .unwrap_or_default();
            let text = format!(
                "{}{} · {}",
                marker(i),
                row.target.text().resolve(locale),
                sentence
            );
            lines.push(Line::from(Span::styled(
                crate::ui::safe(&text).into_owned(),
                if i == self.selected {
                    accent
                } else {
                    Style::default()
                },
            )));
            if self.editing.is_some() && i == self.selected {
                lines.extend(self.budget_fields(row, provider, accent, dim));
            }
            if let Some(note) = self.row_note(&problems, "budgets", i, i) {
                lines.push(Line::from(Span::styled(
                    format!("    {}", note.resolve(locale)),
                    bad,
                )));
            }
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            tr("wizard.budgets.floors", "Balance floors"),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if self.floors.is_empty() {
            lines.push(Line::from(Span::styled(
                tr("wizard.budgets.none", "  none"),
                dim,
            )));
        }
        for (j, row) in self.floors.iter().enumerate() {
            let index = self.budgets.len() + j;
            let text = trf(
                "wizard.budgets.floor_row",
                "{marker}{target}: warn below {amount} {unit}",
                &[
                    ("marker", &marker(index)),
                    ("target", &row.target.text().resolve(locale)),
                    ("amount", &crate::ui::safe(&row.amount)),
                    ("unit", &crate::ui::safe(&row.unit)),
                ],
            );
            lines.push(Line::from(Span::styled(
                text.trim_end().to_owned(),
                if index == self.selected {
                    accent
                } else {
                    Style::default()
                },
            )));
            if self.editing.is_some() && index == self.selected {
                lines.extend(self.floor_fields(row, provider, accent, dim));
            }
            if let Some(note) = self.row_note(&problems, "floors", j, index) {
                lines.push(Line::from(Span::styled(
                    format!("    {}", note.resolve(locale)),
                    bad,
                )));
            }
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            trf(
                "wizard.budgets.help",
                "Up to {budgets} budgets and {floors} floors. Cuotai only reads and warns; it never changes anything at the provider.",
                &[("budgets", &MAX_BUDGETS), ("floors", &MAX_FLOORS)],
            ),
            dim,
        )));
        lines
    }

    fn field_line(
        &self,
        index: usize,
        label: &str,
        value: String,
        choice: bool,
        styles: (Style, Style),
    ) -> Line<'static> {
        let focused = self.editing == Some(index);
        let value = if choice && focused {
            format!("‹ {value} ›")
        } else {
            value
        };
        Line::from(vec![
            Span::raw(if focused { "    » " } else { "      " }),
            Span::styled(format!("{label}: "), styles.1),
            Span::styled(value, if focused { styles.0 } else { Style::default() }),
        ])
    }

    fn filter_hint(&self, dim: Style) -> Option<Line<'static>> {
        (self.editing == Some(0)).then(|| {
            let text = if self.filter.is_empty() {
                tr("wizard.budgets.filter_empty", "type to filter the metrics").to_owned()
            } else {
                trf(
                    "wizard.budgets.filter",
                    "filter: {text}",
                    &[("text", &crate::ui::safe(&self.filter))],
                )
            };
            Line::from(Span::styled(format!("        {text}"), dim))
        })
    }

    fn budget_fields(
        &self,
        row: &BudgetRow,
        provider: Provider,
        accent: Style,
        dim: Style,
    ) -> Vec<Line<'static>> {
        let locale = crate::ui::tui_locale();
        let styles = (accent, dim);
        let safe = |text: &str| crate::ui::safe(text).into_owned();
        let choices = editor::targets(provider, &self.filter, locale);
        let target = if choices.is_empty() {
            tr("wizard.budgets.no_match", "no metric matches").to_owned()
        } else {
            row.target.text().resolve(locale)
        };
        let mut lines = vec![self.field_line(
            0,
            tr("wizard.budgets.field.metric", "Metric"),
            target,
            true,
            styles,
        )];
        lines.extend(self.filter_hint(dim));
        lines.extend([
            self.field_line(
                1,
                tr("wizard.budgets.field.amount", "Amount"),
                safe(&row.amount),
                false,
                styles,
            ),
            self.field_line(
                2,
                tr("wizard.budgets.field.unit", "Unit (empty: the metric's)"),
                safe(&row.unit),
                false,
                styles,
            ),
            self.field_line(
                3,
                tr("wizard.budgets.field.period", "Period"),
                editor::period_text(row.period).resolve(locale),
                true,
                styles,
            ),
            self.field_line(
                4,
                tr("wizard.budgets.field.start", "Starts"),
                editor::start_text(row.period, row.start).resolve(locale),
                true,
                styles,
            ),
            self.field_line(
                5,
                tr("wizard.budgets.field.filter", "Label contains (optional)"),
                safe(&row.filter),
                false,
                styles,
            ),
            self.field_line(
                6,
                tr("wizard.budgets.field.label", "Name (optional)"),
                safe(&row.label),
                false,
                styles,
            ),
        ]);
        lines
    }

    fn floor_fields(
        &self,
        row: &FloorRow,
        _provider: Provider,
        accent: Style,
        dim: Style,
    ) -> Vec<Line<'static>> {
        let locale = crate::ui::tui_locale();
        let styles = (accent, dim);
        let safe = |text: &str| crate::ui::safe(text).into_owned();
        let mut lines = vec![self.field_line(
            0,
            tr("wizard.budgets.field.balance", "Balance"),
            row.target.text().resolve(locale),
            true,
            styles,
        )];
        lines.extend(self.filter_hint(dim));
        lines.extend([
            self.field_line(
                1,
                tr("wizard.budgets.field.floor_amount", "Warn below"),
                safe(&row.amount),
                false,
                styles,
            ),
            self.field_line(
                2,
                tr("wizard.budgets.field.unit", "Unit (empty: the metric's)"),
                safe(&row.unit),
                false,
                styles,
            ),
            self.field_line(
                3,
                tr("wizard.budgets.field.filter", "Label contains (optional)"),
                safe(&row.filter),
                false,
                styles,
            ),
        ]);
        lines
    }

    /// The footer hint of the step.
    pub(super) fn hint(&self) -> &'static str {
        if self.is_editing() {
            tr(
                "wizard.hint.budgets_edit",
                " ↑↓ fields  ←→ choices  type to fill (on Metric: to filter)  Ctrl+U clear  Enter/Esc back to the list",
            )
        } else {
            tr(
                "wizard.hint.budgets",
                " a add budget  f add floor  e edit  x remove  ↑↓ rows  Enter next  Shift+Tab back  Esc cancel",
            )
        }
    }

    /// The review's lines: one per budget sentence and floor.
    pub(super) fn summary_lines(&self, provider: Provider, dim: Style) -> Vec<Line<'static>> {
        if self.is_empty() {
            return Vec::new();
        }
        let locale = crate::ui::tui_locale();
        let mut lines = vec![Line::from(Span::styled(
            trf(
                "wizard.summary.budgets",
                "Budgets: {budgets} · floors: {floors}",
                &[
                    ("budgets", &self.budgets.len()),
                    ("floors", &self.floors.len()),
                ],
            ),
            dim,
        ))];
        for sentence in self.check(provider).sentences {
            lines.push(Line::from(format!(
                "  {}",
                crate::ui::safe(&sentence.resolve(locale))
            )));
        }
        lines
    }
}

#[cfg(test)]
#[path = "wizard_budgets_amount_tests.rs"]
mod amount_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn typed(step: &mut BudgetStep, text: &str, provider: Provider) {
        for c in text.chars() {
            step.handle_key(key(KeyCode::Char(c)), provider);
        }
    }

    #[test]
    fn add_edit_and_remove_rows() {
        let provider = Provider::OpenAiApi;
        let mut step = BudgetStep::default();
        assert!(step.problem(provider).is_none());
        step.handle_key(key(KeyCode::Char('a')), provider);
        assert!(step.is_editing());
        // Type-to-filter on the metric: "cred" picks Credits.
        typed(&mut step, "cred", provider);
        assert_eq!(step.budgets[0].target, editor::Target::Credits);
        step.handle_key(key(KeyCode::Down), provider);
        typed(&mut step, "50", provider);
        // Without a valid amount the core rejects the row; with one it passes.
        assert!(step.problem(provider).is_none());
        step.handle_key(key(KeyCode::Down), provider);
        step.handle_key(key(KeyCode::Down), provider);
        step.handle_key(key(KeyCode::Right), provider); // Monthly → Weekly
        step.handle_key(key(KeyCode::Down), provider);
        step.handle_key(key(KeyCode::Right), provider); // default → Monday
        step.handle_key(key(KeyCode::Esc), provider);
        assert!(!step.is_editing());
        let budgets = step.budgets();
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0].amount, 50.0);
        assert_eq!(budgets[0].period, budget::BudgetPeriod::Weekly);
        assert_eq!(
            budgets[0].starts,
            Some(budget::BudgetStart::Weekday(budget::WeekdayName::Monday))
        );
        // A floor without an amount is a problem until it gets one.
        step.handle_key(key(KeyCode::Char('f')), provider);
        step.handle_key(key(KeyCode::Enter), provider);
        assert!(step.problem(provider).is_some());
        step.handle_key(key(KeyCode::Char('x')), provider);
        assert!(step.floors.is_empty());
        assert!(step.problem(provider).is_none());
        // Enter in the list is the wizard's (next step).
        assert_eq!(step.handle_key(key(KeyCode::Enter), provider), None);
        assert_eq!(step.handle_key(key(KeyCode::Esc), provider), None);
    }

    /// F-07: with the account's last reading, a budget in another
    /// currency than its metric is refused, naming both.
    #[test]
    fn a_budget_in_another_currency_than_the_reading_is_refused() {
        let provider = Provider::OpenAiApi;
        let spend = cuotai_core::Metric::usage(
            "Gasto no mês",
            10.0,
            "USD",
            cuotai_core::MetricKind::Spend,
            None,
        );
        let mut step = BudgetStep::default().with_reading(Some(vec![spend]));
        step.handle_key(key(KeyCode::Char('a')), provider);
        step.handle_key(key(KeyCode::Down), provider);
        typed(&mut step, "50", provider);
        step.handle_key(key(KeyCode::Down), provider);
        typed(&mut step, "EUR", provider);
        let note = step.problem(provider).expect("refused");
        assert_eq!(note.id, "budget.error.unit_mismatch");
        let text = note.resolve("en-US");
        assert!(text.contains("USD") && text.contains("EUR"), "{text}");
        // In the metric's own currency the row passes.
        let mut clear = key(KeyCode::Char('u'));
        clear.modifiers = KeyModifiers::CONTROL;
        step.handle_key(clear, provider);
        typed(&mut step, "USD", provider);
        assert!(step.problem(provider).is_none());
    }

    #[test]
    fn rows_of_an_account_round_trip_and_respect_the_limits() {
        let mut account = Account {
            name: "x".into(),
            provider: Provider::OpenRouter,
            ..Account::default()
        };
        account.budgets = vec![Budget {
            kind: Some(budget::BudgetKind::Spend),
            scope: None,
            label_contains: None,
            amount: 25.0,
            unit: Some("USD".into()),
            period: budget::BudgetPeriod::Monthly,
            starts: Some(budget::BudgetStart::Day(3)),
            label: None,
            row: None,
        }];
        let mut step = BudgetStep::of(Some(&account));
        assert_eq!(step.budgets(), account.budgets);
        for _ in 0..20 {
            step.handle_key(key(KeyCode::Char('a')), Provider::OpenRouter);
            step.handle_key(key(KeyCode::Esc), Provider::OpenRouter);
        }
        assert_eq!(step.budgets.len(), MAX_BUDGETS);
        // OpenRouter reports no credits kind: the filter finds nothing and
        // the target stays one the provider reports.
        step.handle_key(key(KeyCode::Char('e')), Provider::OpenRouter);
        typed(&mut step, "cred", Provider::OpenRouter);
        assert_ne!(step.budgets[step.selected].target, editor::Target::Credits);
    }
}
