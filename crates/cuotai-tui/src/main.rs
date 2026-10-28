mod about_view;
mod auth_ui;
mod canary;
#[cfg(test)]
mod capture;
mod changelog_view;
mod cost;
mod dashboard;
mod extension_check;
mod extensions_view;
mod glyphs;
mod headless;
mod l10n;
mod models_view;
mod motion;
mod notify;
mod palette;
mod report_problem;
mod report_view;
mod serve;
mod stats_view;
mod status_line;
mod ui;
mod update_notice;
mod vault;
mod watch;
mod wizard;

use chrono::{DateTime, TimeDelta, Utc};
use clap::Parser;
use cuotai_core::{
    config_file::{ConfigFile, ConfigIdentity},
    export::v1::{Export, Options},
    history::{HistoryStore, MetricHistory, Retention},
    incidents::ProviderStatus,
    install_source::detect_install_source,
    metrics,
    oauth::OAuthSession,
    pace::{self, Point},
    refresh::RefreshCycle,
    Account, Config, EnvironmentCredentials, Monitor, Provider, Report, Snapshot, Status,
};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// How long `--once`/`--json` wait for the readings to reach the history.
const HISTORY_FLUSH: Duration = Duration::from_secs(5);
/// PRD 0006 RF6.2: days `--history` prints without `--history-days`.
const HISTORY_DEFAULT_DAYS: u32 = 7;
/// Sparkline cells of each `--history` metric.
const HISTORY_SPARK_CELLS: usize = 60;

/// PRD 0039 RF10: `--version` says `0.80.0 (dev abc1234)` for a build that
/// is not exactly a release tag, and the plain version for a release.
fn build_version() -> &'static str {
    static SHOWN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SHOWN.get_or_init(cuotai_core::build_info::display_version)
}

