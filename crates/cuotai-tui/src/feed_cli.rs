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

use std::io::{BufRead, IsTerminal, Write};
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

/// RF2.3 (re-validation S-04): how long the commands wait for standard
/// input before they give up, so a writer that never sends nothing (or
/// never closes) cannot hold the agent's prompt.
const STDIN_DEADLINE: Duration = Duration::from_millis(250);

/// The exit code of a silent standard input: nothing on stdout, one fixed
/// line on stderr.
pub(crate) const EXIT_STDIN_TIMEOUT: u8 = 3;

/// What reading standard input gave.
#[derive(Debug)]
enum StdinRead {
    Input(Vec<u8>),
    Failed,
    TimedOut,
}

/// Standard input up to the end of its first JSON value, read on a
/// thread that gets `deadline` (S-04). The thread stays blocked on a
/// silent input until the process exits.
fn read_stdin(deadline: Duration) -> StdinRead {
    read_within(std::io::stdin(), deadline)
}

fn read_within<R: std::io::Read + Send + 'static>(reader: R, deadline: Duration) -> StdinRead {
    let (sender, receiver) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("feed-stdin".into())
        .spawn(move || {
            let mut reader = std::io::BufReader::new(reader);
            let _ = sender.send(first_value(&mut reader, feed::MAX_INPUT + 1));
        });
    if spawned.is_err() {
        return StdinRead::Failed;
    }
    match receiver.recv_timeout(deadline) {
        Ok(Ok(input)) => StdinRead::Input(input),
        Ok(Err(_)) => StdinRead::Failed,
        Err(_) => StdinRead::TimedOut,
    }
}

/// The bytes of the first JSON value of `reader`, at most `limit` (so an
/// oversized input is detected without reading all of it). An object or
/// an array ends at its closing bracket, whatever follows it; any other
/// start is read to the end (the parser refuses it).
fn first_value(reader: &mut impl BufRead, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut input = Vec::new();
    let mut depth = 0usize;
    let mut started = false;
    let mut in_string = false;
    let mut escaped = false;
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            return Ok(input);
        }
        let mut used = 0;
        let mut done = false;
        for &byte in chunk {
            used += 1;
            input.push(byte);
            if input.len() >= limit {
                done = true;
                break;
            }
            if in_string {
                match byte {
                    _ if escaped => escaped = false,
                    b'\\' => escaped = true,
                    b'"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match byte {
                b'{' | b'[' => {
                    started = true;
                    depth += 1;
                }
                b'}' | b']' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        done = true;
                        break;
                    }
                }
                b'"' if started => in_string = true,
                b' ' | b'\t' | b'\r' | b'\n' => {}
                _ if !started => started = true,
                _ => {}
            }
        }
        reader.consume(used);
        if done {
            return Ok(input);
        }
    }
}

fn read_timeout(locale: &str) -> &'static str {
    crate::l10n::tr_in(
        locale,
        "feed.read_timeout",
        "No status-line input arrived in time.",
    )
}

/// RF2.1/RF2.2: read, store, and (with `print`) compose. Exit `0` on a
/// valid object, whatever windows it carries; `1` on malformed input.
/// `forget` erases the stored readings instead (RF1.2): `0` done, `1` when
/// they could not be erased.
pub(crate) async fn run_feed(
    print: bool,
    setup: bool,
    forget: bool,
    line: &LineOptions,
    locale: &str,
) -> u8 {
    if setup {
        print_setup(locale);
        return 0;
    }
    if forget {
        return run_forget(locale);
    }
    let reading = match read_and_store(locale) {
        Ok(reading) => reading,
        Err(code) => return code,
    };
    if print {
        let shown = with_account_windows(Some(reading));
        println!("{}", compose(shown.as_ref(), line, locale).await);
    }
    0
}

/// RF5.1 (V-02): the stdin reading (when there is one) with Claude's
/// windows merged from every recent session, so a new Claude Code window
/// that has not reported its limits yet keeps the account's windows.
fn with_account_windows(own: Option<feed::FeedReading>) -> Option<feed::FeedReading> {
    let now = Utc::now();
    let mut readings = FeedStore::in_data_dir()
        .map(|store| store.sessions(FeedSource::ClaudeCode, now))
        .unwrap_or_default();
    // The stdin reading counts even when it could not be stored.
    readings.extend(own.iter().cloned());
    let account = feed::AccountReading::merge(&readings, now);
    statusline::line_reading(own.as_ref(), account.as_ref())
}

