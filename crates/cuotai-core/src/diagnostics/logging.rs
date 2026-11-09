//! PRD 0052 RF1/RF2 — the app log: one levelled, redacted facade over
//! `tracing`, written to a private rotating file, plus the panic record.
//!
//! - Frontends call [`init`] once with their [`AppKind`]; library code emits
//!   through [`event`] (or `tracing` macros under a `cuotai*` target).
//!   Events of any other target (dependencies) never reach the file.
//! - `CUOTAI_LOG` wins over `RUST_LOG`; both take `EnvFilter` syntax. An
//!   invalid filter falls back to `info` and records one fixed warning;
//!   `off` writes nothing (and creates no file).
//! - The file is `<data root>/logs/cuotai.log` (the root is
//!   `CUOTAI_DATA_DIR` or `<OS local data dir>/cuotai`), `0600` on
//!   Unix, rotated to `cuotai.log.1` at 256 KiB (at most 512 KiB in all).
//! - Every field value passes the fail-closed [`redact::text`]; an
//!   `account` field is kept only at `debug`/`trace`. Writing never panics
//!   or blocks a refresh on an error: a failure flips [`health`] to
//!   [`LogHealth::Error`] and the line is dropped.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use tracing::field::{Field as TracingField, Visit};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{EnvFilter, Layer};

use super::redact;
use crate::private_fs::RotatingFile;

/// Size at which `cuotai.log` moves to `cuotai.log.1`.
pub const LOG_ROTATE_BYTES: u64 = 256 * 1024;
/// Longest panic payload kept, in bytes.
pub const PANIC_PAYLOAD_BYTES: usize = 2 * 1024;
/// Filter used when neither variable is set, or the one set is invalid.
pub const DEFAULT_FILTER: &str = "info";

/// Which front end writes the log (a field of every line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// `cuotai` one-shot and headless modes (`--once`, `--json`, `--watch`,
    /// `--serve` and the other flags).
    Cli,
    /// The interactive terminal dashboard.
    Tui,
    /// The Tauri desktop app.
    Desktop,
}

impl AppKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Tui => "tui",
            Self::Desktop => "desktop",
        }
    }
}

/// Whether the app log works (RF3 reports it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogHealth {
    /// [`init`] has not run in this process.
    Uninitialized,
    /// `CUOTAI_LOG`/`RUST_LOG` is `off`.
    Off,
    /// Lines are being written.
    Ok,
    /// No local data directory: nothing can be written.
    Unavailable,
    /// A write failed; later lines are still attempted.
    Error,
}

static HEALTH: AtomicU8 = AtomicU8::new(0);
/// The app kind lines name once [`set_app_kind`] ran (0: the one [`init`] got).
static KIND: AtomicU8 = AtomicU8::new(0);

/// Relabel this process's lines (the CLI binary becomes `tui` when it
/// opens the interactive dashboard).
pub fn set_app_kind(kind: AppKind) {
    KIND.store(kind as u8 + 1, Ordering::Relaxed);
}

fn app_kind(fallback: AppKind) -> AppKind {
    match KIND.load(Ordering::Relaxed) {
        1 => AppKind::Cli,
        2 => AppKind::Tui,
        3 => AppKind::Desktop,
        _ => fallback,
    }
}
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();
/// Set by [`suspend`]: nothing reaches the file any more.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

/// Stop writing the app log for the rest of this process (PRD 0052 RF5.2:
/// the desktop's erase-all removes the log and must not recreate it before
/// it quits). Events are still accepted and dropped.
pub fn suspend() {
    SUSPENDED.store(true, Ordering::SeqCst);
}

/// Whether [`suspend`] ran.
pub fn is_suspended() -> bool {
    SUSPENDED.load(Ordering::SeqCst)
}

fn set_health(health: LogHealth) {
    HEALTH.store(health as u8, Ordering::Relaxed);
}