#[derive(Parser)]
#[command(
    version = build_version(),
    about = "AI quota and usage: APIs and subscriptions, without generating tokens.",
    // PRD 0015: one local cost action at a time, never with a query mode.
    group(
        clap::ArgGroup::new("cost_action")
            .multiple(false)
            .conflicts_with_all(["once", "json_v1", "history", "notify", "serve", "watch"])
    ),
    // PRD 0026 RF6.2: one stored view at a time, never with a query mode.
    group(
        clap::ArgGroup::new("stored_view")
            .args(["stats", "timeline", "events_log"])
            .multiple(false)
            .conflicts_with_all([
                "once", "json_v1", "history", "notify", "serve", "watch", "changelog",
                "extension_check", "report_problem", "install_source", "list_providers",
                "cost_action",
            ])
    ),
    // PRD 0031 RF10.2: `--strict-exit` belongs to the foreground modes.
    group(
        clap::ArgGroup::new("headless")
            .args(["serve", "watch"])
            .multiple(true)
    )
)]
struct Args {
    /// TOML file (created if missing). Without it: CUOTAI_CONFIG, an existing ./cuotai.toml,
    /// or <OS config dir>/cuotai/config.toml (where the wizard creates a new one).
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Queries once, without opening the TUI.
    #[arg(long)]
    once: bool,
    /// PRD 0031 RF2: queries once and prints one line for status bars (tmux,
    /// Waybar, SketchyBar) with the same summary as the menu bar. Exit 0 when
    /// every account is ready, 2 when one failed, 1 on a local error.
    #[arg(
        long,
        conflicts_with_all = [
            "list_providers", "changelog", "extension_check", "once", "json",
            "json_v1", "history", "notify", "serve", "watch", "install_source",
            "cost_action", "report_problem", "canary",
        ]
    )]
    status_line: bool,
    /// With --status-line, the line's style.
    #[arg(
        long,
        value_name = "STYLE",
        value_parser = status_line::STYLES,
        default_value = "tightest_percent",
        requires = "status_line"
    )]
    style: String,
    /// With --status-line, only this account (id); repeat it to pick several,
    /// in the given order. Without it: all of them, in the TOML's order.
    #[arg(long = "account", value_name = "ID", requires = "status_line")]
    status_accounts: Vec<String>,
    /// With --status-line, colors the line (ANSI) when the output is a
    /// terminal and NO_COLOR is not set.
    #[arg(long, requires = "status_line")]
    color: bool,
    /// With --status-line, reads /v1/limits from a running cuotai --serve
    /// (default http://127.0.0.1 on the default port) instead of querying the
    /// providers; loopback addresses only. The token comes from --token-file
    /// or from --serve's default file.
    #[arg(
        long,
        value_name = "URL",
        num_args = 0..=1,
        default_missing_value = "",
        requires = "status_line"
    )]
    from_server: Option<String>,
    /// Queries once and prints the Snapshot as JSON (no credentials); with
    /// --history, prints the history as JSON.
    #[arg(long)]
    json: bool,
    /// Shows the local measurement history of ACCOUNT (or of all of them) and
    /// exits, without querying the providers.
    #[arg(long, value_name = "ACCOUNT", num_args = 0..=1, conflicts_with = "once")]
    history: Option<Option<String>>,
    /// Lists the identifiers the configuration accepts.
    #[arg(long)]
    list_providers: bool,
    /// Usage statistics from the local history (limits reached, average peak
    /// of the tightest limit, extra spending, peak per day and per account),
    /// without querying the providers. With --days 7|30 (default 7) and
    /// --json. Exit 0; 2 when the history could not be read; 1 on a local
    /// error.
    #[arg(long)]
    stats: bool,
    /// When each account's tightest limit comes back, from the last recorded
    /// readings, without querying the providers. With --range 24h|7d
    /// (default 24h) and --json. Same exit codes as --stats.
    #[arg(long)]
    timeline: bool,
    /// The event journal recorded in the history (threshold crossed, quota
    /// exhausted, renewed, query failure...), newest first. With --days N
    /// (1 to 365; default 7) and --json. Same exit codes as --stats.
    #[arg(long)]
    events_log: bool,
    /// The --timeline range: 24h or 7d.
    #[arg(long, value_name = "RANGE", value_parser = ["24h", "7d"], requires = "timeline")]
    range: Option<String>,
    /// Queries ACCOUNT once and prints the problem report (English text, no
    /// secrets, and the prefilled GitHub URL). Nothing is sent: you open the
    /// URL. Exit 0; 1 when the account does not exist or on a local error.
    #[arg(
        long,
        value_name = "ACCOUNT",
        conflicts_with_all = [
            "list_providers", "changelog", "extension_check", "once", "json",
            "json_v1", "history", "notify", "serve", "watch", "install_source",
            "cost_action",
        ]
    )]
    report_problem: Option<String>,
    /// PRD 0028 RF4: the local contract canary. Queries the configured
    /// accounts once (those billed per query are left out), writes a record
    /// without values to `<data>/cuotai/canary/`, compares it with the
    /// previous run and prints the Markdown report and one issue draft per
    /// drift. Nothing is opened or sent on its own. Exit 0 without drift, 2
    /// with drift, 1 on a local error.
    #[arg(
        long,
        conflicts_with_all = [
            "list_providers", "changelog", "extension_check", "once", "json",
            "json_v1", "history", "notify", "serve", "watch", "install_source",
            "cost_action", "report_problem",
        ]
    )]
    canary: bool,
    /// With --canary, the records' directory (default: `<data>/cuotai/canary`).
    #[arg(long, value_name = "DIR", requires = "canary")]
    out: Option<std::path::PathBuf>,
    /// With --canary, computes and prints without writing anything.
    #[arg(long, requires = "canary")]
    dry_run: bool,
    /// With --canary, opens each issue draft's link in the browser.
    #[arg(long, requires = "canary")]
    canary_open: bool,
    /// With --canary, also includes the accounts billed per query.
    #[arg(long, requires = "canary")]
    canary_include_metered: bool,
    /// PRD 0031 RF3: prints the completion script of the given shell
    /// (generated from the command-line definition) and exits with 0. It is
    /// local: it reads no configuration and queries no network.
    #[arg(
        long,
        value_name = "SHELL",
        conflicts_with_all = [
            "list_providers",
            "install_source",
            "once",
            "json",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "cost_action",
        ]
    )]
    completions: Option<clap_complete::Shell>,
    /// Says where this copy was installed from, why the built-in updater
    /// must stay off and the update command, when known. It is local: it
    /// reads no configuration and queries no network. Exit 0; 1 on a local
    /// error.
    #[arg(
        long,
        conflicts_with_all = [
            "list_providers",
            "changelog",
            "extension_check",
            "once",
            "json",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "cost_action",
        ]
    )]
    install_source: bool,
    /// Shows the version history of the embedded CHANGELOG (no network and
    /// no provider queries): all of it, or only the section of VERSION (a
    /// version such as 0.68.2, or a series such as 0.68.x). An internal
    /// version that was never published (0.1.0 to 0.79.0) shows the
    /// pre-release history that summarizes it, with a notice. The category
    /// labels follow the interface language; the entries stay in English,
    /// the changelog's language, as the command itself says. Does not
    /// combine with the query modes, --list-providers or the cost options.
    /// Exit 0; 1 on a local error (unknown version).
    #[arg(
        long,
        value_name = "VERSION",
        num_args = 0..=1,
        conflicts_with_all = [
            "list_providers",
            "once",
            "json",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "extension_check",
            "cost_action",
        ]
    )]
    changelog: Option<Option<String>>,
    /// Runs the extension ID once and shows the normalized report or the
    /// contract violations (PRD 0017 RF2.4), never the raw output. An
    /// extension starts disabled: the first time, and after any change to
    /// the manifest or the program, the exact command, the folder, the
    /// requested environment variables and the interval are listed and ask
    /// for confirmation in the terminal (without a terminal: error). With
    /// --json, as JSON. Exit 0 when the reading is ready; 2 when the run
    /// failed or the output broke the contract; 1 on a local error (unknown
    /// id, approval declined). Does not combine with --once, --json-v1,
    /// --history, --notify, --serve, --watch, --list-providers or the cost
    /// options.
    #[arg(
        long,
        value_name = "ID",
        conflicts_with_all = [
            "list_providers",
            "once",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "cost_action",
        ]
    )]
    extension_check: Option<String>,
    /// DEVELOPMENT ONLY: unencrypted tokens in SQLite (release builds refuse it).
    #[arg(long, value_name = "FILE")]
    dev_credentials_sqlite: Option<PathBuf>,
    /// Lists the credentials Cuotai stored in the vault (OAuth and web
    /// sessions), the account that uses each one and the orphans no account
    /// uses; never shows a value. With --json, as JSON. Exit 0; 2 with
    /// orphans.
    #[arg(long, conflicts_with_all = VAULT_CONFLICTS)]
    vault_audit: bool,
    /// Deletes the orphan credentials of --vault-audit from the vault, after
    /// a confirmation in the terminal (without a terminal: error).
    #[arg(long, conflicts_with_all = VAULT_CONFLICTS, conflicts_with = "vault_audit")]
    vault_prune: bool,
    /// Turns the TUI's animations off (also: CUOTAI_NO_ANIMATIONS=1).
    #[arg(long)]
    no_animations: bool,
    /// Days of local measurement history kept (7 to 365; default 90); with
    /// --history, the days shown (default 7).
    #[arg(long, value_name = "DAYS")]
    history_days: Option<u32>,
    /// With the TUI open, notifies when a used quota renews and when a quota
    /// with a limit reaches 75% or 90% of use, runs out or has room again: a
    /// header banner, the terminal bell (BEL) and an OSC 9 notification in
    /// iTerm2, WezTerm and Ghostty.
    #[arg(long, conflicts_with_all = ["once", "json", "history"])]
    notify: bool,
    /// With --notify, also notifies when, at the recent pace, a quota will
    /// run out before it renews: 15, 30 or 60 minutes before (60 without a
    /// value). A linear estimate from the recent readings; off without this
    /// option. This session only.
    #[arg(
        long,
        value_name = "MINUTES",
        num_args = 0..=1,
        default_missing_value = "60",
        value_parser = ["15", "30", "60"],
        requires = "notify"
    )]
    notify_projected: Option<String>,
    /// How to show the pace deviation: `points` (percentage points, the
    /// default) or `percent` (relative to the expected use: "20% faster than
    /// pace"). Also: CUOTAI_PACE_UNIT.
    #[arg(long, value_name = "UNIT", value_parser = ["points", "percent"])]
    pace_unit: Option<String>,
    /// Queries the public status pages of the configured providers (no
    /// credentials, cookies or account data; each page at most every 5 min)
    /// and shows incidents on the cards, in the details and in --once. Off
    /// by default; also: CUOTAI_STATUS=1.
    #[arg(long)]
    status: bool,
    /// Does not query the status pages, even with --status or CUOTAI_STATUS.
    #[arg(long)]
    no_status: bool,
    /// Queries once and prints the v1 export as JSON: the versioned contract
    /// (docs/export-v1.md) with class, age, preserved reading, limit, pace
    /// and access per account, without credentials.
    #[arg(long, conflicts_with_all = ["once", "json", "history", "notify"])]
    json_v1: bool,
    /// Read-only local HTTP server on 127.0.0.1, without opening the TUI and
    /// without needing a terminal: keeps the refresh cycle and serves the v1
    /// export (/health, /v1/snapshot, /v1/accounts/{key}, /v1/limits)
    /// without querying the providers per request. SIGINT/SIGTERM stop it,
    /// with exit 0 (with --strict-exit: 2 when no account was healthy until
    /// the shutdown); 1 on a local error (port taken, configuration, token).
    #[arg(long, conflicts_with_all = ["once", "json", "json_v1", "history", "notify"])]
    serve: bool,
    /// The --serve port, and only that one (taken: error, exit 1). Without
    /// --port: 6738, also without falling back to another port.
    #[arg(long, value_name = "PORT", requires = "serve")]
    port: Option<u16>,
    /// File with the token --serve requires in Authorization: Bearer (16 to
    /// 512 characters, no spaces). Without it, --serve creates (the first
    /// time) and requires the token kept in the data directory, readable
    /// only by your user, and says where it is. With --status-line
    /// --from-server, the token sent to the server (without it: the data
    /// directory's one, if any).
    #[arg(long, value_name = "FILE", conflicts_with = "no_token")]
    token_file: Option<PathBuf>,
    /// Serves without a token: any user or program on this computer can read
    /// your usage (web pages and other computers stay refused).
    #[arg(long, requires = "serve")]
    no_token: bool,
    /// With --serve, also serves GET /v1/stats?range=7d|30d: usage
    /// statistics from the local history (peaks, times a limit was reached,
    /// no raw samples, e-mails or URLs). Off by default; requires the token
    /// (does not combine with --no-token), loopback only, 6 requests per
    /// minute per origin, and answers 503 history_off without a recorded
    /// history.
    #[arg(long, requires = "serve", conflicts_with = "no_token")]
    serve_stats: bool,
    /// Watches the accounts without opening the TUI and without needing a
    /// terminal: keeps the refresh cycle and emits one event per real
    /// transition (a quota crossed a threshold, ran out, renewed, an account
    /// blocked, a query failed or recovered, authentication needed, provider
    /// status, usage updated at most every 10 min). The first reading of each
    /// account is only the baseline. Combines with --serve in the same cycle.
    /// SIGINT/SIGTERM/SIGHUP stop it, with exit 0 (with --strict-exit: 2 when
    /// an account ended failing); 1 on a local error.
    #[arg(long, conflicts_with_all = ["once", "json", "json_v1", "history", "notify"])]
    watch: bool,
    /// The --watch interval in seconds (60 to 86400; lower is an error,
    /// never adjusted): replaces the global refresh_seconds in this run;
    /// accounts with their own refresh_seconds keep it and AWS Bedrock never
    /// goes below 300.
    #[arg(long, value_name = "SECONDS", requires = "watch")]
    interval: Option<u64>,
    /// Only these events in --watch, comma-separated (default: all), e.g.
    /// quota_threshold_crossed,refresh_failed.
    #[arg(long, value_name = "LIST", value_delimiter = ',', requires = "watch")]
    events: Option<Vec<String>>,
    /// With --serve or --watch, returns exit 2 on a signal shutdown when an
    /// account ended failing (--watch) or none was healthy (--serve), for
    /// supervisors that want it; without the option, a SIGINT/SIGTERM
    /// shutdown exits with 0.
    #[arg(long, requires = "headless")]
    strict_exit: bool,
    /// How many minutes ahead the --watch event quota_exhaustion_projected
    /// fires: 15, 30 or 60 (default 60).
    #[arg(
        long,
        value_name = "MINUTES",
        value_parser = ["15", "30", "60"],
        requires = "watch"
    )]
    projected_lead: Option<String>,
    /// With --watch, one JSON event per line (NDJSON) on stdout; messages
    /// and diagnostics go to stderr.
    #[arg(long, requires = "watch")]
    jsonl: bool,
    /// With --watch, runs the rules of hooks.toml (in the TOML file's
    /// directory) on every event: the command is a list of arguments, no
    /// shell; the event arrives as JSON on stdin and in CUOTAI_EVENT,
    /// CUOTAI_ACCOUNT, CUOTAI_PROVIDER and CUOTAI_METRIC; the environment is
    /// reduced to PATH, HOME and LANG. The first time, and after any change
    /// to the rules, lists the commands and asks for confirmation in the
    /// terminal (without a terminal: error). Without --jsonl, stdout stays
    /// empty.
    #[arg(long, requires = "watch")]
    hooks: bool,
    /// With --watch, how many minutes (MIN) before a quota window renews the
    /// quota_reset_soon event fires (default 15; 0 turns the event off).
    #[arg(long, value_name = "MIN", requires = "watch", default_value_t = 15)]
    reset_lead: u32,
    /// Renders the `fallback` texts (and the plain `notes`) of --json,
    /// --json-v1, --serve and --watch --jsonl in this language (pt-BR, es or
    /// it) for consumers that show them verbatim; also
    /// CUOTAI_FALLBACK_LOCALE. Without it they are English.
    #[arg(long, value_name = "LOCALE")]
    fallback_locale: Option<String>,
    /// Lists the local cost sources (Claude Code, Codex CLI): whether they
    /// are on, the exact directories they read (or would read) and what the
    /// index already follows; reads no logs. With --json, as JSON. All off
    /// by default.
    #[arg(long, group = "cost_action")]
    cost_sources: bool,
    /// Turns on reading the local logs of SOURCE (claude_code or codex_cli):
    /// shows exactly which directories will be read, what for and that only
    /// numbers (time, model, token counts, an anonymous session summary) are
    /// extracted, and asks for confirmation in the terminal. Never reads the
    /// conversations' content.
    #[arg(long, value_name = "SOURCE", group = "cost_action")]
    cost_enable: Option<String>,
    /// With --cost-enable, reads DIR instead of the default directories
    /// (repeatable); without it, back to the defaults.
    #[arg(long, value_name = "DIR", requires = "cost_enable")]
    cost_path: Vec<PathBuf>,
    /// Turns SOURCE off: none of its logs is read and everything derived
    /// from it is deleted from the index.
    #[arg(long, value_name = "SOURCE", group = "cost_action")]
    cost_disable: Option<String>,
    /// Deletes the whole local cost index (the sources stay on or off as
    /// they are).
    #[arg(long, group = "cost_action")]
    cost_erase: bool,
    /// Reads now, in the foreground, what grew in the logs of the sources
    /// that are on and says what it found; with --json, as JSON. Exit 0; 2
    /// when no source is on or one has no logs or no access; 1 on a local
    /// error.
    #[arg(long, group = "cost_action")]
    cost_scan: bool,
    /// Shows the prices (USD per million tokens) the local cost estimate
    /// uses: the built-in table, with date and source, under your
    /// pricing.json (reviewed by hand, never downloaded); with --json, as
    /// JSON. Exit 0; 1 when pricing.json is invalid (it is ignored). Reads
    /// no logs.
    #[arg(long, group = "cost_action")]
    cost_prices: bool,
    /// With --cost-prices, prints a pricing.json fragment with these MODELs
    /// and null prices to fill in (the date is today); never suggests a
    /// price.
    #[arg(long, value_name = "MODEL", num_args = 1.., requires = "cost_prices")]
    template: Vec<String>,
    /// Stores the plan price of an account (typed by you, on this computer
    /// only): ACCOUNT is provider:name (e.g. claude:personal), PRICE the
    /// amount, CURRENCY the three-letter code and DAY (1 to 28, default 1)
    /// the start of the cycle. Compares the estimate at API prices with the
    /// plan.
    #[arg(
        long,
        value_names = ["ACCOUNT", "PRICE", "CURRENCY", "DAY"],
        num_args = 3..=4,
        group = "cost_action"
    )]
    cost_plan: Option<Vec<String>>,
    /// Removes the plan price of ACCOUNT (provider:name).
    #[arg(long, value_name = "ACCOUNT", group = "cost_action")]
    cost_plan_clear: Option<String>,
    /// A second currency for display only: `--cost-currency BRL 5.42` (1 USD
    /// = 5.42 BRL, a rate typed by you, never downloaded) or
    /// `--cost-currency off`. The totals stay in USD.
    #[arg(
        long,
        value_names = ["CURRENCY", "RATE"],
        num_args = 1..=2,
        group = "cost_action"
    )]
    cost_currency: Option<Vec<String>>,
    /// Shows the cost estimated at API prices of the local usage of the
    /// sources that are on: today, yesterday and the saved period (or the
    /// last --days N days), per model and per day, after reading in the
    /// foreground what grew in the logs; with --json, as JSON. It is an
    /// estimate, not a charge. Exit 0; 2 when no source is on or one has no
    /// logs or no access; 1 on a local error.
    #[arg(long, group = "cost_action")]
    cost: bool,
    /// With --cost, the last DAYS local days, today included (1 to 730),
    /// instead of the saved period. With --stats, 7 or 30; with
    /// --events-log, 1 to 365.
    #[arg(long, value_name = "DAYS")]
    days: Option<u32>,
    /// The local cost period shown: today, last_7_days (default),
    /// last_30_days, current_month or all.
    #[arg(long, value_name = "PERIOD", conflicts_with_all = COST_OPTION_CONFLICTS)]
    cost_period: Option<String>,
    /// Days kept in the local cost index (7 to 730; default 120).
    #[arg(long, value_name = "DAYS", conflicts_with_all = COST_OPTION_CONFLICTS)]
    cost_retention_days: Option<u32>,
}

