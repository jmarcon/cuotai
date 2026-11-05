//! `cuotai --serve` (PRD 0012 RF4): the core's local server in the
//! foreground, without the TUI and without needing a terminal. It serves
//! the `export::v1` built from the headless refresh cycle
//! ([`crate::headless`]); a request never starts a query.
//!
//! Exit codes (decided by [`crate::headless::run`]): `0` on a signal stop,
//! or `2` with `--strict-exit` when no account had a healthy (`ready`)
//! reading until shutdown; local errors (a taken port, configuration, token) return `Err`,
//! which `main` turns into `1`.

use std::{
    error::Error,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError, RwLock},
};

use chrono::Utc;
use cuotai_core::{
    config_file::ConfigIdentity,
    export::v1::{Export, Options, StatsExport},
    refresh::RefreshCycle,
    serve::{
        default_token_file, is_cuotai_server, resolve_token, AccessEntry, LocalServer, ServeConfig,
        ServeError, StatsAnswer, Token, TokenSource, DEFAULT_PORT,
    },
    stats::StatsRange,
    Account, Config, Monitor,
};

use crate::cost::LocalCost;

/// What `--serve` was given on the command line.
pub(crate) struct ServeArgs {
    /// `--port`: only this port (no fallback); without it [`DEFAULT_PORT`],
    /// also without fallback.
    pub port: Option<u16>,
    /// `--token-file`: the bearer token every request must carry.
    pub token_file: Option<PathBuf>,
    /// `--no-token`: serve without a token (INT-04 opt-out).
    pub no_token: bool,
    /// `--serve-stats`: also serve `/v1/stats` (PRD 0031 RF11; needs a token).
    pub stats: bool,
    /// PRD 0034 RF5.2: `--fallback-locale` / `CUOTAI_FALLBACK_LOCALE`.
    pub fallback_locale: Option<String>,
    /// PRD 0050 RF6.1: `--access-log off|errors|all` (errors by default).
    pub access_log: cuotai_core::serve::AccessLogMode,
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
        crate::cli::message(
            crate::cli::Level::Warn,
            "serve.token_notice",
            format_args!("cuotai: {}", notice.resolve(locale)),
        );
    }
    Ok(resolved.token)
}

/// What the server reads on every request: the configuration, its
/// identity and the monitor built for it. PRD 0031 RF12: a reload replaces
/// it, so the server answers for the reread file without restarting.
pub(crate) struct Live {
    pub config: Config,
    pub identity: ConfigIdentity,
    pub monitor: Arc<Monitor>,
}

pub(crate) type SharedLive = Arc<RwLock<Live>>;

fn read(live: &RwLock<Live>) -> std::sync::RwLockReadGuard<'_, Live> {
    live.read().unwrap_or_else(PoisonError::into_inner)
}

/// Open the port and serve the export of `cycle` (the one `live`'s
/// monitor queries feed), with `local_cost` from the cost index (PRD 0015
/// RF4.3), until the returned server is shut down.
pub(crate) async fn start(
    args: &ServeArgs,
    token: Option<Token>,
    live: SharedLive,
    cycle: Arc<Mutex<RefreshCycle>>,
    local_cost: Option<LocalCost>,
) -> Result<LocalServer, Box<dyn Error>> {
    let generation = {
        let (live, cycle) = (live.clone(), cycle.clone());
        move || {
            let incidents = read(&live).monitor.incident_statuses();
            let cycle = cycle.lock().unwrap_or_else(PoisonError::into_inner);
            cuotai_core::serve::generation_of(&cycle, &incidents)
        }
    };
    let stats_live = live.clone();
    let source = move || {
        let live = read(&live);
        let incidents = live.monitor.incident_statuses();
        let now = Utc::now();
        let local_cost = local_cost.as_ref().and_then(|cost| cost.export(now));
        let cycle = cycle.lock().unwrap_or_else(PoisonError::into_inner);
        // A reload between reading `live` and the cycle could leave them
        // apart for one request; never build an export from such a pair.
        if cycle.len() != live.config.accounts.len() {
            return None;
        }
        Some(
            Export::from_cycle_with_history(
                &live.config,
                &live.identity,
                &cycle,
                &incidents,
                now,
                Options::default(),
                |index, metrics, at| live.monitor.recent_points(index, metrics, at),
            )
            .with_local_cost(local_cost),
        )
    };
    let config = serve_config(args, token);
    let port = config.port;
    let source = cuotai_core::serve::with_generation(source, generation);
    let server = match LocalServer::start(
        config,
        cuotai_core::serve::with_stats(source, move |range| {
            let live = read(&stats_live);
            stats(&live.monitor, &live.config.accounts, range)
        }),
    )
    .await
    {
        Ok(server) => server,
        Err(error) => {
            let text = error.l10n().resolve(crate::ui::tui_locale());
            return Err(taken_port_message(&error, port, &text).await.into());
        }
    };
    crate::cli::message(
        crate::cli::Level::Info,
        "serve.listening",
        crate::l10n::tr(
            "serve.started",
            "cuotai: read-only local server; Ctrl+C stops it.",
        ),
    );
    Ok(server)
}

