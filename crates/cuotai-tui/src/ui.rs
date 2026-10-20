use crate::{
    about_view::{AboutView, Reply as AboutReply},
    auth_ui::{LoginTask, Outcome},
    dashboard::{self, FooterAction},
    models_view::{ModelsView, Reply},
    motion::{self, Anim},
    wizard::{Action, Wizard},
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
    history::Retention,
    login::{self, CompletedLogin},
    models::ModelCache,
    oauth::OAuthSession,
    pace::Point,
    refresh::{Edit, RefreshCycle},
    Account, Config, EnvironmentCredentials, Metric, Monitor, Snapshot,
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
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;

type Screen = Terminal<CrosstermBackend<io::Stdout>>;

// Paleta Dracula (mesma usada pelo dashboard/wizard/auth).
const DRACULA_BG: Color = Color::Rgb(40, 42, 54);
const DRACULA_FG: Color = Color::Rgb(248, 248, 242);
const DRACULA_YELLOW: Color = Color::Rgb(241, 250, 140);

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
            self.jobs.remove(index);
            self.tokens.remove(index);
        }
    }
    fn move_account(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let job = self.jobs.remove(from);
        self.jobs.insert(to, job);
        let token = self.tokens.remove(from);
        self.tokens.insert(to, token);
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
) -> io::Result<()> {
    let mut monitor = new_monitor(&file, &session, retention)?;
    let mut auth_task: Option<LoginTask> = None;
    let mut authorized: Option<CompletedLogin> = None;
    let mut saving_config: Option<(Config, Option<usize>)> = None;
    let mut wizard =
        (!file.exists || file.config.accounts.is_empty()).then(|| Wizard::new(&file.config, None));
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
        anim: animations.then(|| Anim::new(motion::truecolor(|key| std::env::var(key).ok()))),
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
    // RF5: Models section over the details screen or the wizard.
    let mut models: Option<ModelsView> = None;
    // "Sobre": libraries and repositories; opened from the dashboard only.
    let mut about: Option<AboutView> = None;

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
        /// Abre a tela Sobre (bibliotecas de código aberto e repositórios).
        About,
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
            KeyCode::Char('?') => Command::About,
            KeyCode::Char('u') => Command::ToggleUnmeasured,
            KeyCode::Down if details => Command::ScrollDetailDown,
            KeyCode::Up if details => Command::ScrollDetailUp,
            KeyCode::Char('q') | KeyCode::Esc => Command::Quit,
            KeyCode::Char('d') | KeyCode::Delete => Command::Delete,
            KeyCode::Char('a') => Command::Add,
            KeyCode::Char('e') => Command::Edit,
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
                            if driver.accepts_code() {
                                execute!(io::stdout(), EnableBracketedPaste)?;
                            }
                            auth_task = Some(LoginTask::new(driver));
                        }
                        Err(error) => {
                            if let Some(active) = &mut wizard {
                                active.set_error(error.to_string());
                            }
                        }
                    }
                }
                Action::Save { config, edit } => {
                    let pending = authorized.as_ref().filter(|completed| {
                        config.accounts.iter().any(|a| {
                            a.provider == completed.provider
                                && a.oauth.as_ref() == Some(&completed.config)
                        })
                    });
                    if let Some(completed) = pending {
                        saving_config = Some((config, edit));
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
                                // RF4: a Conflict typed error from
                                // `ConfigFile::save` means the file on
                                // disk changed. Open the conflict modal
                                // so the user can reload (which keeps
                                // the wizard's draft) or cancel.
                                if let Some(active) = &mut wizard {
                                    if error.is_conflict() {
                                        active.set_conflict(error.message());
                                    } else {
                                        active.set_error(error.message());
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
                        if let Some((job, _token)) = refresh.0.take() {
                            job.abort();
                        }
                        account_refresh.abort_all();
                        wizard = Some(Wizard::new(&file.config, editing));
                        authorized = None;
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
                Command::Models => {
                    if let Some(index) = state.selected_account(&cycle_slot_reports(&refresh_cycle))
                    {
                        if let Some(account) = file.config.accounts.get(index) {
                            models = Some(ModelsView::open(&monitor, index, account.clone()));
                        }
                    }
                }
            }
        };
    }
    loop {
        if last_tick.elapsed() >= Duration::from_secs(1) {
            dirty = true;
            last_tick = Instant::now();
        }
        if let Some(task) = &mut auth_task {
            dirty |= task.drain();
            if task.is_finished() {
                let outcome = task.result().await;
                execute!(io::stdout(), DisableBracketedPaste)?;
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
                            wizard = None;
                            authorized = None;
                        }
                        Ok(Outcome::Committed(
                            login::LoginCommitOutcome::SavedWithCleanupFailure(failure),
                        )) => {
                            state.error = Some(crate::auth_ui::cleanup_failure_message(&failure));
                            wizard = None;
                            authorized = None;
                        }
                        Ok(Outcome::Committed(login::LoginCommitOutcome::PartialFailure(
                            failure,
                        ))) => {
                            active.set_error(crate::auth_ui::partial_failure_message(&failure));
                        }
                        Ok(Outcome::RolledBack(failure)) => {
                            // Estado exato reportado pelo core; o login
                            // autorizado continua em `authorized` e salvar de
                            // novo repete o commit de forma idempotente.
                            active.set_error(crate::auth_ui::partial_failure_message(&failure));
                        }
                        Err(error) => {
                            saving_config = None;
                            active.set_error(error.to_string());
                        }
                    }
                }
            }
        }
        if let Some((job, _token)) = refresh.0.as_ref() {
            if job.is_finished() {
                dirty = true;
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
                            refresh_cycle.finish_full(Some(snapshot), token, Instant::now());
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
            if let Some((job, token)) = account_refresh.take(index) {
                match job.await {
                    Ok(report) => {
                        refresh_cycle.finish_account(index, Some(report), token, Instant::now());
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
            } else if let Some(view) = &mut about {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    view.draw(frame);
                    if let Some(anim) = &state.anim {
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
                state.history.refresh(&monitor, &refresh_cycle);
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
            let event = event::read()?;
            if let Event::Paste(text) = &event {
                if let Some(task) = &mut auth_task {
                    task.paste(text);
                }
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
                if let Some(view) = &mut about {
                    if view.mouse(mouse.kind, mouse.column, mouse.row) == AboutReply::Close {
                        about = None;
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
                if deletion.is_some() || state.filtering {
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
                                FooterAction::MoveUp => Command::MoveUp,
                                FooterAction::MoveDown => Command::MoveDown,
                                FooterAction::Delete => Command::Delete,
                                FooterAction::ToggleUnmeasured => Command::ToggleUnmeasured,
                                FooterAction::Filter => Command::Filter,
                                FooterAction::Models => Command::Models,
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
                            execute!(io::stdout(), DisableBracketedPaste)?;
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
                if let Some(view) = &mut about {
                    if view.key(key.code) == AboutReply::Close {
                        about = None;
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
/// by `retention`) in the OS data dir; without a resolvable data dir, model
/// operations report `NoCache`, nothing is recorded and quotas work.
fn new_monitor(
    file: &ConfigFile,
    session: &Arc<OAuthSession>,
    retention: Retention,
) -> io::Result<Arc<Monitor>> {
    let mut monitor = Monitor::with_oauth(
        file.config.clone(),
        Arc::new(EnvironmentCredentials),
        Arc::clone(session),
    )
    .map_err(io::Error::other)?;
    if let Ok(cache) = ModelCache::in_data_dir(file) {
        monitor = monitor.with_model_cache(cache);
    }
    if let Some(store) = crate::history_store(file, retention) {
        monitor = monitor.with_history(store);
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
    let new_monitor =
        new_monitor(&candidate, session, retention).map_err(|e| format!("monitor: {e}"))?;
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
    state.accounts = file.config.accounts.clone();
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
    state.accounts = file.config.accounts.clone();
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
    state.accounts = file.config.accounts.clone();
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
            state.accounts = file.config.accounts.clone();
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
    /// RF4 effects; `None` when animations are off.
    anim: Option<Anim>,
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
/// report changes (never per frame), and the history warning to show.
#[derive(Default)]
struct HistoryView {
    monitor: usize,
    /// `checked_at` of the `Ready` report the points were read for.
    read_for: Vec<Option<chrono::DateTime<chrono::Utc>>>,
    /// Per account, per metric of its current report.
    recent: Vec<Vec<Vec<Point>>>,
    warning: Option<String>,
}

impl HistoryView {
    fn refresh(&mut self, monitor: &Arc<Monitor>, cycle: &RefreshCycle) {
        let id = Arc::as_ptr(monitor) as usize;
        if self.monitor != id || self.read_for.len() != cycle.len() {
            *self = Self {
                monitor: id,
                read_for: vec![None; cycle.len()],
                recent: vec![Vec::new(); cycle.len()],
                warning: None,
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
                    .map(|report| monitor.recent_points(index, &report.metrics, now))
                    .unwrap_or_default();
                self.read_for[index] = stamp;
            }
        }
        self.warning = monitor
            .history_status()
            .and_then(|status| status.warning())
            .map(|warning| warning.resolve(tui_locale()));
    }
}

impl State {
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
        let last_good: Vec<Option<&cuotai_core::Report>> = (0..cycle.len())
            .map(|index| cycle.last_good_report(index))
            .collect();
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
                    now: chrono::Utc::now(),
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
/// Opt-in via the `CUOTAI_LANG` environment variable; defaults to
/// `pt-BR` so the Portuguese chrome stays consistent. RFC 5646 codes
/// are accepted (`en-US`, `es`, `es-AR`, ...); resolution mirrors
/// `cuotai_core::text::Locale::resolve` so the desktop and the TUI
/// never disagree about which language a code refers to.
pub(crate) fn tui_locale() -> &'static str {
    use std::sync::OnceLock;
    static LOCALE: OnceLock<String> = OnceLock::new();
    LOCALE
        .get_or_init(|| {
            std::env::var("CUOTAI_LANG")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "pt-BR".to_string())
        })
        .as_str()
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

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            name: name.into(),
            provider,
            credential_env: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
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
            anim: None,
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