/// The app log's state in this process.
pub fn health() -> LogHealth {
    match HEALTH.load(Ordering::Relaxed) {
        1 => LogHealth::Off,
        2 => LogHealth::Ok,
        3 => LogHealth::Unavailable,
        4 => LogHealth::Error,
        _ => LogHealth::Uninitialized,
    }
}

/// The file [`init`] writes to, once it ran with a data directory.
pub fn log_file() -> Option<&'static Path> {
    LOG_FILE.get().map(PathBuf::as_path)
}

/// `<root>/logs/cuotai.log`, where `root` is the data root
/// ([`crate::data_dir::root`]).
pub fn log_path(root: &Path) -> PathBuf {
    root.join("logs").join("cuotai.log")
}

/// The app log under the data root, `None` without one (or with an
/// invalid `CUOTAI_DATA_DIR`).
pub fn default_log_path() -> Option<PathBuf> {
    crate::data_dir::root().map(|dir| log_path(&dir))
}

/// The filter text the environment selects: `CUOTAI_LOG`, then `RUST_LOG`.
fn filter_text() -> Option<String> {
    ["CUOTAI_LOG", "RUST_LOG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
}

/// The filter for `text`, and whether it was invalid (then `info`).
fn parse_filter(text: Option<&str>) -> (EnvFilter, bool) {
    match text {
        None => (EnvFilter::new(DEFAULT_FILTER), false),
        Some(text) => match EnvFilter::try_new(text) {
            Ok(filter) => (filter, false),
            Err(_) => (EnvFilter::new(DEFAULT_FILTER), true),
        },
    }
}

fn is_off(text: Option<&str>) -> bool {
    text.is_some_and(|t| t.trim().eq_ignore_ascii_case("off"))
}

/// Start the app log for this process (once; later calls only report the
/// state). `data_dir` is the data root (normally
/// [`crate::data_dir::root`]); `None` leaves the log unavailable.
pub fn init(kind: AppKind, data_dir: Option<&Path>) -> LogHealth {
    static ONCE: OnceLock<LogHealth> = OnceLock::new();
    *ONCE.get_or_init(|| {
        let text = filter_text();
        if is_off(text.as_deref()) {
            set_health(LogHealth::Off);
            return LogHealth::Off;
        }
        let Some(data_dir) = data_dir else {
            set_health(LogHealth::Unavailable);
            return LogHealth::Unavailable;
        };
        let path = log_path(data_dir);
        if let Some(dir) = path.parent() {
            if crate::private_fs::ensure_dir(dir).is_err() {
                set_health(LogHealth::Unavailable);
                return LogHealth::Unavailable;
            }
        }
        let _ = LOG_FILE.set(path.clone());
        let (filter, invalid) = parse_filter(text.as_deref());
        let file = Arc::new(RotatingFile::new(path, LOG_ROTATE_BYTES));
        set_health(LogHealth::Ok);
        if tracing::subscriber::set_global_default(subscriber(kind, file, filter)).is_err() {
            // Another subscriber owns this process (an embedding host).
            return health();
        }
        if invalid {
            event(Level::Warn, "log.filter_invalid", &[]);
        }
        health()
    })
}

/// [`init`] under the data root (`CUOTAI_DATA_DIR` included).
pub fn init_default(kind: AppKind) -> LogHealth {
    init(kind, crate::data_dir::root().as_deref())
}

/// The subscriber [`init`] installs, over any rotating file (tests use a
/// temporary one with [`with_test_file`]).
fn subscriber(
    kind: AppKind,
    file: Arc<RotatingFile>,
    filter: EnvFilter,
) -> impl Subscriber + Send + Sync {
    let ours = tracing_subscriber::filter::filter_fn(|meta| meta.target().starts_with("cuotai"));
    tracing_subscriber::registry()
        .with(filter)
        .with(AppLog { kind, file }.with_filter(ours))
}

/// Levels of the facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

/// A typed, safe field value.
#[derive(Debug, Clone, Copy)]
pub enum Field<'a> {
    /// A fixed class or slug (`rate_limited`, `claude_code`); still redacted.
    Text(&'a str),
    Count(u64),
    Int(i64),
    Flag(bool),
    /// A duration in milliseconds.
    Millis(u64),
    /// An account name: kept only at [`Level::Debug`], e-mails redacted.
    Account(&'a str),
}

/// Emit `id` (a stable dotted event id) with `fields`. Nothing is written
/// before [`init`] or when the filter excludes the level.
pub fn event(level: Level, id: &'static str, fields: &[(&'static str, Field<'_>)]) {
    let mut rendered = String::new();
    for (name, value) in fields {
        let (value, unit) = match value {
            Field::Account(_) if level != Level::Debug => continue,
            Field::Text(text) | Field::Account(text) => (redact::field(name, text), ""),
            Field::Count(n) => (n.to_string(), ""),
            Field::Millis(n) => (n.to_string(), "ms"),
            Field::Int(n) => (n.to_string(), ""),
            Field::Flag(b) => (b.to_string(), ""),
        };
        if !rendered.is_empty() {
            rendered.push(' ');
        }
        let _ = write!(rendered, "{name}={}{unit}", quoted(&value));
    }
    let cuotai_fields = rendered.as_str();
    match level {
        Level::Error => {
            tracing::event!(target: "cuotai", TracingLevel::ERROR, event = id, cuotai_fields)
        }
        Level::Warn => {
            tracing::event!(target: "cuotai", TracingLevel::WARN, event = id, cuotai_fields)
        }
        Level::Info => {
            tracing::event!(target: "cuotai", TracingLevel::INFO, event = id, cuotai_fields)
        }
        Level::Debug => {
            tracing::event!(target: "cuotai", TracingLevel::DEBUG, event = id, cuotai_fields)
        }
    }
}

fn quoted(value: &str) -> String {
    if value.is_empty() || value.contains([' ', '"', '=']) {
        format!("{value:?}")
    } else {
        value.to_string()
    }
}

/// The layer that formats and writes each event.
struct AppLog {
    kind: AppKind,
    file: Arc<RotatingFile>,
}

impl<S: Subscriber> Layer<S> for AppLog {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if is_suspended() {
            return;
        }
        let level = *event.metadata().level();
        let mut line = Line {
            keep_account: level >= TracingLevel::DEBUG,
            ..Line::default()
        };
        event.record(&mut line);
        let mut text = format!(
            "{} {:<5} {}",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            level,
            app_kind(self.kind).as_str()
        );
        if let Some(id) = &line.event {
            let _ = write!(text, " {id}");
        }
        if let Some(fields) = &line.fields {
            if !fields.is_empty() {
                let _ = write!(text, " {fields}");
            }
        }
        for (name, value) in &line.others {
            let _ = write!(text, " {name}={}", quoted(value));
        }
        if let Some(message) = &line.message {
            let _ = write!(text, " msg={}", quoted(message));
        }
        text.push('\n');
        match self.file.append(text.as_bytes()) {
            Ok(()) if health() == LogHealth::Error => set_health(LogHealth::Ok),
            Ok(()) => {}
            Err(_) => set_health(LogHealth::Error),
        }
    }
}

/// The fields of one event, already redacted.
#[derive(Default)]
struct Line {
    keep_account: bool,
    event: Option<String>,
    fields: Option<String>,
    message: Option<String>,
    others: Vec<(&'static str, String)>,
}

impl Line {
    fn put(&mut self, field: &TracingField, raw: &str) {
        match field.name() {
            "event" => self.event = Some(event_id(raw)),
            // Rendered by [`event`] from typed values, each one already
            // redacted by name and value (a second pass over the joined
            // text would let one `cookie: …` swallow the fields after it).
            "cuotai_fields" => {
                self.fields = Some(
                    raw.chars()
                        .map(|c| if c.is_control() { ' ' } else { c })
                        .collect(),
                );
            }
            "message" => self.message = Some(redact::text(raw)),
            "account" if !self.keep_account => {}
            name => self.others.push((name, redact::field(name, raw))),
        }
    }
}

/// An event id is a dotted slug; anything else is replaced.
fn event_id(raw: &str) -> String {
    let ok = !raw.is_empty()
        && raw.len() <= 64
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if ok {
        raw.to_string()
    } else {
        "invalid_event_id".to_string()
    }
}

impl Visit for Line {
    fn record_str(&mut self, field: &TracingField, value: &str) {
        self.put(field, value);
    }

    fn record_i64(&mut self, field: &TracingField, value: i64) {
        self.put(field, &value.to_string());
    }

    fn record_u64(&mut self, field: &TracingField, value: u64) {
        self.put(field, &value.to_string());
    }

    fn record_bool(&mut self, field: &TracingField, value: bool) {
        self.put(field, &value.to_string());
    }

    fn record_f64(&mut self, field: &TracingField, value: f64) {
        self.put(field, &value.to_string());
    }

    fn record_error(&mut self, field: &TracingField, _value: &(dyn std::error::Error + 'static)) {
        // RF1.3: an arbitrary error is never formatted; only its presence.
        self.put(field, "[error]");
    }

    fn record_debug(&mut self, field: &TracingField, value: &dyn std::fmt::Debug) {
        // `?value` and `%value` arrive here; both are redacted like text.
        let mut text = String::new();
        let _ = write!(
            LimitedWriter::new(&mut text, 4 * redact::MAX_FIELD_CHARS),
            "{value:?}"
        );
        self.put(field, &text);
    }
}

/// A `fmt::Write` that stops accepting text past `limit` bytes, so a huge
/// `Debug` value never builds a huge string.
struct LimitedWriter<'a> {
    out: &'a mut String,
    limit: usize,
}

impl<'a> LimitedWriter<'a> {
    fn new(out: &'a mut String, limit: usize) -> Self {
        Self { out, limit }
    }
}

impl std::fmt::Write for LimitedWriter<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let room = self.limit.saturating_sub(self.out.len());
        if room == 0 {
            return Err(std::fmt::Error);
        }
        let mut cut = s.len().min(room);
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        self.out.push_str(&s[..cut]);
        if cut < s.len() {
            Err(std::fmt::Error)
        } else {
            Ok(())
        }
    }
}

