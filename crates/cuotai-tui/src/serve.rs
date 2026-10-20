//! `cuotai --serve` (PRD 0012 RF4): the core's local server in the
//! foreground, without the TUI and without needing a terminal. It keeps one
//! `RefreshCycle` driven like the dashboard's (a first full sweep, then each
//! account at its own deadline with the core's backoff) and serves the
//! `export::v1` built from it; a request never starts a query.
//!
//! SIGINT and SIGTERM (Ctrl+C and Ctrl+Break on Windows) close the port and
//! end the process. Exit codes: `0` when some account had a healthy
//! (`ready`) reading during the run, `2` when none had one until shutdown;
//! local errors (port, configuration, token) return `Err`, which `main`
//! turns into `1`.

use std::{
    error::Error,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use chrono::Utc;
use cuotai_core::{
    config_file::ConfigFile,
    export::v1::{Export, Options},
    refresh::{Generation, RefreshCycle},
    serve::{AccessEntry, LocalServer, ServeConfig, Token, DEFAULT_PORT},
    Monitor, Report, Snapshot, Status,
};
use tokio::task::JoinHandle;

/// How often finished queries are applied and due accounts are started.
const TICK: Duration = Duration::from_millis(250);

/// What `--serve` was given on the command line.
pub(crate) struct ServeArgs {
    /// `--port`: only this port (no fallback); without it [`DEFAULT_PORT`]
    /// and, when taken, the next nine.
    pub port: Option<u16>,
    /// `--token-file`: the bearer token every request must carry.
    pub token_file: Option<PathBuf>,
}

/// The queries in flight, each with the generation its finish hands back.
struct Jobs {
    full: Option<(JoinHandle<Snapshot>, Generation)>,
    accounts: Vec<Option<(JoinHandle<Report>, Generation)>>,
    status: Option<JoinHandle<()>>,
    status_due: Instant,
}

impl Jobs {
    fn abort(self) {
        if let Some((job, _)) = self.full {
            job.abort();
        }
        for (job, _) in self.accounts.into_iter().flatten() {
            job.abort();
        }
        if let Some(job) = self.status {
            job.abort();
        }
    }
}

/// Run the server for `file` with `monitor` (built for `file.config`) until
/// a shutdown signal arrives.
pub(crate) async fn run(
    file: ConfigFile,
    monitor: Monitor,
    args: ServeArgs,
) -> Result<u8, Box<dyn Error>> {
    let token = args.token_file.as_deref().map(read_token).transpose()?;
    let identity = file.identity()?;
    // Registered before the port opens, so a failure is a local error and a
    // signal sent right after the address is printed is never lost.
    let mut shutdown = Shutdown::listen()?;
    let monitor = Arc::new(monitor);
    let config = file.config;
    let mut initial = RefreshCycle::new(&config, Instant::now());
    // PRD 0010 RF2.3: the last recorded readings serve, stale, until the
    // first query of each account answers.
    initial.restore_last_good(monitor.restored_readings());
    let cycle = Arc::new(Mutex::new(initial));
    let accounts = config.accounts.len();
    let source = {
        let (cycle, monitor) = (Arc::clone(&cycle), Arc::clone(&monitor));
        move || {
            let incidents = monitor.incident_statuses();
            let cycle = lock(&cycle);
            Some(Export::from_cycle(
                &config,
                &identity,
                &cycle,
                &incidents,
                Utc::now(),
                Options::default(),
            ))
        }
    };
    let server = LocalServer::start(serve_config(&args, token), source)
        .await
        .map_err(|error| error.l10n().resolve(crate::ui::tui_locale()))?;
    if server.fell_back() {
        eprintln!(
            "cuotai: a porta {DEFAULT_PORT} está em uso; servindo na porta {}.",
            server.addr().port()
        );
    }
    eprintln!("cuotai: servidor local somente leitura; Ctrl+C encerra.");
    // The address alone on stdout, for a script that started it.
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", server.url())?;
    stdout.flush()?;
    drop(stdout);

    let mut jobs = Jobs {
        full: None,
        accounts: (0..accounts).map(|_| None).collect(),
        status: None,
        status_due: Instant::now() + crate::ui::STATUS_TICK,
    };
    // The first sweep runs at once (the cycle's deadlines are for the next
    // ones); `Monitor::refresh` also reads the provider statuses when on.
    jobs.full = lock(&cycle)
        .begin_full()
        .map(|token| (spawn_full(&monitor), token));
    let mut healthy = false;
    loop {
        healthy |= apply_finished(&mut jobs, &cycle).await;
        start_due(&mut jobs, &cycle, &monitor);
        tokio::select! {
            () = shutdown.wait() => break,
            () = tokio::time::sleep(TICK) => {}
        }
    }
    server.shutdown().await;
    jobs.abort();
    // Readings queued to the history land before the process ends.
    monitor.flush_history(crate::HISTORY_FLUSH);
    if let Some(warning) = monitor.history_status().and_then(|s| s.warning()) {
        eprintln!("cuotai: {}", warning.resolve(crate::ui::tui_locale()));
    }
    eprintln!("cuotai: servidor local encerrado.");
    Ok(if healthy { 0 } else { 2 })
}

/// The bearer token in `path`, trimmed (a trailing newline is fine).
fn read_token(path: &Path) -> Result<Token, String> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        format!(
            "não foi possível ler o arquivo de token {}.",
            path.display()
        )
    })?;
    Token::parse(&text).map_err(|error| error.l10n().resolve(crate::ui::tui_locale()))
}

