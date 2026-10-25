use crate::{
    about_view::{AboutView, Reply as AboutReply},
    auth_ui::{LoginTask, Outcome},
    changelog_view::{ChangelogView, Reply as ChangelogReply},
    cost::LocalCost,
    dashboard::{self, FooterAction},
    extensions_view::{Action as ExtensionAction, ExtensionsView},
    models_view::{ModelsView, Reply},
    motion::{self, Anim},
    notify::{self, Notices},
    wizard::{Action, VaultChange, Wizard},
};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use cuotai_core::{
    account_edit::{self, RemoveError, RemoveOutcome, RemovedAccount},
    config_file::ConfigFile,
    history::{HistoryRange, Retention},
    incidents::{IncidentMonitor, ProviderStatus},
    login::{self, CompletedLogin},
    models::ModelCache,
    oauth::OAuthSession,
    pace::Point,
    refresh::{Edit, RefreshCycle},
    user_session, Account, Config, EnvironmentCredentials, Metric, Monitor, Provider, Snapshot,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, TableState, Wrap},
    Terminal,
};
use std::{
    borrow::Cow,
    io::{self, Write},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;

type Screen = Terminal<CrosstermBackend<io::Stdout>>;

// Paleta Dracula (mesma usada pelo dashboard/wizard/auth).
const DRACULA_BG: Color = Color::Rgb(40, 42, 54);
const DRACULA_FG: Color = Color::Rgb(248, 248, 242);
const DRACULA_YELLOW: Color = Color::Rgb(241, 250, 140);

/// PRD 0011 RF5: how often the status loop lets the core read the due
/// sources (each is read at most once every 5 minutes), as the desktop.
pub(crate) const STATUS_TICK: Duration = Duration::from_secs(30);

/// PRD 0015 RF4.2: how often the details' local cost is read again from the
/// index (the background scan itself runs every 5 minutes).
const COST_TICK: Duration = Duration::from_secs(15);

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

struct Refresh(Option<(JoinHandle<Snapshot>, cuotai_core::refresh::Generation)>);
/// Per-account refresh table. `jobs` is the in-flight Tokio handle;
/// `tokens` is the matching cycle generation tag returned by
/// `RefreshCycle::begin`. Finish paths pass the token back so the cycle
/// can reject results that landed after an edit/remove re-tagged the slot.
struct AccountRefresh {
    jobs: Vec<Option<JoinHandle<cuotai_core::Report>>>,
    tokens: Vec<Option<cuotai_core::refresh::Generation>>,
}
impl AccountRefresh {
    fn new(len: usize) -> Self {
        Self {
            jobs: (0..len).map(|_| None).collect(),
            tokens: (0..len).map(|_| None).collect(),
        }
    }
    fn slot(&self, index: usize) -> Option<&JoinHandle<cuotai_core::Report>> {
        self.jobs.get(index).and_then(Option::as_ref)
    }
    fn install(
        &mut self,
        index: usize,
        job: JoinHandle<cuotai_core::Report>,
        token: cuotai_core::refresh::Generation,
    ) {
        self.jobs[index] = Some(job);
        self.tokens[index] = Some(token);
    }
    fn take(
        &mut self,
        index: usize,
    ) -> Option<(
        JoinHandle<cuotai_core::Report>,
        cuotai_core::refresh::Generation,
    )> {
        let job = self.jobs.get_mut(index).and_then(Option::take)?;
        let token = self.tokens.get_mut(index).and_then(Option::take)?;
        Some((job, token))
    }
    fn len(&self) -> usize {
        self.jobs.len()
    }
    fn extend_to(&mut self, new_len: usize) {
        while self.jobs.len() < new_len {
            self.jobs.push(None);
            self.tokens.push(None);
        }
    }
    fn truncate_to(&mut self, new_len: usize) {
        self.jobs.truncate(new_len);
        self.tokens.truncate(new_len);
    }
    fn remove(&mut self, index: usize) {
        if index < self.jobs.len() {
            let last = self.jobs.len() - 1;
            self.jobs[index..].rotate_left(1);
            self.jobs.truncate(last);
            self.tokens[index..].rotate_left(1);
            self.tokens.truncate(last);
        }
    }
    fn move_account(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        rotate_span(&mut self.jobs, from, to);
        rotate_span(&mut self.tokens, from, to);
    }
    fn clear_slot(&mut self, index: usize) {
        if let Some(slot) = self.jobs.get_mut(index) {
            if let Some(handle) = slot.take() {
                handle.abort();
            }
        }
        if let Some(slot) = self.tokens.get_mut(index) {
            *slot = None;
        }
    }
}
/// Moves the element at `from` to `to`, shifting the ones in between by one
/// slot. Same result as `remove(from)` followed by `insert(to, ..)`, done by
/// rotating the affected span in place. Panics when either index is out of
/// range, like the remove/insert pair it replaces.
fn rotate_span<T>(items: &mut [T], from: usize, to: usize) {
    if from < to {
        items[from..=to].rotate_left(1);
    } else {
        items[to..=from].rotate_right(1);
    }
}

impl Drop for AccountRefresh {
    fn drop(&mut self) {
        self.abort_all();
    }
}
impl AccountRefresh {
    fn abort_all(&mut self) {
        for job in &mut self.jobs {
            if let Some(job) = job.take() {
                job.abort();
            }
        }
        self.tokens.fill(None);
    }
}
impl Drop for Refresh {
    fn drop(&mut self) {
        if let Some((handle, _token)) = self.0.take() {
            handle.abort();
        }
    }
}

/// TUI-level state machine for account removal. RF3 splits the wizard into
/// two confirmation steps because the second question (purge credentials?)
/// has a different default and a different consequence: declining the
/// first leaves the account in place; declining the second keeps a vault
/// entry that the user might not realise is shared.
///
/// `Result` carries the typed `RemoveOutcome` so the dialog can render a
/// truthful partial-failure state without leaking secrets.
enum DeletionState {
    Confirm {
        index: usize,
    },
    AskPurge {
        index: usize,
    },
    Executing {
        index: usize,
        purge: bool,
    },
    Result {
        index: usize,
        outcome: Box<RemoveOutcome>,
    },
}

pub async fn run(
    mut file: ConfigFile,
    session: Arc<OAuthSession>,
    animations: bool,
    retention: Retention,
    notify: bool,
    status: bool,
    local_cost: Option<LocalCost>,
) -> io::Result<()> {
    // PRD 0011 RF5: off unless opted in; shared by every rebuilt monitor.
    let incidents = IncidentMonitor::new();
    incidents.set_enabled(status);
    let mut monitor = new_monitor(&file, &session, retention, &incidents)?;
    let mut auth_task: Option<LoginTask> = None;
    let mut authorized: Option<CompletedLogin> = None;
    let mut saving_config: Option<(Config, Option<usize>)> = None;
    // PRD 0016 RF2: pasted sessions written to the vault for a save that
    // waits on a login commit; undone when that commit rolls back.
    let mut saving_paste: Option<user_session::StagedSession> = None;
    // PRD 0016 RF2.1: the wizard's read-only session test, at most one.
    let mut session_test: Option<JoinHandle<cuotai_core::Report>> = None;
    // Whether the terminal currently sends pastes as one event.
    let mut bracketed_paste = false;
    // PRD 0017 RF5.11: onboarding is for a first run with no accounts at all.
    // A configured launch shows the dashboard and starts its refresh, even with
    // extension folders present; the manager opens with `x`.
    let first_run = !file.exists || file.config.accounts.is_empty();
    let mut wizard = first_run.then(|| Wizard::new(&file.config, None));
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        previous_hook(info);
    }));
    enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut state = State {
        accounts: file.config.accounts.clone(),
        refresh_seconds: file.config.refresh_seconds,
        selection: TableState::default(),
        filter: String::new(),
        filtering: false,
        location: location(&file),
        error: None,
        detail_scroll: 0,
        show_unmeasured: true,
        details: false,
        targets: dashboard::Targets::default(),
        model_counts: ModelCounts::default(),
        history: HistoryView::default(),
        history_range: HistoryRange::default(),
        anim: animations.then(|| Anim::new(motion::truecolor(|key| std::env::var(key).ok()))),
        notices: notify.then(|| {
            Notices::new(
                chrono::Utc::now(),
                notify::osc9_supported(|key| std::env::var(key).ok()),
            )
        }),
        incidents: Vec::new(),
        local_cost: None,
    };
    let mut frame_area = Rect::default();
    let mut refresh_cycle = RefreshCycle::new(&file.config, Instant::now());
    // PRD 0010 RF2.3: the last recorded readings show, stale, until the
    // first query of each account answers.
    refresh_cycle.restore_last_good(monitor.restored_readings());
    let mut refresh = Refresh(if wizard.is_none() {
        // Claim the full-sweep token BEFORE spawning the task: any
        // edit/remove that lands while the task is in flight bumps
        // the generation, so the token we capture here is the one
        // the eventual `finish_full` must hand back. The task does
        // not need to know about the token; the cycle matches it.
        refresh_cycle
            .begin_full()
            .map(|token| (start_refresh(&monitor), token))
    } else {
        None
    });
    let mut account_refresh = AccountRefresh::new(file.config.accounts.len());
    let mut dirty = true;
    let mut last_tick = Instant::now();
    let mut deletion: Option<DeletionState> = None;
    // PRD 0017 RF5: core-backed extension manager and its single test run.
    let mut extensions: Option<ExtensionsView> = None;
    let mut extension_test: Option<(
        String,
        JoinHandle<Result<cuotai_core::extensions::output::Checked, cuotai_core::text::Localized>>,
    )> = None;
    // RF5: Models section over the details screen or the wizard.
    let mut models: Option<ModelsView> = None;
    // "Sobre": libraries and repositories; opened from the dashboard only.
    let mut about: Option<AboutView> = None;
    // PRD 0019 RF7.7: "Novidades", opened from "Sobre" (`n`); read-only.
    let mut changelog: Option<ChangelogView> = None;
    // PRD 0011 RF5: the status read in flight and when the next one is due.
    let mut status_job: Option<JoinHandle<Vec<ProviderStatus>>> = None;
    let mut status_due = Instant::now();
    // PRD 0015 RF4.2: the index read in flight and when the next is due.
    let mut cost_job: Option<JoinHandle<Option<cuotai_core::cost::CostSummary>>> = None;
    let mut cost_due = Instant::now();

    // Both the keyboard and mouse event paths run the same dashboard
    // transitions through `run_command!` and `dispatch_wizard_action!` so a
    // click never behaves differently from a key press (and vice versa).
    /// Dashboard commands shared by keyboard and mouse.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Command {
        ToggleDetails,
        CloseDetails,
        ToggleUnmeasured,
        ScrollDetailDown,
        ScrollDetailUp,
        Quit,
        Delete,
        Add,
        Edit,
        MoveUp,
        MoveDown,
        Refresh,
        RefreshSelected,
        Down,
        Up,
        PageDown,
        PageUp,
        WheelDown,
        WheelUp,
        Home,
        End,
        Filter,
        ClearFilter,
        /// Avança entre contas visíveis no detalhe (Left/Right quando detalhes abertos).
        NextDetail,
        /// Retrocede entre contas visíveis no detalhe.
        PrevDetail,
        /// Abre a seção Modelos da conta em detalhe.
        Models,
        /// Opens discovered executable extensions.
        Extensions,
        /// Abre a tela Sobre (bibliotecas de código aberto e repositórios).
        About,
        /// PRD 0006 RF6.1: span of the details' history (1, 2, 3).
        HistoryRange(HistoryRange),
        /// SES-01: "Paste a new session" for the selected account whose
        /// vault session the provider rejected (the core decides).
        PasteSession,
    }

    /// Map a keyboard event onto a dashboard command. Returns `None` when the
    /// key does not drive a dashboard transition (e.g. Ctrl-C, paste, or a key
    /// already handled by the auth/wizard/deletion/filtering layers above).
    fn key_command(code: KeyCode, details: bool, refresh_idle: bool) -> Option<Command> {
        // Left/Right só navegam entre contas enquanto o detalhe está aberto;
        // fora do detalhe, são no-op (outros atalhos: ←→ nas opções do wizard).
        if details {
            match code {
                KeyCode::Right => return Some(Command::NextDetail),
                KeyCode::Left => return Some(Command::PrevDetail),
                _ => {}
            }
        }
        Some(match code {
            KeyCode::Enter => Command::ToggleDetails,
            KeyCode::Esc if details => Command::CloseDetails,
            KeyCode::Char('m') if details => Command::Models,
            KeyCode::Char('1') if details => Command::HistoryRange(HistoryRange::Day),
            KeyCode::Char('2') if details => Command::HistoryRange(HistoryRange::Week),
            KeyCode::Char('3') if details => Command::HistoryRange(HistoryRange::Month),
            KeyCode::Char('?') => Command::About,
            KeyCode::Char('x') => Command::Extensions,
            KeyCode::Char('u') => Command::ToggleUnmeasured,
            KeyCode::Down if details => Command::ScrollDetailDown,
            KeyCode::Up if details => Command::ScrollDetailUp,
            KeyCode::Char('q') | KeyCode::Esc => Command::Quit,
            KeyCode::Char('d') | KeyCode::Delete => Command::Delete,
            KeyCode::Char('a') => Command::Add,
            KeyCode::Char('e') => Command::Edit,
            KeyCode::Char('p') => Command::PasteSession,
            KeyCode::Char('K') => Command::MoveUp,
            KeyCode::Char('J') => Command::MoveDown,
            KeyCode::Char('/') => Command::Filter,
            KeyCode::Char(']') => Command::ScrollDetailDown,
            KeyCode::Char('[') => Command::ScrollDetailUp,
            KeyCode::Char('r') if refresh_idle => Command::Refresh,
            KeyCode::Char('R') => Command::RefreshSelected,
            KeyCode::Char('j') | KeyCode::Down => Command::Down,
            KeyCode::Char('k') | KeyCode::Up => Command::Up,
            KeyCode::PageDown => Command::PageDown,
            KeyCode::PageUp => Command::PageUp,
            KeyCode::Home => Command::Home,
            KeyCode::End => Command::End,
            KeyCode::Char('c') => Command::ClearFilter,
            _ => return None,
        })
    }

    /// Dispatch a wizard action identically whether it came from a key or a
    /// mouse event forwarded by `Wizard::handle_mouse`. Only references
    /// definition-site locals so both call sites reuse the same flow.
    macro_rules! dispatch_wizard_action {
        ($action:expr) => {
            match $action {
                Action::Continue => {}
                Action::Cancel => {
                    wizard = None;
                    authorized = None;
                }
                Action::Login { provider, request } => {
                    authorized = None;
                    // Every mode discovered by the core follows this same path.
                    match login::start_login(Arc::clone(&session), provider, *request) {
                        Ok(driver) => {
                            auth_task = Some(LoginTask::new(driver));
                        }
                        Err(error) => {
                            if let Some(active) = &mut wizard {
                                active.set_error(error.to_string());
                            }
                        }
                    }
                }
                Action::Save {
                    config,
                    edit,
                    sessions,
                    clearance,
                } => {
                    // PRD 0016 RF2/RF3.4: the pasted sessions reach the vault
                    // first (an existing reference keeps its entry); any later
                    // failure takes them out again.
                    let staged_paste = if sessions.is_empty() {
                        Ok(None)
                    } else {
                        let index = edit.unwrap_or(config.accounts.len().saturating_sub(1));
                        // SES-02: only with the clearance of the test before save.
                        user_session::stage_pasted(
                            session.as_ref(),
                            &config,
                            index,
                            &sessions,
                            clearance.as_ref(),
                        )
                        .await
                        .map(Some)
                    };
                    drop(sessions);
                    // FIX-06: a provider switch drops the account's session
                    // reference; its vault entries go with the save and come
                    // back, with the paste, through the same rollback.
                    let staged_paste = match staged_paste {
                        Ok(staged) => {
                            let after = staged
                                .as_ref()
                                .map_or_else(|| config.clone(), |s| s.config().clone());
                            user_session::stage_dropped(
                                session.as_ref(),
                                &file.config,
                                &after,
                                staged,
                            )
                            .await
                        }
                        Err(note) => Err(note),
                    };
                    match staged_paste {
                        Err(note) => {
                            if let Some(active) = &mut wizard {
                                active.set_error(resolve_l10n(&note));
                            }
                        }
                        Ok(staged_paste) => {
                            let config = staged_paste
                                .as_ref()
                                .map_or(config, |staged| staged.config().clone());
                            let pending = authorized.as_ref().filter(|completed| {
                                config.accounts.iter().any(|a| {
                                    a.provider == completed.provider
                                        && a.oauth.as_ref() == Some(&completed.config)
                                })
                            });
                            if let Some(completed) = pending {
                                saving_config = Some((config, edit));
                                saving_paste = staged_paste;
                                auth_task = Some(LoginTask::begin_commit(
                                    Arc::clone(&session),
                                    completed.clone(),
                                ));
                            } else {
                                let expected_identity = wizard
                                    .as_ref()
                                    .and_then(|w| w.editing_identity_borrowed())
                                    .map(|(p, n)| (p, n.to_string()));
                                match apply_edit_to_config(
                                    &mut file,
                                    &monitor,
                                    &mut refresh_cycle,
                                    &mut account_refresh,
                                    &mut state,
                                    config,
                                    edit,
                                    expected_identity,
                                ) {
                                    Err(error) => {
                                        let mut message = error.message();
                                        if let Some(staged) = staged_paste {
                                            if let Err(note) =
                                                user_session::rollback(session.as_ref(), staged)
                                                    .await
                                            {
                                                message =
                                                    format!("{message} {}", resolve_l10n(&note));
                                            }
                                        }
                                        // RF4: a Conflict typed error from
                                        // `ConfigFile::save` means the file on
                                        // disk changed. Open the conflict modal
                                        // so the user can reload (which keeps
                                        // the wizard's draft) or cancel.
                                        if let Some(active) = &mut wizard {
                                            if error.is_conflict() {
                                                active.set_conflict(message);
                                            } else {
                                                active.set_error(message);
                                            }
                                        }
                                    }
                                    Ok((new_monitor, refresh_target)) => {
                                        monitor = new_monitor;
                                        wizard = None;
                                        authorized = None;
                                        // The touched slot is pending. Fire a single
                                        // per-account refresh immediately so the
                                        // dashboard does not wait for the new
                                        // deadline (which is one full interval away).
                                        if let Some(index) = refresh_target {
                                            if index < account_refresh.len()
                                                && account_refresh.slot(index).is_none()
                                            {
                                                if let Some(token) = refresh_cycle.begin(index) {
                                                    account_refresh.install(
                                                        index,
                                                        start_account_refresh(&monitor, index),
                                                        token,
                                                    );
                                                    dirty = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Action::TestSession { account, sessions } => {
                    // RF2.1: read-only, never stored or recorded.
                    let monitor = Arc::clone(&monitor);
                    session_test = Some(tokio::spawn(async move {
                        user_session::test_pasted(&monitor, &account, &sessions).await
                    }));
                }
                Action::ConflictReload => {
                    // RF4: the user accepted the conflict-recovery
                    // modal. Reload the file from disk and refresh the
                    // runtime state. The wizard's draft is kept intact
                    // so the user can re-validate the schema they were
                    // about to save. On any failure we surface a
                    // typed error in the wizard and stay in the modal
                    // so the user can cancel or retry.
                    match reload_config_after_conflict(
                        &mut file,
                        &mut monitor,
                        &mut refresh_cycle,
                        &mut refresh,
                        &mut account_refresh,
                        &mut state,
                        &session,
                    ) {
                        Ok(()) => {
                            if let Some(active) = &mut wizard {
                                active.replace_base(&file.config);
                                active.clear_conflict();
                                active.set_notice(
                                    "Arquivo recarregado do disco; revise e grave novamente."
                                        .to_string(),
                                );
                            }
                        }
                        Err(message) => {
                            if let Some(active) = &mut wizard {
                                active.set_conflict(message);
                            }
                        }
                    }
                    dirty = true;
                }
                Action::ConflictDismiss => {
                    // RF4: keep the wizard's draft and the in-memory
                    // snapshot; the user can edit and retry.
                    if let Some(active) = &mut wizard {
                        active.clear_conflict();
                    }
                }
                Action::Models { index } => {
                    // Only an account still saved at `index` (the wizard base
                    // may predate a reload) is ever queried.
                    let base = wizard.as_ref().and_then(|w| w.base_account(index));
                    match file.config.accounts.get(index) {
                        Some(account) if base == Some(account) => {
                            models = Some(ModelsView::open(&monitor, index, account.clone()));
                        }
                        _ => {
                            if let Some(active) = &mut wizard {
                                active.set_error(
                                    "Conta alterada no arquivo; reabra o assistente.".into(),
                                );
                            }
                        }
                    }
                }
                Action::SessionVault { index, change } => {
                    // PRD 0016 RF1.2/RF4.2: vault first, then the TOML through
                    // the same staged edit as a save; a failed save undoes the
                    // vault change (new entries removed, forgotten ones back).
                    let base = wizard.as_ref().and_then(|w| w.base_account(index));
                    if base.is_none() || base != file.config.accounts.get(index) {
                        if let Some(active) = &mut wizard {
                            active.set_error(
                                "Conta alterada no arquivo; reabra o assistente.".into(),
                            );
                        }
                    } else {
                        let staged = match change {
                            VaultChange::Migrate => {
                                user_session::stage_migration(
                                    session.as_ref(),
                                    &EnvironmentCredentials,
                                    &file.config,
                                    index,
                                )
                                .await
                            }
                            VaultChange::Forget => {
                                user_session::stage_forget(session.as_ref(), &file.config, index)
                                    .await
                            }
                        };
                        match staged {
                            Err(note) => {
                                if let Some(active) = &mut wizard {
                                    active.set_error(resolve_l10n(&note));
                                }
                            }
                            Ok(staged) => match apply_edit_to_config(
                                &mut file,
                                &monitor,
                                &mut refresh_cycle,
                                &mut account_refresh,
                                &mut state,
                                staged.config().clone(),
                                Some(index),
                                None,
                            ) {
                                Ok((new_monitor, refresh_target)) => {
                                    monitor = new_monitor;
                                    if let Some(active) = &mut wizard {
                                        match change {
                                            VaultChange::Migrate => {
                                                active.session_migrated(&file.config)
                                            }
                                            VaultChange::Forget => {
                                                active.session_forgotten(&file.config)
                                            }
                                        }
                                    }
                                    if let Some(index) = refresh_target {
                                        if index < account_refresh.len()
                                            && account_refresh.slot(index).is_none()
                                        {
                                            if let Some(token) = refresh_cycle.begin(index) {
                                                account_refresh.install(
                                                    index,
                                                    start_account_refresh(&monitor, index),
                                                    token,
                                                );
                                            }
                                        }
                                    }
                                    dirty = true;
                                }
                                Err(error) => {
                                    let rollback =
                                        user_session::rollback(session.as_ref(), staged).await;
                                    if let Some(active) = &mut wizard {
                                        let mut message = error.message();
                                        if let Err(note) = rollback {
                                            message = format!("{message} {}", resolve_l10n(&note));
                                        }
                                        if error.is_conflict() {
                                            active.set_conflict(message);
                                        } else {
                                            active.set_error(message);
                                        }
                                    }
                                }
                            },
                        }
                    }
                }
            }
        };
    }

    /// Run a dashboard command — the same set of transitions that were
    /// previously inlined in the key match arm — regardless of whether the
    /// command came from a key press, a footer click, or a wheel tick.
    macro_rules! run_command {
        ($command:ident) => {
            match $command {
                Command::ToggleDetails => {
                    state.details = !state.details;
                    state.detail_scroll = 0;
                }
                Command::CloseDetails => {
                    state.details = false;
                    state.detail_scroll = 0;
                }
                Command::ToggleUnmeasured => {
                    let selected = state.selected_account(&cycle_slot_reports(&refresh_cycle));
                    state.show_unmeasured = !state.show_unmeasured;
                    state.restore_selection(selected, &cycle_slot_reports(&refresh_cycle));
                    state.detail_scroll = 0;
                }
                Command::ScrollDetailDown => {
                    state.detail_scroll = state.detail_scroll.saturating_add(1);
                }
                Command::ScrollDetailUp => {
                    state.detail_scroll = state.detail_scroll.saturating_sub(1);
                }
                Command::Quit => break,
                Command::Delete => {
                    if let Some(index) = state.selection.selected().and_then(|i| {
                        state
                            .visible(&cycle_slot_reports(&refresh_cycle))
                            .get(i)
                            .copied()
                    }) {
                        if let Some((job, _token)) = refresh.0.take() {
                            job.abort();
                        }
                        account_refresh.abort_all();
                        deletion = Some(DeletionState::Confirm { index });
                    }
                }
                Command::PasteSession => {
                    // SES-01: straight to the paste step of that account,
                    // only when the core offers it for its current report.
                    let reports = cycle_slot_reports(&refresh_cycle);
                    let target = state
                        .selection
                        .selected()
                        .and_then(|i| state.visible(&reports).get(i).copied())
                        .filter(|&index| {
                            reports
                                .get(index)
                                .and_then(Option::as_ref)
                                .is_some_and(user_session::offers_new_paste)
                        });
                    if let Some(index) = target {
                        if let Some((job, _token)) = refresh.0.take() {
                            job.abort();
                        }
                        account_refresh.abort_all();
                        wizard = Some(Wizard::paste_session(&file.config, index));
                        authorized = None;
                    }
                }
                Command::Add | Command::Edit => {
                    let editing = if $command == Command::Edit {
                        state.selection.selected().and_then(|i| {
                            state
                                .visible(&cycle_slot_reports(&refresh_cycle))
                                .get(i)
                                .copied()
                        })
                    } else {
                        None
                    };
                    if matches!($command, Command::Add) || editing.is_some() {
                        // PRD 0017 RF5.12: an extension account has no form
                        // in the built-in wizard (the provider is not in the
                        // creation catalog); its identity, approval and
                        // removal live in the extension manager, so `e`
                        // opens it instead of silently editing an OpenAI
                        // form.
                        let is_extension = editing.is_some_and(|index| {
                            file.config
                                .accounts
                                .get(index)
                                .is_some_and(|account| account.provider == Provider::Extension)
                        });
                        if is_extension {
                            if let Some(manager) = monitor.extension_manager() {
                                let id = editing
                                    .and_then(|index| file.config.accounts.get(index))
                                    .and_then(|account| account.extension_id())
                                    .map(str::to_owned);
                                extensions = Some(
                                    ExtensionsView::new(manager.list(&file.config)).focused(id.as_deref()),
                                );
                            } else {
                                state.error = Some(
                                    "O diretório local de extensões não está disponível.".into(),
                                );
                            }
                        } else {
                            if let Some((job, _token)) = refresh.0.take() {
                                job.abort();
                            }
                            account_refresh.abort_all();
                            wizard = Some(Wizard::new(&file.config, editing));
                            authorized = None;
                        }
                    }
                }
                Command::MoveUp | Command::MoveDown => {
                    let reports = cycle_slot_reports(&refresh_cycle);
                    let visible = state.visible(&reports);
                    let delta = if $command == Command::MoveUp { -1 } else { 1 };
                    if let Some(position) = state.selection.selected() {
                        let target = position
                            .saturating_add_signed(delta)
                            .min(visible.len().saturating_sub(1));
                        if let (Some(from), Some(to)) =
                            (visible.get(position).copied(), visible.get(target).copied())
                        {
                            if from != to {
                                match move_account_in_config(
                                    &mut file,
                                    &monitor,
                                    &mut refresh_cycle,
                                    &mut account_refresh,
                                    &mut state,
                                    from,
                                    to,
                                ) {
                                    Ok(new_monitor) => {
                                        monitor = new_monitor;
                                        if let Some((job, _token)) = refresh.0.take() {
                                            job.abort();
                                        }
                                    }
                                    Err(error) => state.error = Some(error.message()),
                                }
                            }
                        }
                    }
                }
                Command::Filter => {
                    state.filtering = true;
                }
                Command::Refresh => {
                    for index in 0..account_refresh.len() {
                        if account_refresh.slot(index).is_none() && refresh.0.is_none() {
                            if let Some(token) = refresh_cycle.begin(index) {
                                account_refresh.install(
                                    index,
                                    start_account_refresh(&monitor, index),
                                    token,
                                );
                            }
                        }
                    }
                }
                Command::RefreshSelected => {
                    if let Some(index) = state.selected_account(&cycle_slot_reports(&refresh_cycle))
                    {
                        if account_refresh.slot(index).is_none() && refresh.0.is_none() {
                            if let Some(token) = refresh_cycle.begin(index) {
                                account_refresh.install(
                                    index,
                                    start_account_refresh(&monitor, index),
                                    token,
                                );
                            }
                        }
                    }
                }
                Command::Down => state.move_selection(1, &cycle_slot_reports(&refresh_cycle)),
                Command::Up => state.move_selection(-1, &cycle_slot_reports(&refresh_cycle)),
                Command::PageDown => state.move_selection(10, &cycle_slot_reports(&refresh_cycle)),
                Command::PageUp => state.move_selection(-10, &cycle_slot_reports(&refresh_cycle)),
                Command::WheelDown => {
                    let step = state.targets.columns.max(1) as isize;
                    state.move_selection(step, &cycle_slot_reports(&refresh_cycle));
                }
                Command::WheelUp => {
                    let step = state.targets.columns.max(1) as isize;
                    state.move_selection(-step, &cycle_slot_reports(&refresh_cycle));
                }
                Command::Home => {
                    state.selection.select(Some(0));
                    state.detail_scroll = 0;
                }
                Command::End => {
                    let last = state
                        .visible(&cycle_slot_reports(&refresh_cycle))
                        .len()
                        .saturating_sub(1);
                    state.selection.select(Some(last));
                    state.detail_scroll = 0;
                }
                Command::ClearFilter => {
                    state.filter.clear();
                    state.selection.select(Some(0));
                    state.detail_scroll = 0;
                }
                // Navegação no detalhe: move a seleção entre contas visíveis
                // (sem wrap, igual a Down/Up) e zera o scroll lateral.
                Command::NextDetail => state.move_selection(1, &cycle_slot_reports(&refresh_cycle)),
                Command::PrevDetail => {
                    state.move_selection(-1, &cycle_slot_reports(&refresh_cycle))
                }
                Command::About => about = Some(AboutView::default()),
                Command::HistoryRange(range) => state.history_range = range,
                Command::Models => {
                    if let Some(index) = state.selected_account(&cycle_slot_reports(&refresh_cycle))
                    {
                        if let Some(account) = file.config.accounts.get(index) {
                            models = Some(ModelsView::open(&monitor, index, account.clone()));
                        }
                    }
                }
                Command::Extensions => {
                    if let Some(manager) = monitor.extension_manager() {
                        extensions = Some(ExtensionsView::new(manager.list(&file.config)));
                    } else {
                        state.error = Some(
                            "O diretório local de extensões não está disponível.".into(),
                        );
                    }
                }
            }
        };
    }
    loop {
        if last_tick.elapsed() >= Duration::from_secs(1) {
            dirty = true;
            last_tick = Instant::now();
            // PRD 0003 RF4 and PRD 0008 RF4: reset notices over the
            // readings already shown and the usage alerts of the refreshes.
            if let Some(notices) = &mut state.notices {
                let readings = notify::readings(&refresh_cycle);
                let bell = notices.tick(&file.config, &readings, chrono::Utc::now(), tui_locale());
                if !bell.is_empty() {
                    let mut stdout = io::stdout();
                    stdout.write_all(&bell)?;
                    stdout.flush()?;
                }
            }
        }
        if let Some(task) = &mut auth_task {
            dirty |= task.drain();
            if task.is_finished() {
                let outcome = task.result().await;
                // Bracketed paste follows the login and the wizard step below.
                auth_task = None;
                dirty = true;
                // Capture the identity BEFORE the mutable borrow of
                // `wizard` so the borrow does not extend through
                // `apply_edit_to_config`.
                let expected_identity = wizard
                    .as_ref()
                    .and_then(|w| w.editing_identity_borrowed())
                    .map(|(p, n)| (p, n.to_string()));
                if let Some(active) = &mut wizard {
                    match outcome {
                        Ok(Outcome::Authorized(completed)) => {
                            active
                                .login_finished(completed.config.clone(), completed.params.clone());
                            authorized = Some(completed);
                        }
                        Ok(Outcome::External(login)) => {
                            active.external_login_finished(login.params);
                        }
                        Ok(Outcome::CredentialReady(commit)) => {
                            // RF2.4: cofre gravado; agora o TOML. Em caso de
                            // falha, o core compensa o cofre (restaura o token
                            // anterior ou remove o novo) e reporta o estado.
                            if let Some((config, edit)) = saving_config.take() {
                                match apply_edit_to_config(
                                    &mut file,
                                    &monitor,
                                    &mut refresh_cycle,
                                    &mut account_refresh,
                                    &mut state,
                                    config,
                                    edit,
                                    expected_identity,
                                ) {
                                    Ok((new_monitor, _refresh_target)) => {
                                        monitor = new_monitor;
                                        auth_task = Some(LoginTask::confirm(
                                            Arc::clone(&session),
                                            commit,
                                            file.config.clone(),
                                        ));
                                    }
                                    Err(error) => {
                                        auth_task = Some(LoginTask::rollback(
                                            Arc::clone(&session),
                                            commit,
                                            error.message(),
                                        ));
                                    }
                                }
                            }
                        }
                        Ok(Outcome::Committed(login::LoginCommitOutcome::Saved)) => {
                            saving_paste = None;
                            wizard = None;
                            authorized = None;
                        }
                        Ok(Outcome::Committed(
                            login::LoginCommitOutcome::SavedWithCleanupFailure(failure),
                        )) => {
                            saving_paste = None;
                            state.error = Some(crate::auth_ui::cleanup_failure_message(&failure));
                            wizard = None;
                            authorized = None;
                        }
                        Ok(Outcome::Committed(login::LoginCommitOutcome::PartialFailure(
                            failure,
                        ))) => {
                            saving_paste = None;
                            active.set_error(crate::auth_ui::partial_failure_message(&failure));
                        }
                        Ok(Outcome::RolledBack(failure)) => {
                            // Estado exato reportado pelo core; o login
                            // autorizado continua em `authorized` e salvar de
                            // novo repete o commit de forma idempotente.
                            let mut message = crate::auth_ui::partial_failure_message(&failure);
                            if let Some(staged) = saving_paste.take() {
                                if let Err(note) =
                                    user_session::rollback(session.as_ref(), staged).await
                                {
                                    message = format!("{message} {}", resolve_l10n(&note));
                                }
                            }
                            active.set_error(message);
                        }
                        Err(error) => {
                            saving_config = None;
                            let mut message = error.to_string();
                            if let Some(staged) = saving_paste.take() {
                                if let Err(note) =
                                    user_session::rollback(session.as_ref(), staged).await
                                {
                                    message = format!("{message} {}", resolve_l10n(&note));
                                }
                            }
                            active.set_error(message);
                        }
                    }
                }
            }
        }
        if session_test.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(job) = session_test.take() {
                dirty = true;
                let then = match (job.await, &mut wizard) {
                    (Ok(report), Some(active)) => active.session_tested(&report),
                    _ => None,
                };
                // SES-02: a test started by the save saves once cleared.
                if let Some(action) = then {
                    dispatch_wizard_action!(action);
                }
            }
        }
        if extension_test
            .as_ref()
            .is_some_and(|(_, job)| job.is_finished())
        {
            if let Some((_id, job)) = extension_test.take() {
                dirty = true;
                let outcome = job.await;
                if let Some(view) = &mut extensions {
                    match outcome {
                        Ok(Ok(checked)) if checked.status == cuotai_core::Status::Ready => {
                            view.set_message("Teste concluído com sucesso.".into(), false);
                        }
                        Ok(Ok(checked)) => {
                            let message =
                                checked.notes.first().map(resolve_l10n).unwrap_or_else(|| {
                                    "A extensão não retornou um relatório utilizável.".into()
                                });
                            view.set_message(message, true);
                        }
                        Ok(Err(note)) => view.set_message(resolve_l10n(&note), true),
                        Err(_) => {
                            view.set_message("Falha na tarefa de teste da extensão.".into(), true)
                        }
                    }
                    if let Some(manager) = monitor.extension_manager() {
                        view.replace(manager.list(&file.config));
                    }
                }
            }
        }
        // PRD 0016 RF2.1: bracketed paste while a login takes a code or the
        // wizard shows the paste step, so a multi-line paste arrives whole.
        let wants_paste = auth_task.as_ref().is_some_and(LoginTask::accepts_code)
            || wizard.as_ref().is_some_and(Wizard::accepts_paste);
        if wants_paste != bracketed_paste {
            if wants_paste {
                execute!(io::stdout(), EnableBracketedPaste)?;
            } else {
                execute!(io::stdout(), DisableBracketedPaste)?;
            }
            bracketed_paste = wants_paste;
        }
        if let Some((job, _token)) = refresh.0.as_ref() {
            if job.is_finished() {
                dirty = true;
                // RF5.6: the same for a full sweep.
                if let Some(view) = &mut extensions {
                    if let Some(manager) = monitor.extension_manager() {
                        view.replace(manager.list(&file.config));
                    }
                }
                if let Some((job, token)) = refresh.0.take() {
                    let selected_account =
                        state.selected_account(&cycle_slot_reports(&refresh_cycle));
                    // The token was claimed BEFORE the task was
                    // spawned (in `Refresh::new` and in the scheduler
                    // block below). Any edit that bumps
                    // `refresh_cycle.generation` between spawn and
                    // finish will cause `finish_full` to drop the
                    // result silently, which is exactly the stale
                    // guard we want.
                    match job.await {
                        Ok(snapshot) => {
                            notify::finish_full(
                                &mut refresh_cycle,
                                state.notices.as_mut(),
                                Some(snapshot),
                                token,
                                chrono::Utc::now(),
                            );
                            state.restore_selection(
                                selected_account,
                                &cycle_slot_reports(&refresh_cycle),
                            );
                            state.error = None;
                        }
                        Err(_) => {
                            refresh_cycle.finish_full(None, token, Instant::now());
                            state.error = Some(
                                "Falha na tarefa de consulta; r para tentar novamente.".to_string(),
                            );
                        }
                    }
                }
            }
        }
        for index in 0..account_refresh.len() {
            let finished = account_refresh
                .slot(index)
                .is_some_and(JoinHandle::is_finished);
            if !finished {
                continue;
            }
            dirty = true;
            // RF5.6: an open manager shows the core's last run and error, so
            // it reads them again whenever a refresh lands.
            if let Some(view) = &mut extensions {
                if let Some(manager) = monitor.extension_manager() {
                    view.replace(manager.list(&file.config));
                }
            }
            if let Some((job, token)) = account_refresh.take(index) {
                match job.await {
                    Ok(report) => {
                        notify::finish_account(
                            &mut refresh_cycle,
                            state.notices.as_mut(),
                            index,
                            Some(report),
                            token,
                            chrono::Utc::now(),
                        );
                        state.error = None;
                    }
                    Err(_) => {
                        refresh_cycle.finish_account(index, None, token, Instant::now());
                        state.error =
                            Some("Falha na consulta; R para tentar novamente.".to_string());
                    }
                }
            }
        }
        if status_job.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(job) = status_job.take() {
                if let Ok(statuses) = job.await {
                    if statuses != state.incidents {
                        state.incidents = statuses;
                        dirty = true;
                    }
                }
            }
            status_due = Instant::now() + STATUS_TICK;
        } else if status_job.is_none()
            && monitor.incidents().is_enabled()
            && Instant::now() >= status_due
        {
            let monitor = Arc::clone(&monitor);
            status_job = Some(tokio::spawn(
                async move { monitor.refresh_incidents().await },
            ));
        }
        if cost_job.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(job) = cost_job.take() {
                if let Ok(summary) = job.await {
                    if summary != state.local_cost {
                        state.local_cost = summary;
                        dirty = true;
                    }
                }
            }
            cost_due = Instant::now() + COST_TICK;
        } else if let Some(cost) = local_cost.as_ref().filter(|_| cost_job.is_none()) {
            if Instant::now() >= cost_due {
                let cost = cost.clone();
                cost_job = Some(tokio::task::spawn_blocking(move || {
                    cost.summary(chrono::Utc::now())
                }));
            }
        }
        if wizard.is_none() && deletion.is_none() && !file.config.accounts.is_empty() {
            // Drive either an initial full sweep (no data yet at all and
            // the cycle has never finished a sweep) or per-account
            // retries honouring the cycle's per-slot deadlines. The
            // deadline-aware view (`pending_now_indices`) excludes slots
            // whose backoff clock is still ticking, so a failed sweep
            // does NOT immediately re-fire every account — the scheduler
            // waits for the natural deadline before retrying.
            if refresh.0.is_none() {
                let now = Instant::now();
                let pending_any: Vec<usize> = refresh_cycle.pending_indices();
                let all_pending =
                    !pending_any.is_empty() && pending_any.len() == file.config.accounts.len();
                if all_pending
                    && !refresh_cycle.attempted_full()
                    && refresh_cycle.pending_now_indices(now).len() == file.config.accounts.len()
                {
                    // No data anywhere yet AND no full sweep has ever
                    // been attempted AND every slot's deadline
                    // permits an immediate retry. Full sweep is cheaper
                    // than spinning up N independent joins for paid
                    // providers.
                    if let Some(token) = refresh_cycle.begin_full() {
                        refresh.0 = Some((start_refresh(&monitor), token));
                        dirty = true;
                    }
                } else {
                    // Per-account retries: edit-driven pending (deadline
                    // already elapsed) + periodic due (Some slot, deadline
                    // elapsed). Failed-but-pending slots are NOT in
                    // either list — they wait for their backoff window.
                    let pending_now = refresh_cycle.pending_now_indices(now);
                    let due = refresh_cycle.due_indices(now);
                    let mut all: Vec<usize> = pending_now.into_iter().chain(due).collect();
                    all.sort_unstable();
                    all.dedup();
                    for index in all {
                        if account_refresh.slot(index).is_none() {
                            if let Some(token) = refresh_cycle.begin(index) {
                                account_refresh.install(
                                    index,
                                    start_account_refresh(&monitor, index),
                                    token,
                                );
                                dirty = true;
                            }
                        }
                    }
                }
            }
        }
        if let Some(view) = &mut models {
            dirty |= view.busy();
            // A sync may change the catalog; recount once the cards show again.
            state.model_counts.stale = true;
            if !view.poll(&file.config.accounts).await {
                models = None;
                state.error =
                    Some("A conta mudou; o resultado de modelos foi descartado.".to_string());
                dirty = true;
            }
        }
        if let Some(anim) = &mut state.anim {
            let full = refresh.0.is_some();
            anim.track(
                (0..account_refresh.len()).map(|i| full || account_refresh.slot(i).is_some()),
                (state.details, about.is_some(), models.is_some()),
                Instant::now(),
            );
            dirty |= anim.active();
        }
        if dirty {
            if let Some(task) = &mut auth_task {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    task.draw(frame)
                })?;
            } else if let Some(view) = &mut changelog {
                let locale = tui_locale();
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    view.draw(frame, locale);
                    if let Some(anim) = &mut state.anim {
                        anim.wipe(frame.buffer_mut());
                    }
                })?;
            } else if let Some(view) = &mut about {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    view.draw(frame);
                    if let Some(anim) = &mut state.anim {
                        anim.wipe(frame.buffer_mut());
                    }
                })?;
            } else if let Some(view) = &mut models {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    view.draw(frame);
                    if let Some(anim) = &state.anim {
                        anim.wipe(frame.buffer_mut());
                    }
                })?;
            } else if let Some(view) = &mut extensions {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    view.draw(frame)
                })?;
            } else if let Some(wizard) = &mut wizard {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    wizard.draw(frame, &file.path)
                })?;
            } else if let Some(state_) = &deletion {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    // Dracula frame background para a confirmação de exclusão.
                    frame.render_widget(
                        Block::default().style(Style::default().bg(DRACULA_BG).fg(DRACULA_FG)),
                        frame.area(),
                    );
                    let areas = Layout::vertical([Constraint::Min(3), Constraint::Length(3)])
                        .split(frame.area());
                    let (title, body, footer) = render_deletion(state_, &file);
                    frame.render_widget(
                        Paragraph::new(body)
                            .wrap(Wrap { trim: true })
                            .block(Block::default().borders(Borders::ALL).title(title)),
                        areas[0],
                    );
                    frame.render_widget(
                        Paragraph::new(footer)
                            .wrap(Wrap { trim: true })
                            .style(Style::default().fg(DRACULA_YELLOW)),
                        areas[1],
                    );
                })?;
            } else {
                let selected = state.selected_account(&cycle_slot_reports(&refresh_cycle));
                let refreshing = refresh.0.is_some()
                    || selected.is_some_and(|index| account_refresh.slot(index).is_some());
                let next = selected.and_then(|index| {
                    (refresh.0.is_none() && account_refresh.slot(index).is_none())
                        .then(|| refresh_cycle.next_refresh_in(index, Instant::now()))
                });
                state.model_counts.refresh(&monitor, state.accounts.len());
                let detail = selected
                    .filter(|_| state.details)
                    .map(|index| (index, state.history_range));
                state.history.refresh(&monitor, &refresh_cycle, detail);
                state.draw(&mut terminal, &refresh_cycle, refreshing, next)?;
                frame_area = state.targets.area;
            }
            dirty = false;
        }
        // RF4.1: ~30 fps only while an effect runs; otherwise today's 100 ms.
        let frame = if state.anim.as_ref().is_some_and(Anim::active) {
            33
        } else {
            100
        };
        if event::poll(Duration::from_millis(frame))? {
            dirty = true;
            let mut event = event::read()?;
            if let Event::Paste(text) = &mut event {
                if let Some(task) = &mut auth_task {
                    task.paste(text);
                } else if let Some(active) = &mut wizard {
                    // PRD 0016 RF2.1: the pasted session reaches the
                    // wizard's masked field; this copy is overwritten.
                    active.paste(text);
                }
                user_session::wipe(text);
                continue;
            }
            // Mouse events: wheel scrolls, clicks select/trigger the same
            // commands as the keyboard. The auth dialog only honours the
            // wheel so clicks never accidentally confirm or cancel a login.
            if let Event::Mouse(mouse) = event {
                if let Some(task) = &mut auth_task {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => task.scroll(true),
                        MouseEventKind::ScrollUp => task.scroll(false),
                        _ => {}
                    }
                    continue;
                }
                if let Some(view) = &mut changelog {
                    if view.mouse(mouse.kind, mouse.column, mouse.row) == ChangelogReply::Close {
                        changelog = None;
                    }
                    continue;
                }
                if let Some(view) = &mut about {
                    match view.mouse(mouse.kind, mouse.column, mouse.row) {
                        AboutReply::Close => about = None,
                        AboutReply::Changelog => changelog = Some(ChangelogView::default()),
                        AboutReply::Stay => {}
                    }
                    continue;
                }
                if let Some(view) = &mut models {
                    match view.mouse(mouse.kind, mouse.column, mouse.row) {
                        Reply::Close => models = None,
                        Reply::Sync => view.sync(&monitor),
                        Reply::Stay => {}
                    }
                    continue;
                }
                if let Some(active) = &mut wizard {
                    let action = active.handle_mouse(mouse, frame_area);
                    dispatch_wizard_action!(action);
                    continue;
                }
                if extensions.is_some() || deletion.is_some() || state.filtering {
                    // Clicks while a confirm dialog is open or the filter is
                    // being typed are ignored — keep keyboard as the only
                    // path through sensitive states.
                    continue;
                }
                let command = match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        if state.details {
                            Some(Command::ScrollDetailDown)
                        } else {
                            Some(Command::WheelDown)
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        if state.details {
                            Some(Command::ScrollDetailUp)
                        } else {
                            Some(Command::WheelUp)
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) if state.details => {
                        Some(Command::CloseDetails)
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Refuse stale clicks recorded against a frame size
                        // that has since changed — the next `draw` will
                        // refresh `targets` and the user can try again.
                        if state.targets.area != frame_area {
                            None
                        } else if let Some(url) = state.targets.source_at(mouse.column, mouse.row) {
                            let url = url.to_owned();
                            tokio::spawn(async move {
                                let _ = cuotai_core::login::open_browser(&url).await;
                            });
                            None
                        } else if let Some(action) =
                            state.targets.footer_at(mouse.column, mouse.row)
                        {
                            Some(match action {
                                FooterAction::OpenDetails => Command::ToggleDetails,
                                FooterAction::CloseDetails => Command::CloseDetails,
                                FooterAction::Add => Command::Add,
                                FooterAction::Edit => Command::Edit,
                                FooterAction::PasteSession => Command::PasteSession,
                                FooterAction::MoveUp => Command::MoveUp,
                                FooterAction::MoveDown => Command::MoveDown,
                                FooterAction::Delete => Command::Delete,
                                FooterAction::ToggleUnmeasured => Command::ToggleUnmeasured,
                                FooterAction::Filter => Command::Filter,
                                FooterAction::Models => Command::Models,
                                FooterAction::Extensions => Command::Extensions,
                                FooterAction::About => Command::About,
                                FooterAction::ClearFilter => Command::ClearFilter,
                                FooterAction::Refresh => Command::Refresh,
                                FooterAction::RefreshSelected => Command::RefreshSelected,
                                FooterAction::Quit => Command::Quit,
                            })
                        } else if let Some(position) =
                            state.targets.card_at(mouse.column, mouse.row)
                        {
                            if state.selection.selected() == Some(position) {
                                // Click on the already-selected card opens the
                                // detail view, mirroring Enter.
                                Some(Command::ToggleDetails)
                            } else {
                                state.selection.select(Some(position));
                                state.detail_scroll = 0;
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(command) = command {
                    run_command!(command);
                }
                continue;
            }
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if let Some(task) = &mut auth_task {
                    match key.code {
                        KeyCode::Esc if task.can_cancel() => {
                            auth_task = None;
                            if let Some(wizard) = &mut wizard {
                                wizard.set_error(
                                    "Login cancelado; nenhuma nova conta foi salva.".into(),
                                );
                            }
                        }
                        KeyCode::Down => task.scroll(true),
                        KeyCode::Up => task.scroll(false),
                        _ => task.input(key),
                    }
                    continue;
                }
                if let Some(view) = &mut changelog {
                    if view.key(key.code) == ChangelogReply::Close {
                        changelog = None;
                    }
                    continue;
                }
                if let Some(view) = &mut about {
                    match view.key(key.code) {
                        AboutReply::Close => about = None,
                        AboutReply::Changelog => changelog = Some(ChangelogView::default()),
                        AboutReply::Stay => {}
                    }
                    continue;
                }
                if let Some(view) = &mut models {
                    match view.key(key.code, key.modifiers) {
                        Reply::Close => models = None,
                        Reply::Sync => view.sync(&monitor),
                        Reply::Stay => {}
                    }
                    continue;
                }
                if extensions.is_some() {
                    let action = extensions
                        .as_mut()
                        .map(|view| view.key(key.code))
                        .unwrap_or(ExtensionAction::Stay);
                    match action {
                        ExtensionAction::Stay => {}
                        ExtensionAction::Close => extensions = None,
                        ExtensionAction::Disclose(id) => {
                            if let Some(manager) = monitor.extension_manager() {
                                match manager.disclosure(&id) {
                                    Ok(disclosure) => {
                                        if let Some(view) = &mut extensions {
                                            view.show_disclosure(
                                                disclosure.lines,
                                                disclosure.candidate,
                                            );
                                        }
                                    }
                                    Err(note) => {
                                        if let Some(view) = &mut extensions {
                                            view.set_message(resolve_l10n(&note), true);
                                        }
                                    }
                                }
                            }
                        }
                        ExtensionAction::Approve(candidate) => {
                            let manager = monitor.extension_manager().cloned();
                            if let Some(manager) = manager {
                                let editing = candidate.id().and_then(|id| {
                                    file.config
                                        .accounts
                                        .iter()
                                        .position(|account| account.extension_id() == Some(id))
                                });
                                match manager.enable(&candidate, true, &file.config) {
                                    Err(note) => {
                                        if let Some(view) = &mut extensions {
                                            view.set_message(resolve_l10n(&note), true);
                                        }
                                    }
                                    Ok(enabled) => {
                                        let message = resolve_l10n(&enabled.action.message);
                                        let id = enabled.action.id.clone();
                                        match apply_edit_to_config(
                                            &mut file,
                                            &monitor,
                                            &mut refresh_cycle,
                                            &mut account_refresh,
                                            &mut state,
                                            enabled.config,
                                            editing,
                                            None,
                                        ) {
                                            Ok((new_monitor, refresh_target)) => {
                                                monitor = new_monitor;
                                                // Persistence succeeded: only
                                                // now does the newly approved
                                                // version replace the old
                                                // one's cached failure.
                                                manager.confirm_commit(&id);
                                                if let Some(view) = &mut extensions {
                                                    view.replace(manager.list(&file.config));
                                                    view.set_message(message, false);
                                                }
                                                if let Some(index) = refresh_target {
                                                    if index < account_refresh.len()
                                                        && account_refresh.slot(index).is_none()
                                                    {
                                                        if let Some(token) =
                                                            refresh_cycle.begin(index)
                                                        {
                                                            account_refresh.install(
                                                                index,
                                                                start_account_refresh(
                                                                    &monitor, index,
                                                                ),
                                                                token,
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            Err(error) => {
                                                // Approval and config persistence form one UI
                                                // action: core knows exactly what its
                                                // approval step changed and restores
                                                // that. A rollback that itself fails
                                                // (a newer approval landed meanwhile)
                                                // is reported with the original
                                                // error, never swallowed.
                                                let mut message = error.message();
                                                if let Err(conflict) =
                                                    manager.rollback(enabled.consent_rollback)
                                                {
                                                    message = format!(
                                                        "{message} {}",
                                                        resolve_l10n(&conflict)
                                                    );
                                                }
                                                if let Some(view) = &mut extensions {
                                                    view.replace(manager.list(&file.config));
                                                    view.set_message(message, true);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ExtensionAction::Disable(id) => {
                            if let Some(manager) = monitor.extension_manager().cloned() {
                                match manager.disable(&id) {
                                    Ok(action) => {
                                        let message = resolve_l10n(&action.message);
                                        if let Some(view) = &mut extensions {
                                            view.replace(manager.list(&file.config));
                                            view.set_message(message, false);
                                        }
                                        if let Some(index) =
                                            file.config.accounts.iter().position(|account| {
                                                account.extension_id() == Some(id.as_str())
                                            })
                                        {
                                            if account_refresh.slot(index).is_none() {
                                                if let Some(token) = refresh_cycle.begin(index) {
                                                    account_refresh.install(
                                                        index,
                                                        start_account_refresh(&monitor, index),
                                                        token,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    Err(note) => {
                                        if let Some(view) = &mut extensions {
                                            view.set_message(resolve_l10n(&note), true);
                                        }
                                    }
                                }
                            }
                        }
                        ExtensionAction::Test(id) => {
                            if extension_test.is_none() {
                                if let Some(manager) = monitor.extension_manager().cloned() {
                                    let run_id = id.clone();
                                    extension_test = Some((
                                        id,
                                        tokio::spawn(async move { manager.test(&run_id).await }),
                                    ));
                                    if let Some(view) = &mut extensions {
                                        view.set_message("Testando extensão…".into(), false);
                                    }
                                }
                            }
                        }
                        ExtensionAction::Remove(index) => {
                            extensions = None;
                            deletion = Some(DeletionState::Confirm { index });
                        }
                    }
                    continue;
                }
                if let Some(active) = &mut wizard {
                    let action = active.handle(key);
                    dispatch_wizard_action!(action);
                    continue;
                }
                if let Some(state_) = &mut deletion {
                    let close = handle_deletion_key(
                        state_,
                        key.code,
                        &mut file,
                        &mut monitor,
                        &mut refresh_cycle,
                        &mut account_refresh,
                        &mut state,
                        Arc::clone(&session),
                    );
                    if close {
                        deletion = None;
                    }
                    continue;
                }
                if state.filtering {
                    match key.code {
                        _ if key.modifiers.contains(KeyModifiers::CONTROL) => {}
                        KeyCode::Esc | KeyCode::Enter => state.filtering = false,
                        KeyCode::Backspace => {
                            state.filter.pop();
                            state.selection.select(Some(0));
                            state.detail_scroll = 0;
                        }
                        KeyCode::Char(c) => {
                            state.filter.push(c);
                            state.selection.select(Some(0));
                            state.detail_scroll = 0;
                        }
                        _ => {}
                    }
                    continue;
                }
                if let Some(command) = key_command(key.code, state.details, true) {
                    run_command!(command);
                }
            }
        }
    }
    Ok(())
}

/// Header line: effective path plus the rule that selected it, and the
/// RF1.2 notice when the desktop app last opened another file. Recording
/// happens here, i.e. on every load/reload.
fn location(file: &ConfigFile) -> String {
    use cuotai_core::config_file::PathSource;
    let source = match file.source {
        PathSource::Argument => "--config",
        PathSource::Env => "CUOTAI_CONFIG",
        PathSource::Chosen => "escolhido",
        PathSource::Local => "local",
        PathSource::System => "padrão do SO",
    };
    let mut line = format!("{} ({source})", file.path.display());
    // Tests never touch the real OS data dir.
    let other = (!cfg!(test))
        .then(|| cuotai_core::last_config::note(cuotai_core::last_config::Frontend::Tui, file))
        .flatten();
    if let Some(other) = other {
        line.push_str(&format!(
            "\nAviso: o desktop usa {}; contas e modelos separados (alinhe com --config ou CUOTAI_CONFIG).",
            other.display()
        ));
    }
    line
}

/// Monitor for `file` with its model cache and measurement history (pruned
/// by `retention`) in the OS data dir and the session's status preference
/// and cache (`incidents`); without a resolvable data dir, model
/// operations report `NoCache`, nothing is recorded and quotas work.
fn new_monitor(
    file: &ConfigFile,
    session: &Arc<OAuthSession>,
    retention: Retention,
    incidents: &IncidentMonitor,
) -> io::Result<Arc<Monitor>> {
    let mut monitor = Monitor::with_oauth(
        file.config.clone(),
        Arc::new(EnvironmentCredentials),
        Arc::clone(session),
    )
    .map_err(io::Error::other)?
    .with_incidents(incidents.clone());
    if let Ok(cache) = ModelCache::in_data_dir(file) {
        monitor = monitor.with_model_cache(cache);
    }
    if let Some(store) = crate::history_store(file, retention) {
        monitor = monitor.with_history(store);
    }
    // RF5.8: the conflict reload rebuilds the monitor from scratch; the
    // process-wide extension manager (runner, cap, schedules, revocation
    // guard) is shared, so the two never run side by side.
    if let Some(manager) = cuotai_core::extensions::management::Manager::default_shared() {
        monitor = monitor.with_extensions(manager);
    }
    // Just loaded from the TOML: drop catalogs of accounts gone from it.
    // A cache failure only keeps stale digests; model ops report it.
    let _ = monitor.prune_models();
    monitor.mark_history_removed();
    Ok(Arc::new(monitor))
}

/// RF4 conflict recovery: re-read the file from disk and rebuild the
/// runtime (cycle, monitor, per-account refresh table, UI state)
/// against the freshly-loaded configuration. The wizard's draft is
/// kept intact by the caller; this helper only touches the
/// `ConfigFile` + cycle + monitor + UI state.
///
/// Failure modes are surfaced as the typed `EditError` converted to a
/// `String`; the caller shows the message verbatim inside the
/// conflict modal so the user can dismiss or retry.
fn reload_config_after_conflict(
    file: &mut ConfigFile,
    monitor: &mut Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    refresh: &mut Refresh,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    session: &Arc<OAuthSession>,
) -> Result<(), String> {
    // RF4 review: stage the reload as a candidate BEFORE mutating
    // any live state. The candidate re-opens the same path and source
    // the original file used (preserving the symlink semantics and the
    // missing-file fallback) and is rebuilt against the freshly-loaded
    // `Config`. Only on success do we abort the in-flight tasks and
    // commit. On any failure the live `file`, `monitor`, `cycle`, and
    // `account_refresh` table are untouched.
    let candidate = ConfigFile::open(
        cuotai_core::config_file::ResolvedPath {
            path: file.path.clone(),
            source: file.source,
        },
        true,
    )
    .map_err(|e| e.message())?;
    let retention = monitor
        .history()
        .map_or_else(Retention::default, |store| store.retention());
    let new_monitor = new_monitor(&candidate, session, retention, monitor.incidents())
        .map_err(|e| format!("monitor: {e}"))?;
    // Commit: from here on a panic would leave the runtime in a
    // consistent (old) configuration; the candidate has been
    // validated but not yet swapped.
    if let Some((job, _token)) = refresh.0.take() {
        job.abort();
    }
    account_refresh.abort_all();
    *file = candidate;
    *monitor = new_monitor;
    *refresh_cycle = RefreshCycle::new(&file.config, Instant::now());
    refresh_cycle.restore_last_good(monitor.restored_readings());
    *account_refresh = AccountRefresh::new(file.config.accounts.len());
    state.set_accounts(&file.config);
    state.detail_scroll = 0;
    state.details = false;
    state.location = location(file);
    Ok(())
}

/// Build the cycle's per-account report view. Pending slots stay `None`
/// so the dashboard and selectors know to render an "Aguardando"
/// placeholder rather than a stale row from the previous configuration.
/// Returning a `Vec` (rather than borrowing through the cycle) keeps the
/// borrow checker happy across `RefreshCycle` reborrows inside the
/// event loop and stays cheap: this is called at most once per frame.
fn cycle_slot_reports(cycle: &RefreshCycle) -> Vec<Option<cuotai_core::Report>> {
    match cycle.snapshot() {
        Some(snapshot) => snapshot.reports.iter().cloned().map(Some).collect(),
        None => (0..cycle.len())
            .map(|index| cycle.account_report(index).cloned())
            .collect(),
    }
}

/// Apply a caller-supplied edit (`Some(index)` for Upsert, `None` for the
/// first save) and rebuild the monitor and refresh cycle from the new
/// configuration. Returns the new monitor plus the index (if any) the
/// caller must queue for a follow-up refresh.
///
/// Failure invariant: when this function returns `Err`, the on-disk
/// file, the cycle, the monitor, the per-account refresh table and the
/// UI state are left exactly as they were. We achieve that by validating
/// the edit against a staged cycle clone, building the new monitor
/// (which validates the full new config without touching disk), then
/// persisting to disk, then committing the cycle mutation. If the disk
/// write fails the cycle and monitor never see the new configuration;
/// if the monitor rebuild fails the disk has not been touched yet, so no
/// rollback is needed.
///
/// `edit` is `Some(i)` when the wizard was editing the account at `i`, or
/// `None` when the wizard was creating a brand-new account. In both cases
/// the operation is an `Edit::Upsert` (replace or append); removals are
/// routed through `account_edit::remove_account` (the core RF3 entry
/// point) plus the `run_remove_account` helper in this module instead.
#[allow(clippy::too_many_arguments)]
fn apply_edit_to_config(
    file: &mut ConfigFile,
    monitor: &Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    config: Config,
    edit: Option<usize>,
    expected_identity: Option<(crate::Provider, String)>,
) -> Result<(Arc<Monitor>, Option<usize>), cuotai_core::edit_error::EditError> {
    // RF1 contract + RF4 reload guard: when the caller is editing
    // (not appending), the slot at `edit` must still describe the
    // (provider, name) the wizard is editing. If the file on disk
    // was renamed externally, refuse the write with `EditError::Conflict`
    // so the wizard can decide between cancel / reload / retry.
    if let Some((provider, ref name)) = expected_identity {
        if let Some(idx) = edit {
            let matches = file
                .config
                .accounts
                .get(idx)
                .is_some_and(|a| a.provider == provider && a.name.eq_ignore_ascii_case(name));
            if !matches {
                return Err(cuotai_core::edit_error::EditError::Conflict {
                    path: file.path.clone(),
                });
            }
        }
    }
    let prev = file.config.clone();
    let prev_len = prev.accounts.len();
    let edit_index = edit.unwrap_or(prev_len);
    let operation = Edit::Upsert(edit_index);
    // 1) Stage the new cycle on a clone so a structural-diff rejection
    //    leaves the live cycle untouched.
    let mut staged_cycle = refresh_cycle.clone_state_for_edit();
    let due = staged_cycle
        .apply_edit(&prev, &config, operation, Instant::now())
        .map_err(|e| {
            cuotai_core::edit_error::validation_note(
                None,
                cuotai_core::text::Localized::fixed("edit_error.refresh_cycle_diff", e.to_string()),
            )
        })?;
    // 2) Build the new monitor BEFORE touching disk. `Monitor::with_config`
    //    validates the new config up-front (refresh/timeout/concurrency
    //    bounds, account-name uniqueness, OAuth shape). A failure here
    //    means the user-visible config is wrong; surface it and leave
    //    runtime state bound to the previous configuration.
    let new_monitor = monitor.with_config(config.clone()).map_err(|e| {
        cuotai_core::edit_error::validation_note(
            None,
            cuotai_core::text::Localized::fixed("edit_error.monitor_with_config", e.to_string()),
        )
    })?;
    let new_monitor = Arc::new(new_monitor);
    // 3) Persist to disk. `save` runs `validate` again before opening
    //    the file; a failure here is a real I/O problem (disk full,
    //    perms) — runtime stays bound to the previous configuration.
    //    The staged cycle and new monitor drop on return.
    file.save(config.clone())?;
    // Saved: an edited identity (rename, credential, region) loses its entry.
    let _ = new_monitor.prune_models();
    new_monitor.mark_history_removed();
    // 4) Commit: cycle, monitor, per-account refresh table and UI state
    //    all change together. From this point on a panic must not roll
    //    back; callers depend on the cycle and monitor agreeing with the
    //    on-disk file.
    *refresh_cycle = staged_cycle;
    // Drop any pending per-account query for the edited index — its
    // answer would belong to the previous configuration. Other slots are
    // untouched and may still finish their in-flight work.
    account_refresh.clear_slot(edit_index);
    // Resize the per-account refresh table only when the account count
    // changed (an append grows the table; a replace keeps its length).
    // Untouched slots keep their in-flight jobs so we don't abort queries
    // for accounts that didn't change.
    match account_refresh.len().cmp(&file.config.accounts.len()) {
        std::cmp::Ordering::Less => {
            account_refresh.extend_to(file.config.accounts.len());
        }
        std::cmp::Ordering::Greater => {
            // Replacements keep the same length; the cycle's `apply_edit`
            // already validated the diff, so an over-long table here would
            // mean the previous length was wrong. Truncate just in case.
            account_refresh.truncate_to(file.config.accounts.len());
        }
        std::cmp::Ordering::Equal => {}
    }
    state.set_accounts(&file.config);
    state.restore_selection(
        state.selected_account(&cycle_slot_reports(refresh_cycle)),
        &cycle_slot_reports(refresh_cycle),
    );
    state.error = None;
    state.filter.clear();
    state.detail_scroll = 0;
    state.details = false;
    Ok((new_monitor, due.first().copied()))
}

/// Persist one account move and reindex reports/deadlines without scheduling
/// any provider query. Disk, monitor, cycle and selection commit only after
/// every staging step succeeds; the selected account is tracked by its old
/// identity-position mapping rather than left on the old numeric row.
fn move_account_in_config(
    file: &mut ConfigFile,
    monitor: &Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    from: usize,
    to: usize,
) -> Result<Arc<Monitor>, cuotai_core::edit_error::EditError> {
    let selected = state.selected_account(&cycle_slot_reports(refresh_cycle));
    let selected_after = selected.map(|index| {
        if index == from {
            to
        } else if from < to && index > from && index <= to {
            index - 1
        } else if to < from && index >= to && index < from {
            index + 1
        } else {
            index
        }
    });
    let prev = file.config.clone();
    let config = prev.move_account(from, to)?;
    let mut staged_cycle = refresh_cycle.clone_state_for_edit();
    let due = staged_cycle
        .apply_edit(&prev, &config, Edit::Move { from, to }, Instant::now())
        .map_err(|error| {
            cuotai_core::edit_error::validation_note(
                None,
                cuotai_core::text::Localized::fixed(
                    "edit_error.refresh_cycle_move",
                    error.to_string(),
                ),
            )
        })?;
    debug_assert!(due.is_empty(), "a move must never schedule a query");
    let new_monitor = monitor.with_config(config.clone()).map_err(|error| {
        cuotai_core::edit_error::validation_note(
            None,
            cuotai_core::text::Localized::fixed(
                "edit_error.monitor_with_config_move",
                error.to_string(),
            ),
        )
    })?;
    file.save(config)?;

    *refresh_cycle = staged_cycle;
    account_refresh.move_account(from, to);
    state.set_accounts(&file.config);
    state.restore_selection(selected_after, &cycle_slot_reports(refresh_cycle));
    state.detail_scroll = 0;
    state.error = None;
    Ok(Arc::new(new_monitor))
}

// The actual removal flow now lives in `account_edit::remove_account`
// (core) plus the `run_remove_account` helper below; that helper keeps
// the runtime bound to the previous configuration until the TOML save
// succeeds, so the dashboard never observes a half-applied deletion.

/// Render the deletion dialog for the current state. Title + body + footer
/// are kept together so the layout block can use them without leaking
/// secrets into the displayed text (the storage_key is opaque; OAuth
/// fields are stripped of the refresh/access token and never re-rendered).
fn render_deletion(state_: &DeletionState, file: &ConfigFile) -> (&'static str, String, String) {
    let account_label = |index: usize| -> String {
        file.config
            .accounts
            .get(index)
            .map(|a| format!("\"{}\" ({})", safe(&a.name), a.provider.label()))
            .unwrap_or_else(|| format!("conta #{}", index))
    };
    match state_ {
        DeletionState::Confirm { index } => (
            " Excluir conta ",
            format!(
                "Excluir a conta {}?\n\nSerá removida de {}.\n\nVariáveis de ambiente, credenciais no Keychain e acesso no provedor não serão apagados nem revogados nesta etapa.",
                account_label(*index),
                safe(&file.path.display().to_string()),
            ),
            "s confirma exclusão • n/Esc cancela • Ctrl-C sai".into(),
        ),
        DeletionState::AskPurge { index } => (
            " Apagar credencial do cofre? ",
            format!(
                "A conta {} tem credencial OAuth salva no cofre do sistema.\n\nApagar também a credencial no cofre?\n\n• s apaga do TOML e do cofre\n• n mantém a credencial no cofre (a conta some do TOML mesmo assim)\n• Esc cancela a exclusão",
                account_label(*index),
            ),
            "s apaga credencial • n mantém • Esc cancela".into(),
        ),
        DeletionState::Executing { .. } => (
            " Removendo conta ",
            "Salvando a configuração e atualizando o cofre…".into(),
            "".into(),
        ),
        DeletionState::Result { index, outcome } => {
            let (body, footer) = render_deletion_result(*index, outcome);
            (" Resultado ", body, footer)
        }
    }
}

fn render_deletion_result(index: usize, outcome: &RemoveOutcome) -> (String, String) {
    match outcome {
        RemoveOutcome::RemovedNoCredential(_) => (
            format!(
                "Conta #{} removida do TOML. Esta conta não tinha credencial no cofre; nada foi apagado do cofre do sistema.",
                index,
            ),
            "Enter/Esc fecha".into(),
        ),
        RemoveOutcome::RemovedFromVault(_) => (
            format!(
                "Conta #{} removida do TOML e credencial apagada do cofre do sistema.",
                index,
            ),
            "Enter/Esc fecha".into(),
        ),
        RemoveOutcome::CredentialKept { reason, .. } => match reason {
            account_edit::KeptReason::Declined => (
                format!(
                    "Conta #{} removida do TOML. Você optou por manter a credencial no cofre do sistema.",
                    index,
                ),
                "r tenta apagar a credencial agora • Enter/Esc fecha".into(),
            ),
            account_edit::KeptReason::SharedWith { other_account } => (
                format!(
                    "Conta #{} removida do TOML. A credencial foi mantida porque a conta \"{}\" ainda a usa; apagá-la quebraria a sobrevivente.",
                    index,
                    safe(other_account),
                ),
                "r tenta apagar a credencial agora (refere se a sobrevivente sumir) • Enter/Esc fecha".into(),
            ),
        },
        RemoveOutcome::PartialFailure { vault_error, .. } => (
            format!(
                "Conta #{} removida do TOML, mas a credencial NÃO pôde ser apagada do cofre.\n\nMotivo: {}\n\nA credencial continua no cofre. Pressione r para tentar de novo agora.",
                index,
                safe(&vault_error.to_string()),
            ),
            "r tenta apagar a credencial agora • Enter/Esc fecha".into(),
        ),
    }
}

/// Drive the deletion state machine from a key event. The actual core
/// call is synchronous from the event loop's perspective
/// ([`run_remove_account`] uses `tokio::task::block_in_place`), which
/// DOES block the event loop for the duration of the vault round-trip.
/// That is acceptable because the dialog is modal and there is no
/// other work the loop can do while the user is being asked to
/// confirm; an alternative would be a `JoinHandle` polled alongside
/// `refresh.0` / `account_refresh.0`, but the modal dialog makes the
/// blocking call simpler and the user-perceived delay is identical
/// (no other UI can render while the dialog is up).
///
/// Returns `true` when the deletion dialog should be closed (Esc on
/// confirm, Enter/Esc on the result step). The caller assigns
/// `deletion = None` accordingly.
///
/// `monitor` is reassigned in place when the core reports a successful
/// removal — the caller passes `&mut Arc<Monitor>` so we can rebuild the
/// monitor from the now-saved `ConfigFile.config` without leaking the
/// half-applied state.
#[allow(clippy::too_many_arguments)]
fn handle_deletion_key(
    state_: &mut DeletionState,
    code: KeyCode,
    file: &mut ConfigFile,
    monitor: &mut Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    session: Arc<OAuthSession>,
) -> bool {
    match state_ {
        DeletionState::Confirm { index } => handle_confirm(
            code,
            *index,
            state_,
            file,
            monitor,
            refresh_cycle,
            account_refresh,
            state,
            session,
        ),
        DeletionState::AskPurge { index } => handle_ask_purge(
            code,
            *index,
            state_,
            file,
            monitor,
            refresh_cycle,
            account_refresh,
            state,
            session,
        ),
        DeletionState::Executing { .. } => false,
        DeletionState::Result { outcome, .. } => {
            handle_result(code, outcome, file, session.as_ref())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_confirm(
    code: KeyCode,
    index: usize,
    state_: &mut DeletionState,
    file: &mut ConfigFile,
    monitor: &mut Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    session: Arc<OAuthSession>,
) -> bool {
    match code {
        KeyCode::Esc | KeyCode::Char('n' | 'N') => true,
        KeyCode::Char('s' | 'S') => {
            let has_oauth = file
                .config
                .accounts
                .get(index)
                .is_some_and(|a| a.oauth.is_some());
            if has_oauth {
                *state_ = DeletionState::AskPurge { index };
            } else {
                *state_ = DeletionState::Executing {
                    index,
                    purge: false,
                };
                run_remove_account(
                    state_,
                    file,
                    monitor,
                    refresh_cycle,
                    account_refresh,
                    state,
                    session,
                );
            }
            false
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_ask_purge(
    code: KeyCode,
    index: usize,
    state_: &mut DeletionState,
    file: &mut ConfigFile,
    monitor: &mut Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    session: Arc<OAuthSession>,
) -> bool {
    match code {
        KeyCode::Esc => true,
        KeyCode::Char('n' | 'N') => {
            // n = keep the credential in the vault (default safe choice).
            *state_ = DeletionState::Executing {
                index,
                purge: false,
            };
            run_remove_account(
                state_,
                file,
                monitor,
                refresh_cycle,
                account_refresh,
                state,
                session,
            );
            false
        }
        KeyCode::Char('s' | 'S') => {
            *state_ = DeletionState::Executing { index, purge: true };
            run_remove_account(
                state_,
                file,
                monitor,
                refresh_cycle,
                account_refresh,
                state,
                session,
            );
            false
        }
        _ => false,
    }
}

fn handle_result(
    code: KeyCode,
    outcome: &mut RemoveOutcome,
    file: &ConfigFile,
    session: &OAuthSession,
) -> bool {
    match code {
        KeyCode::Enter | KeyCode::Esc => true,
        KeyCode::Char('r' | 'R') => {
            // Retry the purge via the recovery primitive. We
            // re-derive the shared-key guard against the live config,
            // so a retry that becomes safe (the survivor was dropped)
            // succeeds, and a retry that is still unsafe (new shared
            // ref) is refused with an honest message.
            let removed = match outcome {
                RemoveOutcome::CredentialKept { account, .. }
                | RemoveOutcome::PartialFailure { account, .. } => account.clone(),
                // Removed variants have nothing to retry.
                RemoveOutcome::RemovedNoCredential(_) | RemoveOutcome::RemovedFromVault(_) => {
                    return false
                }
            };
            match retry_purge(session, &file.config, &removed) {
                Ok(()) => {
                    *outcome = RemoveOutcome::RemovedFromVault(removed);
                }
                Err(message) => {
                    *outcome = RemoveOutcome::PartialFailure {
                        account: removed,
                        vault_error: message,
                    };
                }
            }
            false
        }
        _ => false,
    }
}

/// Re-attempt a vault purge via the core's recovery primitive against
/// the *live* configuration. The shared-key guard runs on every call;
/// callers MUST NOT pass an account index — the recovery primitive is
/// the `RemovedAccount` retained from the original outcome.
fn retry_purge(
    session: &OAuthSession,
    config: &Config,
    removed: &RemovedAccount,
) -> Result<(), cuotai_core::text::Localized> {
    let oauth = removed.oauth.as_ref().ok_or_else(|| {
        cuotai_core::text::Localized::fixed(
            "remove_error.purge_no_oauth",
            "Conta não tinha credencial OAuth para apagar.",
        )
    })?;
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            account_edit::purge_credential(session, config, removed.provider, oauth).await
        })
    })
}

/// Execute `account_edit::remove_account` against the live `ConfigFile`
/// and, on TOML save success, rebuild the monitor + cycle to keep
/// runtime state consistent with the new file. The vault purge runs
/// asynchronously via the existing `OAuthSession`; we use
/// `tokio::task::block_in_place` because the TUI event loop is itself
/// driving the dialog and cannot `.await` directly.
///
/// Returns `true` when the deletion dialog should close (the pre-save
/// staging failed or the index became invalid); returns `false` when
/// the caller should keep the dialog open in the `Result` step so the
/// user can read the typed outcome.
#[allow(clippy::too_many_arguments)]
fn run_remove_account(
    state_: &mut DeletionState,
    file: &mut ConfigFile,
    monitor: &mut Arc<Monitor>,
    refresh_cycle: &mut RefreshCycle,
    account_refresh: &mut AccountRefresh,
    state: &mut State,
    session: Arc<OAuthSession>,
) -> bool {
    let (index, purge) = match state_ {
        DeletionState::Executing { index, purge } => (*index, *purge),
        _ => return false,
    };
    let prev = file.config.clone();
    let new_config = match prev.clone().without_account(index) {
        Ok(cfg) => cfg,
        Err(_) => {
            // `without_account` enforces the same bounds check; if it
            // fails here the index was invalid by the time we got here
            // (e.g. concurrent removal). Runtime stays bound to the
            // previous configuration.
            state.error = Some("Conta já removida.".to_string());
            return true;
        }
    };

    // Step 1: stage cycle + rebuild monitor BEFORE the disk write so a
    // cycle / monitor failure leaves the runtime untouched. This mirrors
    // the RF1 invariant and is the local equivalent of the same check
    // that `remove_account` performs against the TOML.
    let mut staged_cycle = refresh_cycle.clone_state_for_edit();
    if staged_cycle
        .apply_edit(&prev, &new_config, Edit::Remove(index), Instant::now())
        .is_err()
    {
        state.error = Some("Edição rejeitada pela agenda; nenhuma alteração aplicada.".to_string());
        return true;
    }
    let new_monitor = match monitor.with_config(new_config.clone()) {
        Ok(m) => Arc::new(m),
        Err(_) => {
            state.error = Some("Configuração inválida após remoção.".to_string());
            return true;
        }
    };

    // Step 2: drive the core removal. The core owns the TOML save + vault
    // purge + shared-key guard. We pre-validated the new configuration
    // above; if the core's save step still fails, the vault was never
    // touched and runtime stays bound to the previous configuration.
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            account_edit::remove_account(file, session.as_ref(), index, purge).await
        })
    });
    match outcome {
        Ok(outcome) => {
            // Disk saved (and possibly vault updated). Commit the staged
            // cycle + new monitor so runtime matches disk.
            *refresh_cycle = staged_cycle;
            *monitor = Arc::clone(&new_monitor);
            let _ = monitor.prune_models();
            monitor.mark_history_removed();
            account_refresh.clear_slot(index);
            if account_refresh.len() > file.config.accounts.len() {
                account_refresh.remove(index);
            }
            state.set_accounts(&file.config);
            state.restore_selection(
                state.selected_account(&cycle_slot_reports(refresh_cycle)),
                &cycle_slot_reports(refresh_cycle),
            );
            state.error = None;
            state.detail_scroll = 0;
            state.details = false;
            *state_ = DeletionState::Result {
                index,
                outcome: Box::new(outcome),
            };
            // Keep the dialog open so the user reads the outcome.
            false
        }
        Err(RemoveError::Index(_)) => {
            state.error = Some("Conta já removida.".to_string());
            true
        }
        Err(RemoveError::Save(_)) => {
            // Disk save failed BEFORE any vault action; runtime stays
            // bound to the previous configuration (we did not assign
            // `*refresh_cycle` or `*monitor`).
            state.error = Some("Falha ao salvar; nenhuma alteração aplicada.".to_string());
            true
        }
        Err(RemoveError::InvalidOAuth(message)) => {
            // The TOML was NOT saved; runtime stays bound to the
            // previous configuration. Surface the typed message so
            // the user knows why the removal was refused and what to
            // do about it (manual cleanup of the vault entry).
            state.error = Some(message);
            true
        }
        Err(error @ RemoveError::Session(_)) => {
            // FIX-06: the vault refused to delete the account's web
            // session; nothing was saved or deleted.
            state.error = Some(resolve_l10n(&error.l10n()));
            true
        }
    }
}

fn start_account_refresh(monitor: &Arc<Monitor>, index: usize) -> JoinHandle<cuotai_core::Report> {
    let monitor = Arc::clone(monitor);
    tokio::spawn(async move { monitor.refresh_account(index).await })
}

fn start_refresh(monitor: &Arc<Monitor>) -> JoinHandle<Snapshot> {
    let monitor = Arc::clone(monitor);
    tokio::spawn(async move { monitor.refresh().await })
}

struct State {
    accounts: Vec<Account>,
    /// The configuration's global `refresh_seconds`: with each account's
    /// own it decides whether a reading is fresh enough for a pace (PRD
    /// 0024 FIX-11).
    refresh_seconds: u64,
    selection: TableState,
    filter: String,
    filtering: bool,
    location: String,
    error: Option<String>,
    detail_scroll: u16,
    show_unmeasured: bool,
    details: bool,
    targets: dashboard::Targets,
    model_counts: ModelCounts,
    history: HistoryView,
    /// PRD 0006 RF6.1: span of the details' history.
    history_range: HistoryRange,
    /// RF4 effects; `None` when animations are off.
    anim: Option<Anim>,
    /// PRD 0003 RF4 and PRD 0008 RF4: reset notices and usage alerts;
    /// `None` without `--notify`.
    notices: Option<Notices>,
    /// PRD 0011 RF5: the last provider statuses the status loop read;
    /// empty while status queries are off.
    incidents: Vec<ProviderStatus>,
    /// PRD 0015 RF4.2: the local cost the details show; `None` before the
    /// first read or without a local data directory.
    local_cost: Option<cuotai_core::cost::CostSummary>,
}

/// Per-account synced catalog sizes shown on the cards. Read from the local
/// cache only when the monitor changes (load, edit, reload) or after the
/// Models screen was open, never on every frame.
#[derive(Default)]
struct ModelCounts {
    monitor: usize,
    accounts: usize,
    counts: Vec<Option<usize>>,
    stale: bool,
}

impl ModelCounts {
    fn refresh(&mut self, monitor: &Arc<Monitor>, accounts: usize) {
        let id = Arc::as_ptr(monitor) as usize;
        if !self.stale && self.monitor == id && self.accounts == accounts {
            return;
        }
        self.counts = (0..accounts)
            .map(|index| monitor.model_count(index))
            .collect();
        self.monitor = id;
        self.accounts = accounts;
        self.stale = false;
    }
}

/// PRD 0007 RF1.5 over PRD 0006: the recorded points the pace of each
/// account's metrics reads, re-read only when the monitor or the account's
/// report changes (never per frame), the details' history (RF6.1) and the
/// history warning to show.
#[derive(Default)]
struct HistoryView {
    monitor: usize,
    /// `checked_at` of the `Ready` report the points were read for.
    read_for: Vec<Option<chrono::DateTime<chrono::Utc>>>,
    /// Per account, per metric of its current report.
    recent: Vec<Vec<Vec<Point>>>,
    /// RF6.1: the open details' history, `None` when closed or without a
    /// history store.
    detail: Option<dashboard::DetailHistory>,
    /// Account, span and reading time `detail` was read for, and when.
    detail_for: Option<(usize, HistoryRange, Option<chrono::DateTime<chrono::Utc>>)>,
    detail_read: Option<Instant>,
    warning: Option<String>,
}

/// The details' history is read again this often while open, so the
/// reading the recorder writes after the refresh shows and the span moves.
const DETAIL_REREAD: Duration = Duration::from_secs(30);

impl HistoryView {
    /// `detail`: the account and span of the open details, if any.
    fn refresh(
        &mut self,
        monitor: &Arc<Monitor>,
        cycle: &RefreshCycle,
        detail: Option<(usize, HistoryRange)>,
    ) {
        let id = Arc::as_ptr(monitor) as usize;
        if self.monitor != id || self.read_for.len() != cycle.len() {
            *self = Self {
                monitor: id,
                read_for: vec![None; cycle.len()],
                recent: vec![Vec::new(); cycle.len()],
                ..Self::default()
            };
        }
        let now = chrono::Utc::now();
        for index in 0..cycle.len() {
            let report = cycle
                .account_report(index)
                .filter(|report| report.status == cuotai_core::Status::Ready);
            let stamp = report.map(|report| report.checked_at);
            if stamp != self.read_for[index] {
                self.recent[index] = report
                    .map(|report| monitor.recent_points(index, &report.metrics, report.checked_at))
                    .unwrap_or_default();
                self.read_for[index] = stamp;
            }
        }
        let wanted = detail.map(|(index, range)| {
            let reading = cycle.account_report(index).map(|report| report.checked_at);
            (index, range, reading)
        });
        let due = self
            .detail_read
            .is_none_or(|read| read.elapsed() >= DETAIL_REREAD);
        match wanted {
            None => {
                self.detail = None;
                self.detail_for = None;
            }
            Some((index, range, _)) if wanted != self.detail_for || due => {
                let start = range.since(now);
                self.detail = monitor.history().map(|_| dashboard::DetailHistory {
                    account: index,
                    range,
                    start,
                    end: now,
                    metrics: monitor
                        .account_history(index, start..=now)
                        .map_err(|error| error.l10n().resolve(tui_locale())),
                });
                self.detail_for = wanted;
                self.detail_read = Some(Instant::now());
            }
            Some(_) => {}
        }
        self.warning = monitor
            .history_status()
            .and_then(|status| status.warning())
            .map(|warning| warning.resolve(tui_locale()));
    }
}

impl State {
    /// The accounts of `config` after a save or reload; usage alerts of
    /// the accounts no longer there are forgotten.
    fn set_accounts(&mut self, config: &Config) {
        self.accounts = config.accounts.clone();
        self.refresh_seconds = config.refresh_seconds;
        if let Some(notices) = &mut self.notices {
            notices.keep_accounts(config);
        }
    }

    fn visible(&self, reports: &[Option<cuotai_core::Report>]) -> Vec<usize> {
        dashboard::visible_indices(&self.accounts, reports, &self.filter, self.show_unmeasured)
    }

    fn selected_account(&self, reports: &[Option<cuotai_core::Report>]) -> Option<usize> {
        self.selection
            .selected()
            .and_then(|i| self.visible(reports).get(i).copied())
    }

    fn restore_selection(
        &mut self,
        account: Option<usize>,
        reports: &[Option<cuotai_core::Report>],
    ) {
        let visible = self.visible(reports);
        let position = account.and_then(|account| visible.iter().position(|i| *i == account));
        self.selection
            .select(position.or_else(|| (!visible.is_empty()).then_some(0)));
    }

    fn move_selection(&mut self, delta: isize, reports: &[Option<cuotai_core::Report>]) {
        self.detail_scroll = 0;
        let max = self.visible(reports).len().saturating_sub(1);
        self.selection.select(Some(
            self.selection
                .selected()
                .unwrap_or(0)
                .saturating_add_signed(delta)
                .min(max),
        ));
    }

    fn draw(
        &mut self,
        terminal: &mut Screen,
        cycle: &RefreshCycle,
        refreshing: bool,
        next_refresh: Option<Duration>,
    ) -> io::Result<()> {
        // Build the per-slot report view the dashboard expects. The cycle
        // owns the canonical list of slots; we borrow the snapshot when it
        // is complete and synthesize the "all pending" view otherwise so
        // the dashboard can show "Aguardando" placeholders instead of
        // leaking stale rows from the previous configuration.
        let slot_reports: Vec<Option<cuotai_core::Report>> = match cycle.snapshot() {
            Some(snapshot) => snapshot.reports.iter().cloned().map(Some).collect(),
            None => (0..cycle.len())
                .map(|index| cycle.account_report(index).cloned())
                .collect(),
        };
        // PRD 0033 RF2.1: expired metrics of a preserved reading collapse.
        let now = chrono::Utc::now();
        let collapsed: Vec<Option<cuotai_core::Report>> = (0..cycle.len())
            .map(|index| {
                cycle
                    .last_good_report(index)
                    .map(|report| report.collapse_expired(now))
            })
            .collect();
        let last_good: Vec<Option<&cuotai_core::Report>> =
            collapsed.iter().map(Option::as_ref).collect();
        let visible = self.visible(&slot_reports);
        if visible.is_empty() {
            self.selection.select(None);
        } else {
            self.selection.select(Some(
                self.selection
                    .selected()
                    .unwrap_or(0)
                    .min(visible.len() - 1),
            ));
        }
        if let Some(anim) = &mut self.anim {
            anim.observe(&self.accounts, &slot_reports);
        }
        let mut targets = dashboard::Targets::default();
        let now = chrono::Utc::now();
        terminal.draw(|frame| {
            targets = dashboard::draw(
                frame,
                dashboard::View {
                    accounts: &self.accounts,
                    reports: &slot_reports,
                    model_counts: &self.model_counts.counts,
                    visible: &visible,
                    selected: self.selection.selected(),
                    details: self.details,
                    detail_scroll: self.detail_scroll,
                    show_unmeasured: self.show_unmeasured,
                    refreshing,
                    location: &self.location,
                    filter: &self.filter,
                    filtering: self.filtering,
                    next_refresh,
                    // A storage failure, size-ceiling pruning or dropped
                    // readings of the history (PRD 0006) show when nothing
                    // more urgent does.
                    error: self.error.as_deref().or(self.history.warning.as_deref()),
                    anim: self.anim.as_ref(),
                    last_good: &last_good,
                    recent: &self.history.recent,
                    refresh_seconds: self.refresh_seconds,
                    history: self.history.detail.as_ref(),
                    banner: self
                        .notices
                        .as_ref()
                        .and_then(|notices| notices.banner(now)),
                    incidents: &self.incidents,
                    local_cost: self.local_cost.as_ref(),
                    now,
                },
            );
            if let Some(anim) = &self.anim {
                anim.wipe(frame.buffer_mut());
            }
        })?;
        self.targets = targets;
        Ok(())
    }
}

/// Strip terminal controls from account names and remote labels.
pub(crate) fn safe(text: &str) -> Cow<'_, str> {
    if text.chars().any(char::is_control) {
        Cow::Owned(text.chars().filter(|c| !c.is_control()).collect())
    } else {
        Cow::Borrowed(text)
    }
}

/// Locale for resolving RF8 core messages in the TUI.
///
/// Uses `CUOTAI_LANG` when set, otherwise the operating-system language
/// when supported, and `en-US` as the fallback.
pub(crate) fn tui_locale() -> &'static str {
    use std::sync::OnceLock;
    static LOCALE: OnceLock<String> = OnceLock::new();
    LOCALE
        .get_or_init(|| {
            let preferred = std::env::var("CUOTAI_LANG").ok();
            #[cfg(test)]
            let system = Some("pt-BR".to_string());
            #[cfg(not(test))]
            let system = sys_locale::get_locale();
            tui_locale_for(preferred.as_deref(), system.as_deref())
        })
        .as_str()
}

fn tui_locale_for(preferred: Option<&str>, system: Option<&str>) -> String {
    if let Some(preferred) = preferred.filter(|locale| !locale.trim().is_empty()) {
        return preferred.to_string();
    }
    match system.map(cuotai_core::text::Locale::resolve) {
        Some(cuotai_core::text::Locale::Pt) => "pt-BR",
        Some(cuotai_core::text::Locale::Es) => "es",
        Some(cuotai_core::text::Locale::It) => "it",
        Some(cuotai_core::text::Locale::EnUs) | None => "en-US",
    }
    .to_string()
}

/// Resolve a [`cuotai_core::text::Localized`] for the TUI locale.
pub(crate) fn resolve_l10n(note: &cuotai_core::text::Localized) -> String {
    note.resolve(tui_locale())
}

/// Local `HH:MM` for an instant today, `YYYY-MM-DD HH:MM` otherwise.
pub(crate) fn local_time(at: chrono::DateTime<chrono::Utc>) -> String {
    let local = at.with_timezone(&chrono::Local);
    if local.date_naive() == chrono::Local::now().date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// PRD 0010 RF5: measurement age from the core's `freshness::age` in the
/// TUI's Portuguese chrome ("há 42 s", "há 3 min", "há 2 h", "há 1 d"). A
/// reading "from the future" (clock adjusted backwards) reads "agora".
pub(crate) fn age_text(
    checked_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let secs = cuotai_core::freshness::age(checked_at, now).as_secs();
    match secs {
        0 => "agora".into(),
        1..=59 => format!("há {secs} s"),
        60..=3_599 => format!("há {} min", secs / 60),
        3_600..=86_399 => format!("há {} h", secs / 3_600),
        _ => format!("há {} d", secs / 86_400),
    }
}

pub(crate) fn metric_text(metric: &Metric) -> String {
    let number = |value: Option<f64>| {
        value
            .map(|n| format!("{n:.2}"))
            .unwrap_or_else(|| "—".into())
    };
    let mut text = format!("{} {}", number(metric.used), safe(&metric.unit));
    if metric.limit.is_some() {
        text.push_str(&format!(" / {}", number(metric.limit)));
    }
    if metric.remaining.is_some() {
        text.push_str(&format!(" (restam {})", number(metric.remaining)));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;
    use std::collections::BTreeMap;

    #[test]
    fn locale_uses_supported_system_language_or_english_fallback() {
        assert_eq!(tui_locale_for(None, Some("es-AR")), "es");
        assert_eq!(tui_locale_for(None, Some("it-IT")), "it");
        assert_eq!(tui_locale_for(None, Some("pt_BR.UTF-8")), "pt-BR");
        assert_eq!(tui_locale_for(None, Some("en-GB")), "en-US");
        assert_eq!(tui_locale_for(None, Some("fr-FR")), "en-US");
        assert_eq!(tui_locale_for(None, None), "en-US");
    }

    #[test]
    fn locale_environment_override_takes_precedence() {
        assert_eq!(tui_locale_for(Some("es-AR"), Some("pt-BR")), "es-AR");
        assert_eq!(tui_locale_for(Some("  "), Some("es-ES")), "es");
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider,
            credential_env: None,
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    #[test]
    fn rotate_span_matches_remove_then_insert_for_every_pair() {
        let n = 5;
        for from in 0..n {
            for to in 0..n {
                let mut expected: Vec<usize> = (0..n).collect();
                let moved = expected.remove(from);
                expected.insert(to, moved);
                let mut got: Vec<usize> = (0..n).collect();
                rotate_span(&mut got, from, to);
                assert_eq!(got, expected, "from {from} to {to}");
            }
        }
    }

    #[tokio::test]
    async fn move_account_reorders_jobs_and_tokens_for_every_pair() {
        let n = 4;
        for from in 0..n {
            for to in 0..n {
                let mut table = AccountRefresh::new(n);
                let mut ids = Vec::new();
                for index in 0..n {
                    let handle = tokio::spawn(std::future::pending::<cuotai_core::Report>());
                    ids.push(handle.id());
                    table.install(
                        index,
                        handle,
                        cuotai_core::refresh::Generation(index as u64),
                    );
                }
                let mut expected_ids = ids.clone();
                let id = expected_ids.remove(from);
                expected_ids.insert(to, id);
                let mut expected_tokens: Vec<u64> = (0..n as u64).collect();
                let token = expected_tokens.remove(from);
                expected_tokens.insert(to, token);

                table.move_account(from, to);

                let got_ids: Vec<_> = table
                    .jobs
                    .iter()
                    .map(|job| job.as_ref().expect("job kept").id())
                    .collect();
                let got_tokens: Vec<u64> = table
                    .tokens
                    .iter()
                    .map(|token| token.expect("token kept").0)
                    .collect();
                assert_eq!(got_ids, expected_ids, "jobs from {from} to {to}");
                assert_eq!(got_tokens, expected_tokens, "tokens from {from} to {to}");
            }
        }
    }

    #[tokio::test]
    async fn remove_drops_only_the_indexed_job_and_keeps_order() {
        let n = 4;
        for index in 0..n {
            let mut table = AccountRefresh::new(n);
            let mut ids = Vec::new();
            let mut watchers = Vec::new();
            for slot in 0..n {
                let handle = tokio::spawn(std::future::pending::<cuotai_core::Report>());
                ids.push(handle.id());
                watchers.push(handle.abort_handle());
                table.install(slot, handle, cuotai_core::refresh::Generation(slot as u64));
            }
            let expected_ids: Vec<_> = ids
                .iter()
                .enumerate()
                .filter(|(slot, _)| *slot != index)
                .map(|(_, id)| *id)
                .collect();

            table.remove(index);
            tokio::task::yield_now().await;

            let got_ids: Vec<_> = table
                .jobs
                .iter()
                .map(|job| job.as_ref().expect("job kept").id())
                .collect();
            assert_eq!(got_ids, expected_ids, "jobs after removing {index}");
            let got_generations: Vec<u64> = table
                .tokens
                .iter()
                .map(|slot| slot.expect("generation kept").0)
                .collect();
            let expected_generations: Vec<u64> =
                (0..n as u64).filter(|slot| *slot != index as u64).collect();
            assert_eq!(
                got_generations, expected_generations,
                "generations after removing {index}"
            );
            // Dropping the handle detaches the task; it is not aborted.
            assert!(
                !watchers[index].is_finished(),
                "removed job {index} must not be aborted"
            );
        }
        let mut table = AccountRefresh::new(2);
        table.remove(2);
        assert_eq!(table.len(), 2, "out-of-range index is ignored");
    }

    fn three_accounts() -> Vec<Account> {
        vec![
            account("alpha", Provider::OpenAiApi),
            account("beta", Provider::AnthropicApi),
            account("gamma", Provider::Cursor),
        ]
    }

    fn state_with(accounts: Vec<Account>) -> State {
        State {
            accounts,
            refresh_seconds: Config::default().refresh_seconds,
            selection: TableState::default(),
            filter: String::new(),
            filtering: false,
            location: String::new(),
            error: None,
            detail_scroll: 0,
            show_unmeasured: true,
            details: true,
            targets: dashboard::Targets::default(),
            model_counts: ModelCounts::default(),
            history: HistoryView::default(),
            history_range: HistoryRange::default(),
            anim: None,
            notices: None,
            incidents: Vec::new(),
            local_cost: None,
        }
    }

    #[test]
    fn age_text_steps_at_minute_hour_and_day_boundaries() {
        use chrono::TimeZone;
        let at = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let age = |secs: i64| age_text(at, at + chrono::Duration::seconds(secs));
        assert_eq!(age(-30), "agora", "clock adjusted backwards");
        assert_eq!(age(0), "agora");
        assert_eq!(age(59), "há 59 s");
        assert_eq!(age(60), "há 1 min");
        assert_eq!(age(3_599), "há 59 min");
        assert_eq!(age(3_600), "há 1 h");
        assert_eq!(age(86_399), "há 23 h");
        assert_eq!(age(86_400), "há 1 d");
    }

    #[test]
    fn cycle_detail_advances_through_visible_accounts_and_resets_scroll() {
        let mut state = state_with(three_accounts());
        state.selection.select(Some(0));
        state.detail_scroll = 7;
        // move_selection(1) reflete exatamente o que NextDetail faz no run_command.
        state.move_selection(1, &[]);
        assert_eq!(state.selection.selected(), Some(1));
        assert_eq!(state.detail_scroll, 0, "scroll lateral precisa zerar");
        state.detail_scroll = 3;
        state.move_selection(1, &[]);
        assert_eq!(state.selection.selected(), Some(2));
        assert_eq!(state.detail_scroll, 0);
        // Sem wrap: avançar do último não volta ao início.
        state.detail_scroll = 1;
        state.move_selection(1, &[]);
        assert_eq!(state.selection.selected(), Some(2));
        assert_eq!(state.detail_scroll, 0);
    }

    #[test]
    fn cycle_detail_moves_backwards_without_underflowing() {
        let mut state = state_with(three_accounts());
        state.selection.select(Some(2));
        state.detail_scroll = 4;
        state.move_selection(-1, &[]);
        assert_eq!(state.selection.selected(), Some(1));
        assert_eq!(state.detail_scroll, 0);
        // Sem wrap: retroceder do primeiro fica parado.
        state.selection.select(Some(0));
        state.detail_scroll = 5;
        state.move_selection(-1, &[]);
        assert_eq!(state.selection.selected(), Some(0));
        assert_eq!(state.detail_scroll, 0);
    }

    #[test]
    fn cycle_detail_skips_filtered_out_accounts() {
        let mut state = state_with(three_accounts());
        state.accounts[0].name = "first-x".into();
        state.accounts[1].name = "beto".into();
        state.accounts[2].name = "last-x".into();
        state.selection.select(Some(0));
        state.filter = "-x".into();
        let visible = state.visible(&[]);
        assert_eq!(visible, vec![0, 2]);
        state.detail_scroll = 2;
        state.move_selection(1, &[]);
        assert_eq!(state.selection.selected(), Some(1));
        assert_eq!(state.selected_account(&[]), Some(2));
        assert_eq!(state.detail_scroll, 0);
    }

    #[test]
    fn key_command_maps_left_right_only_when_details_open() {
        // Espelha a função `key_command` local em `run`: extraída aqui só
        // para fixar a regra de gating — Left/Right não devem virar comando
        // fora do detalhe e não devem cair em outros comandos.
        fn map(code: KeyCode, details: bool) -> Option<&'static str> {
            if details {
                match code {
                    KeyCode::Right => return Some("next_detail"),
                    KeyCode::Left => return Some("prev_detail"),
                    _ => {}
                }
            }
            match code {
                KeyCode::Enter => Some("toggle_details"),
                KeyCode::Esc if details => Some("close_details"),
                KeyCode::Esc => Some("quit"),
                _ => None,
            }
        }
        assert_eq!(map(KeyCode::Right, true), Some("next_detail"));
        assert_eq!(map(KeyCode::Left, true), Some("prev_detail"));
        // Fora do detalhe, Left/Right são silenciosamente ignorados.
        assert_eq!(map(KeyCode::Right, false), None);
        assert_eq!(map(KeyCode::Left, false), None);
        // Esc continua saindo mesmo com detalhe aberto (mapeia para quit, não para detail).
        assert_eq!(map(KeyCode::Esc, true), Some("close_details"));
    }
}
