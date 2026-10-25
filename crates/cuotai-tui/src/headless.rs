//! The foreground modes without the TUI: `cuotai --serve` (PRD 0012 RF4),
//! `cuotai --watch` (PRD 0013 RF2) or both in one process. One core
//! [`Driver`] keeps the refresh cycle (a first full sweep, then each account
//! at its own deadline with the core's backoff); the local server and the
//! event stream read the same cycle, so watching and serving never add a
//! query.
//!
//! SIGINT and SIGTERM (Ctrl+C and Ctrl+Break on Windows) end the process,
//! and so does SIGHUP while watching. Exit codes (PRD 0031 RF10): `0` when
//! stopped by a signal, whatever the accounts' state; with `--strict-exit`,
//! `2` when serving and no account was healthy (`ready`) until shutdown, or
//! when watching and some account's latest reading had failed; local
//! errors return `Err`, which `main` turns into `1`.

use std::{
    error::Error,
    io::{self, Write},
    sync::Arc,
    time::Instant,
};

use chrono::Utc;
use cuotai_core::{
    config_file::ConfigIdentity,
    headless::{Driver, TICK},
    Config, Monitor,
};

use crate::{cost::LocalCost, serve::ServeArgs, watch::WatchArgs};

/// Run until a shutdown signal arrives (or, with `--watch`, until stdout
/// is closed). `monitor` was built for `config`; with `--serve`, the
/// server publishes `local_cost` from `local_cost` (PRD 0015 RF4.3).
/// `strict_exit` keeps the failing-account exit code `2` (PRD 0031 RF10.2).
pub(crate) async fn run(
    config: Config,
    identity: ConfigIdentity,
    monitor: Monitor,
    serve: Option<ServeArgs>,
    watch: Option<WatchArgs>,
    local_cost: Option<LocalCost>,
    strict_exit: bool,
) -> Result<u8, Box<dyn Error>> {
    let token = serve
        .as_ref()
        .map(crate::serve::token)
        .transpose()?
        .flatten();
    // Registered before the port opens or a query starts, so a failure is
    // a local error and a signal sent right after the start is never lost.
    let mut shutdown = Shutdown::listen(watch.is_some())?;
    let monitor = Arc::new(monitor);
    let mut driver = Driver::new(&config, Arc::clone(&monitor), Instant::now());
    let server = match &serve {
        Some(args) => Some(
            crate::serve::start(
                args,
                token,
                config.clone(),
                identity.clone(),
                Arc::clone(driver.cycle()),
                Arc::clone(&monitor),
                local_cost,
            )
            .await?,
        ),
        None => None,
    };
    if let Some(server) = &server {
        if watch.is_some() {
            // stdout carries the events.
            eprintln!("cuotai: servidor local em {}", server.url());
        } else {
            // The address alone on stdout, for a script that started it.
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{}", server.url())?;
            stdout.flush()?;
        }
    }
    let mut watch = watch.map(crate::watch::Watch::new);
    if let Some(watch) = &watch {
        eprintln!("{}", watch.started(config.accounts.len()));
    }

    loop {
        // The first step starts the full sweep at once (the cycle's
        // deadlines are for the next readings).
        if driver.step(Instant::now()).await {
            if let Some(watch) = watch.as_mut() {
                let now = Utc::now();
                if !watch.emit(&driver.export(&config, &identity, now), now)? {
                    break;
                }
            }
        }
        tokio::select! {
            () = shutdown.wait() => break,
            () = tokio::time::sleep(TICK) => {}
        }
    }
    if let Some(server) = server {
        server.shutdown().await;
    }
    let (ready_seen, failing) = (driver.ready_seen(), driver.failing());
    drop(driver);
    // Readings queued to the history land before the process ends.
    monitor.flush_history(crate::HISTORY_FLUSH);
    if let Some(warning) = monitor.history_status().and_then(|s| s.warning()) {
        eprintln!("cuotai: {}", warning.resolve(crate::ui::tui_locale()));
    }
    let mut code = 0;
    if serve.is_some() {
        eprintln!("cuotai: servidor local encerrado.");
        if strict_exit && !ready_seen {
            code = 2;
        }
    }
    if watch.is_some() {
        if failing {
            eprintln!("cuotai: acompanhamento encerrado com contas em falha.");
            if strict_exit {
                code = 2;
            }
        } else {
            eprintln!("cuotai: acompanhamento encerrado.");
        }
    }
    Ok(code)
}

/// SIGINT, SIGTERM and, while watching, SIGHUP.
#[cfg(unix)]
struct Shutdown {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
    hangup: Option<tokio::signal::unix::Signal>,
}

#[cfg(unix)]
impl Shutdown {
    fn listen(hangup: bool) -> io::Result<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
            hangup: hangup.then(|| signal(SignalKind::hangup())).transpose()?,
        })
    }

    /// Cancel-safe: a signal that arrives between two calls is kept.
    async fn wait(&mut self) {
        let hangup = async {
            match self.hangup.as_mut() {
                Some(hangup) => {
                    hangup.recv().await;
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
            () = hangup => {}
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
    fn listen(_hangup: bool) -> io::Result<Self> {
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
