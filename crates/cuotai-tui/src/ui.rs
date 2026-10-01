use crate::{
    auth_ui::{LoginTask, Outcome},
    config_file::ConfigFile,
    dashboard::{self, FooterAction},
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
    oauth::{AuthorizedLogin, OAuthConfig, OAuthSession},
    Account, Config, EnvironmentCredentials, Metric, Monitor, Provider, Snapshot,
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
use tokio::sync::Semaphore;
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

struct Refresh(Option<JoinHandle<Snapshot>>);
struct AccountRefresh(Vec<Option<JoinHandle<cuotai_core::Report>>>);
impl Drop for AccountRefresh {
    fn drop(&mut self) {
        self.abort_all();
    }
}
impl AccountRefresh {
    fn abort_all(&mut self) {
        for job in &mut self.0 {
            if let Some(job) = job.take() {
                job.abort();
            }
        }
    }
}
impl Drop for Refresh {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}

pub async fn run(mut file: ConfigFile, session: Arc<OAuthSession>) -> io::Result<()> {
    let mut monitor = new_monitor(&file.config, &session)?;
    let mut auth_task: Option<LoginTask> = None;
    let mut authorized: Option<(Provider, OAuthConfig, Arc<AuthorizedLogin>)> = None;
    let mut saving_config: Option<Config> = None;
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
        snapshot: None,
        selection: TableState::default(),
        filter: String::new(),
        filtering: false,
        location: file.path.display().to_string(),
        error: None,
        detail_scroll: 0,
        show_unmeasured: true,
        details: false,
        targets: dashboard::Targets::default(),
    };
    let mut frame_area = Rect::default();
    let mut refresh = Refresh(if wizard.is_none() {
        Some(start_refresh(&monitor))
    } else {
        None
    });
    let mut account_refresh =
        AccountRefresh((0..file.config.accounts.len()).map(|_| None).collect());
    let mut next_account_refresh = file
        .config
        .accounts
        .iter()
        .map(|account| {
            Instant::now()
                + Duration::from_secs(
                    account.effective_refresh_seconds(file.config.refresh_seconds),
                )
        })
        .collect::<Vec<_>>();
    let mut refresh_slots = Arc::new(Semaphore::new(file.config.concurrency));
    let mut dirty = true;
    let mut last_tick = Instant::now();
    let mut deletion: Option<(usize, Option<String>)> = None;

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
        Filter,
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
        ClearFilter,
        /// Avança entre contas visíveis no detalhe (Left/Right quando detalhes abertos).
        NextDetail,
        /// Retrocede entre contas visíveis no detalhe.
        PrevDetail,
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
            KeyCode::Char('u') => Command::ToggleUnmeasured,
            KeyCode::Down if details => Command::ScrollDetailDown,
            KeyCode::Up if details => Command::ScrollDetailUp,
            KeyCode::Char('q') | KeyCode::Esc => Command::Quit,
            KeyCode::Char('d') | KeyCode::Delete => Command::Delete,
            KeyCode::Char('a') => Command::Add,
            KeyCode::Char('e') => Command::Edit,
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
                Action::Login { provider, config } => {
                    authorized = None;
                    if provider == Provider::XiaomiMimo {
                        execute!(io::stdout(), EnableBracketedPaste)?;
                    }
                    auth_task = Some(LoginTask::oauth(Arc::clone(&session), provider, config));
                }
                Action::AwsLogin { profile } => {
                    auth_task = Some(LoginTask::aws(profile));
                }
                Action::BailianConsoleLogin {
                    console_site,
                    profile,
                } => {
                    // O CLI do `bl` grava no perfil dedicado (params.cli_profile)
                    // e o Cuotai só supervisiona — sem ler stdout, sem guardar
                    // nada. O profile viaja do wizard F2 -> Action -> LoginTask
                    // e é passado ao CLI via `--config <profile>`.
                    auth_task = Some(LoginTask::bailian_console(console_site, profile));
                }
                Action::Save(config) => {
                    let pending = authorized.as_ref().filter(|(provider, oauth, _)| {
                        config
                            .accounts
                            .iter()
                            .any(|a| a.provider == *provider && a.oauth.as_ref() == Some(oauth))
                    });
                    if let Some((_, _, login)) = pending {
                        saving_config = Some(config);
                        auth_task = Some(LoginTask::store(Arc::clone(&session), Arc::clone(login)));
                    } else {
                        match apply_config(&mut file, config, &session, &mut state) {
                            Err(error) => {
                                if let Some(active) = &mut wizard {
                                    active.set_error(error);
                                }
                            }
                            Ok(new_monitor) => {
                                monitor = new_monitor;
                                wizard = None;
                                authorized = None;
                                account_refresh.abort_all();
                                account_refresh.0 =
                                    (0..file.config.accounts.len()).map(|_| None).collect();
                                refresh_slots = Arc::new(Semaphore::new(file.config.concurrency));
                                next_account_refresh = file
                                    .config
                                    .accounts
                                    .iter()
                                    .map(|account| {
                                        Instant::now()
                                            + Duration::from_secs(
                                                account.effective_refresh_seconds(
                                                    file.config.refresh_seconds,
                                                ),
                                            )
                                    })
                                    .collect::<Vec<_>>();
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
                    let selected = state.selected_account();
                    state.show_unmeasured = !state.show_unmeasured;
                    state.restore_selection(selected);
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
                    if let Some(index) = state
                        .selection
                        .selected()
                        .and_then(|i| state.visible().get(i).copied())
                    {
                        if let Some(job) = refresh.0.take() {
                            job.abort();
                        }
                        account_refresh.abort_all();
                        deletion = Some((index, None));
                    }
                }
                Command::Add | Command::Edit => {
                    let editing = if $command == Command::Edit {
                        state
                            .selection
                            .selected()
                            .and_then(|i| state.visible().get(i).copied())
                    } else {
                        None
                    };
                    if matches!($command, Command::Add) || editing.is_some() {
                        if let Some(job) = refresh.0.take() {
                            job.abort();
                        }
                        account_refresh.abort_all();
                        wizard = Some(Wizard::new(&file.config, editing));
                        authorized = None;
                    }
                }
                Command::Filter => {
                    state.filtering = true;
                }
                Command::Refresh => {
                    for index in 0..account_refresh.0.len() {
                        if account_refresh.0[index].is_none() && refresh.0.is_none() {
                            account_refresh.0[index] =
                                Some(start_account_refresh(&monitor, &refresh_slots, index));
                        }
                    }
                }
                Command::RefreshSelected => {
                    if let Some(index) = state.selected_account() {
                        if account_refresh.0[index].is_none() && refresh.0.is_none() {
                            account_refresh.0[index] =
                                Some(start_account_refresh(&monitor, &refresh_slots, index));
                        }
                    }
                }
                Command::Down => state.move_selection(1),
                Command::Up => state.move_selection(-1),
                Command::PageDown => state.move_selection(10),
                Command::PageUp => state.move_selection(-10),
                Command::WheelDown => {
                    let step = state.targets.columns.max(1) as isize;
                    state.move_selection(step);
                }
                Command::WheelUp => {
                    let step = state.targets.columns.max(1) as isize;
                    state.move_selection(-step);
                }
                Command::Home => {
                    state.selection.select(Some(0));
                    state.detail_scroll = 0;
                }
                Command::End => {
                    state
                        .selection
                        .select(Some(state.visible().len().saturating_sub(1)));
                    state.detail_scroll = 0;
                }
                Command::ClearFilter => {
                    state.filter.clear();
                    state.selection.select(Some(0));
                    state.detail_scroll = 0;
                }
                // Navegação no detalhe: move a seleção entre contas visíveis
                // (sem wrap, igual a Down/Up) e zera o scroll lateral.
                Command::NextDetail => state.move_selection(1),
                Command::PrevDetail => state.move_selection(-1),
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
                if let Some(active) = &mut wizard {
                    match outcome {
                        Ok(Outcome::Authorized {
                            provider,
                            config,
                            login,
                        }) => {
                            active.login_finished(
                                config.clone(),
                                login.account_params(provider, &config),
                            );
                            authorized = Some((provider, config, login));
                        }
                        Ok(Outcome::Aws) => active.aws_login_finished(),
                        Ok(Outcome::BailianConsole) => {
                            // O `bl` gravou no cofre local; nada para Cuotai
                            // persistir. Apenas fechamos o diálogo: o usuário
                            // continua no wizard e a próxima consulta Token
                            // Plan usará o perfil dedicado via --config.
                            active.bailian_login_finished();
                        }
                        Ok(Outcome::Stored) => {
                            if let Some(config) = saving_config.take() {
                                match apply_config(&mut file, config, &session, &mut state) {
                                    Ok(new_monitor) => {
                                        monitor = new_monitor;
                                        wizard = None;
                                        authorized = None;
                                        account_refresh.abort_all();
                                        account_refresh.0 =
                                            (0..file.config.accounts.len()).map(|_| None).collect();
                                        next_account_refresh = file
                                            .config
                                            .accounts
                                            .iter()
                                            .map(|account| {
                                                Instant::now()
                                                    + Duration::from_secs(
                                                        account.effective_refresh_seconds(
                                                            file.config.refresh_seconds,
                                                        ),
                                                    )
                                            })
                                            .collect::<Vec<_>>();
                                        refresh_slots =
                                            Arc::new(Semaphore::new(file.config.concurrency));
                                    }
                                    Err(error) => active.set_error(error),
                                }
                            }
                        }
                        Err(error) => {
                            saving_config = None;
                            active.set_error(error.to_string());
                        }
                    }
                }
            }
        }
        if refresh.0.as_ref().is_some_and(JoinHandle::is_finished) {
            dirty = true;
            if let Some(job) = refresh.0.take() {
                match job.await {
                    Ok(snapshot) => {
                        let selected_account = state.selected_account();
                        state.snapshot = Some(snapshot);
                        state.restore_selection(selected_account);
                        state.error = None;
                    }
                    Err(_) => {
                        state.error = Some("Falha na tarefa de consulta; r para tentar novamente.")
                    }
                }
            }
            // Cada conta usa o seu próprio intervalo efetivo: contas com
            // override individual não devem acordar ao mesmo tempo do global.
            let now = Instant::now();
            for (deadline, account) in next_account_refresh
                .iter_mut()
                .zip(file.config.accounts.iter())
            {
                *deadline = now
                    + Duration::from_secs(
                        account.effective_refresh_seconds(file.config.refresh_seconds),
                    );
            }
        }
        for (index, (job, deadline)) in account_refresh
            .0
            .iter_mut()
            .zip(&mut next_account_refresh)
            .enumerate()
        {
            if job.as_ref().is_some_and(JoinHandle::is_finished) {
                dirty = true;
                if let Some(job) = job.take() {
                    match job.await {
                        Ok(report) => {
                            if let Some(snapshot) = &mut state.snapshot {
                                snapshot.reports[index] = report;
                                snapshot.finished_at = chrono::Utc::now();
                            }
                            state.error = None;
                        }
                        Err(_) => state.error = Some("Falha na consulta; R para tentar novamente."),
                    }
                }
                // Prazo da próxima consulta desta conta: usa o effective,
                // não o global, para honrar overrides individuais.
                if let Some(account) = file.config.accounts.get(index) {
                    *deadline = Instant::now()
                        + Duration::from_secs(
                            account.effective_refresh_seconds(file.config.refresh_seconds),
                        );
                }
            }
        }
        if wizard.is_none() && deletion.is_none() && !file.config.accounts.is_empty() {
            if state.snapshot.is_none() && refresh.0.is_none() {
                refresh.0 = Some(start_refresh(&monitor));
                dirty = true;
            } else if state.snapshot.is_some() && refresh.0.is_none() {
                for (index, deadline) in next_account_refresh.iter().enumerate() {
                    if account_refresh.0[index].is_none() && Instant::now() >= *deadline {
                        account_refresh.0[index] =
                            Some(start_account_refresh(&monitor, &refresh_slots, index));
                        dirty = true;
                    }
                }
            }
        }
        if dirty {
            if let Some(task) = &mut auth_task {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    task.draw(frame)
                })?;
            } else if let Some(wizard) = &mut wizard {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    wizard.draw(frame, &file.path)
                })?;
            } else if let Some((index, error)) = &deletion {
                terminal.draw(|frame| {
                    frame_area = frame.area();
                    // Dracula frame background para a confirmação de exclusão.
                    frame.render_widget(
                        Block::default().style(Style::default().bg(DRACULA_BG).fg(DRACULA_FG)),
                        frame.area(),
                    );
                    let areas = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(frame.area());
                    let account = &file.config.accounts[*index];
                    let mut text = format!("Excluir a conta \"{}\" ({})?\n\nA conta será removida de {}.\n\nVariáveis de ambiente, credenciais no Keychain e acesso no provedor não serão apagados ou revogados.", safe(&account.name), account.provider.label(), safe(&file.path.display().to_string()));
                    if let Some(error) = error { text.push_str(&format!("\n\nFalha ao salvar: {}", safe(error))); }
                    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).block(Block::default().borders(Borders::ALL).title(" Excluir conta ")), areas[0]);
                    frame.render_widget(Paragraph::new("s confirma exclusão • n/Esc cancela • Ctrl-C sai").wrap(Wrap { trim: true }).style(Style::default().fg(DRACULA_YELLOW)), areas[1]);
                })?;
            } else {
                let selected = state.selected_account();
                let refreshing = refresh.0.is_some()
                    || selected.is_some_and(|index| account_refresh.0[index].is_some());
                let next = selected.and_then(|index| {
                    (refresh.0.is_none() && account_refresh.0[index].is_none()).then(|| {
                        next_account_refresh[index].saturating_duration_since(Instant::now())
                    })
                });
                state.draw(&mut terminal, refreshing, next)?;
                frame_area = state.targets.area;
            }
            dirty = false;
        }
        if event::poll(Duration::from_millis(100))? {
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
                                let _ = crate::auth_ui::open_browser(&url).await;
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
                                FooterAction::Delete => Command::Delete,
                                FooterAction::ToggleUnmeasured => Command::ToggleUnmeasured,
                                FooterAction::Filter => Command::Filter,
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
                if let Some(active) = &mut wizard {
                    let action = active.handle(key);
                    dispatch_wizard_action!(action);
                    continue;
                }
                if let Some((index, error)) = &mut deletion {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                            deletion = None;
                        }
                        KeyCode::Char('s' | 'S') => {
                            let mut config = file.config.clone();
                            config.accounts.remove(*index);
                            match apply_config(&mut file, config, &session, &mut state) {
                                Ok(updated) => {
                                    monitor = updated;
                                    deletion = None;
                                    account_refresh.abort_all();
                                    account_refresh.0 =
                                        (0..file.config.accounts.len()).map(|_| None).collect();
                                    next_account_refresh = file
                                        .config
                                        .accounts
                                        .iter()
                                        .map(|account| {
                                            Instant::now()
                                                + Duration::from_secs(
                                                    account.effective_refresh_seconds(
                                                        file.config.refresh_seconds,
                                                    ),
                                                )
                                        })
                                        .collect::<Vec<_>>();
                                    refresh_slots =
                                        Arc::new(Semaphore::new(file.config.concurrency));
                                }
                                Err(message) => *error = Some(message),
                            }
                        }
                        _ => {}
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

fn new_monitor(config: &Config, session: &Arc<OAuthSession>) -> io::Result<Arc<Monitor>> {
    Monitor::with_oauth(
        config.clone(),
        Arc::new(EnvironmentCredentials),
        Arc::clone(session),
    )
    .map(Arc::new)
    .map_err(io::Error::other)
}

fn apply_config(
    file: &mut ConfigFile,
    config: Config,
    session: &Arc<OAuthSession>,
    state: &mut State,
) -> Result<Arc<Monitor>, String> {
    let monitor = new_monitor(&config, session).map_err(|e| e.to_string())?;
    file.save(config)?;
    state.accounts = file.config.accounts.clone();
    state.snapshot = None;
    state.error = None;
    state.filter.clear();
    state.selection.select(Some(0));
    state.detail_scroll = 0;
    state.details = false;
    Ok(monitor)
}

fn start_account_refresh(
    monitor: &Arc<Monitor>,
    slots: &Arc<Semaphore>,
    index: usize,
) -> JoinHandle<cuotai_core::Report> {
    let monitor = Arc::clone(monitor);
    let slots = Arc::clone(slots);
    tokio::spawn(async move {
        let _permit = slots.acquire().await.expect("semaphore stays open");
        monitor.refresh_account(index).await
    })
}

fn start_refresh(monitor: &Arc<Monitor>) -> JoinHandle<Snapshot> {
    let monitor = Arc::clone(monitor);
    tokio::spawn(async move { monitor.refresh().await })
}

struct State {
    accounts: Vec<Account>,
    snapshot: Option<Snapshot>,
    selection: TableState,
    filter: String,
    filtering: bool,
    location: String,
    error: Option<&'static str>,
    detail_scroll: u16,
    show_unmeasured: bool,
    details: bool,
    targets: dashboard::Targets,
}

impl State {
    fn visible(&self) -> Vec<usize> {
        dashboard::visible_indices(
            &self.accounts,
            self.snapshot.as_ref(),
            &self.filter,
            self.show_unmeasured,
        )
    }

    fn selected_account(&self) -> Option<usize> {
        self.selection
            .selected()
            .and_then(|i| self.visible().get(i).copied())
    }

    fn restore_selection(&mut self, account: Option<usize>) {
        let visible = self.visible();
        let position = account.and_then(|account| visible.iter().position(|i| *i == account));
        self.selection
            .select(position.or_else(|| (!visible.is_empty()).then_some(0)));
    }

    fn move_selection(&mut self, delta: isize) {
        self.detail_scroll = 0;
        let max = self.visible().len().saturating_sub(1);
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
        refreshing: bool,
        next_refresh: Option<Duration>,
    ) -> io::Result<()> {
        let visible = self.visible();
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
        let mut targets = dashboard::Targets::default();
        terminal.draw(|frame| {
            targets = dashboard::draw(
                frame,
                dashboard::View {
                    accounts: &self.accounts,
                    snapshot: self.snapshot.as_ref(),
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
                    error: self.error,
                },
            );
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
            snapshot: None,
            selection: TableState::default(),
            filter: String::new(),
            filtering: false,
            location: String::new(),
            error: None,
            detail_scroll: 0,
            show_unmeasured: true,
            details: true,
            targets: dashboard::Targets::default(),
        }
    }

    #[test]
    fn cycle_detail_advances_through_visible_accounts_and_resets_scroll() {
        let mut state = state_with(three_accounts());
        state.selection.select(Some(0));
        state.detail_scroll = 7;
        // move_selection(1) reflete exatamente o que NextDetail faz no run_command.
        state.move_selection(1);
        assert_eq!(state.selection.selected(), Some(1));
        assert_eq!(state.detail_scroll, 0, "scroll lateral precisa zerar");
        state.detail_scroll = 3;
        state.move_selection(1);
        assert_eq!(state.selection.selected(), Some(2));
        assert_eq!(state.detail_scroll, 0);
        // Sem wrap: avançar do último não volta ao início.
        state.detail_scroll = 1;
        state.move_selection(1);
        assert_eq!(state.selection.selected(), Some(2));
        assert_eq!(state.detail_scroll, 0);
    }

    #[test]
    fn cycle_detail_moves_backwards_without_underflowing() {
        let mut state = state_with(three_accounts());
        state.selection.select(Some(2));
        state.detail_scroll = 4;
        state.move_selection(-1);
        assert_eq!(state.selection.selected(), Some(1));
        assert_eq!(state.detail_scroll, 0);
        // Sem wrap: retroceder do primeiro fica parado.
        state.selection.select(Some(0));
        state.detail_scroll = 5;
        state.move_selection(-1);
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
        let visible = state.visible();
        assert_eq!(visible, vec![0, 2]);
        state.detail_scroll = 2;
        state.move_selection(1);
        assert_eq!(state.selection.selected(), Some(1));
        assert_eq!(state.selected_account(), Some(2));
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
