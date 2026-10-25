//! Hooks (PRD 0013 RF3): user commands run on the events of
//! [`crate::watch::Watcher`], configured in `hooks.toml` next to the
//! accounts' TOML and run only after the user confirmed exactly those
//! rules.
//!
//! - [`Hooks`] reads and validates the file: each rule has a `name`, the
//!   `events` it runs on, optional `provider`/`account`/`min_threshold`
//!   filters, a `command` given as an argument vector (never a shell
//!   string) and a `timeout_seconds` up to [`MAX_TIMEOUT_SECONDS`].
//! - [`Consent`] keeps, in the local data directory, the digest of the
//!   rules the user confirmed for each `hooks.toml`; a [`Confirmed`] set
//!   (the only input [`Runner::start`] takes) exists only when the current
//!   rules match a confirmation, so any change to them needs a new one.
//! - [`Runner`] runs the matching rules of each event: the event as JSON
//!   on stdin and in `CUOTAI_*` variables (always data, never part of the
//!   command), an environment reduced to [`INHERITED_ENV`], a private
//!   working directory ([`work_dir`], `0700` on Unix, never the shared
//!   temporary directory), a timeout that kills the hook's whole process
//!   tree (a process group on Unix, a kill-on-close Job Object on Windows;
//!   the tree is also killed when the hook exits, so nothing it left behind
//!   escapes the [`MAX_RUNNING`] limit), [`OUTPUT_LIMIT`] bytes of captured
//!   output, at most [`MAX_RUNNING`] at once and a queue of
//!   [`QUEUE_CAPACITY`] that drops the oldest. A hook never blocks or
//!   fails its caller.
//! - [`RunLog`] records each run without the payload: rule, event, start,
//!   duration, outcome and exit code; [`RunLog::latest`] reads them back.
//! - For an editor (PRD 0013 RF4, the desktop's Settings → Integration):
//!   [`Document`] reads the rules as written with the file's revision and
//!   saves edited [`RuleDraft`]s only over that same revision, and
//!   [`Rule::sample`] builds the made-up event of a rule's test run
//!   ([`Runner::submit_to`]).
//!
//! See `docs/events.md`.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::config_file::ConfigIdentity;
use crate::events::{Event, EventKind};
use crate::process_tree::{self, Tree};
use crate::text::{params, Localized, ParamValue};
use crate::Provider;

/// The rules' file, in the directory of the accounts' TOML.
pub const FILE_NAME: &str = "hooks.toml";

/// A rule's time limit when it sets none.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 10;

/// The longest `timeout_seconds` accepted.
pub const MAX_TIMEOUT_SECONDS: u64 = 60;

/// Captured output (stdout and stderr together) kept per run; the rest is
/// read and discarded.
pub const OUTPUT_LIMIT: usize = 16 * 1024;

/// Hooks running at the same time.
pub const MAX_RUNNING: usize = 4;

/// Runs waiting for a free slot; a new one beyond it drops the oldest.
pub const QUEUE_CAPACITY: usize = 64;

/// The only variables a hook inherits from Cuotai's environment (besides
/// its own `CUOTAI_*` ones).
pub const INHERITED_ENV: [&str; 3] = ["PATH", "HOME", "LANG"];

/// Longest rule name, in characters.
const NAME_MAX_CHARS: usize = 64;

/// The run log is moved to `<name>.1` (replacing it) past this size.
const LOG_ROTATE_BYTES: u64 = 256 * 1024;

/// PRD 0032 RF5.1: what a rule does with an event that occurs inside a
/// quiet window (the schedule of `desktop.json`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuietMode {
    /// Run as always (the default, today's behaviour).
    #[default]
    Run,
    /// Do not run for events inside a quiet window; there is no digest for
    /// hooks. The `--watch` stream itself is never held.
    Skip,
}

impl QuietMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Skip => "skip",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "run" => Some(Self::Run),
            "skip" => Some(Self::Skip),
            _ => None,
        }
    }

    fn is_run(&self) -> bool {
        *self == Self::Run
    }
}

/// One rule of `hooks.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    pub name: String,
    /// The events it runs on, without repetitions, in the file's order.
    pub events: Vec<EventKind>,
    /// Only events of this provider.
    pub provider: Option<Provider>,
    /// Only events of the account with this name (for an aggregator
    /// account, also those of its connections).
    pub account: Option<String>,
    /// PRD 0029 RF4.3: only events of the account with this stable id
    /// (survives renames; display names are not unique across providers).
    pub account_id: Option<String>,
    /// Only threshold crossings at or above this percentage (events without
    /// a threshold are not filtered by it).
    pub min_threshold: Option<u8>,
    /// The program (an absolute path or a name looked up in `PATH`) and its
    /// arguments, passed as they are, without a shell.
    pub command: Vec<String>,
    pub timeout_seconds: u64,
    /// Left out of the confirmation digest while it is `run`, so rules
    /// written before PRD 0032 keep the confirmation they have.
    #[serde(skip_serializing_if = "QuietMode::is_run")]
    pub quiet: QuietMode,
}

/// PRD 0036 RF7.3: `event` belongs to a connection of the aggregator
/// account named `account`, so a rule on the aggregator covers them.
fn parent_is(event: &Event, account: &str) -> bool {
    event
        .parent
        .as_deref()
        .and_then(|key| key.split_once(':'))
        .is_some_and(|(_, name)| name == account)
}