/// RF1.2 (S-01): `--feed claude-statusline --forget`.
fn run_forget(locale: &str) -> u8 {
    let erased = FeedStore::in_data_dir()
        .ok_or(())
        .and_then(|store| feed::forget(&store, FeedSource::ClaudeCode).map_err(drop));
    match erased {
        Ok(text) => {
            println!("{}", text.resolve(locale));
            0
        }
        Err(()) => {
            eprintln!(
                "cuotai: {}",
                crate::l10n::tr_in(
                    locale,
                    "feed.forget.failed",
                    "Could not erase the stored Claude Code readings.",
                )
            );
            1
        }
    }
}

/// RF5: store the stdin reading (when there is one) and print the line.
pub(crate) async fn run_statusline(line: &LineOptions, locale: &str) -> u8 {
    let reading = if std::io::stdin().is_terminal() {
        // Run by hand: no input, the stored readings stand in.
        with_account_windows(None)
    } else {
        match read_and_store(locale) {
            Ok(reading) => with_account_windows(Some(reading)),
            Err(code) => return code,
        }
    };
    println!("{}", compose(reading.as_ref(), line, locale).await);
    0
}

/// Parse standard input and write the feed file. A write failure is
/// reported on stderr only and never fails the agent's footer. `Err` is
/// the exit code: `1` for a malformed input, [`EXIT_STDIN_TIMEOUT`] for a
/// silent one.
fn read_and_store(locale: &str) -> Result<feed::FeedReading, u8> {
    let input = match read_stdin(STDIN_DEADLINE) {
        StdinRead::Input(input) => input,
        StdinRead::TimedOut => {
            eprintln!("cuotai: {}", read_timeout(locale));
            return Err(EXIT_STDIN_TIMEOUT);
        }
        StdinRead::Failed => {
            eprintln!(
                "cuotai: {}",
                claude::ParseError::Malformed.l10n().resolve(locale)
            );
            return Err(1);
        }
    };
    let now = Utc::now();
    let reading = match claude::parse(&input, now) {
        Ok(reading) => reading,
        Err(error) => {
            eprintln!("cuotai: {}", error.l10n().resolve(locale));
            return Err(1);
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
    Ok(reading)
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

#[cfg(test)]
mod stdin_tests {
    use super::*;

    /// S-04: a complete object is read without waiting for the writer to
    /// close standard input.
    #[test]
    fn the_first_object_is_read_while_the_writer_stays_open() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        writer
            .write_all(br#"{"a":"}{\"","b":[1,{"c":2}]} {"next":1}"#)
            .unwrap();
        let started = std::time::Instant::now();
        let StdinRead::Input(input) = read_within(reader, Duration::from_secs(5)) else {
            panic!("no input");
        };
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(input, br#"{"a":"}{\"","b":[1,{"c":2}]}"#);
        drop(writer);
    }

    /// S-04: a writer that sends nothing gives up after the deadline.
    #[test]
    fn a_silent_input_times_out() {
        let (reader, writer) = std::io::pipe().unwrap();
        let started = std::time::Instant::now();
        assert!(matches!(
            read_within(reader, Duration::from_millis(100)),
            StdinRead::TimedOut
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(writer);
    }

    /// An oversized or a non-object input is still read to its bound or
    /// its end, so the parser can refuse it.
    #[test]
    fn the_reader_is_bounded_and_reads_other_inputs_to_the_end() {
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(100));
        let mut cursor = std::io::Cursor::new(big.into_bytes());
        assert_eq!(first_value(&mut cursor, 10).unwrap().len(), 10);
        let mut cursor = std::io::Cursor::new(b"  not json at all".to_vec());
        assert_eq!(first_value(&mut cursor, 100).unwrap(), b"  not json at all");
        let mut cursor = std::io::Cursor::new(b"\n {\"k\":1}\n".to_vec());
        assert_eq!(first_value(&mut cursor, 100).unwrap(), b"\n {\"k\":1}");
    }
}