/// PRD 0029 RF6.2: the vault audit runs alone.
const VAULT_CONFLICTS: [&str; 7] = [
    "cost_action",
    "once",
    "json_v1",
    "history",
    "notify",
    "serve",
    "watch",
];

/// `--cost-period`/`--cost-retention-days` only save preferences.
const COST_OPTION_CONFLICTS: [&str; 8] = [
    "cost_action",
    "once",
    "json",
    "json_v1",
    "history",
    "notify",
    "serve",
    "watch",
];

/// PRD 0015: the local cost action the flags ask for, if any.
fn cost_action(args: &mut Args) -> Option<cost::CostAction> {
    use cost::CostAction;
    if args.cost_sources {
        return Some(CostAction::Sources);
    }
    if let Some(source) = args.cost_enable.take() {
        let paths = std::mem::take(&mut args.cost_path);
        return Some(CostAction::Enable { source, paths });
    }
    if let Some(source) = args.cost_disable.take() {
        return Some(CostAction::Disable(source));
    }
    if args.cost_erase {
        return Some(CostAction::Erase);
    }
    if args.cost_scan {
        return Some(CostAction::Scan);
    }
    if let Some(values) = args.cost_plan.take() {
        let mut values = values.into_iter();
        return Some(CostAction::Plan {
            account: values.next().unwrap_or_default(),
            price: values.next().unwrap_or_default(),
            currency: values.next().unwrap_or_default(),
            cycle_day: values.next(),
        });
    }
    if let Some(account) = args.cost_plan_clear.take() {
        return Some(CostAction::PlanClear(account));
    }
    if let Some(values) = args.cost_currency.take() {
        let mut values = values.into_iter();
        return Some(CostAction::Currency {
            code: values.next().unwrap_or_default(),
            rate: values.next(),
        });
    }
    if args.cost_prices {
        return Some(if args.template.is_empty() {
            CostAction::Prices
        } else {
            CostAction::PricesTemplate(std::mem::take(&mut args.template))
        });
    }
    if args.cost {
        return Some(CostAction::Usage { days: args.days });
    }
    (args.cost_period.is_some() || args.cost_retention_days.is_some()).then(|| {
        CostAction::Options {
            period: args.cost_period.take(),
            retention_days: args.cost_retention_days,
        }
    })
}

/// PRD 0021 RF8.1: report package ownership without loading configuration or
/// contacting a provider/package manager.
fn print_install_source() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let source = detect_install_source(
        &executable,
        |key| std::env::var_os(key),
        || executable_dir_writable(&executable),
    );
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "source={}", source.as_str())?;
    writeln!(
        stdout,
        "updater_disabled_reason={}",
        source.updater_disabled_reason().unwrap_or("none")
    )?;
    writeln!(
        stdout,
        "upgrade_command={}",
        source.upgrade_command().unwrap_or("none")
    )
}