/// RF2.2: the title of the native modal a failed desktop start shows.
pub fn startup_failed_title() -> crate::text::Localized {
    crate::text::Localized::fixed("diagnostics.startup_failed_title", "Cuotai could not start")
}

/// RF2.2: the modal's text, naming the app log (home-abbreviated) or
/// saying that none could be written. Never the failure's own text.
pub fn startup_failed(log: Option<&Path>) -> crate::text::Localized {
    match log {
        Some(path) => crate::text::Localized::with_params(
            "diagnostics.startup_failed",
            crate::text::params([("path", redact::home_abbreviated(path).into())]),
            "Cuotai could not start. The app log has the details: {path}",
        ),
        None => crate::text::Localized::fixed(
            "diagnostics.startup_failed_no_log",
            "Cuotai could not start, and no app log could be written.",
        ),
    }
}

/// RF2.1: record one bounded `panic` event, then run the hook that was
/// installed before (the default printer, or a frontend's own). The record
/// carries the app kind, version, thread name, the source file's base name
/// and line, and the redacted payload (at most 2 KiB) — never a backtrace,
/// the environment or a full path.
pub fn install_panic_hook(kind: AppKind) {
    let prior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record_panic(kind, info);
        // An adapter panic caught by the refresh (`crate::panic_guard`)
        // becomes that account's failed report: record it, but skip the
        // prior hook (the default printer would write over a TUI, a
        // frontend's own hook would tear it down).
        if !crate::panic_isolated() {
            prior(info);
        }
    }));
}