impl Rule {
    /// Whether `event` passes this rule's events and filters.
    pub fn matches(&self, event: &Event) -> bool {
        self.events.contains(&event.event)
            && self
                .provider
                .is_none_or(|provider| provider == event.provider)
            && self
                .account
                .as_ref()
                .is_none_or(|account| *account == event.account_name || parent_is(event, account))
            && self
                .account_id
                .as_ref()
                .is_none_or(|id| *id == event.account_id)
            && self.min_threshold.is_none_or(|min| {
                event
                    .current
                    .threshold
                    .is_none_or(|threshold| threshold >= min)
            })
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds)
    }

    /// PRD 0013 RF4.1: a made-up event of this rule's first event, marked
    /// `sample`, that passes its filters (its provider, account and
    /// threshold when it has them; [`SAMPLE_ACCOUNT`] and the first
    /// provider otherwise), with plausible values. What the desktop's test
    /// button sends through [`Runner::submit_to`].
    pub fn sample(&self, now: DateTime<Utc>) -> Event {
        use crate::events::{FailureCategory, MetricReading, MetricRef, Values, SCHEMA};
        use crate::incidents::IncidentState;
        use crate::{MetricWindow, Status};
        let kind = self.events[0];
        let provider = self.provider.unwrap_or(Provider::ALL[0]);
        let account_name = self
            .account
            .clone()
            .unwrap_or_else(|| SAMPLE_ACCOUNT.to_owned());
        let resets_at = Some(now + chrono::TimeDelta::hours(5));
        let quota = |used: f64| Values {
            used: Some(used),
            limit: Some(100.0),
            remaining: Some(100.0 - used),
            unit: Some("%".into()),
            used_ratio: Some(used / 100.0),
            resets_at,
            ..Values::default()
        };
        let status = |status, category| Values {
            status: Some(status),
            category,
            ..Values::default()
        };
        let incident = |state| Values {
            incident: Some(state),
            ..Values::default()
        };
        let usage = |used: f64| Values {
            metrics: vec![MetricReading {
                label: SAMPLE_METRIC.into(),
                label_text: crate::metric_text::metric_text(SAMPLE_METRIC),
                window: Some(MetricWindow::Window5h),
                used: Some(used),
                limit: Some(100.0),
                remaining: Some(100.0 - used),
                unit: "%".into(),
                unit_text: crate::metric_text::metric_text("%"),
                used_ratio: Some(used / 100.0),
                resets_at,
            }],
            ..Values::default()
        };
        let failed = || status(Status::Error, Some(FailureCategory::ProviderError));
        let (about_metric, current, previous) = match kind {
            EventKind::QuotaThresholdCrossed => {
                let threshold = self.min_threshold.unwrap_or(90);
                let current = Values {
                    threshold: Some(threshold),
                    ..quota(f64::from(threshold))
                };
                (true, current, quota(50.0))
            }
            EventKind::QuotaExhausted | EventKind::PoolExhausted => {
                (true, quota(100.0), quota(90.0))
            }
            EventKind::AccountBlocked => {
                let current = Values {
                    blocked_until: resets_at,
                    ..quota(100.0)
                };
                (true, current, quota(90.0))
            }
            EventKind::QuotaRecovered | EventKind::AccountUnblocked | EventKind::PoolRecovered => {
                (true, quota(0.0), quota(100.0))
            }
            EventKind::QuotaReset => {
                let previous = Values {
                    resets_at: Some(now),
                    ..quota(60.0)
                };
                (true, quota(0.0), previous)
            }
            EventKind::RefreshFailed => (false, failed(), status(Status::Ready, None)),
            EventKind::RefreshRecovered => (false, status(Status::Ready, None), failed()),
            EventKind::AuthRequired => (
                false,
                status(
                    Status::AuthenticationError,
                    Some(FailureCategory::AuthRequired),
                ),
                status(Status::Ready, None),
            ),
            EventKind::ProviderIncident => (
                false,
                incident(IncidentState::MajorOutage),
                incident(IncidentState::Operational),
            ),
            EventKind::UsageUpdated => (false, usage(42.0), usage(40.0)),
            EventKind::QuotaExhaustionProjected => {
                let current = Values {
                    projected_at: Some(now + chrono::TimeDelta::minutes(40)),
                    resets_at,
                    lead_minutes: Some(60),
                    used_ratio: Some(0.8),
                    source: Some(crate::pace::PaceSource::Recent),
                    ..Values::default()
                };
                (true, current, quota(70.0))
            }
            EventKind::OverageStarted | EventKind::OverageEnded => {
                let state = |state, spent| Values {
                    overage_state: Some(state),
                    spent,
                    unit: Some("USD".into()),
                    currency: Some("USD".into()),
                    ..Values::default()
                };
                let active = state(crate::overage::OverageState::Active, Some(0.4));
                let armed = state(crate::overage::OverageState::Armed, Some(0.0));
                if kind == EventKind::OverageStarted {
                    (false, active, armed)
                } else {
                    (false, armed, active)
                }
            }
            EventKind::SessionExpiring => {
                let current = Values {
                    expires_at: Some(now + chrono::TimeDelta::days(3)),
                    days_left: Some(3),
                    ..Values::default()
                };
                (false, current, Values::default())
            }
            EventKind::QuotaResetSoon => {
                let current = Values {
                    resets_at: Some(now + chrono::TimeDelta::minutes(10)),
                    ..quota(60.0)
                };
                (true, current, quota(60.0))
            }
        };
        Event {
            schema: SCHEMA,
            event: kind,
            occurred_at: now,
            account_key: format!("{}:{account_name}", provider.slug()),
            account_id: self
                .account_id
                .clone()
                .unwrap_or_else(|| "acc_sample000000".into()),
            account_name,
            provider,
            accounts: Vec::new(),
            parent: None,
            metric: about_metric
                .then(|| MetricRef::new(SAMPLE_METRIC, Some(MetricWindow::Window5h))),
            current,
            previous: Some(previous),
            sample: true,
        }
    }
}

/// The account name of a [`Rule::sample`] whose rule names none.
pub const SAMPLE_ACCOUNT: &str = "sample";