/// PRD 0031 RF11: the statistics `/v1/stats` answers, from the local history
/// only (never a provider): `history_off` while nothing is being recorded.
fn stats(monitor: &Monitor, accounts: &[Account], range: StatsRange) -> StatsAnswer {
    let Some(store) = monitor.history().filter(|_| monitor.history_recording()) else {
        return StatsAnswer::HistoryOff;
    };
    let now = Utc::now();
    match cuotai_core::stats::summarize(&store, accounts, range, now, &chrono::Local) {
        Ok(summary) => StatsAnswer::Ready(Box::new(
            // PRD 0043 RF5: the rhythm and the limit changes, additive.
            StatsExport::from_summary(&summary, accounts, now).with_insights(
                &store,
                accounts,
                now,
                &chrono::Local,
                cuotai_core::notify_prefs::Source::open()
                    .loaded()
                    .notify
                    .days_off,
            ),
        )),
        Err(_) => StatsAnswer::Failed,
    }
}

/// PRD 0031 RF9.1: a taken port is a local error that says whether the
/// holder is another Cuotai server (`serve::is_cuotai_server`) and
/// suggests `--port`; any other error keeps the core's text.
async fn taken_port_message(error: &ServeError, port: u16, text: &str) -> String {
    if !matches!(error, ServeError::PortInUse { .. }) {
        return text.to_owned();
    }
    let holder = if is_cuotai_server(port).await {
        crate::l10n::tr(
            "serve.port.cuotai_holder",
            " Another local Cuotai server uses it.",
        )
    } else {
        ""
    };
    let hint = crate::l10n::tr("serve.port.hint", " Use --port to choose another port.");
    format!("{text}{holder}{hint}")
}

/// Loopback on exactly the chosen port (PRD 0031 RF9.1): a taken port is an
/// error, never a silent move to another one.
fn serve_config(args: &ServeArgs, token: Option<Token>) -> ServeConfig {
    // PRD 0050 RF6.3: stderr; retention belongs to whoever runs the server.
    let access_log: cuotai_core::serve::AccessLog = Arc::new(|entry: &AccessEntry| {
        crate::cli::message_with(
            crate::cli::Level::Info,
            "serve.access",
            &[
                ("status", crate::cli::Field::Count(u64::from(entry.status))),
                (
                    "elapsed",
                    crate::cli::Field::Millis(
                        u64::try_from(entry.elapsed.as_millis()).unwrap_or(u64::MAX),
                    ),
                ),
            ],
            entry,
        );
    });
    ServeConfig {
        port: args.port.unwrap_or(DEFAULT_PORT),
        port_fallback: false,
        token,
        stats: args.stats,
        fallback_locale: args.fallback_locale.clone(),
        access_log: Some(access_log),
        access_log_mode: args.access_log,
        ..ServeConfig::default()
    }
}