fn executable_dir_writable(executable: &Path) -> bool {
    let Some(directory) = executable.parent() else {
        return false;
    };
    let probe = directory.join(format!(".cuotai-write-probe-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

/// PRD 0011 RF5: public status queries are opt-in (`--status`, or
/// `CUOTAI_STATUS` with any non-empty value except `0`); `--no-status`
/// always turns them off.
fn status_enabled(status: bool, no_status: bool, var: impl Fn(&str) -> Option<String>) -> bool {
    let set = var("CUOTAI_STATUS").is_some_and(|v| !v.is_empty() && v != "0");
    !no_status && (status || set)
}

/// PRD 0024 RF1.3: the unit of the pace deviation: `--pace-unit`, else
/// `CUOTAI_PACE_UNIT`, else points. An unknown environment value reads as
/// the default (the flag is validated by the parser).
fn pace_unit(
    arg: Option<&str>,
    var: impl Fn(&str) -> Option<String>,
) -> cuotai_core::pace::PaceUnit {
    arg.map(str::to_owned)
        .or_else(|| var("CUOTAI_PACE_UNIT"))
        .and_then(|value| cuotai_core::pace::PaceUnit::parse(value.trim()))
        .unwrap_or_default()
}

/// PRD 0006 RF4.1: the retention of `--history-days`, or the default.
fn retention(days: Option<u32>) -> Result<Retention, String> {
    days.map_or(Ok(Retention::default()), |days| {
        Retention::days(days).map_err(|error| error.l10n().resolve(ui::tui_locale()))
    })
}

/// PRD 0006: the history of `file` in the OS data dir, pruned by
/// `retention`; `None` (nothing recorded) without a resolvable data dir.
pub(crate) fn history_store(file: &ConfigFile, retention: Retention) -> Option<HistoryStore> {
    HistoryStore::in_data_dir(file)
        .ok()
        .map(|store| store.with_retention(retention))
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("cuotai: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<u8, Box<dyn std::error::Error>> {
    if let Some(warning) = ui::unknown_lang_warning() {
        eprintln!("cuotai: {warning}");
    }
    let mut args = match Args::try_parse() {
        Ok(args) => args,
        // `--help`/`--version` succeed; an invalid argument is a local error
        // (1), never the 2 that `--once`/`--json`/`--serve` keep for accounts.
        Err(error) => {
            error.print()?;
            return Ok(u8::from(error.use_stderr()));
        }
    };
    if args.install_source {
        print_install_source()?;
        return Ok(0);
    }
    if let Some(shell) = args.completions {
        let mut command = <Args as clap::CommandFactory>::command();
        clap_complete::generate(shell, &mut command, "cuotai", &mut std::io::stdout());
        return Ok(0);
    }
    if args.list_providers {
        for provider in Provider::ALL {
            println!(
                "{:<20} {}{}",
                serde_json::to_value(provider)?.as_str().unwrap_or_default(),
                ui::provider_label(provider),
                // PRD 0036 RF9.2: an aggregator relays several upstreams.
                if provider.is_aggregator() {
                    " (aggregator)"
                } else {
                    ""
                }
            );
        }
        return Ok(0);
    }
    // PRD 0019 RF7.7: the embedded changelog, without loading the
    // accounts' configuration and without touching the network.
    if let Some(version) = args.changelog.take() {
        return changelog_view::print(version.as_deref(), ui::tui_locale());
    }
    // PRD 0017 RF2.4: one run of an approved extension, without loading
    // the accounts' configuration.
    if let Some(id) = args.extension_check.take() {
        return extension_check::run(&id, args.json, args.dev_credentials_sqlite.as_deref()).await;
    }
    if let Some(action) = cost_action(&mut args) {
        // PRD 0029 RF2: plans are keyed by the account id when a
        // configuration can be read; the cost commands work without one.
        let configured = ConfigFile::load(args.config.clone(), false)
            .ok()
            .map(|file| file.config.accounts);
        return Ok(cost::run(action, args.json, ui::tui_locale(), configured)?);
    }
    if args.days.is_some() && !(args.stats || args.events_log) {
        return Err(l10n::tr(
            "cli.days_needs_mode",
            "--days only works with --cost, --stats or --events-log.",
        )
        .into());
    }
    if let Some(account) = &args.history {
        let days = retention(Some(args.history_days.unwrap_or(HISTORY_DEFAULT_DAYS)))?.keep_days();
        let file = ConfigFile::load(args.config, false)?;
        let store = HistoryStore::in_data_dir(&file)
            .map_err(|error| error.l10n().resolve(ui::tui_locale()))?;
        let now = Utc::now();
        let since = now - TimeDelta::days(i64::from(days));
        let found = account_histories(&store, &file.config, account.as_deref(), since, now)?;
        let mut stdout = io::stdout().lock();
        if args.json {
            serde_json::to_writer_pretty(&mut stdout, &history_json(&found, since, now, days))?;
            writeln!(stdout)?;
        } else {
            write!(
                stdout,
                "{}",
                history_text(&found, since, now, days, ui::local_time)
            )?;
        }
        return Ok(history_exit_code(&found));
    }
    if args.token_file.is_some() && !args.serve && args.from_server.is_none() {
        return Err(l10n::tr(
            "cli.token_file_needs_mode",
            "--token-file needs --serve or --status-line --from-server.",
        )
        .into());
    }
    if let Some(url) = args.from_server.take() {
        // PRD 0031 RF2.1: the running server's readings, without a query.
        let style = cuotai_core::tray::TrayStyle::parse(&args.style).unwrap_or_default();
        let color = status_line::use_color(
            args.color,
            io::stdout().is_terminal(),
            std::env::var("NO_COLOR").ok().as_deref(),
        );
        let (line, code) = status_line::from_server(
            &url,
            args.token_file.as_deref(),
            style,
            &args.status_accounts,
            color,
        )
        .await?;
        println!("{line}");
        return Ok(code);
    }
    if args.vault_audit || args.vault_prune {
        let file = ConfigFile::load(args.config, false)?;
        let session = match &args.dev_credentials_sqlite {
            Some(path) => OAuthSession::development_sqlite(path)?,
            None => OAuthSession::system()?,
        };
        return if args.vault_prune {
            vault::prune(&session, &file.config).await
        } else {
            vault::audit(&session, &file.config, args.json).await
        };
    }
    if args.stats || args.timeline || args.events_log {
        return stored_view(&args);
    }
    let interactive = !args.once
        && !args.json
        && !args.json_v1
        && !args.serve
        && !args.watch
        && !args.status_line
        && args.report_problem.is_none()
        && !args.canary;
    let status = status_enabled(args.status, args.no_status, |key| std::env::var(key).ok());
    dashboard::set_pace_unit(pace_unit(args.pace_unit.as_deref(), |key| {
        std::env::var(key).ok()
    }));
    if interactive && (!io::stdout().is_terminal() || !io::stdin().is_terminal()) {
        return Err(
            l10n::tr(
                "cli.needs_terminal",
                "The TUI needs an interactive terminal. Use --once, --json, --json-v1, --serve or --watch.",
            )
                .into(),
        );
    }
    let locale = ui::tui_locale();
    // The browser page after a sign-in callback speaks the TUI's language
    // and follows the browser's light/dark preference.
    cuotai_core::oauth::callback_page::set_callback_style(
        cuotai_core::oauth::callback_page::CallbackStyle {
            locale: locale.to_string(),
            theme: None,
        },
    );
    let events = args
        .events
        .as_ref()
        .map(|names| cuotai_core::watch::parse_events(names.iter().map(String::as_str)))
        .transpose()
        .map_err(|error| error.resolve(locale))?;
    let retention = retention(args.history_days)?;
    let file = ConfigFile::load(args.config, interactive)?;
    // PRD 0028 RF8: the opt-in update notice, TUI and `--once` only.
    let update_notice = update_notice::start(
        &file,
        interactive || (args.once && !args.json && !args.json_v1 && !args.serve && !args.watch),
    );
    // PRD 0013 RF3.2: no hook runs before its exact commands were
    // confirmed, and no query starts before that is settled.
    let hooks = if args.hooks {
        Some(watch::confirmed_hooks(&file.path, locale)?)
    } else {
        None
    };
    let session = Arc::new(match args.dev_credentials_sqlite {
        Some(path) => {
            let session = OAuthSession::development_sqlite(&path)?;
            eprintln!(
                "{}",
                l10n::tr(
                    "cli.dev_sqlite_warning",
                    "WARNING: credentials in SQLite WITHOUT ENCRYPTION (development); the Keychain will not be used. Do not share the database.",
                )
            );
            session
        }
        None => OAuthSession::system()?,
    });
    if args.canary {
        return canary::run(
            &file.config,
            canary::CanaryArgs {
                out: args.out.take(),
                dry_run: args.dry_run,
                open: args.canary_open,
                include_metered: args.canary_include_metered,
                status,
            },
            session,
        )
        .await;
    }
    if let Some(name) = args.report_problem.take() {
        return report_problem::run(&file.config, &name, session).await;
    }
    if args.once || args.json || args.json_v1 || args.serve || args.watch || args.status_line {
        let fallback_locale = fallback_locale_of(
            args.fallback_locale.as_deref(),
            args.json || args.json_v1 || args.serve || args.watch,
        )?;
        // PRD 0013 RF2.1: `--interval` holds for this run only.
        let config = match args.interval {
            Some(seconds) => cuotai_core::watch::with_interval(&file.config, seconds)
                .map_err(|error| error.resolve(locale))?,
            None => file.config.clone(),
        };
        let history = history_store(&file, retention);
        let mut monitor =
            Monitor::with_login_session(config.clone(), Arc::new(EnvironmentCredentials), session)?;
        monitor.incidents().set_enabled(status);
        if let Some(store) = history {
            monitor = monitor.with_history(store);
        }
        monitor.mark_history_removed();
        if args.serve || args.watch {
            let serve = args.serve.then_some(serve::ServeArgs {
                port: args.port,
                token_file: args.token_file,
                no_token: args.no_token,
                stats: args.serve_stats,
                fallback_locale: fallback_locale.clone(),
            });
            let watch = args.watch.then_some(watch::WatchArgs {
                events,
                jsonl: args.jsonl,
                hooks,
                projected_lead: lead(args.projected_lead.as_deref()),
                reset_lead: args.reset_lead,
                fallback_locale: fallback_locale.clone(),
            });
            // PRD 0015 RF4.3: the server publishes `local_cost`, kept
            // current by the background scan while it runs.
            let local_cost = args.serve.then(cost::LocalCost::start).flatten();
            return headless::run(
                config,
                file.identity()?,
                monitor,
                serve,
                watch,
                local_cost,
                args.strict_exit,
                headless::Reload {
                    path: file.path.clone(),
                    interval: args.interval,
                    hooks: args.hooks,
                },
            )
            .await;
        }
        // PRD 0010 RF2.3: read before this query's readings are recorded.
        let restored = if args.json_v1 {
            monitor.restored_readings()
        } else {
            Vec::new()
        };
        // PRD 0015 RF4.3: `local_cost` reads what grew in the logs of the
        // sources that are on (bounded, on its own thread) during the query.
        let local_cost = args
            .json_v1
            .then(|| std::thread::spawn(cost::one_shot_export));
        let snapshot = monitor.refresh().await;
        // The readings are queued off the refresh; let them land first.
        monitor.flush_history(HISTORY_FLUSH);
        let code = if snapshot.reports.iter().all(|r| r.status == Status::Ready) {
            0
        } else {
            2
        };
        if args.status_line {
            // PRD 0031 RF2: one line from the core's `tray::summary`.
            let style = cuotai_core::tray::TrayStyle::parse(&args.style).unwrap_or_default();
            let color = status_line::use_color(
                args.color,
                io::stdout().is_terminal(),
                std::env::var("NO_COLOR").ok().as_deref(),
            );
            println!(
                "{}",
                status_line::render(
                    &file.config,
                    &snapshot,
                    style,
                    &args.status_accounts,
                    color,
                    Utc::now(),
                    ui::local_time,
                )
            );
            return Ok(code);
        }
        if args.json {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(
                &mut stdout,
                &localized(serde_json::to_value(&snapshot)?, &fallback_locale),
            )?;
            writeln!(stdout)?;
        } else if args.json_v1 {
            let export = single_export(
                &file.config,
                &file.identity()?,
                restored,
                snapshot,
                &monitor.incident_statuses(),
                Utc::now(),
                // PRD 0031 RF13: the pace reads the recent samples, as the app does.
                |index, metrics, at| monitor.recent_points(index, metrics, at),
            )
            .with_local_cost(local_cost.and_then(|job| job.join().ok()).flatten());
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(
                &mut stdout,
                &localized(serde_json::to_value(&export)?, &fallback_locale),
            )?;
            writeln!(stdout)?;
        } else {
            let now = Utc::now();
            let recent: Vec<Vec<Vec<Point>>> = snapshot
                .reports
                .iter()
                .enumerate()
                .map(|(index, report)| {
                    monitor.recent_points(index, &report.metrics, report.checked_at)
                })
                .collect();
            print_snapshot(
                &file.config,
                &snapshot,
                &recent,
                &monitor.incident_statuses(),
                now,
            );
        }
        if let Some(warning) = monitor.history_status().and_then(|s| s.warning()) {
            eprintln!("cuotai: {}", warning.resolve(ui::tui_locale()));
        }
        update_notice::print(update_notice, locale).await;
        return Ok(code);
    }
    let animations = motion::enabled(args.no_animations, |key| std::env::var(key).ok());
    // PRD 0015 RF2.2/RF4.2: the local cost index follows the logs of the
    // sources that are on from a thread of its own while the TUI is open,
    // and the account details show it.
    let local_cost = cost::LocalCost::start();
    ui::run(
        file,
        session,
        animations,
        retention,
        args.notify.then(|| lead(args.notify_projected.as_deref())),
        status,
        local_cost,
    )
    .await?;
    update_notice::print(update_notice, locale).await;
    Ok(0)
}

/// PRD 0024: the lead of `--notify-projected`/`--projected-lead` (clap
/// accepts only 15, 30 or 60).
fn lead(minutes: Option<&str>) -> Option<cuotai_core::alerts::Lead> {
    cuotai_core::alerts::Lead::from_minutes(minutes?.parse().ok()?)
}

/// PRD 0012 RF4.3: the `--json-v1` export of one run: `snapshot` applied to a
/// new cycle over the `restored` readings (PRD 0010 RF2.3, in account
/// order), so an account whose query failed carries its last recorded
/// reading as `last_good` with `stale`, as the server would publish it.
fn single_export(
    config: &Config,
    identity: &ConfigIdentity,
    restored: Vec<Option<Report>>,
    snapshot: Snapshot,
    incidents: &[ProviderStatus],
    now: DateTime<Utc>,
    recent: impl Fn(usize, &[cuotai_core::Metric], DateTime<Utc>) -> Vec<Vec<Point>>,
) -> Export {
    let mut cycle = RefreshCycle::new(config, Instant::now());
    cycle.restore_last_good_at(restored, now);
    if let Some(token) = cycle.begin_full() {
        cycle.finish_full(Some(snapshot), token, Instant::now());
    }
    Export::from_cycle_with_history(
        config,
        identity,
        &cycle,
        incidents,
        now,
        Options::default(),
        recent,
    )
}

fn print_snapshot(
    config: &Config,
    snapshot: &Snapshot,
    recent: &[Vec<Vec<Point>>],
    incidents: &[ProviderStatus],
    now: DateTime<Utc>,
) {
    println!("CUOTAI • {}", snapshot.finished_at.to_rfc3339());
    print!(
        "\n{}",
        summary_lines(snapshot, ui::tui_locale(), ui::local_time)
    );
    for (index, report) in snapshot.reports.iter().enumerate() {
        let recent = recent.get(index).map_or(&[][..], Vec::as_slice);
        let status = incidents.iter().find(|s| s.provider == report.provider);
        // The snapshot keeps the configuration's account order.
        let interval = config.accounts.get(index).map_or(
            Duration::from_secs(config.refresh_seconds),
            |account| {
                Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds))
            },
        );
        print!(
            "{}",
            report_text(
                report,
                recent,
                status,
                interval,
                now,
                ui::tui_locale(),
                ui::local_time
            )
        );
    }
}

/// One `--once` account block: heading, the provider's flagged status (PRD
/// 0011 RF5, the card's rule), metrics (with the PRD 0007 RF4 pace line
/// under each eligible one, projected from the recorded `recent` points of
/// each metric when there are enough, at the reading's `checked_at` and only
/// while it is fresh for the account's `interval`, PRD 0024 FIX-11), notes
/// and source.
fn report_text(
    report: &Report,
    recent: &[Vec<Point>],
    status: Option<&ProviderStatus>,
    interval: Duration,
    now: DateTime<Utc>,
    locale: &str,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = format!("\n{}\n", report_heading(report, now));
    if let Some(line) = status.and_then(|status| incident_line(status, locale, now, &at)) {
        out += &line;
    }
    if let Some(line) = dashboard::extra_usage_text(report, locale) {
        out += &l10n::trf_in(
            locale,
            "once.extra_usage",
            "  Extra usage: {text}",
            &[("text", &ui::safe(&line))],
        );
        out += "\n";
    }
    for (index, metric) in report.metrics.iter().enumerate() {
        out += &format!(
            "  {}: {}\n",
            ui::safe(&ui::metric_label_text(&metric.label)),
            ui::metric_text(metric)
        );
        if let Some(period) = &metric.period {
            out += &l10n::trf_in(
                locale,
                "once.period",
                "    Period: {period}",
                &[("period", &ui::safe(&ui::unit_text(period)))],
            );
            out += "\n";
        }
        if let Some(reset) = metric.resets_at {
            out += &renews_line(locale, "    ", &reset.to_rfc3339());
        }
        let points = recent.get(index).map(Vec::as_slice);
        if let Some(pace) = pace::assess_reading(metric, report.checked_at, interval, now, points) {
            let text = dashboard::pace_text(&pace, locale, &at);
            out += &l10n::trf_in(
                locale,
                "once.pace",
                "    Pace: {text} (linear estimate)",
                &[("text", &text)],
            );
            out += "\n";
        }
    }
    out += &sub_reports_text(report, locale, &at);
    for (idx, fallback_text) in report.notes.iter().enumerate() {
        let text = report
            .notes_l10n
            .get(idx)
            .map(|note| note.resolve(locale))
            .unwrap_or_else(|| fallback_text.clone());
        out += &format!("  {}\n", ui::safe(&text));
    }
    if !report.source.is_empty() {
        out += &l10n::trf_in(
            locale,
            "once.source",
            "  Source: {source}",
            &[("source", &ui::safe(&report.source))],
        );
        out += "\n";
    }
    out
}

/// "    Renews: <time>" under a metric of `--once`.
fn renews_line(locale: &str, indent: &str, at: &str) -> String {
    format!(
        "{indent}{}\n",
        l10n::trf_in(locale, "once.renews", "Renews: {at}", &[("at", &at)])
    )
}

/// PRD 0036 RF9.3: the block of an aggregator account, indented under its
/// line: one line per pool ("Claude · 2 de 3 disponíveis"), then every
/// connection with its status and metrics. Empty for any other account.
fn sub_reports_text(
    report: &Report,
    locale: &str,
    at: &impl Fn(DateTime<Utc>) -> String,
) -> String {
    if report.sub_reports.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for pool in cuotai_core::aggregators::pools(report) {
        out += &l10n::trf_in(
            locale,
            "once.pool",
            "  Pool: {summary}",
            &[("summary", &ui::safe(&pool.summary().resolve(locale)))],
        );
        out += "\n";
    }
    for sub in &report.sub_reports {
        let name = sub
            .provider
            .map_or_else(|| sub.upstream.clone(), ui::provider_label);
        let mut flags = Vec::new();
        if sub.enabled_upstream == Some(false) {
            flags.push(l10n::tr_in(locale, "once.sub.disabled", "off in 9router").to_string());
        }
        if sub.duplicate_of.is_some() {
            flags.push(
                l10n::tr_in(locale, "once.sub.duplicate", "also monitored directly").to_string(),
            );
        }
        if let Some(plan) = &sub.plan {
            flags.push(l10n::trf_in(
                locale,
                "once.sub.plan",
                "plan {plan}",
                &[("plan", &ui::safe(plan))],
            ));
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join("; "))
        };
        out += &format!(
            "  • {} · {} — {}{}\n",
            ui::safe(&name),
            ui::safe(&sub.label),
            ui::status_label(sub.status),
            flags
        );
        for metric in &sub.metrics {
            out += &format!(
                "      {}: {}\n",
                ui::safe(&ui::metric_label_text(&metric.label)),
                ui::metric_text(metric)
            );
            if let Some(reset) = metric.resets_at {
                out += &renews_line(locale, "        ", &at(reset));
            }
        }
        if sub.status != cuotai_core::Status::Ready {
            // The reason a connection has no reading; the two standing
            // caveats ("cached", "zero") are shown in the details instead.
            let reason = |note: &&cuotai_core::text::Localized| {
                let id = note.id.as_ref();
                id.starts_with("query.")
                    || (id.starts_with("note.nine_router.")
                        && !id.ends_with("cached")
                        && !id.ends_with("zero_caveat"))
            };
            if let Some(note) = sub.notes_l10n.iter().find(reason) {
                out += &format!("      {}\n", ui::safe(&note.resolve(locale)));
            }
        }
    }
    out
}

/// PRD 0011 RF5: `--once` line of a flagged provider status: state (or an
/// unreadable page), incident title, start and the page to open.
fn incident_line(
    status: &ProviderStatus,
    locale: &str,
    now: DateTime<Utc>,
    at: impl Fn(DateTime<Utc>) -> String,
) -> Option<String> {
    let badge = dashboard::incident_badge(status, locale, now)?;
    let mut parts = vec![badge.label];
    parts.extend(badge.title);
    if let Some(start) = status.incident.as_ref().and_then(|i| i.started_at) {
        parts.push(l10n::trf_in(
            locale,
            "dashboard.incident.since",
            "since {at}",
            &[("at", &at(start))],
        ));
    }
    parts.push(ui::safe(&badge.url).into_owned());
    Some(format!(
        "{}\n",
        l10n::trf_in(
            locale,
            "once.provider_status",
            "  Provider status: {parts}",
            &[("parts", &parts.join(" · "))],
        )
    ))
}

/// PRD 0009 RF3.2: one line per account with the core's binding summary;
/// accounts without one show their status (or class when ready).
fn summary_lines(
    snapshot: &Snapshot,
    locale: &str,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = String::new();
    for report in &snapshot.reports {
        let state = match metrics::binding(report) {
            Some(limit) => limit.summary(&at).resolve(locale),
            None if report.status == Status::Ready => {
                dashboard::class_label(dashboard::classification(Some(report))).to_string()
            }
            None => ui::status_label(report.status),
        };
        out.push_str(&format!(
            "{}  {}\n",
            ui::safe(&report.display_name()),
            ui::safe(&state)
        ));
    }
    out
}

/// PRD 0034 RF5.2: the language of the `fallback` texts of the machine
/// outputs, from `--fallback-locale` or `CUOTAI_FALLBACK_LOCALE` (`None`:
/// English). The flag needs one of those outputs and a shipped catalog.
fn fallback_locale_of(flag: Option<&str>, machine: bool) -> Result<Option<String>, String> {
    let Some(flag) = flag else {
        return Ok(cuotai_core::text::fallback_locale(None));
    };
    if !machine {
        return Err("--fallback-locale needs --json, --json-v1, --serve or --watch.".into());
    }
    match cuotai_core::text::fallback_locale(Some(flag)) {
        Some(locale) => Ok(Some(locale)),
        None if cuotai_core::text::normalize(flag)
            .is_some_and(|row| row.status == cuotai_core::text::LocaleStatus::Source) =>
        {
            Ok(None)
        }
        None => Err(format!(
            "--fallback-locale {flag:?} is not supported; use {}.",
            fallback_locale_codes()
        )),
    }
}

/// The codes `--fallback-locale` accepts, from the core registry: the
/// source language, then every language with a core catalog.
fn fallback_locale_codes() -> String {
    let source = cuotai_core::text::Locale::resolve("").code();
    let mut codes = vec![source];
    codes.extend(
        cuotai_core::text::catalog_locales()
            .into_iter()
            .filter(|code| *code != source),
    );
    match codes.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => codes.join(""),
    }
}