/// The metric label of a [`Rule::sample`].
const SAMPLE_METRIC: &str = "sample (5h)";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    hooks: Vec<RuleDraft>,
}

/// One `[[hooks]]` rule as written, before validation: what an editor
/// reads ([`Document::drafts`]) and saves ([`Document::save`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDraft {
    pub name: String,
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_threshold: Option<i64>,
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i64>,
    /// `"run"` (the default) or `"skip"` (PRD 0032 RF5.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet: Option<String>,
}

/// The validated rules of one `hooks.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hooks {
    path: PathBuf,
    rules: Vec<Rule>,
}

impl Hooks {
    /// `hooks.toml` in the directory of the accounts' TOML at `config`.
    pub fn path_beside(config: &Path) -> PathBuf {
        config
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .join(FILE_NAME)
    }

    /// Read and validate the file at `path`.
    pub fn load(path: &Path) -> Result<Self, Localized> {
        Document::read(path).hooks
    }

    /// Validate `text`, the contents of the file at `path`. Errors name the
    /// line or the rule, never echo the file.
    pub fn from_toml(path: PathBuf, text: &str) -> Result<Self, Localized> {
        Self::from_drafts(path, parse(text)?)
    }

    /// Validate `drafts` as the rules of the file at `path`, in order.
    pub fn from_drafts(path: PathBuf, drafts: Vec<RuleDraft>) -> Result<Self, Localized> {
        if drafts.is_empty() {
            return Err(Localized::fixed(
                "hooks.error.empty",
                "hooks.toml has no [[hooks]] rule.",
            ));
        }
        let mut rules: Vec<Rule> = Vec::with_capacity(drafts.len());
        for (index, draft) in drafts.into_iter().enumerate() {
            let rule = validate(index + 1, draft)?;
            if rules.iter().any(|known| known.name == rule.name) {
                let name = rule.name;
                return Err(Localized::with_param(
                    "hooks.error.duplicate",
                    "name",
                    name.as_str(),
                    "More than one rule is named \"{name}\".",
                ));
            }
            rules.push(rule);
        }
        Ok(Self { path, rules })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// SHA-256 (lowercase hex) of every rule's name, events, filters,
    /// command and timeout: what a confirmation covers. Comments and layout
    /// do not change it.
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let canonical = serde_json::to_vec(&self.rules).unwrap_or_default();
        format!("{:x}", Sha256::digest(canonical))
    }

    /// What the user confirms, one line each: the file, then per rule its
    /// name, events, filters, time limit and exact command (as a JSON array,
    /// so quotes, spaces and control characters stay visible).
    pub fn listing(&self) -> Vec<Localized> {
        let path = self.path.display().to_string();
        let mut lines = vec![Localized::with_param(
            "hooks.confirm.header",
            "path",
            path.as_str(),
            "The hooks in {path} run these commands when the listed events happen:",
        )];
        for (index, rule) in self.rules.iter().enumerate() {
            let index = index + 1;
            let name = rule.name.as_str();
            lines.push(Localized::with_params(
                "hooks.confirm.rule",
                params([("index", index.into()), ("name", name.into())]),
                "{index}. {name}",
            ));
            let events = rule
                .events
                .iter()
                .map(|kind| kind.name())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(Localized::with_param(
                "hooks.confirm.events",
                "events",
                events.as_str(),
                "events: {events}",
            ));
            let mut filters = Vec::new();
            if let Some(provider) = rule.provider {
                filters.push(format!("provider = {}", quoted(provider.slug())));
            }
            if let Some(account) = &rule.account {
                filters.push(format!("account = {}", quoted(account)));
            }
            if let Some(id) = &rule.account_id {
                filters.push(format!("account_id = {}", quoted(id)));
            }
            if let Some(min) = rule.min_threshold {
                filters.push(format!("min_threshold = {min}"));
            }
            if rule.quiet == QuietMode::Skip {
                filters.push(format!("quiet = {}", quoted(rule.quiet.name())));
            }
            if !filters.is_empty() {
                let filters = filters.join(", ");
                lines.push(Localized::with_param(
                    "hooks.confirm.filters",
                    "filters",
                    filters.as_str(),
                    "filters: {filters}",
                ));
            }
            lines.push(Localized::with_param(
                "hooks.confirm.timeout",
                "seconds",
                rule.timeout_seconds,
                "time limit: {seconds} s",
            ));
            let command = serde_json::to_string(&rule.command).unwrap_or_default();
            lines.push(Localized::with_param(
                "hooks.confirm.command",
                "command",
                command.as_str(),
                "command: {command}",
            ));
        }
        lines
    }
}

/// The rules of `text` as written; an error names the line, never echoes
/// the file.
fn parse(text: &str) -> Result<Vec<RuleDraft>, Localized> {
    toml::from_str::<RawFile>(text)
        .map(|raw| raw.hooks)
        .map_err(|error| {
            let at = error.span().map_or(0, |span| span.start.min(text.len()));
            let line = text.as_bytes()[..at]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                + 1;
            Localized::with_param(
                "hooks.error.toml",
                "line",
                line,
                "Invalid hooks.toml at line {line}: each [[hooks]] accepts name, events, provider, account, min_threshold, command (a list of arguments), timeout_seconds and quiet.",
            )
        })
}

/// PRD 0013 RF4: `hooks.toml` as an editor shows it: the rules as written
/// and whether they are valid, plus the revision a save must still find.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    path: PathBuf,
    revision: Option<String>,
    drafts: Option<Vec<RuleDraft>>,
    hooks: Result<Hooks, Localized>,
}

