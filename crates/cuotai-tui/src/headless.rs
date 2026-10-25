//! The foreground modes without the TUI: `cuotai --serve` (PRD 0012 RF4),
//! `cuotai --watch` (PRD 0013 RF2) or both in one process. One core
//! [`Driver`] keeps the refresh cycle (a first full sweep, then each account
//! at its own deadline with the core's backoff); the local server and the
//! event stream read the same cycle, so watching and serving never add a
//! query.
//!
//! SIGINT and SIGTERM (Ctrl+C and Ctrl+Break on Windows) end the process.
//! On Unix, SIGHUP rereads the configuration (PRD 0031 RF12): the cycle,
//! the server's source and the event stream switch to the reread file, an
//! invalid one keeps the previous configuration. Windows has no SIGHUP and
//! no equivalent: restart the process to apply an edit there. Exit codes
//! (PRD 0031 RF10): `0` when stopped by a signal, whatever the accounts'
//! state; with `--strict-exit`, `2` when serving and no account was
//! healthy (`ready`) until shutdown, or when watching and some account's
//! latest reading had failed; local errors return `Err`, which `main`
//! turns into `1`.

use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, PoisonError, RwLock, RwLockReadGuard},
    time::Instant,
};

use crate::l10n::{tr, trf};
use chrono::Utc;
use cuotai_core::{
    config_file::{ConfigFile, ConfigIdentity},
    headless::{Driver, TICK},
    Config, Monitor,
};

use crate::{
    cost::LocalCost,
    serve::{Live, ServeArgs},
    watch::WatchArgs,
};

/// PRD 0031 RF12: what a `SIGHUP` reload needs to reread the file the way
/// the process first read it.
pub(crate) struct Reload {
    /// The accounts' TOML that was loaded.
    pub path: PathBuf,
    /// `--interval`, which holds for the whole run.
    pub interval: Option<u64>,
    /// `--hooks`: also reread `hooks.toml`.
    pub hooks: bool,
}