/// `json` with its `fallback` texts in `locale` (unchanged without one).
fn localized(mut json: serde_json::Value, locale: &Option<String>) -> serde_json::Value {
    if let Some(locale) = locale {
        cuotai_core::text::localize_json_fallbacks(&mut json, locale);
    }
    json
}

/// PRD 0010 RF5.2: `--once` account heading with the measurement age.
fn report_heading(report: &Report, now: DateTime<Utc>) -> String {
    l10n::trf(
        "once.heading",
        "{account} / {provider} — {status} · queried {age}{as_of}",
        &[
            ("account", &ui::safe(&report.display_name())),
            ("provider", &ui::provider_label(report.provider)),
            ("status", &ui::status_label(report.status)),
            ("age", &ui::age_text(report.checked_at, now)),
            ("as_of", &ui::data_as_of_suffix(report)),
        ],
    )
}

/// PRD 0006 RF6.2: the recorded metrics of each account named `name` (all
/// accounts without one) within `since..=now`, in configuration order. An
/// unknown name is a local error; a storage failure is never "no data".
fn account_histories<'a>(
    store: &HistoryStore,
    config: &'a Config,
    name: Option<&str>,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Vec<(&'a Account, Vec<MetricHistory>)>, String> {
    let accounts: Vec<&Account> = config
        .accounts
        .iter()
        .filter(|account| {
            name.is_none_or(|name| account.name == name || account.display_name() == name)
        })
        .collect();
    if let (Some(name), true) = (name, accounts.is_empty()) {
        return Err(l10n::trf(
            "cli.account_not_found",
            "Account not found in the configuration: {name}",
            &[("name", &ui::safe(name))],
        ));
    }
    accounts
        .into_iter()
        .map(|account| {
            store
                .account_history(account, since..=now)
                .map(|metrics| (account, metrics))
                .map_err(|error| error.l10n().resolve(ui::tui_locale()))
        })
        .collect()
}