/// The fields of a panic record, for [`install_panic_hook`] and its tests.
pub(crate) fn panic_fields(info: &std::panic::PanicHookInfo<'_>) -> (String, String, u32, String) {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string payload".to_string());
    let mut cut = payload.len().min(PANIC_PAYLOAD_BYTES);
    while !payload.is_char_boundary(cut) {
        cut -= 1;
    }
    let payload = redact::text(&payload[..cut]);
    let (file, line) = info.location().map_or(("unknown".to_string(), 0), |loc| {
        let base = Path::new(loc.file()).file_name().map_or_else(
            || "unknown".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        (base, loc.line())
    });
    let thread = std::thread::current()
        .name()
        .map_or_else(|| "unnamed".to_string(), str::to_string);
    (payload, file, line, thread)
}

fn record_panic(kind: AppKind, info: &std::panic::PanicHookInfo<'_>) {
    let (payload, file, line, thread) = panic_fields(info);
    event(
        Level::Error,
        "panic",
        &[
            ("app", Field::Text(kind.as_str())),
            ("version", Field::Text(env!("CARGO_PKG_VERSION"))),
            ("thread", Field::Text(&thread)),
            ("file", Field::Text(&file)),
            ("line", Field::Count(u64::from(line))),
            ("payload", Field::Text(&payload)),
        ],
    );
}

/// Run `body` with a subscriber that writes to `file` with `filter`
/// (tests: the rotation and redaction of the real formatter, without the
/// process-wide [`init`]).
#[cfg(test)]
pub(crate) fn with_test_file<R>(
    kind: AppKind,
    file: Arc<RotatingFile>,
    filter: &str,
    body: impl FnOnce() -> R,
) -> R {
    tracing::subscriber::with_default(subscriber(kind, file, EnvFilter::new(filter)), body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::redact::{assert_clean, SEEDS};

    const ACCOUNT: &str = "Seed Work Account";

    fn emit_everything() {
        let seeded = format!(
            "failed with token={} for seed.person@example.com and session_id=SESSIONCOOKIEVALUE42",
            SEEDS[1]
        );
        let url = "https://seeduser:seedpass@api.example.com/v1/users/SEEDPATHID?q=SEEDQUERY#SEEDFRAGMENT";
        let body = r#"{"SEEDBODYFIELD":"sk-ant-api03-SEEDSECRETVALUE1234567890"}"#;
        for level in [Level::Error, Level::Warn, Level::Info, Level::Debug] {
            event(
                level,
                "test.seeded",
                &[
                    ("text", Field::Text(&seeded)),
                    ("url", Field::Text(url)),
                    ("body", Field::Text(body)),
                    ("cookie", Field::Text("Cookie: sid=SESSIONCOOKIEVALUE42")),
                    ("account", Field::Account(ACCOUNT)),
                    ("email_account", Field::Account("seed.person@example.com")),
                    ("n", Field::Count(3)),
                    ("ms", Field::Millis(12)),
                    ("ok", Field::Flag(true)),
                    ("delta", Field::Int(-4)),
                ],
            );
        }
        tracing::error!(target: "cuotai_core::test", account = ACCOUNT, "Bearer {}", SEEDS[0]);
        tracing::warn!(target: "cuotai_core::test", value = ?body, "{url}");
        tracing::info!(target: "cuotai_core::test", value = %seeded);
        tracing::debug!(target: "cuotai_core::test", account = ACCOUNT, "{}", SEEDS[4]);
        tracing::trace!(target: "cuotai_core::test", cookie = "SESSIONCOOKIEVALUE42");
        // A dependency's event never reaches the file.
        tracing::error!(target: "hyper::proto", "DEPENDENCY-LINE");
    }

    #[test]
    fn seeded_secrets_never_reach_either_generation() {
        let dir = tempfile::tempdir().unwrap();
        // A small limit forces several rotations.
        let file = Arc::new(RotatingFile::new(
            dir.path().join("logs/cuotai.log"),
            2 * 1024,
        ));
        with_test_file(AppKind::Tui, Arc::clone(&file), "trace", || {
            for _ in 0..6 {
                emit_everything();
            }
        });
        let [current, older] = file.generations();
        assert!(!current.is_empty() && !older.is_empty(), "rotated");
        for text in [&current, &older] {
            assert_clean(text);
            assert!(!text.contains("DEPENDENCY-LINE"));
            assert!(text.len() as u64 <= 2 * 1024);
            for line in text.lines() {
                let level = line.split_whitespace().nth(1).unwrap();
                if !matches!(level, "DEBUG" | "TRACE") {
                    assert!(!line.contains(ACCOUNT), "account below debug: {line}");
                }
            }
        }
        let all = format!("{current}{older}");
        assert!(all.contains("test.seeded"));
        assert!(
            all.contains("account=\"Seed Work Account\""),
            "kept at debug"
        );
        assert!(
            all.contains("n=3") && all.contains("ms=12ms") && all.contains("ok=true"),
            "{all}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_filter_selects_levels_and_invalid_falls_back_to_info() {
        assert!(!parse_filter(None).1);
        assert!(!parse_filter(Some("cuotai=debug,warn")).1);
        assert!(parse_filter(Some("[[[not a filter")).1);
        assert!(is_off(Some(" OFF ")));
        let dir = tempfile::tempdir().unwrap();
        let file = Arc::new(RotatingFile::new(
            dir.path().join("cuotai.log"),
            LOG_ROTATE_BYTES,
        ));
        with_test_file(AppKind::Cli, Arc::clone(&file), "warn", || {
            event(Level::Info, "test.info", &[]);
            event(Level::Warn, "test.warn", &[]);
        });
        let [current, _] = file.generations();
        assert!(!current.contains("test.info"));
        assert!(current.contains(" WARN  cli test.warn"));
        with_test_file(AppKind::Cli, Arc::clone(&file), "off", || {
            event(Level::Error, "test.off", &[]);
        });
        assert!(!file.generations()[0].contains("test.off"));
    }

    #[test]
    fn an_event_id_must_be_a_slug() {
        assert_eq!(event_id("serve.started"), "serve.started");
        assert_eq!(event_id("Bearer sk-x"), "invalid_event_id");
    }

    #[test]
    fn a_panic_record_is_bounded_and_redacted_and_the_prior_hook_runs() {
        use std::sync::atomic::AtomicBool;
        static PRIOR_RAN: AtomicBool = AtomicBool::new(false);
        let dir = tempfile::tempdir().unwrap();
        let file = Arc::new(RotatingFile::new(
            dir.path().join("cuotai.log"),
            LOG_ROTATE_BYTES,
        ));
        let saved = std::panic::take_hook();
        // The "frontend" hook installed before ours (like the TUI's
        // terminal restoration installed after it, it must still run).
        std::panic::set_hook(Box::new(|_| PRIOR_RAN.store(true, Ordering::SeqCst)));
        install_panic_hook(AppKind::Tui);
        let file2 = Arc::clone(&file);
        let result = std::thread::Builder::new()
            .name("seed-worker".into())
            .spawn(move || {
                with_test_file(AppKind::Tui, file2, "info", || {
                    let _ = std::panic::catch_unwind(|| {
                        panic!("boom {} {} {}", SEEDS[0], SEEDS[4], "x".repeat(10 * 1024))
                    });
                })
            })
            .unwrap()
            .join();
        std::panic::set_hook(saved);
        assert!(result.is_ok());
        assert!(
            PRIOR_RAN.load(Ordering::SeqCst),
            "the prior hook is chained"
        );
        let [current, _] = file.generations();
        assert_clean(&current);
        let line = current.lines().find(|l| l.contains(" panic ")).unwrap();
        assert!(line.contains("thread=seed-worker"));
        assert!(line.contains("file=logging.rs"));
        assert!(!line.contains("src/diagnostics"), "base name only");
        assert!(line.len() < PANIC_PAYLOAD_BYTES + 512);
    }
}