impl Document {
    /// The file at `path` as it is now; a missing file is an empty one
    /// (no revision, no rules, [`Self::hooks`] says it is missing).
    pub fn read(path: &Path) -> Self {
        let shown = path.display().to_string();
        let unreadable = || {
            Localized::with_param(
                "hooks.error.unreadable",
                "path",
                shown.as_str(),
                "Could not read {path} (a UTF-8 text file).",
            )
        };
        let (revision, text) = match std::fs::read(path) {
            Ok(bytes) => (Some(revision_of(&bytes)), String::from_utf8(bytes).ok()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Self {
                    path: path.to_path_buf(),
                    revision: None,
                    drafts: Some(Vec::new()),
                    hooks: Err(Localized::with_param(
                        "hooks.error.missing",
                        "path",
                        shown.as_str(),
                        "No hooks file at {path}.",
                    )),
                };
            }
            Err(_) => (None, None),
        };
        let Some(text) = text else {
            return Self {
                path: path.to_path_buf(),
                revision,
                drafts: None,
                hooks: Err(unreadable()),
            };
        };
        let drafts = parse(&text);
        Self {
            path: path.to_path_buf(),
            revision,
            hooks: drafts
                .clone()
                .and_then(|drafts| Hooks::from_drafts(path.to_path_buf(), drafts)),
            drafts: drafts.ok(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// SHA-256 (lowercase hex) of the bytes read; `None` without a file.
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    /// The rules as written, valid or not; `None` when the file is not
    /// TOML the editor can show (fix it by hand first).
    pub fn drafts(&self) -> Option<&[RuleDraft]> {
        self.drafts.as_deref()
    }

    /// The validated rules, or why they are not usable.
    pub fn hooks(&self) -> Result<&Hooks, &Localized> {
        self.hooks.as_ref()
    }

    pub fn into_hooks(self) -> Result<Hooks, Localized> {
        self.hooks
    }

    /// Replace the rules of the file at `path` with `drafts`, validated as
    /// the file is, when it still is the one read at `revision` (`None`:
    /// there was no file). Comments of a rule kept under the same name
    /// survive, as does the rest of a rule's layout. Returns the file as
    /// written. Commands are written exactly as given, never changed.
    pub fn save(
        path: &Path,
        revision: Option<&str>,
        drafts: Vec<RuleDraft>,
    ) -> Result<Self, Localized> {
        let hooks = Hooks::from_drafts(path.to_path_buf(), drafts.clone())?;
        let current = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err(unwritable(path)),
        };
        if current.as_deref().map(revision_of).as_deref() != revision {
            return Err(changed());
        }
        let existing = current.and_then(|bytes| String::from_utf8(bytes).ok());
        let text = render(existing.as_deref(), &drafts)
            .filter(|text| Hooks::from_toml(path.to_path_buf(), text).as_ref() == Ok(&hooks))
            .ok_or_else(|| unwritable(path))?;
        write_atomically(path, text.as_bytes()).map_err(|_| unwritable(path))?;
        Ok(Self::read(path))
    }
}

fn revision_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// `drafts` as `[[hooks]]` tables over `existing` (when it is a hooks
/// file `toml_edit` reads): a rule found there by name keeps its table,
/// with its comments, and each kept key its decoration.
fn render(existing: Option<&str>, drafts: &[RuleDraft]) -> Option<String> {
    use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};
    let mut doc = existing
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .filter(|doc| doc.get("hooks").is_none_or(Item::is_array_of_tables))
        .unwrap_or_default();
    let old: Vec<Table> = doc
        .get("hooks")
        .and_then(Item::as_array_of_tables)
        .map(|tables| tables.iter().cloned().collect())
        .unwrap_or_default();
    let mut tables = ArrayOfTables::new();
    for draft in drafts {
        let fresh = toml_edit::ser::to_document(draft).ok()?;
        let mut table = old
            .iter()
            .find(|table| {
                table.get("name").and_then(Item::as_str).map(str::trim) == Some(draft.name.trim())
            })
            .cloned()
            .unwrap_or_default();
        let gone: Vec<String> = table
            .iter()
            .map(|(key, _)| key.to_owned())
            .filter(|key| !fresh.contains_key(key))
            .collect();
        for key in gone {
            table.remove(&key);
        }
        for (key, item) in fresh.iter() {
            let value = item.as_value()?.clone();
            match table.get_mut(key).and_then(Item::as_value_mut) {
                Some(kept) => {
                    let decor = kept.decor().clone();
                    *kept = value;
                    *kept.decor_mut() = decor;
                }
                None => {
                    table.insert(key, Item::Value(value));
                }
            }
        }
        tables.push(table);
    }
    doc.insert("hooks", Item::ArrayOfTables(tables));
    Some(doc.to_string())
}

/// Replace `path` with `bytes` through a temporary file in its directory
/// (created private: 0600 on Unix).
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// The rules on disk are not the ones the user was shown: a save over a
/// file edited meanwhile, or a confirmation of rules that changed since
/// they were listed.
pub fn changed() -> Localized {
    Localized::fixed(
        "hooks.error.changed",
        "hooks.toml changed since it was shown; review the rules again.",
    )
}

fn unwritable(path: &Path) -> Localized {
    let path = path.display().to_string();
    Localized::with_param(
        "hooks.error.unwritable",
        "path",
        path.as_str(),
        "Could not write {path}.",
    )
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn rule_error(id: &'static str, name: &str, fallback: &'static str) -> Localized {
    Localized::with_param(id, "name", name, fallback)
}

fn validate(index: usize, raw: RuleDraft) -> Result<Rule, Localized> {
    let name = raw.name.trim().to_owned();
    if name.is_empty()
        || name.chars().count() > NAME_MAX_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(Localized::with_params(
            "hooks.error.name",
            params([("index", index.into()), ("max", NAME_MAX_CHARS.into())]),
            "Rule {index} needs a name of 1 to {max} characters, without control characters.",
        ));
    }
    if raw.events.is_empty() {
        return Err(rule_error(
            "hooks.error.no_events",
            &name,
            "Rule \"{name}\" lists no event.",
        ));
    }
    let mut events = Vec::with_capacity(raw.events.len());
    for event in &raw.events {
        let Some(kind) = EventKind::from_name(event.trim()) else {
            let known = EventKind::ALL.map(EventKind::name).join(", ");
            return Err(Localized::with_params(
                "hooks.error.unknown_event",
                params([
                    ("name", name.as_str().into()),
                    ("event", event.as_str().into()),
                    ("known", known.as_str().into()),
                ]),
                "Rule \"{name}\" names the unknown event \"{event}\". Valid events: {known}.",
            ));
        };
        if !events.contains(&kind) {
            events.push(kind);
        }
    }
    let provider = match raw.provider {
        None => None,
        // Every serialized provider is accepted, `extension` included: its
        // accounts exist, so a rule must be able to filter on them.
        Some(slug) => Some(Provider::from_slug(&slug).ok_or_else(|| {
            Localized::with_params(
                "hooks.error.provider",
                params([
                    ("name", name.as_str().into()),
                    ("provider", slug.as_str().into()),
                ]),
                "Rule \"{name}\" filters by the unknown provider \"{provider}\" (use the identifier, as in cuotai --list-providers).",
            )
        })?),
    };
    if raw
        .account
        .as_ref()
        .is_some_and(|account| account.trim().is_empty())
    {
        return Err(rule_error(
            "hooks.error.account",
            &name,
            "Rule \"{name}\" has an empty account filter.",
        ));
    }
    let account_id = match &raw.account_id {
        None => None,
        Some(id) => Some(
            crate::AccountId::parse(id.trim())
                .map(|id| id.as_str().to_owned())
                .ok_or_else(|| {
                    rule_error(
                        "hooks.error.account_id",
                        &name,
                        "Rule \"{name}\" has an account_id that is not a valid account id (like acc_0123456789ab, shown by cuotai --json-v1).",
                    )
                })?,
        ),
    };
    let min_threshold = match raw.min_threshold {
        None => None,
        Some(min) => Some(
            u8::try_from(min)
                .ok()
                .filter(|min| (1..=100).contains(min))
                .ok_or_else(|| {
                    rule_error(
                        "hooks.error.min_threshold",
                        &name,
                        "The min_threshold of rule \"{name}\" must be between 1 and 100.",
                    )
                })?,
        ),
    };
    if !valid_command(&raw.command) {
        return Err(rule_error(
            "hooks.error.command",
            &name,
            "The command of rule \"{name}\" must be a list of arguments whose first item is an absolute path or a name looked up in PATH, such as [\"/path/script\", \"--flag\"].",
        ));
    }
    let timeout_seconds = match raw.timeout_seconds {
        None => DEFAULT_TIMEOUT_SECONDS,
        Some(seconds) => u64::try_from(seconds)
            .ok()
            .filter(|seconds| (1..=MAX_TIMEOUT_SECONDS).contains(seconds))
            .ok_or_else(|| {
                Localized::with_params(
                    "hooks.error.timeout",
                    params([
                        ("name", name.as_str().into()),
                        ("max", MAX_TIMEOUT_SECONDS.into()),
                    ]),
                    "The timeout_seconds of rule \"{name}\" must be between 1 and {max}.",
                )
            })?,
    };
    let quiet = match raw.quiet.as_deref() {
        None => QuietMode::Run,
        Some(mode) => QuietMode::from_name(mode).ok_or_else(|| {
            rule_error(
                "hooks.error.quiet",
                &name,
                "The quiet of rule \"{name}\" must be \"run\" or \"skip\".",
            )
        })?,
    };
    Ok(Rule {
        name,
        events,
        provider,
        account: raw.account,
        account_id,
        min_threshold,
        command: raw.command,
        timeout_seconds,
        quiet,
    })
}

/// A non-empty argument vector without NUL whose program is absolute or a
/// bare name (a relative path would depend on the working directory).
fn valid_command(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let path = Path::new(program);
    let bare = !program.contains(['/', '\\'])
        && matches!(
            path.components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        );
    (path.is_absolute() || bare) && command.iter().all(|arg| !arg.contains('\0'))
}

/// The confirmations, by `hooks.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ConsentRecord {
    /// [`ConfigIdentity::digest`] of the file → [`Hooks::digest`] confirmed.
    #[serde(default)]
    confirmed: BTreeMap<String, String>,
}