/// `0` when every listed account has recorded data in the span, `2` when
/// one (or the configuration) has none, like the other single-run outputs.
/// PRD 0026 RF6.2: `--stats`, `--timeline` and `--events-log` read the
/// history store and the last recorded readings; no provider is queried.
/// 0 on success, 2 when the store cannot be read, an `Err` (exit 1) for
/// invalid arguments and local errors.
fn stored_view(args: &Args) -> Result<u8, Box<dyn std::error::Error>> {
    use stats_view as view;
    let locale = ui::tui_locale();
    // Arguments are checked before the configuration is touched.
    let stats_range = view::stats_range(if args.stats { args.days } else { None });
    let timeline_range = view::timeline_range(args.range.as_deref());
    let events_days = view::event_days(if args.events_log { args.days } else { None });
    let file = ConfigFile::load(args.config.clone(), false)?;
    let store = HistoryStore::in_data_dir(&file).map_err(|error| error.l10n().resolve(locale))?;
    let now = Utc::now();
    let mut stdout = io::stdout().lock();
    let unreadable = |error: cuotai_core::history::HistoryError| {
        eprintln!("cuotai: {}", error.l10n().resolve(locale));
        2u8
    };
    if args.stats {
        let summary = match cuotai_core::stats::summarize(
            &store,
            &file.config.accounts,
            stats_range?,
            now,
            &chrono::Local,
        ) {
            Ok(summary) => summary,
            Err(error) => return Ok(unreadable(error)),
        };
        if args.json {
            serde_json::to_writer_pretty(&mut stdout, &summary)?;
            writeln!(stdout)?;
        } else {
            write!(stdout, "{}", view::stats_text(&summary, locale))?;
        }
    } else if args.timeline {
        let reports = match store.latest_reports(&file.config) {
            Ok(reports) => reports,
            Err(error) => return Ok(unreadable(error)),
        };
        let timeline = view::timeline_of(&file.config, &reports, timeline_range?, now);
        if args.json {
            serde_json::to_writer_pretty(&mut stdout, &timeline)?;
            writeln!(stdout)?;
        } else {
            write!(stdout, "{}", view::timeline_text(&timeline, locale))?;
        }
    } else {
        let events = match view::read_events(&store, &file.config, events_days?, now) {
            Ok(events) => events,
            Err(error) => return Ok(unreadable(error)),
        };
        if args.json {
            serde_json::to_writer_pretty(&mut stdout, &view::events_json(&events, locale))?;
            writeln!(stdout)?;
        } else {
            write!(stdout, "{}", view::events_text(&events, locale))?;
        }
    }
    Ok(0)
}

fn history_exit_code(found: &[(&Account, Vec<MetricHistory>)]) -> u8 {
    if !found.is_empty() && found.iter().all(|(_, metrics)| !metrics.is_empty()) {
        0
    } else {
        2
    }
}

/// `--history --json`: the span and, per account, the core's metrics
/// (points and window instances); no credentials, only the account name
/// and provider the TOML already holds.
fn history_json(
    found: &[(&Account, Vec<MetricHistory>)],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    days: u16,
) -> serde_json::Value {
    serde_json::json!({
        "from": since,
        "to": now,
        "days": days,
        "accounts": found
            .iter()
            .map(|(account, metrics)| serde_json::json!({
                "account": account.display_name(),
                "provider": account.provider,
                "metrics": metrics,
            }))
            .collect::<Vec<_>>(),
    })
}