/// Run until a shutdown signal arrives (or, with `--watch`, until stdout
/// is closed). `monitor` was built for `config`; with `--serve`, the
/// server publishes `local_cost` from `local_cost` (PRD 0015 RF4.3).
/// `strict_exit` keeps the failing-account exit code `2` (PRD 0031 RF10.2).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(
    config: Config,
    identity: ConfigIdentity,
    monitor: Monitor,
    serve: Option<ServeArgs>,
    watch: Option<WatchArgs>,
    local_cost: Option<LocalCost>,
    strict_exit: bool,
    reload: Reload,
) -> Result<u8, Box<dyn Error>> {
    let token = serve
        .as_ref()
        .map(crate::serve::token)
        .transpose()?
        .flatten();
    // Registered before the port opens or a query starts, so a failure is
    // a local error and a signal sent right after the start is never lost.
    let mut signals = Signals::listen()?;
    let monitor = Arc::new(monitor);
    let mut driver = Driver::new(&config, Arc::clone(&monitor), Instant::now());
    let live = Arc::new(RwLock::new(Live {
        config,
        identity,
        monitor,
    }));
    let server = match &serve {
        Some(args) => Some(
            crate::serve::start(
                args,
                token,
                Arc::clone(&live),
                Arc::clone(driver.cycle()),
                local_cost,
            )
            .await?,
        ),
        None => None,
    };
    if let Some(server) = &server {
        if watch.is_some() {
            // stdout carries the events.
            eprintln!(
                "{}",
                trf(
                    "headless.server_at",
                    "cuotai: local server at {url}",
                    &[("url", &server.url())],
                )
            );
        } else {
            // The address alone on stdout, for a script that started it.
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{}", server.url())?;
            stdout.flush()?;
        }
    }
    let mut watch = watch.map(crate::watch::Watch::new);
    if let Some(watch) = &watch {
        eprintln!("{}", watch.started(read(&live).config.accounts.len()));
    }

    // PRD 0026 RF3.2: the event journal follows this cycle too.
    let mut journal = cuotai_core::watch::JournalTap::new();
    loop {
        // The first step starts the full sweep at once (the cycle's
        // deadlines are for the next readings).
        if driver.step(Instant::now()).await {
            {
                // The journal reads the live configuration (a SIGHUP reload
                // swaps it) and the pace input, like --watch.
                let live = read(&live);
                if live.monitor.history_recording() {
                    if let Some(store) = live.monitor.history() {
                        let now = Utc::now();
                        let pace = |index, report: &cuotai_core::Report| {
                            crate::ui::pace_input(&live.monitor, &live.config, index, report)
                        };
                        let export = driver.export(&live.config, &live.identity, now);
                        let _ = journal.observe_with(&store, &export, now, pace);
                    }
                }
            }
            if let Some(watch) = watch.as_mut() {
                let now = Utc::now();
                let live = read(&live);
                let pace = |index, report: &cuotai_core::Report| {
                    crate::ui::pace_input(&live.monitor, &live.config, index, report)
                };
                let export = driver.export(&live.config, &live.identity, now);
                if !watch.emit(&export, now, &pace)? {
                    break;
                }
            }
        }
        tokio::select! {
            signal = signals.wait() => match signal {
                Signal::Stop => break,
                Signal::Reload => {
                    if reload_config(&reload, &live, &mut driver, watch.as_mut(), &mut signals)
                        .await
                        == Signal::Stop
                    {
                        break;
                    }
                }
            },
            () = tokio::time::sleep(TICK) => {}
        }
    }
    if let Some(server) = server {
        server.shutdown().await;
    }
    let (ready_seen, failing) = (driver.ready_seen(), driver.failing());
    drop(driver);
    let monitor = Arc::clone(&read(&live).monitor);
    // Readings queued to the history land before the process ends.
    monitor.flush_history(crate::HISTORY_FLUSH);
    if let Some(warning) = monitor.history_status().and_then(|s| s.warning()) {
        eprintln!("cuotai: {}", warning.resolve(crate::ui::tui_locale()));
    }
    let mut code = 0;
    if serve.is_some() {
        eprintln!(
            "{}",
            tr("headless.server_stopped", "cuotai: local server stopped.")
        );
        if strict_exit && !ready_seen {
            code = 2;
        }
    }
    if watch.is_some() {
        if failing {
            eprintln!(
                "{}",
                tr(
                    "headless.watch_stopped_failing",
                    "cuotai: watching stopped with failing accounts."
                )
            );
            if strict_exit {
                code = 2;
            }
        } else {
            eprintln!(
                "{}",
                tr("headless.watch_stopped", "cuotai: watching stopped.")
            );
        }
    }
    Ok(code)
}

fn read(live: &RwLock<Live>) -> RwLockReadGuard<'_, Live> {
    live.read().unwrap_or_else(PoisonError::into_inner)
}