/// Where confirmations are kept; injectable so tests never touch OS dirs.
#[derive(Debug, Clone)]
pub struct Consent {
    file: PathBuf,
}

/// The word that confirms the listing ([`Hooks::listing`]) in a terminal.
pub fn confirm_answer() -> Localized {
    Localized::fixed("hooks.confirm.answer", "yes")
}

/// The question after the listing, asking for [`confirm_answer`].
pub fn confirm_question(answer: &str) -> Localized {
    Localized::with_param(
        "hooks.confirm.question",
        "answer",
        answer,
        "To run them with --watch, type \"{answer}\" and press Enter (any other answer declines):",
    )
}

/// The rules of `hooks` are not the ones confirmed, and nobody is there to
/// confirm them.
pub fn not_confirmed(hooks: &Hooks) -> Localized {
    let path = hooks.path.display().to_string();
    Localized::with_param(
        "hooks.error.not_confirmed",
        "path",
        path.as_str(),
        "The hooks in {path} were not confirmed yet, or changed since they were confirmed; review and confirm their commands in a terminal with cuotai --watch --hooks or in the desktop app's Settings → Integration.",
    )
}

/// The user answered anything but [`confirm_answer`].
pub fn declined() -> Localized {
    Localized::fixed(
        "hooks.error.declined",
        "Hooks not confirmed; nothing was run.",
    )
}

