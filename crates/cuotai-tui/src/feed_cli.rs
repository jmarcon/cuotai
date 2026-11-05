//! PRD 0040 RF2 and RF5: `cuotai --feed claude-statusline` and
//! `cuotai --statusline-claude`, the two commands Claude Code runs as its
//! status line.
//!
//! Both run in the agent's process context, so they are fast, silent and
//! failure-proof: no network request (except the optional loopback read of
//! `--from-server`), no vault, no provider query, no panic (a panic prints
//! one line on stderr and exits `1`). The parsing, the store, the words and
//! the composition are the core's (`cuotai_core::feed`); this module only
//! reads standard input and picks the other accounts.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use cuotai_core::feed::{self, claude, statusline, FeedSource, FeedStore};
use cuotai_core::freshness::Freshness;
use cuotai_core::tray::AccountView;

/// The `--feed` sources this release accepts.
pub(crate) const SOURCES: [&str; 1] = ["claude-statusline"];

/// RF5.4: how long `--from-server` may take before the line falls back to
/// the history.
const SERVER_BUDGET: Duration = Duration::from_millis(120);

/// Whether this run is one of the agent-side commands, decided from the raw
/// arguments before the app log starts (RF2.3: nothing but the data
/// directory is touched).
pub(crate) fn requested() -> bool {
    std::env::args_os().any(|arg| arg == "--feed" || arg == "--statusline-claude")
}

/// RF6 (isolation): a panic prints one line on stderr and exits `1`.
pub(crate) fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_| {
        let _ = writeln!(
            std::io::stderr(),
            "cuotai: internal error in the status-line feed"
        );
        std::process::exit(1);
    }));
}

/// Standard input, at most one byte over the limit (so an oversized input
/// is detected without reading all of it).
fn read_stdin() -> std::io::Result<Vec<u8>> {
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(feed::MAX_INPUT as u64 + 1)
        .read_to_end(&mut input)?;
    Ok(input)
}

/// RF2.1/RF2.2: read, store, and (with `print`) compose. Exit `0` on a
/// valid object, whatever windows it carries; `1` on malformed input.
pub(crate) async fn run_feed(print: bool, setup: bool, line: &LineOptions, locale: &str) -> u8 {
    if setup {
        print_setup(locale);
        return 0;
    }
    let Some(reading) = read_and_store(locale) else {
        return 1;
    };
    if print {
        println!("{}", compose(Some(&reading), line, locale).await);
    }
    0
}

/// RF5: store the stdin reading (when there is one) and print the line.
pub(crate) async fn run_statusline(line: &LineOptions, locale: &str) -> u8 {
    let reading = if std::io::stdin().is_terminal() {
        // Run by hand: no input, the newest stored reading stands in.
        FeedStore::in_data_dir().and_then(|store| store.latest(FeedSource::ClaudeCode, Utc::now()))
    } else {
        match read_and_store(locale) {
            Some(reading) => Some(reading),
            None => return 1,
        }
    };
    println!("{}", compose(reading.as_ref(), line, locale).await);
    0
}

/// Parse standard input and write the feed file. A write failure is
/// reported on stderr only and never fails the agent's footer.
fn read_and_store(locale: &str) -> Option<feed::FeedReading> {
    let now = Utc::now();
    let input = match read_stdin() {
        Ok(input) => input,
        Err(_) => {
            eprintln!(
                "cuotai: {}",
                claude::ParseError::Malformed.l10n().resolve(locale)
            );
            return None;
        }
    };
    let reading = match claude::parse(&input, now) {
        Ok(reading) => reading,
        Err(error) => {
            eprintln!("cuotai: {}", error.l10n().resolve(locale));
            return None;
        }
    };
    match FeedStore::in_data_dir() {
        Some(store) => {
            if store.write(&reading, now).is_err() {
                eprintln!("cuotai: {}", write_failed(locale));
            }
        }
        None => eprintln!("cuotai: {}", write_failed(locale)),
    }
    Some(reading)
}

fn write_failed(locale: &str) -> &'static str {
    crate::l10n::tr_in(
        locale,
        "feed.write_failed",
        "Could not store the status-line reading in the data folder.",
    )
}