/// Loopback with the core's defaults; an explicit `--port` is binding (a
/// taken port is an error, never a silent move to another one).
fn serve_config(args: &ServeArgs, token: Option<Token>) -> ServeConfig {
    let access_log: cuotai_core::serve::AccessLog =
        Arc::new(|entry: &AccessEntry| eprintln!("{entry}"));
    ServeConfig {
        port: args.port.unwrap_or(DEFAULT_PORT),
        port_fallback: args.port.is_none(),
        token,
        access_log: Some(access_log),
        ..ServeConfig::default()
    }
}

fn lock(cycle: &Mutex<RefreshCycle>) -> std::sync::MutexGuard<'_, RefreshCycle> {
    cycle.lock().unwrap_or_else(PoisonError::into_inner)
}

fn spawn_full(monitor: &Arc<Monitor>) -> JoinHandle<Snapshot> {
    let monitor = Arc::clone(monitor);
    tokio::spawn(async move { monitor.refresh().await })
}

/// Hand every finished query to the cycle (a failed task as `None`, so the
/// core backs it off); `true` when one of them brought a `ready` report.
async fn apply_finished(jobs: &mut Jobs, cycle: &Mutex<RefreshCycle>) -> bool {
    let mut healthy = false;
    if jobs.full.as_ref().is_some_and(|(job, _)| job.is_finished()) {
        if let Some((job, token)) = jobs.full.take() {
            let snapshot = job.await.ok();
            healthy |= snapshot
                .as_ref()
                .is_some_and(|s| s.reports.iter().any(|r| r.status == Status::Ready));
            lock(cycle).finish_full(snapshot, token, Instant::now());
        }
    }
    for (index, slot) in jobs.accounts.iter_mut().enumerate() {
        if !slot.as_ref().is_some_and(|(job, _)| job.is_finished()) {
            continue;
        }
        if let Some((job, token)) = slot.take() {
            let report = job.await.ok();
            healthy |= report.as_ref().is_some_and(|r| r.status == Status::Ready);
            lock(cycle).finish_account(index, report, token, Instant::now());
        }
    }
    if jobs.status.as_ref().is_some_and(JoinHandle::is_finished) {
        jobs.status = None;
        jobs.status_due = Instant::now() + crate::ui::STATUS_TICK;
    }
    healthy
}

/// Start the per-account queries the cycle says are due: pending slots whose
/// deadline elapsed and periodic ones, never a busy slot nor one still
/// backing off; the status read every [`crate::ui::STATUS_TICK`] when on.
fn start_due(jobs: &mut Jobs, cycle: &Mutex<RefreshCycle>, monitor: &Arc<Monitor>) {
    if jobs.full.is_none() {
        let mut cycle = lock(cycle);
        let now = Instant::now();
        let mut due = cycle.pending_now_indices(now);
        due.extend(cycle.due_indices(now));
        due.sort_unstable();
        due.dedup();
        for index in due {
            if jobs.accounts[index].is_some() {
                continue;
            }
            if let Some(token) = cycle.begin(index) {
                let monitor = Arc::clone(monitor);
                let job = tokio::spawn(async move { monitor.refresh_account(index).await });
                jobs.accounts[index] = Some((job, token));
            }
        }
    }
    if jobs.status.is_none()
        && monitor.incidents().is_enabled()
        && Instant::now() >= jobs.status_due
    {
        let monitor = Arc::clone(monitor);
        jobs.status = Some(tokio::spawn(async move {
            monitor.refresh_incidents().await;
        }));
    }
}

/// SIGINT and SIGTERM.
#[cfg(unix)]
struct Shutdown {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Shutdown {
    fn listen() -> io::Result<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    /// Cancel-safe: a signal that arrives between two calls is kept.
    async fn wait(&mut self) {
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
        }
    }
}

/// Ctrl+C and Ctrl+Break.
#[cfg(windows)]
struct Shutdown {
    ctrl_c: tokio::signal::windows::CtrlC,
    ctrl_break: tokio::signal::windows::CtrlBreak,
}

#[cfg(windows)]
impl Shutdown {
    fn listen() -> io::Result<Self> {
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c()?,
            ctrl_break: tokio::signal::windows::ctrl_break()?,
        })
    }

    /// Cancel-safe: a signal that arrives between two calls is kept.
    async fn wait(&mut self) {
        tokio::select! {
            _ = self.ctrl_c.recv() => {}
            _ = self.ctrl_break.recv() => {}
        }
    }
}