/// [`Runner::submit`] dropped `count` queued runs.
pub fn dropped(count: usize) -> Localized {
    Localized::plural(
        "hooks.warning.dropped",
        params([("count", count.into())]),
        "Hooks queue full: {count} oldest pending run dropped.",
        "Hooks queue full: {count} oldest pending runs dropped.",
    )
}

/// A run's line could not be written to the log at `path`.
pub fn log_unwritable(path: &Path) -> Localized {
    let path = path.display().to_string();
    Localized::with_param(
        "hooks.warning.log",
        "path",
        path.as_str(),
        "Could not write the hooks run log at {path}.",
    )
}

/// Rules the user confirmed exactly as they are: the only way to start a
/// [`Runner`].
#[derive(Debug, Clone)]
pub struct Confirmed(Hooks);

impl Confirmed {
    pub fn hooks(&self) -> &Hooks {
        &self.0
    }
}

impl Consent {
    pub fn at(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// `<OS local data dir>/cuotai/hooks/consent.json`; without a local
    /// data directory nothing can be confirmed.
    pub fn in_data_dir() -> Result<Self, Localized> {
        dirs::data_local_dir()
            .map(|dir| Self::at(dir.join("cuotai").join("hooks").join("consent.json")))
            .ok_or_else(consent_storage)
    }

    /// `hooks` confirmed when its rules are the ones last confirmed for its
    /// file; otherwise `hooks` back, still to confirm.
    pub fn check(&self, hooks: Hooks) -> Result<Confirmed, Hooks> {
        let confirmed =
            key(&hooks).is_some_and(|key| self.read().confirmed.get(&key) == Some(&hooks.digest()));
        if confirmed {
            Ok(Confirmed(hooks))
        } else {
            Err(hooks)
        }
    }

    /// Record that the user confirmed `hooks` as they are now.
    pub fn confirm(&self, hooks: Hooks) -> Result<Confirmed, Localized> {
        let key = key(&hooks).ok_or_else(consent_storage)?;
        let mut record = self.read();
        record.confirmed.insert(key, hooks.digest());
        self.write(&record).map_err(|_| consent_storage())?;
        Ok(Confirmed(hooks))
    }

    /// An unreadable or damaged record holds no confirmation.
    fn read(&self) -> ConsentRecord {
        std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, record: &ConsentRecord) -> io::Result<()> {
        write_atomically(&self.file, &serde_json::to_vec(record)?)
    }
}

/// Confirmations cannot be recorded (no local data directory, or it
/// cannot be written).
pub fn consent_storage() -> Localized {
    Localized::fixed(
        "hooks.error.consent_storage",
        "Could not record the hooks confirmation in the local data directory.",
    )
}

fn key(hooks: &Hooks) -> Option<String> {
    ConfigIdentity::of_path(&hooks.path)
        .ok()
        .map(|identity| identity.digest())
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The process exited with [`Run::exit_code`].
    Exited,
    /// The process ended without an exit code (a signal).
    Terminated,
    /// The time limit ran out and the process was killed.
    TimedOut,
    /// The program could not be started (missing, not executable).
    NotStarted,
}

/// One run of one rule for one event.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub rule: String,
    pub event: EventKind,
    pub started_at: DateTime<Utc>,
    pub duration: Duration,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    /// stdout and stderr as they arrived, up to [`OUTPUT_LIMIT`] bytes.
    pub output: String,
    /// More output was written than [`OUTPUT_LIMIT`].
    pub truncated: bool,
}

impl Run {
    /// Exited with code 0.
    pub fn succeeded(&self) -> bool {
        self.outcome == Outcome::Exited && self.exit_code == Some(0)
    }

    /// `hooks.run.<case>`: one line for people.
    pub fn summary(&self) -> Localized {
        self.logged().summary()
    }

    /// What the [`RunLog`] keeps of it.
    pub fn logged(&self) -> LoggedRun {
        LoggedRun {
            at: self.started_at,
            rule: self.rule.clone(),
            event: self.event,
            duration_ms: u64::try_from(self.duration.as_millis()).unwrap_or(u64::MAX),
            outcome: self.outcome,
            exit_code: self.exit_code,
        }
    }
}

/// One line of the [`RunLog`]: a run without its payload or output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedRun {
    /// When it started.
    pub at: DateTime<Utc>,
    pub rule: String,
    pub event: EventKind,
    pub duration_ms: u64,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
}

impl LoggedRun {
    /// `hooks.run.<case>`: one line for people.
    pub fn summary(&self) -> Localized {
        let (rule, event, ms) = (self.rule.as_str(), self.event.name(), self.duration_ms);
        let common = || {
            params([
                ("rule", rule.into()),
                ("event", event.into()),
                ("ms", ms.into()),
            ])
        };
        match (self.outcome, self.exit_code) {
            (Outcome::Exited, Some(0)) => Localized::with_params(
                "hooks.run.succeeded",
                common(),
                "{rule} ({event}): done in {ms} ms",
            ),
            (Outcome::Exited, code) => {
                let code = code.map_or_else(String::new, |code| code.to_string());
                let mut values = common();
                values.insert("code".into(), ParamValue::from(code.as_str()));
                Localized::with_params(
                    "hooks.run.failed",
                    values,
                    "{rule} ({event}): exited with code {code} after {ms} ms",
                )
            }
            (Outcome::Terminated, _) => Localized::with_params(
                "hooks.run.terminated",
                common(),
                "{rule} ({event}): ended without an exit code after {ms} ms",
            ),
            (Outcome::TimedOut, _) => Localized::with_params(
                "hooks.run.timed_out",
                common(),
                "{rule} ({event}): ran out of time and was stopped after {ms} ms",
            ),
            (Outcome::NotStarted, _) => Localized::with_params(
                "hooks.run.not_started",
                common(),
                "{rule} ({event}): the command could not be started",
            ),
        }
    }
}

