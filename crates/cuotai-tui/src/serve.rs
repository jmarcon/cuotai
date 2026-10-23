//! `cuotai --serve` (PRD 0012 RF4): the core's local server in the
//! foreground, without the TUI and without needing a terminal. It serves
//! the `export::v1` built from the headless refresh cycle
//! ([`crate::headless`]); a request never starts a query.
//!
//! Exit codes (decided by [`crate::headless::run`]): `0` when some account
//! had a healthy (`ready`) reading during the run, `2` when none had one
//! until shutdown; local errors (port, configuration, token) return `Err`,
//! which `main` turns into `1`.

use std::{
    error::Error,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

use chrono::Utc;
use cuotai_core::{
    config_file::ConfigIdentity,
    export::v1::{Export, Options},
    refresh::RefreshCycle,
    serve::{
        default_token_file, resolve_token, AccessEntry, LocalServer, ServeConfig, Token,
        TokenSource, DEFAULT_PORT,
    },
    Config, Monitor,
};

use crate::cost::LocalCost;

/// What `--serve` was given on the command line.
pub(crate) struct ServeArgs {
    /// `--port`: only this port (no fallback); without it [`DEFAULT_PORT`]
    /// and, when taken, the next nine.
    pub port: Option<u16>,
    /// `--token-file`: the bearer token every request must carry.
    pub token_file: Option<PathBuf>,
    /// `--no-token`: serve without a token (INT-04 opt-out).
    pub no_token: bool,
}

/// INT-04: the bearer token every request must carry — `--token-file`'s,
/// or by default the one the core keeps in the data directory (created on
/// first use) — or none with `--no-token`. The core's notices (where the
/// default token lives, a token file others can read, what serving
/// without a token means) go to stderr; the token never does.
pub(crate) fn token(args: &ServeArgs) -> Result<Option<Token>, String> {
    let source = match (&args.token_file, args.no_token) {
        (Some(path), _) => TokenSource::File(path.clone()),
        (None, true) => TokenSource::None,
        (None, false) => TokenSource::Default,
    };
    let locale = crate::ui::tui_locale();
    let resolved = resolve_token(&source, default_token_file().as_deref())
        .map_err(|error| error.l10n().resolve(locale))?;
    for notice in &resolved.notices {
        eprintln!("cuotai: {}", notice.resolve(locale));
    }
    Ok(resolved.token)
}

/// Open the port and serve the export of `cycle` (the one `monitor`'s
/// queries feed), with `local_cost` from the cost index (PRD 0015 RF4.3),
/// until the returned server is shut down.
pub(crate) async fn start(
    args: &ServeArgs,
    token: Option<Token>,
    config: Config,
    identity: ConfigIdentity,
    cycle: Arc<Mutex<RefreshCycle>>,
    monitor: Arc<Monitor>,
    local_cost: Option<LocalCost>,
) -> Result<LocalServer, Box<dyn Error>> {
    let source = move || {
        let incidents = monitor.incident_statuses();
        let now = Utc::now();
        let local_cost = local_cost.as_ref().and_then(|cost| cost.export(now));
        let cycle = cycle.lock().unwrap_or_else(PoisonError::into_inner);
        Some(
            Export::from_cycle(
                &config,
                &identity,
                &cycle,
                &incidents,
                now,
                Options::default(),
            )
            .with_local_cost(local_cost),
        )
    };
    let server = LocalServer::start(serve_config(args, token), source)
        .await
        .map_err(|error| error.l10n().resolve(crate::ui::tui_locale()))?;
    if server.fell_back() {
        eprintln!(
            "cuotai: a porta {DEFAULT_PORT} está em uso; servindo na porta {}.",
            server.addr().port()
        );
    }
    eprintln!("cuotai: servidor local somente leitura; Ctrl+C encerra.");
    Ok(server)
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