/// PRD 0031 RF12.1: reread the TOML through `ConfigFile` and apply it to
/// the running cycle, the server's source and the event stream; with
/// `--hooks`, reread `hooks.toml` too, whose rules stay suspended until
/// confirmed (on a terminal) or are refused without one. An invalid file
/// keeps the old configuration and logs why. [`Signal::Stop`] when a stop
/// arrived while the hooks' question was waiting.
async fn reload_config(
    reload: &Reload,
    live: &RwLock<Live>,
    driver: &mut Driver,
    watch: Option<&mut crate::watch::Watch>,
    signals: &mut Signals,
) -> Signal {
    let loaded = load(reload).and_then(|(config, identity)| {
        let monitor = read(live)
            .monitor
            .with_config(config.clone())
            .map_err(|error| error.to_string())?;
        Ok((config, identity, monitor))
    });
    let (config, identity, monitor) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!(
                "{}",
                trf(
                    "headless.reload_failed",
                    "cuotai: configuration not reloaded; the previous one stays in use: {error}",
                    &[("error", &crate::ui::safe(&error))],
                )
            );
            return Signal::Reload;
        }
    };
    {
        // The server reads `live`, then the cycle: replacing both under
        // this write lock keeps every answer consistent.
        let mut live = live.write().unwrap_or_else(PoisonError::into_inner);
        let monitor = Arc::new(monitor);
        driver.reload(&live.config, &config, Arc::clone(&monitor), Instant::now());
        *live = Live {
            config,
            identity,
            monitor,
        };
        eprintln!(
            "{}",
            trf(
                "headless.reloaded",
                "cuotai: configuration reloaded ({count} account(s)).",
                &[("count", &live.config.accounts.len())],
            )
        );
    }
    let Some(watch) = watch.filter(|_| reload.hooks) else {
        return Signal::Reload;
    };
    // Changed rules never run before they are confirmed again.
    watch.set_hooks(None);
    let path = reload.path.clone();
    let job = tokio::task::spawn_blocking(move || {
        crate::watch::confirmed_hooks(&path, crate::ui::tui_locale())
    });
    tokio::select! {
        result = job => match result {
            Ok(Ok(confirmed)) => {
                watch.set_hooks(Some(confirmed));
                eprintln!("{}", tr("headless.hooks_reloaded", "cuotai: hooks reloaded."));
            }
            Ok(Err(error)) => eprintln!(
                "{}",
                trf(
                    "headless.hooks_suspended_error",
                    "cuotai: hooks suspended: {error}",
                    &[("error", &error)],
                )
            ),
            Err(_) => eprintln!("{}", hooks_suspended()),
        },
        // A stop while the question waits ends the process as usual; the
        // blocked read of the answer ends with it.
        signal = signals.wait() => {
            if signal == Signal::Stop {
                return Signal::Stop;
            }
            eprintln!("{}", hooks_suspended());
        }
    }
    Signal::Reload
}

fn hooks_suspended() -> &'static str {
    tr("headless.hooks_suspended", "cuotai: hooks suspended.")
}

/// The file the process first read, reread with the run's `--interval`.
fn load(reload: &Reload) -> Result<(Config, ConfigIdentity), String> {
    let file = ConfigFile::load(Some(reload.path.clone()), false).map_err(|e| e.to_string())?;
    let identity = file.identity().map_err(|e| e.to_string())?;
    let config = match reload.interval {
        Some(seconds) => cuotai_core::watch::with_interval(&file.config, seconds)
            .map_err(|error| error.resolve(crate::ui::tui_locale()))?,
        None => file.config,
    };
    Ok((config, identity))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    /// End the process.
    Stop,
    /// PRD 0031 RF12: reread the configuration (never sent on Windows).
    #[cfg_attr(windows, allow(dead_code))]
    Reload,
}

/// SIGINT and SIGTERM stop; SIGHUP reloads.
#[cfg(unix)]
struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
    hangup: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    fn listen() -> io::Result<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
            hangup: signal(SignalKind::hangup())?,
        })
    }

    /// Cancel-safe: a signal that arrives between two calls is kept.
    async fn wait(&mut self) -> Signal {
        tokio::select! {
            _ = self.interrupt.recv() => Signal::Stop,
            _ = self.terminate.recv() => Signal::Stop,
            _ = self.hangup.recv() => Signal::Reload,
        }
    }
}

/// Ctrl+C and Ctrl+Break stop; Windows has no reload signal.
#[cfg(windows)]
struct Signals {
    ctrl_c: tokio::signal::windows::CtrlC,
    ctrl_break: tokio::signal::windows::CtrlBreak,
}

#[cfg(windows)]
impl Signals {
    fn listen() -> io::Result<Self> {
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c()?,
            ctrl_break: tokio::signal::windows::ctrl_break()?,
        })
    }

    /// Cancel-safe: a signal that arrives between two calls is kept.
    async fn wait(&mut self) -> Signal {
        tokio::select! {
            _ = self.ctrl_c.recv() => Signal::Stop,
            _ = self.ctrl_break.recv() => Signal::Stop,
        }
    }
}