/// The local record of runs: one JSON line each, without the payload or
/// the output.
#[derive(Debug)]
pub struct RunLog {
    path: PathBuf,
    lock: Mutex<()>,
}

impl RunLog {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// `<OS local data dir>/cuotai/hooks/runs.log`.
    pub fn in_data_dir() -> Option<Self> {
        dirs::data_local_dir()
            .map(|dir| Self::at(dir.join("cuotai").join("hooks").join("runs.log")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `runs.log.1`, where the log moves when it rotates.
    fn older(&self) -> PathBuf {
        let mut older = self.path.clone().into_os_string();
        older.push(".1");
        older.into()
    }

    /// Append `run` (creating the file privately: 0600 on Unix). Past
    /// 256 KiB the log moves to `runs.log.1`, replacing the older one.
    pub fn append(&self, run: &Run) -> io::Result<()> {
        let mut line = serde_json::to_vec(&run.logged())?;
        line.push(b'\n');
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(dir) = self.path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::metadata(&self.path).is_ok_and(|meta| meta.len() >= LOG_ROTATE_BYTES) {
            std::fs::rename(&self.path, self.older())?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&self.path)?.write_all(&line)
    }

    /// The last `limit` runs, newest first, from this log and the rotated
    /// one (any process's: `cuotai --watch --hooks` and the desktop share
    /// it). Lines that do not read as a run are skipped; no log is no run.
    pub fn latest(&self, limit: usize) -> Vec<LoggedRun> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut runs = Vec::new();
        for path in [self.path.clone(), self.older()] {
            if runs.len() >= limit {
                break;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            runs.extend(
                text.lines()
                    .rev()
                    .filter_map(|line| serde_json::from_str::<LoggedRun>(line).ok()),
            );
        }
        runs.truncate(limit);
        runs
    }
}

/// `<OS local data dir>/cuotai/hooks/work`: the private working directory
/// of every hook, created `0700` on Unix when a hook first runs. `None`
/// without a local data directory, and then no hook starts.
pub fn work_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai").join("hooks").join("work"))
}

/// Called after each run with the run and, when its log line could not be
/// written, the error.
pub type OnRun = Arc<dyn Fn(&Run, Option<&io::Error>) + Send + Sync>;

struct Job {
    rule: usize,
    event: EventKind,
    payload: Arc<[u8]>,
    vars: Arc<[(&'static str, String)]>,
}

struct Shared {
    queue: Mutex<VecDeque<Job>>,
    wake: Notify,
}

struct Context {
    rules: Vec<Rule>,
    inherited: Vec<(OsString, OsString)>,
    /// The private working directory ([`work_dir`]).
    work_dir: Option<PathBuf>,
    log: Option<RunLog>,
    on_run: OnRun,
}

/// The confirmed rules, run on the events given to [`Runner::submit`].
/// Must be started inside a Tokio runtime; dropping it discards the queue
/// and kills the hooks still running.
pub struct Runner {
    rules: Vec<Rule>,
    shared: Arc<Shared>,
    dispatcher: JoinHandle<()>,
}

impl Runner {
    pub fn start(hooks: Confirmed, log: Option<RunLog>, on_run: OnRun) -> Self {
        Self::start_with_env(hooks, log, on_run, std::env::vars_os(), work_dir())
    }

    /// [`Self::start`] inheriting [`INHERITED_ENV`] from `parent` instead
    /// of the process environment, and running in `work_dir`.
    fn start_with_env(
        hooks: Confirmed,
        log: Option<RunLog>,
        on_run: OnRun,
        parent: impl IntoIterator<Item = (OsString, OsString)>,
        work_dir: Option<PathBuf>,
    ) -> Self {
        let rules = hooks.0.rules;
        let inherited = parent
            .into_iter()
            .filter(|(name, _)| INHERITED_ENV.iter().any(|allowed| name == allowed))
            .collect();
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
        });
        let context = Arc::new(Context {
            rules: rules.clone(),
            inherited,
            work_dir,
            log,
            on_run,
        });
        let dispatcher = tokio::spawn(dispatch(Arc::clone(&shared), context));
        Self {
            rules,
            shared,
            dispatcher,
        }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Queue a run of every rule that matches `event`, without waiting for
    /// any. Returns how many queued runs were dropped (the oldest first)
    /// to keep the queue within [`QUEUE_CAPACITY`].
    pub fn submit(&self, event: &Event) -> usize {
        self.submit_in(event, false)
    }

    /// [`Self::submit`] for an event that occurred `in_quiet` (inside a
    /// quiet window, PRD 0032 RF5.1): the rules with `quiet = "skip"` do
    /// not run for it.
    pub fn submit_in(&self, event: &Event, in_quiet: bool) -> usize {
        let matching: Vec<usize> = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.matches(event))
            .filter(|(_, rule)| !(in_quiet && rule.quiet == QuietMode::Skip))
            .map(|(index, _)| index)
            .collect();
        self.enqueue(matching, event)
    }

    /// Queue a run of the rule named `rule` alone for `event`, whatever its
    /// events and filters (the desktop's test with [`Rule::sample`]).
    /// `None` when no confirmed rule has that name; otherwise as
    /// [`Self::submit`].
    pub fn submit_to(&self, rule: &str, event: &Event) -> Option<usize> {
        let index = self.rules.iter().position(|known| known.name == rule)?;
        Some(self.enqueue(vec![index], event))
    }

    fn enqueue(&self, matching: Vec<usize>, event: &Event) -> usize {
        if matching.is_empty() {
            return 0;
        }
        let Ok(mut payload) = serde_json::to_vec(event) else {
            return 0;
        };
        payload.push(b'\n');
        let payload: Arc<[u8]> = payload.into();
        let vars: Arc<[(&'static str, String)]> = Arc::new([
            ("CUOTAI_EVENT", event.event.name().to_owned()),
            ("CUOTAI_ACCOUNT", event.account_name.clone()),
            ("CUOTAI_ACCOUNT_ID", event.account_id.clone()),
            ("CUOTAI_PROVIDER", event.provider.slug().to_owned()),
            (
                "CUOTAI_METRIC",
                event
                    .metric
                    .as_ref()
                    .map_or_else(String::new, |metric| metric.label.clone()),
            ),
        ]);
        let mut dropped = 0;
        {
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for rule in matching {
                queue.push_back(Job {
                    rule,
                    event: event.event,
                    payload: Arc::clone(&payload),
                    vars: Arc::clone(&vars),
                });
                if queue.len() > QUEUE_CAPACITY {
                    queue.pop_front();
                    dropped += 1;
                }
            }
        }
        self.shared.wake.notify_one();
        dropped
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.dispatcher.abort();
    }
}

/// Start queued runs while fewer than [`MAX_RUNNING`] are running. The
/// running ones live in its `JoinSet`, so aborting it kills them.
async fn dispatch(shared: Arc<Shared>, context: Arc<Context>) {
    let slots = Arc::new(Semaphore::new(MAX_RUNNING));
    let mut running = JoinSet::new();
    loop {
        let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
            return;
        };
        let job = loop {
            let next = shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            if let Some(job) = next {
                break job;
            }
            while running.try_join_next().is_some() {}
            shared.wake.notified().await;
        };
        let context = Arc::clone(&context);
        running.spawn(async move {
            let run = execute(&context, &job).await;
            let logged = context.log.as_ref().map(|log| log.append(&run));
            (context.on_run)(
                &run,
                logged.as_ref().and_then(|result| result.as_ref().err()),
            );
            drop(slot);
        });
        while running.try_join_next().is_some() {}
    }
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Captured {
    fn push(&mut self, chunk: &[u8]) {
        let room = OUTPUT_LIMIT - self.bytes.len();
        if chunk.len() > room {
            self.truncated = true;
        }
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
}

enum Waited {
    Exited(io::Result<std::process::ExitStatus>),
    TimedOut,
}

async fn execute(context: &Context, job: &Job) -> Run {
    let rule = &context.rules[job.rule];
    let started_at = Utc::now();
    let started = Instant::now();
    let run = |outcome, exit_code, captured: Captured| Run {
        rule: rule.name.clone(),
        event: job.event,
        started_at,
        duration: started.elapsed(),
        outcome,
        exit_code,
        output: String::from_utf8_lossy(&captured.bytes).into_owned(),
        truncated: captured.truncated,
    };
    // A private working directory, never the shared temporary one: no hook
    // starts without it.
    let Some(work_dir) = context
        .work_dir
        .as_deref()
        .filter(|dir| crate::private_fs::ensure_dir(dir).is_ok())
    else {
        return run(Outcome::NotStarted, None, Captured::default());
    };
    let mut command = tokio::process::Command::new(&rule.command[0]);
    command
        .args(&rule.command[1..])
        .env_clear()
        .envs(context.inherited.iter().map(|(name, value)| (name, value)))
        .envs(job.vars.iter().map(|(name, value)| (name, value)))
        .current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // The hook leads its own process group, so its whole tree dies with it.
    process_tree::own_group(&mut command, false);
    // Windows processes need SystemRoot to start at all.
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let Ok(mut child) = command.spawn() else {
        return run(Outcome::NotStarted, None, Captured::default());
    };
    // Dropped with this future (a stopped runner), it kills the tree too.
    let tree = Tree::new(&child);
    let deadline = tokio::time::Instant::now() + rule.timeout();
    let captured = Mutex::new(Captured::default());
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let io = async {
        tokio::join!(
            async {
                if let Some(mut stdin) = stdin {
                    // A hook that does not read its stdin is no error.
                    let _ = stdin.write_all(&job.payload).await;
                }
            },
            drain(stdout, &captured),
            drain(stderr, &captured),
        );
    };
    let mut io = std::pin::pin!(io);
    let mut io_done = false;
    let waited = loop {
        tokio::select! {
            status = child.wait() => break Waited::Exited(status),
            () = &mut io, if !io_done => io_done = true,
            () = tokio::time::sleep_until(deadline) => break Waited::TimedOut,
        }
    };
    let (outcome, exit_code) = match waited {
        Waited::Exited(Ok(status)) => {
            // Processes the hook left behind die with it: they would hold
            // its pipes and run outside the `MAX_RUNNING` limit.
            tree.kill();
            if !io_done {
                // Output still arriving is read until the time limit only.
                let _ = tokio::time::timeout_at(deadline, &mut io).await;
            }
            match status.code() {
                Some(code) => (Outcome::Exited, Some(code)),
                None => (Outcome::Terminated, None),
            }
        }
        Waited::Exited(Err(_)) => {
            tree.kill();
            let _ = child.kill().await;
            (Outcome::Terminated, None)
        }
        Waited::TimedOut => {
            tree.kill();
            let _ = child.kill().await;
            (Outcome::TimedOut, None)
        }
    };
    let duration = started.elapsed();
    let captured = std::mem::take(&mut *captured.lock().unwrap_or_else(PoisonError::into_inner));
    Run {
        duration,
        ..run(outcome, exit_code, captured)
    }
}

/// Read `pipe` to its end, keeping what fits in [`OUTPUT_LIMIT`].
async fn drain(pipe: Option<impl AsyncRead + Unpin>, captured: &Mutex<Captured>) {
    let Some(mut pipe) = pipe else {
        return;
    };
    let mut buffer = [0u8; 4096];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => captured
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(&buffer[..read]),
        }
    }
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