/// `--history` text: per account, each metric's summary, sparkline over
/// the span and window instances; an account without samples says so.
fn history_text(
    found: &[(&Account, Vec<MetricHistory>)],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    days: u16,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = l10n::trf(
        "history.heading",
        "CUOTAI • {days}-day history until {until}",
        &[("days", &days), ("until", &now.to_rfc3339())],
    );
    out += "\n";
    if found.is_empty() {
        out += "\n";
        out += l10n::tr(
            "history.no_accounts",
            "No account in the configuration; no measurements to show.",
        );
        out += "\n";
    }
    for (account, metrics) in found {
        out += &format!(
            "\n{} / {}\n",
            ui::safe(&account.display_name()),
            ui::provider_label(account.provider)
        );
        if metrics.is_empty() {
            out += &l10n::trf(
                "history.account_empty",
                "  No measurements recorded in the last {days} days.",
                &[("days", &days)],
            );
            out += "\n";
        }
        for metric in metrics {
            let label = ui::safe(&ui::metric_label_text(&metric.label)).into_owned();
            let Some(trend) = metric.trend() else {
                out += &l10n::trf(
                    "history.metric_empty",
                    "  {label}: no measured values in the period",
                    &[("label", &label)],
                );
                out += "\n";
                continue;
            };
            out += &l10n::trf(
                "history.metric",
                "  {label}: {count} measurements · {trend} ({last})",
                &[
                    ("label", &label),
                    ("count", &metric.points.len()),
                    ("trend", &dashboard::trend_text(&trend, &metric.unit)),
                    ("last", &at(trend.last.taken_at)),
                ],
            );
            out += "\n";
            out += &format!(
                "    {}\n",
                dashboard::sparkline(metric, since, now, HISTORY_SPARK_CELLS).trim_end()
            );
            for window in &metric.windows {
                let resets = window
                    .resets_at
                    .map(|reset| {
                        l10n::trf(
                            "history.window.renews",
                            " · renews {at}",
                            &[("at", &at(reset))],
                        )
                    })
                    .unwrap_or_default();
                out += &l10n::trf(
                    "history.window",
                    "    Window {start} → {end}: {initial} → {final}, peak {peak} at {peak_at}{resets}",
                    &[
                        ("start", &at(window.started_at)),
                        ("end", &at(window.ended_at)),
                        ("initial", &dashboard::compact(window.initial_used)),
                        ("final", &dashboard::compact(window.final_used)),
                        ("peak", &dashboard::compact(window.peak_used)),
                        ("peak_at", &at(window.peak_at)),
                        ("resets", &resets),
                    ],
                );
                out += "\n";
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_locale_error_lists_the_registry_codes() {
        let error = fallback_locale_of(Some("xx-YY"), true).unwrap_err();
        assert!(error.contains("en-US"), "{error}");
        for code in cuotai_core::text::catalog_locales() {
            assert!(error.contains(code), "{code} missing: {error}");
        }
        assert!(error.contains(" or "), "{error}");
    }

    /// PRD 0024 RF3.4/RF5.1: `--notify` alone never projects;
    /// `--notify-projected` needs `--notify` and takes 15, 30 or 60 (60
    /// without a value); `--projected-lead` needs `--watch`.
    #[test]
    fn projected_flags() {
        use cuotai_core::alerts::Lead;
        let parse = |argv: &[&str]| {
            Args::try_parse_from(std::iter::once("cuotai").chain(argv.iter().copied()))
        };
        let notify = parse(&["--notify"]).unwrap();
        assert_eq!(lead(notify.notify_projected.as_deref()), None);
        let bare = parse(&["--notify", "--notify-projected"]).unwrap();
        assert_eq!(lead(bare.notify_projected.as_deref()), Some(Lead::Min60));
        let fifteen = parse(&["--notify", "--notify-projected", "15"]).unwrap();
        assert_eq!(lead(fifteen.notify_projected.as_deref()), Some(Lead::Min15));
        assert!(parse(&["--notify-projected", "30"]).is_err());
        assert!(parse(&["--notify", "--notify-projected", "45"]).is_err());
        let watch = parse(&["--watch", "--projected-lead", "30"]).unwrap();
        assert_eq!(lead(watch.projected_lead.as_deref()), Some(Lead::Min30));
        assert!(parse(&["--projected-lead", "30"]).is_err());
    }

    /// The default refresh interval of the `--once` block tests.
    const FIVE_MIN: Duration = Duration::from_secs(300);
    use chrono::TimeZone;
    use cuotai_core::{Metric, MetricKind, MetricWindow};

    fn quota(window: MetricWindow, used: f64, resets_at: Option<DateTime<Utc>>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: format!("{window:?}"),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(window),
            period: None,
            resets_at,
        }
    }

    fn report(account: &str, status: Status, metrics: Vec<Metric>) -> Report {
        Report {
            account_id: None,
            diagnostic: None,
            account: account.into(),
            provider: Provider::Claude,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    /// PRD 0012 RF4.3: `--json-v1` publishes this run's reports and, for an
    /// account whose query failed, its last recorded reading as stale.
    #[test]
    fn json_v1_keeps_the_recorded_reading_of_a_failed_account() {
        let config = Config::from_toml(
            "[[accounts]]\nname = \"Claude\"\nprovider = \"claude\"\n\n\
             [[accounts]]\nname = \"Work\"\nprovider = \"claude\"\n",
        )
        .unwrap();
        let week = || vec![quota(MetricWindow::Weekly, 40.0, None)];
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                report("Claude", Status::Ready, week()),
                report("Work", Status::RateLimited, Vec::new()),
            ],
        };
        let recorded = vec![
            Some(report("Claude", Status::Ready, Vec::new())),
            Some(report("Work", Status::Ready, week())),
        ];
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 10, 0).unwrap();
        let identity = ConfigIdentity::Explicit("test".into());
        let export = single_export(
            &config,
            &identity,
            recorded,
            snapshot,
            &[],
            now,
            |_, _, _| Vec::new(),
        );
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["schema"], 1);
        let (ready, failed) = (&json["accounts"][0], &json["accounts"][1]);
        assert_eq!(ready["report"]["status"], "ready");
        assert_eq!(ready["report"]["metrics"][0]["used"], 40.0);
        assert_eq!(ready["stale"], false);
        assert!(ready["last_good"].is_null());
        assert_eq!(
            failed["report"]["status"],
            serde_json::to_value(Status::RateLimited).unwrap()
        );
        assert_eq!(failed["stale"], true);
        assert_eq!(failed["last_good"]["metrics"][0]["used"], 40.0);
        assert_eq!(failed["last_good"]["age_seconds"], 600);
    }

    #[test]
    fn once_summary_shows_binding_limit_per_account() {
        let week_reset = Utc.with_ymd_and_hms(2026, 1, 3, 14, 30, 0).unwrap();
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                // 5 h free, week exhausted: blocked by the week.
                report(
                    "Claude",
                    Status::Ready,
                    vec![
                        quota(MetricWindow::Window5h, 0.0, None),
                        quota(MetricWindow::Weekly, 100.0, Some(week_reset)),
                    ],
                ),
                report(
                    "Work",
                    Status::Ready,
                    vec![
                        quota(MetricWindow::Window5h, 10.0, None),
                        quota(MetricWindow::Weekly, 62.0, None),
                    ],
                ),
                // Exhausted without a known return: never "now".
                report(
                    "Night",
                    Status::Ready,
                    vec![quota(MetricWindow::Monthly, 100.0, None)],
                ),
                report("Ops", Status::AuthenticationError, Vec::new()),
                report("Empty", Status::Ready, Vec::new()),
            ],
        };
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        assert_eq!(
            summary_lines(&snapshot, "en-US", at),
            "Claude  blocked until 14:30 by the weekly window\n\
             Work  most restrictive: weekly window at 62%\n\
             Night  blocked by the monthly window; return time unknown\n\
             Ops  Autenticação\n\
             Empty  Sem dados\n"
        );
        assert_eq!(
            summary_lines(&snapshot, "pt-BR", at).lines().next(),
            Some("Claude  bloqueada até 14:30 pela janela semanal")
        );
    }

    #[test]
    fn once_heading_carries_the_measurement_age() {
        let report = report("Work", Status::RateLimited, Vec::new());
        let now = report.checked_at + chrono::Duration::seconds(125);
        assert_eq!(
            report_heading(&report, now),
            "Work / Claude — Rate limit · consultado há 2 min"
        );
    }

    /// PRD 0036 RF9.3: an aggregator account prints its pools and every
    /// connection indented under it; any other account prints nothing extra.
    #[test]
    fn once_prints_the_pools_and_connections_of_an_aggregator() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let sub = |id: &str, upstream: &str, provider, status, metrics: Vec<Metric>| {
            cuotai_core::SubReport {
                id: id.into(),
                upstream: upstream.into(),
                provider,
                label: format!("conn-{id}"),
                status,
                metrics,
                notes_l10n: Vec::new(),
                checked_at: now,
                as_of: None,
                enabled_upstream: Some(true),
                duplicate_of: None,
                plan: None,
                via: None,
                identity: None,
            }
        };
        let mut router = report("home", Status::Ready, Vec::new());
        router.provider = Provider::NineRouter;
        router.sub_reports = vec![
            sub(
                "a",
                "claude",
                Some(Provider::Claude),
                Status::Ready,
                vec![quota(MetricWindow::Weekly, 40.0, None)],
            ),
            sub("b", "glm", None, Status::Unsupported, Vec::new()),
        ];
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let text = report_text(&router, &[], None, FIVE_MIN, now, "en-US", at);
        assert!(
            text.contains("  Pool: Claude · 1 of 1 available\n"),
            "{text}"
        );
        assert!(
            text.contains("  • Claude · conn-a — Atualizado\n"),
            "{text}"
        );
        assert!(text.contains("      Weekly: 40.00 % / 100.00\n"), "{text}");
        // An upstream Cuotai does not know shows its slug.
        assert!(text.contains("  • glm · conn-b — Indisponível\n"), "{text}");
        // Other accounts are unchanged.
        let plain = report("Work", Status::Ready, Vec::new());
        assert_eq!(
            report_text(&plain, &[], None, FIVE_MIN, now, "en-US", at),
            "\nWork / Claude — Atualizado · consultado agora\n"
        );
    }

    #[test]
    fn once_prints_the_pace_line_under_each_eligible_metric() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let resets = |minutes| Some(now + chrono::Duration::minutes(minutes));
        let report = report(
            "Work",
            Status::Ready,
            vec![
                quota(MetricWindow::Window5h, 40.0, resets(120)),
                quota(MetricWindow::Weekly, 88.0, resets(30 * 60)),
                // Freshly opened, then no reset: no invented pace.
                quota(MetricWindow::Daily, 1.0, resets(24 * 60 - 5)),
                quota(MetricWindow::Monthly, 10.0, None),
            ],
        );
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        assert_eq!(
            report_text(&report, &[], None, FIVE_MIN, now, "en-US", at),
            "\nWork / Claude — Atualizado · consultado agora\n\
             \x20 Window5h: 40.00 % / 100.00\n\
             \x20   Renews: 2026-01-01T02:00:00+00:00\n\
             \x20   Pace: 20 points in reserve · lasts until the reset (linear estimate)\n\
             \x20 Weekly: 88.00 % / 100.00\n\
             \x20   Renews: 2026-01-02T06:00:00+00:00\n\
             \x20   Pace: 6 points above the pace · runs out at 18:49, 11h 10m before the reset (linear estimate)\n\
             \x20 Daily: 1.00 % / 100.00\n\
             \x20   Renews: 2026-01-01T23:55:00+00:00\n\
             \x20 Monthly: 10.00 % / 100.00\n"
        );
        assert!(report_text(&report, &[], None, FIVE_MIN, now, "es", at)
            .contains("Ritmo: 6 puntos por encima del ritmo · se agota a las 18:49"));

        // PRD 0007 RF1.5 with the recorded history: the 5-hour window rose
        // 30 points in the last 40 minutes, so it runs out before the reset.
        let point = |minutes: i64, used: f64| Point {
            taken_at: now - chrono::Duration::minutes(minutes),
            used,
        };
        let recent = [vec![point(40, 10.0), point(20, 25.0), point(0, 40.0)]];
        assert!(
            report_text(&report, &recent, None, FIVE_MIN, now, "en-US", at).contains(
                "Pace: 20 points in reserve · runs out at 01:20, 40m 00s before the reset"
            )
        );
    }

    /// PRD 0023 RF8.2: `--once` prints `Extra usage: <summary>` under the
    /// account while extra usage bills, and nothing otherwise.
    #[test]
    fn once_prints_the_extra_usage_line() {
        let mut extra = quota(MetricWindow::Monthly, 6.25, None).overage();
        extra.kind = cuotai_core::MetricKind::Spend;
        extra.unit = "USD".into();
        extra.limit = Some(50.0);
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let active = report("Work", Status::Ready, vec![extra.clone()]);
        let now = active.checked_at;
        let text = report_text(&active, &[], None, FIVE_MIN, now, "en-US", at);
        assert!(
            text.contains(
                "
  Extra usage: On extra · $6.25
"
            ),
            "{text}"
        );
        assert!(
            text.find("Extra usage").unwrap() < text.find("Monthly").unwrap(),
            "under the account heading, before the metrics"
        );
        extra.used = Some(0.0);
        let armed = report("Work", Status::Ready, vec![extra]);
        assert!(!report_text(&armed, &[], None, FIVE_MIN, now, "en-US", at).contains("Uso extra"));
    }

    /// PRD 0024 FIX-11: `--once` evaluates the pace at the reading's
    /// `checked_at` and prints none for a reading that is not fresh.
    #[test]
    fn once_pace_follows_the_reading_time_and_freshness() {
        let mut report = report(
            "Work",
            Status::Ready,
            vec![quota(MetricWindow::Window5h, 40.0, None)],
        );
        let checked_at = report.checked_at;
        // Reset 2 hours after `now`; the reading is 90 minutes older.
        let now = checked_at + chrono::Duration::minutes(90);
        report.metrics[0].resets_at = Some(now + chrono::Duration::minutes(120));
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let hour = Duration::from_secs(3600);
        // Fresh at a 1-hour interval: at `checked_at` 30% of the window had
        // elapsed, so 40% used is 10 points above the pace (the wall clock
        // would say 20 points in reserve).
        assert!(report_text(&report, &[], None, hour, now, "en-US", at)
            .contains("Pace: 10 points above the pace"));
        // Stale at a 5-minute interval: the age only, no pace.
        let text = report_text(&report, &[], None, FIVE_MIN, now, "en-US", at);
        assert!(text.contains("consultado há"), "{text}");
        assert!(!text.contains("Ritmo"), "{text}");
    }

    /// PRD 0024 RF1.3: points by default; the flag beats the environment.
    #[test]
    fn the_pace_unit_follows_the_flag_then_the_environment() {
        use cuotai_core::pace::PaceUnit::{Percent, Points};
        let env = |value: Option<&'static str>| {
            move |key: &str| {
                (key == "CUOTAI_PACE_UNIT")
                    .then_some(value)
                    .flatten()
                    .map(String::from)
            }
        };
        assert_eq!(pace_unit(None, env(None)), Points);
        assert_eq!(pace_unit(None, env(Some("percent"))), Percent);
        assert_eq!(pace_unit(Some("points"), env(Some("percent"))), Points);
        assert_eq!(pace_unit(Some("percent"), env(None)), Percent);
        assert_eq!(pace_unit(None, env(Some("loud"))), Points);
        assert!(Args::try_parse_from(["cuotai", "--pace-unit", "loud"]).is_err());
        assert!(Args::try_parse_from(["cuotai", "--pace-unit", "percent"]).is_ok());
    }

    /// RF5.2: opt-in through `--status` or `CUOTAI_STATUS`; `--no-status`
    /// overrides both.
    #[test]
    fn status_queries_are_opt_in_and_no_status_wins() {
        let env = |value: Option<&'static str>| {
            move |key: &str| {
                (key == "CUOTAI_STATUS")
                    .then_some(value)
                    .flatten()
                    .map(String::from)
            }
        };
        assert!(!status_enabled(false, false, env(None)), "off by default");
        assert!(status_enabled(true, false, env(None)));
        assert!(status_enabled(false, false, env(Some("1"))));
        assert!(!status_enabled(false, false, env(Some("0"))));
        assert!(!status_enabled(false, false, env(Some(""))));
        assert!(!status_enabled(true, true, env(Some("1"))));
        assert!(!status_enabled(false, true, env(Some("yes"))));
    }

    /// RF5.1 text snapshot: `--once` names a flagged provider status under
    /// the heading and keeps the core's incident note; an operational or
    /// plainly unknown provider prints nothing.
    #[test]
    fn once_prints_the_incident_and_its_note() {
        use cuotai_core::incidents::{Incident, IncidentState};
        use cuotai_core::text::Localized;
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let mut failed = report("Work", Status::Error, Vec::new());
        for note in [
            Localized::fixed("query.transport", "Network failure or query timeout."),
            Localized::fixed(
                "note.incident.ongoing",
                "The provider reports an ongoing incident; the failure may not be from your account.",
            ),
        ] {
            failed.notes.push(note.fallback.to_string());
            failed.notes_l10n.push(note);
        }
        let outage = ProviderStatus {
            provider: Provider::Claude,
            state: IncidentState::MajorOutage,
            incident: Some(Incident {
                title: "Elevated errors on Claude Code".into(),
                started_at: Some(now - chrono::Duration::minutes(30)),
                url: "https://status.claude.com/incidents/abc123".into(),
            }),
            page_url: "https://status.claude.com/".into(),
            read_at: Some(now),
            stale: false,
            expired: false,
        };
        assert_eq!(
            report_text(&failed, &[], Some(&outage), FIVE_MIN, now, "en-US", at),
            "\nWork / Claude — Erro · consultado agora\n\
             \x20 Provider status: Major outage · Elevated errors on Claude Code · since 23:30 · https://status.claude.com/incidents/abc123\n\
             \x20 Network failure or query timeout.\n\
             \x20 The provider reports an ongoing incident; the failure may not be from your account.\n"
        );

        let maintenance = ProviderStatus {
            state: IncidentState::Maintenance,
            incident: None,
            stale: true,
            ..outage.clone()
        };
        let ready = report("Work", Status::Ready, Vec::new());
        assert_eq!(
            report_text(&ready, &[], Some(&maintenance), FIVE_MIN, now, "pt-BR", at),
            "\nWork / Claude — Atualizado · consultado agora\n\
             \x20 Status do provedor: Em manutenção · última leitura · lido agora · https://status.claude.com/\n"
        );
        let unreadable = ProviderStatus {
            state: IncidentState::Unknown,
            read_at: None,
            ..maintenance.clone()
        };
        assert!(report_text(&ready, &[], Some(&unreadable), FIVE_MIN, now, "pt-BR", at)
            .contains("  Status do provedor: Página de status indisponível · https://status.claude.com/\n"));
        let expired = ProviderStatus {
            state: IncidentState::Unknown,
            incident: None,
            read_at: Some(now - chrono::Duration::minutes(31)),
            stale: false,
            expired: true,
            ..outage.clone()
        };
        assert!(
            report_text(&ready, &[], Some(&expired), FIVE_MIN, now, "pt-BR", at)
                .contains("Status do provedor: Status desconhecido · lido há 31 min")
        );
        for state in [IncidentState::Operational, IncidentState::Unknown] {
            let quiet = ProviderStatus {
                state,
                stale: false,
                ..unreadable.clone()
            };
            assert!(
                !report_text(&ready, &[], Some(&quiet), FIVE_MIN, now, "pt-BR", at)
                    .contains("Status do provedor")
            );
        }
    }

    #[test]
    fn history_mode_prints_the_span_and_exits_like_the_other_single_runs() {
        use cuotai_core::config_file::ConfigIdentity;
        let dir = std::env::temp_dir().join(format!("cuotai-tui-history-{}", std::process::id()));
        let path = dir.join("history.sqlite3");
        let store = HistoryStore::new(path.clone(), &ConfigIdentity::Explicit("cli".into()));
        let config = Config::from_toml(
            "[[accounts]]\nname = \"Work\"\nprovider = \"groq\"\ncredential_env = \"GROQ_WORK\"\n\
             [[accounts]]\nname = \"Spare\"\nprovider = \"groq\"\ncredential_env = \"GROQ_SPARE\"\n",
        )
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
        let since = now - TimeDelta::days(7);
        let at = |t: DateTime<Utc>| t.format("%m-%d %H:%M").to_string();

        // Nothing recorded: it says so (exit 2) and never creates the file.
        let found = account_histories(&store, &config, None, since, now).unwrap();
        assert_eq!(history_exit_code(&found), 2);
        let text = history_text(&found, since, now, 7, at);
        assert_eq!(
            text.matches("Sem medições registradas nos últimos 7 dias.")
                .count(),
            2
        );
        assert!(!path.exists());
        assert!(account_histories(&store, &config, Some("Nope"), since, now)
            .unwrap_err()
            .contains("Conta não encontrada"));

        // Two readings of one 5-hour window, a reset, and an old sample
        // outside the span.
        let hours = |n: i64| now + TimeDelta::hours(n);
        let work = &config.accounts[0];
        for (taken, used, reset) in [
            (-200, 99.0, -190),
            (-3, 10.0, 2),
            (-2, 70.0, 2),
            (-1, 5.0, 4),
        ] {
            let metric = quota(MetricWindow::Window5h, used, Some(hours(reset)));
            store.record(work, hours(taken), &[metric]).unwrap();
        }
        let found = account_histories(&store, &config, Some("Work"), since, now).unwrap();
        assert_eq!(history_exit_code(&found), 0);
        assert_eq!(
            history_text(&found, since, now, 7, at),
            format!(
                "CUOTAI • histórico de 7 dias até 2026-01-02T00:00:00+00:00\n\
                 \nWork / {}\n\
                 \x20 Window5h: 3 medições · mín 5 · máx 70 · último 5 % (01-01 23:00)\n\
                 \x20   {}▂▆\n\
                 \x20   Janela 01-01 21:00 → 01-01 22:00: 10 → 70, pico 70 em 01-01 22:00 · renova 01-02 02:00\n\
                 \x20   Janela 01-01 23:00 → 01-01 23:00: 5 → 5, pico 5 em 01-01 23:00 · renova 01-02 04:00\n",
                Provider::Groq.label(),
                " ".repeat(58)
            )
        );
        let json = history_json(&found, since, now, 7);
        let metric = &json["accounts"][0]["metrics"][0];
        assert_eq!(json["accounts"][0]["account"], "Work");
        assert_eq!(metric["points"].as_array().unwrap().len(), 3);
        assert_eq!(metric["windows"][0]["peak_used"], 70.0);
        assert_eq!(metric["limit"], 100.0);
        // Every account: one without data makes it partial.
        let found = account_histories(&store, &config, None, since, now).unwrap();
        assert_eq!(history_exit_code(&found), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