/// What the status line shows besides Claude's own windows (RF5.1).
pub(crate) struct LineOptions {
    pub(crate) config: Option<PathBuf>,
    pub(crate) accounts: Vec<String>,
    /// `Some("")`: the default server address.
    pub(crate) from_server: Option<String>,
    pub(crate) token_file: Option<PathBuf>,
    pub(crate) max_width: Option<usize>,
    pub(crate) color: bool,
}

/// RF5.1-RF5.3: the composed line.
async fn compose(
    reading: Option<&feed::FeedReading>,
    options: &LineOptions,
    locale: &str,
) -> String {
    let now = Utc::now();
    let others = others(options).await;
    let line = statusline::compose(
        reading,
        &others,
        now,
        crate::ui::local_time,
        options.max_width,
        locale,
    );
    match (options.color, line.level) {
        (true, Some(level)) if !line.text.is_empty() => {
            format!(
                "\x1b[{}m{}\x1b[0m",
                crate::status_line::ansi(level),
                line.text
            )
        }
        _ => line.text,
    }
}

/// The other accounts: a running server's when asked and it answers
/// within the budget, otherwise the last recorded reading of each account
/// (RF5.2). Claude accounts are left out: their windows come from the feed.
async fn others(options: &LineOptions) -> Vec<statusline::OtherAccount> {
    let keep = |view: &AccountView| {
        view.provider != "claude"
            && (options.accounts.is_empty() || options.accounts.contains(&view.key))
    };
    if let Some(url) = &options.from_server {
        if let Ok(Ok(views)) = tokio::time::timeout(SERVER_BUDGET, server_views(url, options)).await
        {
            return views
                .into_iter()
                .filter(|view| keep(view))
                .map(|view| statusline::OtherAccount { view, age: None })
                .collect();
        }
    }
    history_views(options)
        .into_iter()
        .filter(|other| keep(&other.view))
        .collect()
}

async fn server_views(url: &str, options: &LineOptions) -> Result<Vec<AccountView>, ()> {
    use cuotai_core::serve::{self, client};
    let token = match &options.token_file {
        Some(path) => Some(path.clone()),
        None => serve::default_token_file().filter(|path| path.is_file()),
    }
    .map(|path| serve::read_token_file(&path))
    .transpose()
    .map_err(|_| ())?;
    let url = if url.is_empty() {
        client::default_url()
    } else {
        url.to_owned()
    };
    client::limits_views(&url, token.as_ref())
        .await
        .map_err(|_| ())
}

/// RF5.2: the last recorded reading of every account, with its age; empty
/// when there is no configuration or history.
fn history_views(options: &LineOptions) -> Vec<statusline::OtherAccount> {
    let Ok(file) = cuotai_core::config_file::ConfigFile::load(options.config.clone(), false) else {
        return Vec::new();
    };
    let Ok(history) = cuotai_core::history::HistoryStore::in_data_dir(&file) else {
        return Vec::new();
    };
    let Ok(reports) = history.latest_reports(&file.config) else {
        return Vec::new();
    };
    let now = Utc::now();
    file.config
        .accounts
        .iter()
        .zip(reports)
        .filter_map(|(account, report)| {
            let report = report?;
            Some(statusline::OtherAccount {
                view: AccountView::from_report(account, Some(&report), Freshness::Stale),
                age: Some(cuotai_core::freshness::age(report.checked_at, now)),
            })
        })
        .collect()
}

/// RF2.4: the settings snippets, as text (Cuotai never edits the agent's
/// settings).
fn print_setup(locale: &str) {
    let intro = crate::l10n::tr_in(
        locale,
        "feed.setup.intro",
        "Add this to ~/.claude/settings.json (Cuotai never edits it for you):",
    );
    let chained = crate::l10n::tr_in(
        locale,
        "feed.setup.chained",
        "To keep your own status-line script, chain it instead:",
    );
    let docs = crate::l10n::tr_in(
        locale,
        "feed.setup.docs",
        "Details and the privacy statement: docs/integrations/claude-code.md",
    );
    println!("{intro}\n\n{}\n", claude::SETTINGS_SNIPPET);
    println!("{chained}\n\n{}\n", claude::CHAINED_SNIPPET);
    println!("{docs}");
}
