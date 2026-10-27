//! Bridge between `cuotai-core` and the UI. It holds no business rules: validation,
//! account editing, TOML writes, metric classification and scheduling
//! come from the core; here there is only state, DTOs and orchestration.
use crate::hooks::{HooksBadge, HooksView, Integration as Hooks, HOOKS_EVENT};
use crate::local_server::{Feed, FeedSlot, Integration, ServeView};
use crate::notify::{Notices, SystemNotifier};
use crate::settings::{ServePrefs, WidgetPrefs};
use crate::widgets::{self, WidgetsView};
use cuotai_core::{
    account_edit::{
        self, credential_help, default_env, field_descriptors, visible_field_indices, AccountDraft,
        CredentialKind, DraftField, DraftValue, FieldDescriptorKind, FieldVisibility,
    },
    advice, alerts,
    config_file::{ConfigFile, Discovery, Locations, PathSource, ResolvedPath},
    export::v1::{Export, Options as ExportOptions},
    freshness::{assess, clock_adjusted, Freshness},
    history::{
        Chart, ExportFormat, HistoryError, HistoryRange, HistoryStore, MetricHistory, Trend,
        WindowUsage,
    },
    identity::{account_identity, AccountIdentity},
    incidents::{IncidentMonitor, ProviderStatus},
    login::{self, CompletedLogin, LoginEvent, LoginRequest},
    metrics::{
        binding, binding_where, classify_metric, classify_report, cmp_binding, AccountClass,
        Binding, MetricState,
    },
    models::{self, ModelError, ModelState},
    oauth::{vault_index, OAuthSession},
    pace::{self, Pace, Point},
    refresh::{Edit, RefreshCycle},
    resets::TimelineRange,
    stats::{StatsRange, StatsSummary},
    tray, user_session, Config, EnvironmentCredentials, Metric, MetricKind, MetricWindow, Monitor,
    Provider, Report, Status,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    future::Future,
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{async_runtime::Mutex, AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

fn l10n_to_json(note: &cuotai_core::text::Localized) -> String {
    serde_json::to_string(note).unwrap_or_else(|_| note.fallback.to_string())
}

/// Create (or truncate) the export file at `path`, readable by its owner
/// only on Unix (even when it already existed): it holds the usage history.
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(file)
}

/// Encapsulate an `EditError` for the frontend: the core's RF8
/// `Localized` plus an optional field token the dialog uses to
/// highlight the offending row (`name`, `credential`, `interval`,
/// `params.<key>`, etc.). JSON shape:
///
/// `{ id, params, fallback, field? }`
///
/// `field` is omitted when the error is document-wide (no specific
/// input) or when the variant isn't a `Validation` (e.g. `Conflict`,
/// `Io`, `Unsupported`). Returned as a JSON string so it can cross
/// the Tauri boundary the same way the rest of the errors already
/// do — the frontend parses it in `account-dialog.js`'s `errText`.
#[derive(serde::Serialize)]
struct EditErrorEnvelope<'a> {
    #[serde(flatten)]
    note: &'a cuotai_core::text::Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
}

fn edit_error_to_json(error: &cuotai_core::edit_error::EditError) -> String {
    let envelope = EditErrorEnvelope {
        note: &error.l10n(),
        field: error.field(),
    };
    serde_json::to_string(&envelope).unwrap_or_else(|_| error.message())
}

fn refresh_edit_error(
    error: &cuotai_core::refresh::EditError,
    count: usize,
) -> cuotai_core::text::Localized {
    use cuotai_core::text::{params, Localized};
    match error {
        cuotai_core::refresh::EditError::Index(index) => Localized::with_params(
            "edit_error.index_out_of_range",
            params([("index", (*index).into()), ("count", count.into())]),
            "Index {index} out of range ({count}).",
        ),
        cuotai_core::refresh::EditError::UnexpectedDiff => Localized::with_param(
            "edit_error.refresh_cycle_diff",
            "detail",
            error.to_string(),
            "The new account layout was rejected by the refresh cycle diff guard: {detail}.",
        ),
    }
}

/// The frontend's own failure together with the failure of the consent
/// rollback that should have undone it: both are reported, never dropped.
fn rollback_with(
    rollback: &Result<(), cuotai_core::text::Localized>,
    error: cuotai_core::text::Localized,
) -> String {
    match rollback {
        Ok(()) => l10n_to_json(&error),
        Err(conflict) => l10n_to_json(&combined_failure(&error, conflict)),
    }
}

/// The same combination for an `EditError` (already JSON-serializable).
fn rollback_edit(
    rollback: &Result<(), cuotai_core::text::Localized>,
    error: cuotai_core::edit_error::EditError,
) -> String {
    match rollback {
        Ok(()) => edit_error_to_json(&error),
        Err(conflict) => l10n_to_json(&combined_failure(&error.l10n(), conflict)),
    }
}

/// `extensions.action.rollback_failed`: the operation failed and so did its
/// rollback; the second sentence is the rollback's own reason.
fn combined_failure(
    error: &cuotai_core::text::Localized,
    rollback: &cuotai_core::text::Localized,
) -> cuotai_core::text::Localized {
    cuotai_core::text::Localized::fixed(
        "extensions.action.rollback_failed",
        format!("{} {}", error.fallback, rollback.fallback),
    )
}

/// The typed note of a `ConfigError`, as the rest of the backend reports it.
fn config_error_note(error: &cuotai_core::ConfigError) -> cuotai_core::text::Localized {
    cuotai_core::edit_error::config_error_note(error)
}

const TICK: Duration = Duration::from_secs(5);
/// Longest sleep of the reset notice timer while a notice is ahead.
const NOTICE_NAP: Duration = Duration::from_secs(60);
/// PRD 0011 RF4: how often the status loop looks at the sources. The core
/// queries one only when its 5-minute cache expired, so this only bounds
/// how soon a due source is read.
const STATUS_TICK: Duration = Duration::from_secs(30);
pub const EVENT: &str = "snapshot";

/// Login event channel (RF2): only the serializable core `LoginEvent` —
/// public authorization URLs, device-flow user_code and safe texts.
/// Tokens and callback URLs NEVER travel here. The envelope
/// carries the attempt's `attempt` so the frontend drops events
/// from a stale forwarder (a login already replaced by another).
pub const LOGIN_EVENT: &str = "login";

#[derive(Clone, Serialize)]
pub struct LoginEnvelope<'a> {
    pub attempt: u64,
    #[serde(flatten)]
    pub event: &'a login::LoginEvent,
}

/// Login in progress (RF2). The `CompletedLogin` (opaque in-memory token)
/// lives ONLY here: it is never serialized to the UI, survives the dialog
/// closing during the commit and is discarded without writing to the vault if the
/// user cancels before saving.
struct PendingLogin {
    /// Monotonic identifier: the forwarder task carries it and only mutates the
    /// `PendingLogin` if the current slot is still its own — this protects against a
    /// stale forwarder that finishes after the user has already started another login.
    attempt: u64,
    provider: Provider,
    /// Account being reauthenticated (RF2.8); `None` = new account.
    index: Option<usize>,
    /// Manual codes (e.g. MiMo) typed in the UI. The oneshot returns the core's
    /// IMMEDIATE verdict (`SubmitCodeError`: queue full, flow
    /// ended, no-code mode) so the UI keeps the code in the field;
    /// the validation itself arrives later through `LoginEvent`.
    code_tx: mpsc::Sender<(String, oneshot::Sender<Result<(), String>>)>,
    /// Cancels the forwarder task, which tears down the driver (no vault write).
    cancel_tx: Option<oneshot::Sender<()>>,
    /// The forwarder task finished (Finished/Failed): a new `login_start`
    /// can replace this pending login.
    done: bool,
    /// Present after `Finished` with a writable credential (oauth mode). The
    /// aws_sso/bailian_console modes end without a credential in the Cuotai vault.
    completed: Option<CompletedLogin>,
}

/// What the scheduler decided to do on this tick. Pulled out of the
/// inner lock so the match arm below can call async methods on `self`
/// without re-entering the mutex.
/// The first full round when the app opens. `RefreshCycle::new` schedules each account's first deadline
/// one interval ahead; waiting for that deadline left the screen on "querying" for minutes.
/// It only applies while no full round has been attempted: re-queries follow the per-account deadlines.
fn needs_initial_sweep(
    cycle: &RefreshCycle,
    tokens: &[Option<cuotai_core::refresh::Generation>],
) -> bool {
    cycle.snapshot().is_none()
        && !cycle.attempted_full()
        && tokens.iter().all(Option::is_none)
        && cycle.pending_indices().len() == cycle.len()
}

enum SchedulerPlan {
    /// Nothing to do this tick: no accounts, no pending slots, no due
    /// intervals.
    Idle,
    /// Initial sweep: every slot is pending and no full refresh is
    /// running. This is the only path that should ever do a full
    /// round of paid-provider queries.
    Full,
    /// Per-account refreshes driven by either an edit (pending slot)
    /// or a due periodic interval.
    Accounts(Vec<usize>),
}

struct Loaded {
    file: ConfigFile,
    monitor: Arc<Monitor>,
    cycle: RefreshCycle,
    /// Mirror of the cycle's `generation` counter; the inner mutex
    /// owns the value so concurrent `finish_*` calls can compare
    /// their token against the live one. The cycle is the source of
    /// truth for the generation (we read it via the cycle's own
    /// `begin` calls and forward the result), so this field mostly
    /// exists to detect "another edit landed while we were holding
    /// the lock" — if `Inner.generation` differs from
    /// `Loaded.generation`, an in-flight refresh's token is stale.
    generation: u64,
    /// Per-slot generation token for in-flight refreshes, mirroring
    /// the cycle's `in_flight` table. `None` means the slot is idle.
    /// Stored on `Loaded` so the inner mutex can quickly check "slot
    /// busy?" without going through the cycle.
    tokens: Vec<Option<cuotai_core::refresh::Generation>>,
    /// RF3 retry handles: `RemovedAccount`s retained after a removal
    /// that left the credential in the vault (`CredentialKept`) or
    /// whose vault delete failed (`PartialFailure`). The vault key
    /// changes when `install` rebuilds the live configuration, so we
    /// drop the entire list on reload/replacement to honour the
    /// invariant "a retry must not delete a credential now referenced
    /// again". The core guard still runs on every retry against the
    /// current config; the handle list is the UX primitive that lets
    /// the desktop offer a "Tentar de novo" button without depending
    /// on the removed account index (which no longer exists in TOML).
    pending_recovery: Vec<RecoveryEntry>,
    /// RF1.2: file last opened by the TUI, when it differs from this one.
    other_config: Option<String>,
}

/// RF3 recovery primitive stored in the desktop so the UI can retry
/// a vault purge against the *current* configuration. `handle` is an
/// opaque id the UI round-trips; `account` carries the
/// non-secret [`account_edit::RemovedAccount`] the core needs to
/// re-derive the vault key and run the shared-key guard.
#[derive(Debug, Clone)]
struct RecoveryEntry {
    handle: String,
    account: account_edit::RemovedAccount,
}

#[derive(Default)]
struct Inner {
    loaded: Option<Loaded>,
    load_error: Option<String>,
    session: Option<Arc<OAuthSession>>,
    /// Login RF2 in progress; at most one at a time (the dialog is modal).
    login: Option<PendingLogin>,
    /// Monotonic counter: each `login_start` gets a unique id that
    /// frontend and backend use to match `PendingLogin` with what the UI sees.
    next_login_attempt: u64,
    generation: u64,
    /// PRD 0006 RF5: bumped when the history was erased or recording
    /// switched, so open account details read it again.
    history_epoch: u64,
    /// PRD 0003: reset notice preferences, ledger and language.
    notices: Notices,
    /// PRD 0011 RF4.3: the status preference and cache, shared by every
    /// monitor this backend installs (a reload keeps both).
    incidents: IncidentMonitor,
    /// PRD 0012 RF5.2: the local server's state, for every view.
    serve: ServeView,
    /// PRD 0013 RF4: the hooks' state, for every view.
    hooks: HooksBadge,
    /// PRD 0036 RF2.2: the aggregator account whose Test connection last
    /// succeeded; saving an aggregator needs exactly this account.
    aggregator_tested: Option<cuotai_core::Account>,
}

#[derive(Default)]
pub struct Backend {
    inner: Mutex<Inner>,
    /// Wakes the reset notice timer (PRD 0003 RF2.1) after a refresh, a
    /// configuration change or a preference change.
    notice_nudge: tokio::sync::Notify,
    /// PRD 0011 RF4: wakes the status loop when the preference changes.
    status_nudge: tokio::sync::Notify,
    /// PRD 0011 RF4.2: the tray icon currently shows the outage indicator.
    /// PRD 0012 RF5: the local server. Locked before `inner`, never after.
    serve: Mutex<Integration>,
    /// What the local server reads, published from `inner` while it runs.
    feed: FeedSlot,
    /// The local server is on: every state change publishes the feed.
    serving: AtomicBool,
    /// PRD 0013 RF4: the hooks. Locked before `inner`, never after.
    hooks: Mutex<Hooks>,
    /// The hooks' switch is on: every state change goes to them.
    hooks_on: AtomicBool,
    /// PRD 0026 RF3.2: fills the event journal from the same state changes
    /// while the history records. Never held together with `inner`.
    journal: Mutex<cuotai_core::watch::JournalTap>,
    /// PRD 0014 RF1.4: the widget snapshot publisher. Lock-free by design:
    /// a state change only marks it dirty and its own task writes.
    widgets: widgets::Publisher,
    /// PRD 0018 RF2.4: set before the app restarts to apply an update, so the
    /// background loops stop asking for provider data instead of being killed
    /// mid-request.
    stopping: AtomicBool,
}

// ---------- DTOs (read-only for the UI) ----------

#[derive(Serialize, Clone)]
pub struct MetricView {
    #[serde(flatten)]
    metric: Metric,
    /// Core classification, flattened: `state` ("unmeasured" | "exhausted" |
    /// "available") plus `no_cap` or `ratio`/`uncapped`.
    #[serde(flatten)]
    state: MetricState,
    /// PRD 0007 RF3.1: the core's pace (`pace::assess_reading` at the
    /// reading's `checked_at` and, PRD 0006, the metric's recorded points);
    /// `None` for ineligible metrics, for a reading that is not fresh (PRD
    /// 0024 FIX-11) and for the preserved reading.
    pace: Option<PaceView>,
    /// PRD 0024 RF4.3: while the projected-exhaustion conditions hold
    /// (whether or not the warning is on), the callout text and the outlook
    /// chip; `None` otherwise.
    forecast: Option<ForecastView>,
    /// PRD 0024 RF1.4: "Resets 19:30 · in 1h 35m" from the core, for a
    /// metric whose window resets in the future; `None` otherwise.
    resets_label: Option<cuotai_core::text::Localized>,
    /// PRD 0008 RF3.2/RF3.4: while usage alerts are on and the metric can
    /// alert, whether it is muted in the details and has an active alert.
    alert: Option<MetricAlertView>,
    /// PRD 0033 RF2.1: the window of this preserved or restored metric
    /// renewed after the reading, so it carries no value; the UI says
    /// "Renewed at 14:30; waiting for a reading" in the viewer's local time.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    expired: bool,
}

/// PRD 0024 RF4.3: the in-app forecast, from the core's texts.
#[derive(Serialize, Clone)]
pub struct ForecastView {
    /// "At this rate Session (5h) runs out at 18:53, 37 min before the reset".
    callout: cuotai_core::text::Localized,
    /// The outlook chip: "Runs out at 18:53".
    chip: cuotai_core::pace::Chip,
    /// The projected time, local ("18:53"), for the advice of the group.
    at: String,
}

fn forecast_view(
    metric: &Metric,
    checked_at: chrono::DateTime<chrono::Utc>,
    interval: Duration,
    now: chrono::DateTime<chrono::Utc>,
    recent: &[Point],
) -> Option<ForecastView> {
    let pace = alerts::forecast(metric, checked_at, interval, now, recent)?;
    Some(ForecastView {
        callout: alerts::forecast_text(&metric.label, &pace, now, &chrono::Local)?,
        chip: pace.outlook_chip(local_at)?,
        at: local_at(pace.projected_exhaustion_at?),
    })
}

#[derive(Serialize, Clone)]
pub struct MetricAlertView {
    muted: bool,
    active: bool,
    /// PRD 0032 RF8.2: the highest level (percent) this metric crossed while
    /// the app was closed and nobody was told; the card shows a marker.
    #[serde(skip_serializing_if = "Option::is_none")]
    missed: Option<u8>,
}

/// The core's `Pace` plus its texts: `label` (stage) and `outlook` (`None`
/// when exhausted), with `at` in local time and `margin` as `2h 10m`.
#[derive(Serialize, Clone)]
pub struct PaceView {
    #[serde(flatten)]
    pace: Pace,
    label: cuotai_core::text::Localized,
    outlook: Option<cuotai_core::text::Localized>,
    /// PRD 0024 RF1.3: the stage chip in both units; the interface picks
    /// one by the user's "Show pace as" choice and never converts.
    chips: PaceChips,
}

/// The core's stage chip in each [`cuotai_core::pace::PaceUnit`].
#[derive(Serialize, Clone)]
pub struct PaceChips {
    points: cuotai_core::pace::Chip,
    percent: cuotai_core::pace::Chip,
}

fn pace_view(pace: Pace) -> PaceView {
    use cuotai_core::pace::PaceUnit;
    PaceView {
        label: pace.stage_label(),
        outlook: pace.outlook_label(local_at, compact_span),
        chips: PaceChips {
            points: pace.chip(PaceUnit::Points),
            percent: pace.chip(PaceUnit::Percent),
        },
        pace,
    }
}

/// Local `HH:MM`, with the date when it is not today (as the TUI does).
fn local_at(at: chrono::DateTime<chrono::Utc>) -> String {
    let local = at.with_timezone(&chrono::Local);
    let today = local.date_naive() == chrono::Local::now().date_naive();
    local
        .format(if today { "%H:%M" } else { "%Y-%m-%d %H:%M" })
        .to_string()
}

/// `1d 4h`, `2h 10m`, `40m`: the UI's countdown style.
fn compact_span(span: chrono::TimeDelta) -> String {
    let minutes = span.num_minutes().max(0);
    let (days, hours, mins) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

#[derive(Serialize, Clone)]
pub struct ReportView {
    index: usize,
    account: String,
    provider: Provider,
    checked_at: chrono::DateTime<chrono::Utc>,
    status: Status,
    metrics: Vec<MetricView>,
    notes: Vec<String>,
    /// RF8: structured sidecar parallel to `notes` (same order, same
    /// length). The UI resolves `id` in the catalog and uses `fallback` (PT) as the
    /// fallback. `skip_serializing_if = "Vec::is_empty"` keeps the CLI/Snapshot
    /// JSON identical to pre-RF8 when there are no notes.
    notes_l10n: Vec<cuotai_core::text::Localized>,
    source: String,
    class: AccountClass,
    refresh_seconds: u64,
    /// Instant (Unix, ms) of this account's next scheduled query; `None` while it is being queried.
    next_refresh_at_ms: Option<u64>,
    uses_oauth: bool,
    identity: AccountIdentity,
    /// Models in this account's synced catalog (local cache, no network);
    /// `None` when never synced or without a listing.
    model_count: Option<usize>,
    /// PRD 0009 RF2: the core's binding metric (`index` points into `metrics`)
    /// and the account's position in the core's "most restrictive first" order
    /// (ties and accounts without a value keep the manual order).
    binding: Option<Binding>,
    binding_rank: usize,
    /// PRD 0010 RF3: the core's freshness of `checked_at` against the
    /// account's effective interval, and whether the clock went backwards
    /// since (age reads zero). The UI renders the relative age itself.
    freshness: Freshness,
    clock_adjusted: bool,
    /// PRD 0010 RF3.2: the core's last valid reading, only while the current
    /// report failed (`RefreshCycle::last_good_report`) or, restored from
    /// the history on opening (RF2.3), while no query answered yet.
    /// Display only.
    last_good: Option<LastGoodView>,
    /// No query of this account answered yet: the view only carries the
    /// restored `last_good` (status, metrics and notes are placeholders).
    pending: bool,
    /// PRD 0007 RF3.2: index (into `metrics`) of the most restrictive metric
    /// that has a pace (the core's binding rule restricted to them).
    pace_index: Option<usize>,
    /// PRD 0005 RF4: how the account's usage is read.
    access: ReportAccess,
    /// The core's stable `Account::key` (provider slug + name), which the
    /// per-account notice preference is keyed by (PRD 0003 RF3.2).
    account_key: String,
    /// PRD 0029 RF1: the stable account id, which the menu-bar selection is
    /// keyed by (PRD 0025 RF2.2).
    account_id: String,
    /// PRD 0022 MONO-4: the monogram override kept in `desktop.json` by
    /// account id; absent, the tiles derive the letters from the name.
    #[serde(skip_serializing_if = "Option::is_none")]
    monogram: Option<String>,
    /// PRD 0003 RF3.2: the account's bell (reset notices and, PRD 0008
    /// RF3.2, usage alerts on).
    notify: bool,
    /// PRD 0008 RF3.4: labels of the metrics with an active usage alert
    /// (empty when none); the card and the tray highlight the account.
    usage_alerts: Vec<String>,
    /// PRD 0011 RF4.1: the provider's public status, while status queries
    /// are on and the provider has a verified source.
    incident: Option<IncidentView>,
    /// PRD 0016 RF3.2: the known expiry of the web session the query used
    /// ("expires in N days" on the card), never its value.
    session_expiry: Option<user_session::SessionExpiry>,
    /// SES-01: the core offers "Paste a new session" (the vault session
    /// was rejected or is gone) on the card and in the details.
    paste_session: bool,
    /// PRD 0036 RF2.3/RF2.4: an aggregator's standing warnings (no password
    /// on a non-loopback instance, unencrypted private-network HTTP) for a
    /// persistent chip; absent when there is none.
    #[serde(skip_serializing_if = "cuotai_core::aggregators::ConnectionWarnings::is_empty")]
    connection_warnings: cuotai_core::aggregators::ConnectionWarnings,
    /// PRD 0033 RF3.2: the provider-stated time of the data, only when it
    /// lags `checked_at` by more than 15 minutes ("data as of 06:00").
    #[serde(skip_serializing_if = "Option::is_none")]
    data_as_of: Option<chrono::DateTime<chrono::Utc>>,
    /// PRD 0023 RF3: paid extra usage of this reading; `None` when the
    /// provider exposes no overage data.
    overage: Option<OverageView>,
    /// PRD 0036 RF8: for an aggregator account, its connections as views
    /// of their own (grouped by upstream; `index` is the aggregator's, so
    /// every action acts on it), and the instance's summary.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    connections: Vec<ReportView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aggregator: Option<AggregatorView>,
    /// PRD 0036 RF8: what a connection view adds; absent for accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    connection: Option<ConnectionView>,
    /// PRD 0024 RF7.1: the account's group of comparable accounts: its
    /// `group` value and, when the configuration names it, its display name.
    group: Option<String>,
    group_label: Option<String>,
    /// PRD 0024 RF7.3: the core's "Best now" chip text, only on the best
    /// account of a group with two or more candidates. Advisory.
    best_now: Option<cuotai_core::text::Localized>,
    /// PRD 0024 RF7.3: while this account is exhausted or projected to run
    /// out, what the best account of its group has left.
    advice: Option<AdviceView>,
    /// PRD 0022 RF4: the plan the provider reports (`metrics::plan_name`),
    /// shown as a pill next to the title; `None` draws no pill.
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    /// PRD 0023 RF10.3: the exhausted model-scoped limits, each with the
    /// core's "{model} at its limit until 19:30" sentence.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scope_blocks: Vec<ScopeBlockView>,
}

/// Whether `id` is in the comma-separated list parameter `key` of `account`.
fn connection_listed(account: &cuotai_core::Account, key: &str, id: &str) -> bool {
    account.params.get(key).is_some_and(|raw| {
        raw.split(',')
            .any(|item| item.trim().eq_ignore_ascii_case(id))
    })
}

/// `raw` with `id` added to (or removed from) its comma-separated list;
/// `None` once the list is empty.
fn toggle_listed(raw: Option<&str>, id: &str, on: bool) -> Option<String> {
    let mut items: Vec<String> = raw
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty() && !item.eq_ignore_ascii_case(id))
        .map(str::to_owned)
        .collect();
    if on {
        items.push(id.to_ascii_lowercase());
    }
    (!items.is_empty()).then(|| items.join(","))
}

/// PRD 0023 RF10.3: one model at its limit and when it returns.
#[derive(Serialize, Clone)]
pub struct ScopeBlockView {
    index: usize,
    label: String,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
    text: cuotai_core::text::Localized,
}

fn scope_block_views(report: &Report) -> Vec<ScopeBlockView> {
    cuotai_core::metrics::scope_blocks(report)
        .into_iter()
        .map(|block| ScopeBlockView {
            text: block.summary(local_at),
            index: block.index,
            label: block.label,
            resets_at: block.resets_at,
        })
        .collect()
}

/// The core's overage reading plus its texts, so no surface composes them.
#[derive(Serialize, Clone)]
pub struct OverageView {
    #[serde(flatten)]
    state: cuotai_core::overage::OverageExport,
    /// "On extra · $6.25", "Extra usage on · nothing spent", …
    text: cuotai_core::text::Localized,
    /// The card chip: `text`, or "Limit hit · on extra $6.25" while the
    /// account is also at its limit.
    summary: cuotai_core::text::Localized,
    /// The amount is in provider units, not money (Claude).
    caveat: Option<cuotai_core::text::Localized>,
}

fn overage_view(report: &Report) -> Option<OverageView> {
    let state = cuotai_core::overage::assess(report)?;
    let exhausted = classify_report(Some(report)) == AccountClass::Exhausted;
    let blocks = cuotai_core::metrics::scope_blocks(report);
    Some(OverageView {
        summary: if exhausted {
            state.summary_with_limit_hit()
        } else if !blocks.is_empty() {
            // PRD 0023 RF10.3: a model at its limit marks only its scope.
            state.summary_with_scope_hit(&blocks)
        } else {
            state.text()
        },
        text: state.text(),
        caveat: state.units_caveat(),
        state: cuotai_core::overage::OverageExport::from(&state),
    })
}

/// PRD 0036 RF8.2/RF8.5: the instance of an aggregator account.
#[derive(Serialize, Clone)]
pub struct AggregatorView {
    /// The aggregator's display name ("9router").
    kind: &'static str,
    version: Option<String>,
    /// Host and port of the base address (never its path).
    host: Option<String>,
    /// Active connections with quota left, of all active ones.
    available: usize,
    total: usize,
    /// Connections in the instance (active or not).
    connections: usize,
    pools: Vec<PoolView>,
    /// Whether the connection alerts go one by one (else per pool).
    alert_each: bool,
    /// The instance's dashboard can be opened (`open_aggregator_dashboard`).
    dashboard: bool,
}

#[derive(Serialize, Clone)]
pub struct PoolView {
    upstream: String,
    provider: Option<Provider>,
    available: usize,
    exhausted: usize,
    total: usize,
    returns_at: Option<chrono::DateTime<chrono::Utc>>,
    summary: cuotai_core::text::Localized,
}

/// PRD 0036 RF8: one upstream connection of an aggregator.
#[derive(Serialize, Clone)]
pub struct ConnectionView {
    /// The connection's label (as the aggregator masks it).
    label: String,
    upstream: String,
    /// The mapped Cuotai provider; `None` for an upstream Cuotai does not
    /// know (the view's `provider` is then the aggregator's).
    mapped: Option<Provider>,
    enabled_upstream: Option<bool>,
    duplicate_of: Option<String>,
    plan: Option<String>,
    via: Option<String>,
    /// The connection's stable sub id: `refresh_account`'s `sub` and the
    /// id `set_connection_flag` writes into `hidden` / `show_both`.
    sub: String,
    /// Its id is in the aggregator's `show_both` list.
    show_both: bool,
}

/// PRD 0024 RF7.3: "Work runs out ≈18:53 · Personal has 64% left", and the
/// card the only action ("Show Personal") focuses. Information only.
#[derive(Serialize, Clone)]
pub struct AdviceView {
    text: cuotai_core::text::Localized,
    /// `ReportView::index` of the best account.
    best_index: usize,
    /// Its name, for the "Show {name}" action.
    best_name: String,
}

/// The core's provider status plus its state's text.
#[derive(Serialize, Clone)]
pub struct IncidentView {
    #[serde(flatten)]
    status: ProviderStatus,
    label: cuotai_core::text::Localized,
}

/// PRD 0011 RF4.3: whether status queries are on, the domains they reach
/// for the configured providers (also while off, for Settings → Privacy)
/// and, RF4.2, whether a provider reports a partial outage or worse.
#[derive(Serialize, Clone, Default)]
pub struct StatusView {
    enabled: bool,
    domains: Vec<&'static str>,
    outage: bool,
}

/// The status shown on an account of `provider`, if any.
fn incident_view(statuses: &[ProviderStatus], provider: Provider) -> Option<IncidentView> {
    statuses
        .iter()
        .find(|status| status.provider == provider)
        .map(|status| IncidentView {
            label: status.state.label(),
            status: status.clone(),
        })
}

fn status_view(
    enabled: bool,
    domains: Vec<&'static str>,
    statuses: &[ProviderStatus],
) -> StatusView {
    StatusView {
        enabled,
        domains,
        outage: statuses
            .iter()
            .any(|status| !status.stale && status.state.is_outage()),
    }
}

/// PRD 0003 RF3: whether reset notices are on and, then, the next one
/// (tray and tooltip). Only provider, account name, window and label.
#[derive(Serialize, Clone)]
pub struct NotifyView {
    enabled: bool,
    next: Option<NextNoticeView>,
    /// PRD 0008 RF3: usage alerts are on.
    alerts: bool,
}

#[derive(Serialize, Clone)]
pub struct NextNoticeView {
    index: usize,
    provider: Provider,
    account: String,
    /// Window and label of the group's first metric.
    window: MetricWindow,
    label: String,
    /// When the notice is due (`resets_at − lead`).
    at: chrono::DateTime<chrono::Utc>,
    resets_at: chrono::DateTime<chrono::Utc>,
}

/// The core's descriptor (`access::describe`, flattened: `level` slug,
/// `summary`, `reason`, `evidence`…) plus the level's text and, for a
/// failure the descriptor explains, what replaces the generic one
/// (`UsageAccess::explain`). PRD 0016 RF4.1: `session` is the `UserSession`
/// level and risk warning of an account whose query uses a web session.
#[derive(Serialize, Clone)]
pub struct ReportAccess {
    #[serde(flatten)]
    descriptor: cuotai_core::access::UsageAccess,
    label: cuotai_core::text::Localized,
    explanation: Option<cuotai_core::text::Localized>,
    session: Option<user_session::SessionNotice>,
    /// PRD 0028 RF6.2 (DS-CMP-VER-1): "Verified N months ago" or, when the
    /// evidence is old, "Not verified since {date}". Shown in the details
    /// only, never in the add-account flow.
    verification: cuotai_core::text::Localized,
    verification_stale: bool,
}

fn report_access(account: &cuotai_core::Account, report: &Report) -> ReportAccess {
    // PRD 0028 RF7.3: "Extension (override)" when an override answered.
    let descriptor = cuotai_core::access::describe_reading(account, Some(report));
    let verification = descriptor.verification(chrono::Utc::now().date_naive());
    ReportAccess {
        label: descriptor.label(),
        explanation: descriptor.explain(report),
        session: user_session::notice(account),
        verification_stale: verification.id == "access.verification_stale",
        verification,
        descriptor,
    }
}

/// The preserved reading: its original `checked_at` and metrics. The UI always
/// labels it stale; `binding`, alerts and history never read it.
#[derive(Serialize, Clone)]
pub struct LastGoodView {
    checked_at: chrono::DateTime<chrono::Utc>,
    metrics: Vec<MetricView>,
}

fn last_good_view(report: &Report, now: chrono::DateTime<chrono::Utc>) -> LastGoodView {
    LastGoodView {
        checked_at: report.checked_at,
        metrics: report
            .metrics
            .iter()
            .map(|metric| MetricView {
                expired: metric.is_expired(now),
                ..metric_view(metric)
            })
            .collect(),
    }
}

#[derive(Serialize, Clone)]
pub struct View {
    /// Normalized identity (symlinks/relative paths resolved) of the file in use.
    config_path: Option<String>,
    /// Rule that chose the file: `env`, `chosen` or `system`.
    config_source: Option<PathSource>,
    /// RF1.2: path the TUI used last, only when it differs.
    other_config: Option<String>,
    config_exists: bool,
    account_count: usize,
    /// `false` until the first full round finishes.
    loaded: bool,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    reports: Vec<ReportView>,
    busy: Vec<usize>,
    error: Option<String>,
    /// Global refresh interval (`refresh_seconds` from the file); 0 before the configuration loads.
    refresh_seconds: u64,
    /// RF6.1: identity of the loaded configuration (changes on every save/remove/move/reload/switch);
    /// the UI returns it in model commands. 0 before loading (never accepted).
    generation: u64,
    /// PRD 0006: the history's storage failure, size-ceiling pruning or
    /// dropped readings, to show; `None` when all is well.
    history_warning: Option<cuotai_core::text::Localized>,
    /// Changes when the history was erased or recording switched (RF5).
    history_epoch: u64,
    notify: NotifyView,
    status: StatusView,
    /// PRD 0012 RF5.2: the local server (off, on at its address, or why not).
    serve: ServeView,
    /// PRD 0013 RF4: whether hooks are on and, when on, run or why not.
    hooks: HooksBadge,
    /// PRD 0025 RF2.3: the menu-bar style and the ids it shows, in order
    /// (the popover lists the rest under "Not in menu bar").
    tray: TrayView,
    /// The menu bar's input, per account in dashboard order (not sent).
    #[serde(skip)]
    tray_accounts: Vec<tray::AccountView>,
}

/// PRD 0025 RF2.3: what the popover needs from the menu-bar selection.
#[derive(Serialize, Clone, Default)]
pub struct TrayView {
    style: tray::TrayStyle,
    selected: Vec<String>,
    popover_view: crate::settings::PopoverView,
}

/// PRD 0025 RF1.1: the menu bar's view of one account: the current binding
/// share, else the preserved reading's (marked stale), its alerts, outage
/// and projected exhaustion.
fn tray_account(view: &ReportView, last_good: Option<&Report>) -> tray::AccountView {
    let (current, preserved) = match &view.binding {
        Some(b) => (Some(b.clone()), false),
        None => (last_good.and_then(binding), true),
    };
    let runs_out_at = (!preserved)
        .then_some(current.as_ref())
        .flatten()
        .and_then(|b| view.metrics.get(b.index))
        .and_then(|m| m.pace.as_ref())
        .and_then(|p| p.pace.projected_exhaustion_at);
    tray::AccountView {
        key: view.account_id.clone(),
        provider: view.provider.slug().to_owned(),
        provider_label: view.provider.label().to_owned(),
        name: view.account.clone(),
        remaining: current
            .as_ref()
            .map(|b| (1.0 - b.used_ratio).clamp(0.0, 1.0)),
        stale: current.is_some() && (preserved || view.freshness == Freshness::Stale),
        exhausted: current
            .as_ref()
            .is_some_and(|b| b.reason == cuotai_core::metrics::BindingReason::Exhausted),
        resets_at: current
            .as_ref()
            .and_then(|b| b.blocked_until.or(b.resets_at)),
        runs_out_at,
        alert: !view.usage_alerts.is_empty(),
        incident: view
            .incident
            .as_ref()
            .is_some_and(|i| !i.status.stale && i.status.state.is_outage()),
        // PRD 0023: paying for extra usage right now (active or at its cap).
        on_extra: view
            .overage
            .as_ref()
            .is_some_and(|overage| overage.state.state.is_billing()),
    }
}

fn metric_view(metric: &Metric) -> MetricView {
    MetricView {
        metric: metric.clone(),
        state: classify_metric(metric),
        pace: None,
        forecast: None,
        resets_label: None,
        alert: None,
        expired: false,
    }
}

/// `recent`: the recorded points of each metric of `report` (PRD 0006),
/// which project its pace from the recent slope when there are enough.
fn report_view(
    index: usize,
    report: &Report,
    config: &Config,
    next_refresh_at_ms: Option<u64>,
    model_count: Option<usize>,
    binding_rank: usize,
    recent: &[Vec<Point>],
) -> ReportView {
    let account = &config.accounts[index];
    let binding = binding(report);
    let refresh_seconds = account.effective_refresh_seconds(config.refresh_seconds);
    let interval = Duration::from_secs(refresh_seconds);
    let now = chrono::Utc::now();
    let metrics: Vec<MetricView> = report
        .metrics
        .iter()
        .enumerate()
        .map(|(i, metric)| {
            let points = recent.get(i).map(Vec::as_slice);
            MetricView {
                // PRD 0024 FIX-11: at the reading's time, only while fresh.
                pace: pace::assess_reading(metric, report.checked_at, interval, now, points)
                    .map(pace_view),
                forecast: forecast_view(
                    metric,
                    report.checked_at,
                    interval,
                    now,
                    points.unwrap_or_default(),
                ),
                resets_label: metric
                    .resets_at
                    .filter(|at| *at > now)
                    .map(|at| cuotai_core::resets::pair_label(&local_at(at), at - now)),
                ..metric_view(metric)
            }
        })
        .collect();
    let pace_index = binding_where(report, |i| metrics[i].pace.is_some()).map(|b| b.index);
    ReportView {
        index,
        account: report.account.clone(),
        provider: report.provider,
        checked_at: report.checked_at,
        status: report.status,
        metrics,
        notes: report.notes.clone(),
        notes_l10n: report.notes_l10n.clone(),
        source: report.source.clone(),
        class: classify_report(Some(report)),
        refresh_seconds,
        next_refresh_at_ms,
        uses_oauth: account.oauth.is_some(),
        identity: account_identity(account, report.status),
        model_count,
        binding,
        binding_rank,
        freshness: assess(report.checked_at, interval, now),
        clock_adjusted: clock_adjusted(report.checked_at, now),
        last_good: None,
        pending: false,
        pace_index,
        access: report_access(account, report),
        account_key: account.state_key(),
        account_id: account.id().to_string(),
        monogram: None,
        notify: true,
        usage_alerts: Vec::new(),
        incident: None,
        session_expiry: user_session::report_expiry(report, now),
        paste_session: user_session::offers_new_paste(report),
        connection_warnings: cuotai_core::aggregators::ConnectionWarnings::of(report),
        data_as_of: cuotai_core::freshness::lagging_as_of(report),
        overage: overage_view(report),
        connections: Vec::new(),
        aggregator: None,
        connection: None,
        group: account.group.clone(),
        group_label: advice::group_label(config, account).map(str::to_owned),
        best_now: None,
        advice: None,
        plan: cuotai_core::metrics::plan_name(report),
        scope_blocks: scope_block_views(report),
    }
}

/// PRD 0036 RF8: the views of the connections of the aggregator at
/// `index` and its instance summary. The connections come from the current
/// report when it is `Ready` (each with its own preserved reading while its
/// query failed) or, while the instance failed, from its last good reading
/// (all stale). Each view is a [`report_view`] of the connection presented
/// as an account (its key, alerts and bell are its own; its usage access is
/// the aggregator's), keeping the aggregator's `index` for actions.
#[allow(clippy::too_many_arguments)]
fn aggregator_views(
    index: usize,
    config: &Config,
    current: Option<&Report>,
    last_good: Option<&Report>,
    last_good_sub: impl Fn(&str) -> Option<Report>,
    next_refresh_at_ms: Option<u64>,
    alert_each: bool,
) -> (Vec<ReportView>, AggregatorView) {
    use cuotai_core::aggregators::{
        dashboard_url, instance_host, instance_version, pools, sub_account, sub_as_report,
    };
    let parent = &config.accounts[index];
    let ready = current.filter(|report| report.status == Status::Ready);
    let source = ready.or(last_good);
    let mut upstreams: Vec<&str> = Vec::new();
    for sub in source
        .map(|report| report.sub_reports.as_slice())
        .unwrap_or_default()
    {
        if !upstreams.contains(&sub.upstream.as_str()) {
            upstreams.push(&sub.upstream);
        }
    }
    let mut views = Vec::new();
    if let Some(source) = source {
        for upstream in &upstreams {
            for sub in source
                .sub_reports
                .iter()
                .filter(|sub| sub.upstream == *upstream)
            {
                let mut keyed = config.clone();
                keyed.accounts[index] = sub_account(parent, sub);
                let reading = sub_as_report(source, sub);
                let mut view = if ready.is_some() {
                    let mut view =
                        report_view(index, &reading, &keyed, next_refresh_at_ms, None, 0, &[]);
                    view.last_good = last_good_sub(&sub.id)
                        .map(|good| last_good_view(&good, chrono::Utc::now()));
                    view
                } else {
                    let mut view =
                        pending_view(index, &reading, &keyed, next_refresh_at_ms, None, 0);
                    view.last_good = (sub.status == Status::Ready)
                        .then(|| last_good_view(&reading, chrono::Utc::now()));
                    view
                };
                view.provider = sub.provider.unwrap_or(parent.provider);
                view.account = sub.label.clone();
                view.connection = Some(ConnectionView {
                    label: sub.label.clone(),
                    upstream: sub.upstream.clone(),
                    mapped: sub.provider,
                    enabled_upstream: sub.enabled_upstream,
                    duplicate_of: sub.duplicate_of.clone(),
                    plan: sub.plan.clone(),
                    via: sub.via.clone(),
                    sub: sub.id.clone(),
                    show_both: connection_listed(parent, "show_both", &sub.id),
                });
                if sub.plan.is_some() {
                    view.plan = sub.plan.clone();
                }
                views.push(view);
            }
        }
    }
    let pools = source.map(pools).unwrap_or_default();
    let summary = AggregatorView {
        kind: cuotai_core::aggregators::shape(parent.provider)
            .map_or("9router", |shape| shape.kind()),
        version: source.and_then(instance_version),
        host: instance_host(parent),
        available: pools.iter().map(|pool| pool.available).sum(),
        total: pools.iter().map(|pool| pool.total).sum(),
        connections: views.len(),
        pools: pools
            .into_iter()
            .map(|pool| PoolView {
                summary: pool.summary(),
                upstream: pool.upstream,
                provider: pool.provider,
                available: pool.available,
                exhausted: pool.exhausted,
                total: pool.total,
                returns_at: pool.returns_at,
            })
            .collect(),
        alert_each,
        dashboard: dashboard_url(parent).is_some(),
    };
    (views, summary)
}

/// An account no query answered yet, shown through its restored reading
/// (PRD 0010 RF2.3): no current metrics (so it is unmeasured), notes or
/// source and no "active session" claim (nothing answered with that
/// credential yet); the UI draws `last_good` as stale.
fn pending_view(
    index: usize,
    restored: &Report,
    config: &Config,
    next_refresh_at_ms: Option<u64>,
    model_count: Option<usize>,
    binding_rank: usize,
) -> ReportView {
    let placeholder = Report {
        metrics: Vec::new(),
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        ..restored.clone()
    };
    let view = report_view(
        index,
        &placeholder,
        config,
        next_refresh_at_ms,
        model_count,
        binding_rank,
        &[],
    );
    ReportView {
        pending: true,
        identity: account_identity(&config.accounts[index], Status::Error),
        ..view
    }
}

/// PRD 0024 RF7.3: mark the best account of each group of comparable
/// accounts and, on the others that are exhausted or projected to run out,
/// say what the best one has left. Reads only the cycle's current readings;
/// nothing is written anywhere.
fn mark_best(
    reports: &mut [ReportView],
    config: &Config,
    readings: &[Option<Report>],
    now: chrono::DateTime<chrono::Utc>,
) {
    let views = advice::views(
        config,
        |index| readings.get(index).and_then(Option::as_ref),
        |_| true,
        now,
    );
    for best in advice::best(&views, now) {
        let Some(target) = reports.iter().find(|r| r.index == best.best) else {
            continue;
        };
        let (best_name, left) = (
            target.account.clone(),
            best.candidate(best.best).map_or(0.0, |c| c.remaining),
        );
        for candidate in &best.ranked {
            let Some(view) = reports.iter_mut().find(|r| r.index == candidate.index) else {
                continue;
            };
            if candidate.index == best.best {
                view.best_now = Some(advice::best_chip());
                continue;
            }
            let runs_out = view
                .metrics
                .iter()
                .find_map(|metric| metric.forecast.as_ref())
                .map(|forecast| forecast.at.clone());
            let exhausted = view.class == AccountClass::Exhausted;
            if !exhausted && runs_out.is_none() {
                continue;
            }
            view.advice = Some(AdviceView {
                text: advice::switch_text(
                    &view.account,
                    &best_name,
                    left,
                    runs_out.as_deref().filter(|_| !exhausted),
                ),
                best_index: best.best,
                best_name: best_name.clone(),
            });
        }
    }
}

/// PRD 0024 RF5: the pace input of each exported account, in export order.
type PaceTable = Vec<Option<(Duration, Vec<Vec<Point>>)>>;

/// Position of each slot in the core's "most restrictive first" order; the
/// stable sort keeps the manual order for ties and accounts without a value.
fn binding_ranks(slots: &[Option<Report>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by(|&a, &b| cmp_binding(slots[a].as_ref(), slots[b].as_ref()));
    let mut ranks = vec![0; slots.len()];
    for (rank, slot) in order.into_iter().enumerate() {
        ranks[slot] = rank;
    }
    ranks
}

/// What reset notices read (PRD 0003 RF2): each account's current report
/// or, while it failed or nothing answered yet, its last valid reading
/// (restored from the history on opening), so a reset passed while the
/// app was closed is still known.
fn notice_readings(cycle: &RefreshCycle) -> Vec<Option<&Report>> {
    (0..cycle.len())
        .map(|index| last_reading(cycle, index))
        .collect()
}

/// The account's latest valid reading: the current report or, while it
/// failed or nothing answered yet, the preserved or restored one (PRD 0006;
/// the `prev` of a usage alert's first reading, PRD 0008 RF1.7).
fn last_reading(cycle: &RefreshCycle, index: usize) -> Option<&Report> {
    cycle
        .last_good_report(index)
        .or_else(|| cycle.account_report(index))
}

/// Apply one account's query to the cycle. PRD 0008 RF2.2: the report is
/// judged against the reading it replaces ([`last_reading`]) and, only once
/// the cycle accepted it, the usage alerts it deserves are recorded for
/// [`Backend::run_notices`] to deliver. No query happens here.
fn finish_account(
    cycle: &mut RefreshCycle,
    notices: &mut Notices,
    source: (&Monitor, &Config),
    index: usize,
    report: Report,
    token: cuotai_core::refresh::Generation,
    now: chrono::DateTime<chrono::Utc>,
) {
    notices.adopt_reports(last_reading(cycle, index).into_iter().chain([&report]));
    let pace = notice_pace(notices, source, index, &report);
    let input = pace.as_ref().map(NoticePace::input);
    let events = notices.assess(last_reading(cycle, index), &report, now, input.as_ref());
    let prev = last_reading(cycle, index).cloned();
    if cycle.finish_account(index, Some(report), token, Instant::now()) {
        if let Some(next) = cycle.account_report(index) {
            notices.record(prev.as_ref(), next, events, now, input.as_ref());
        }
    }
}

/// [`finish_account`] for a full sweep: every report judged against the
/// reading it replaces, the alerts recorded once the cycle accepted it.
fn finish_full(
    cycle: &mut RefreshCycle,
    notices: &mut Notices,
    source: (&Monitor, &Config),
    snapshot: cuotai_core::Snapshot,
    token: cuotai_core::refresh::Generation,
    now: chrono::DateTime<chrono::Utc>,
) {
    let prevs: Vec<Option<Report>> = (0..snapshot.reports.len())
        .map(|index| last_reading(cycle, index).cloned())
        .collect();
    notices.adopt_reports(prevs.iter().flatten().chain(&snapshot.reports));
    let paces: Vec<Option<NoticePace>> = snapshot
        .reports
        .iter()
        .enumerate()
        .map(|(index, report)| notice_pace(notices, source, index, report))
        .collect();
    let events: Vec<_> = snapshot
        .reports
        .iter()
        .enumerate()
        .map(|(index, report)| {
            let input = paces[index].as_ref().map(NoticePace::input);
            notices.assess(last_reading(cycle, index), report, now, input.as_ref())
        })
        .collect();
    if cycle.finish_full(Some(snapshot), token, Instant::now()) {
        for (index, events) in events.into_iter().enumerate() {
            if let Some(next) = cycle.account_report(index) {
                let input = paces[index].as_ref().map(NoticePace::input);
                notices.record(prevs[index].as_ref(), next, events, now, input.as_ref());
            }
        }
    }
}

/// PRD 0024 RF3.2: what the projected warning reads beyond the reports: the
/// account's effective refresh interval and the recorded points ending at
/// the reading (the same lookback as its pace, FIX-11).
struct NoticePace {
    interval: Duration,
    recent: Vec<Vec<Point>>,
}

impl NoticePace {
    fn input(&self) -> cuotai_core::alerts::PaceInput<'_> {
        cuotai_core::alerts::PaceInput {
            interval: self.interval,
            recent: &self.recent,
        }
    }
}

/// The pace input of `report` (account `index`), only while the projected
/// warning is on: the history is not read otherwise.
fn notice_pace(
    notices: &Notices,
    (monitor, config): (&Monitor, &Config),
    index: usize,
    report: &Report,
) -> Option<NoticePace> {
    if !notices.projected_on() {
        return None;
    }
    let account = config.accounts.get(index)?;
    Some(NoticePace {
        interval: Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds)),
        recent: monitor.recent_points(index, &report.metrics, report.checked_at),
    })
}

fn next_notice_view(
    event: &cuotai_core::resets::ResetEvent,
    account: &cuotai_core::Account,
) -> NextNoticeView {
    let first = &event.metrics[0];
    NextNoticeView {
        index: event.account_index,
        provider: account.provider,
        account: account.name.clone(),
        window: first.window,
        label: first.label.clone(),
        at: event.notify_at,
        resets_at: event.resets_at,
    }
}

/// The recorded points a view reads for a reading: account index, its
/// metrics and the instant the lookback ends at (see
/// [`Monitor::recent_points`]).
type RecentReader<'a> =
    dyn Fn(usize, &[Metric], chrono::DateTime<chrono::Utc>) -> Vec<Vec<Point>> + 'a;

/// A [`RecentReader`] over `monitor` that reads each (account, instant) pair
/// once: one emit builds the view, the hooks' export and the journal's pace
/// table from the same readings, and every read opens the history file.
struct RecentMemo<'a> {
    monitor: Option<&'a Monitor>,
    seen: std::cell::RefCell<RecentSeen>,
}

/// (account index, lookback end in unix ms, metric count) → the points read.
type RecentSeen = std::collections::HashMap<(usize, i64, usize), Vec<Vec<Point>>>;

/// One reading to prefetch: account index, its metrics, the lookback end.
type RecentWanted = (usize, Vec<Metric>, chrono::DateTime<chrono::Utc>);

impl<'a> RecentMemo<'a> {
    /// A memo primed with `seen`, the readings [`Backend::prefetch_recent`]
    /// read in one pass off the lock; anything else is read on demand.
    fn with(monitor: Option<&'a Monitor>, seen: RecentSeen) -> Self {
        Self {
            monitor,
            seen: std::cell::RefCell::new(seen),
        }
    }

    fn read(
        &self,
        index: usize,
        metrics: &[Metric],
        at: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Vec<Point>> {
        let key = (index, at.timestamp_millis(), metrics.len());
        if let Some(points) = self.seen.borrow().get(&key) {
            return points.clone();
        }
        let points = self.monitor.map_or_else(
            || vec![Vec::new(); metrics.len()],
            |monitor| monitor.recent_points(index, metrics, at),
        );
        self.seen.borrow_mut().insert(key, points.clone());
        points
    }
}

impl Inner {
    /// The readings [`Self::view_with`] asks the history for (each slot's
    /// report at its own time) and, with `export_at`, those
    /// [`Self::hooks_input_with`] adds (the same reports at that instant),
    /// with the monitor to read them from. Cheap: it clones metrics only.
    fn recent_wanted(
        &self,
        export_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Option<(Arc<Monitor>, Vec<RecentWanted>)> {
        let l = self.loaded.as_ref()?;
        let mut wanted = Vec::new();
        for index in 0..l.cycle.len() {
            let Some(report) = l.cycle.account_report(index) else {
                continue;
            };
            wanted.push((index, report.metrics.clone(), report.checked_at));
            if let Some(at) = export_at {
                wanted.push((index, report.metrics.clone(), at));
            }
        }
        Some((Arc::clone(&l.monitor), wanted))
    }

    /// A reader straight from the loaded monitor (no memo).
    fn recent_points(
        &self,
        index: usize,
        metrics: &[Metric],
        at: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Vec<Point>> {
        match &self.loaded {
            Some(l) => l.monitor.recent_points(index, metrics, at),
            None => vec![Vec::new(); metrics.len()],
        }
    }

    fn view(&self) -> View {
        self.view_with(&|index, metrics, at| self.recent_points(index, metrics, at))
    }

    fn view_with(&self, recent: &RecentReader<'_>) -> View {
        let Some(l) = &self.loaded else {
            return View {
                config_path: None,
                config_source: None,
                config_exists: false,
                other_config: None,
                account_count: 0,
                loaded: false,
                started_at: None,
                finished_at: None,
                reports: vec![],
                busy: vec![],
                error: self.load_error.clone(),
                refresh_seconds: 0,
                generation: 0,
                history_warning: None,
                history_epoch: self.history_epoch,
                notify: NotifyView {
                    enabled: self.notices.prefs().enabled,
                    next: None,
                    alerts: self.notices.alerts().enabled,
                },
                status: StatusView {
                    enabled: self.incidents.is_enabled(),
                    ..StatusView::default()
                },
                serve: self.serve.clone(),
                hooks: self.hooks,
                tray: TrayView::default(),
                tray_accounts: Vec::new(),
            };
        };
        // Per-account report lookup that mirrors the cycle's slots. Even
        // when the public snapshot is hidden by an in-flight edit, every
        // previously-refreshed account still has a usable report so the
        // UI does not flash "no data" for unrelated accounts while a
        // single edit refreshes.
        let slot_reports: Vec<Option<Report>> = match l.cycle.snapshot() {
            Some(snapshot) => snapshot.reports.iter().cloned().map(Some).collect(),
            None => (0..l.cycle.len())
                .map(|index| l.cycle.account_report(index).cloned())
                .collect(),
        };
        // The "fully loaded" indicator only flips once every slot has a
        // report, matching the cycle's own gating of `snapshot()`.
        let loaded = slot_reports.iter().all(Option::is_some) || l.file.config.accounts.is_empty();
        let now = Instant::now();
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let started_at = l.cycle.snapshot().map(|snapshot| snapshot.started_at);
        let finished_at = l.cycle.snapshot().map(|snapshot| snapshot.finished_at);
        let ranks = binding_ranks(&slot_reports);
        let clock = chrono::Utc::now();
        // PRD 0011 RF4: cached readings only (empty while off), no network.
        let statuses = l.monitor.incident_statuses();
        // PRD 0022 MONO-4: read once per view, applied by account id.
        let desk = crate::settings::view();
        let reports: Vec<ReportView> = slot_reports
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                // Next query as an absolute instant: the UI counts down without further events.
                let next = (!l.cycle.is_busy(i))
                    .then(|| unix_ms + l.cycle.next_refresh_in(i, now).as_millis() as u64);
                let models = l.monitor.model_count(i);
                let last_good = l.cycle.last_good_report(i);
                let mut view = match (slot, last_good) {
                    (Some(report), _) => {
                        // PRD 0024 FIX-11: the lookback ends at the reading, like its pace.
                        let recent = recent(i, &report.metrics, report.checked_at);
                        let mut view =
                            report_view(i, report, &l.file.config, next, models, ranks[i], &recent);
                        self.mark_usage_alerts(&mut view, report);
                        view
                    }
                    // PRD 0010 RF2.3: a reading restored from the history
                    // shows (stale) until the first query answers.
                    (None, Some(restored)) => {
                        pending_view(i, restored, &l.file.config, next, models, ranks[i])
                    }
                    (None, None) => return None,
                };
                // PRD 0033 RF2.1: windows renewed since the reading collapse.
                let now_utc = chrono::Utc::now();
                view.last_good = last_good
                    .map(|report| report.collapse_expired(now_utc))
                    .map(|report| last_good_view(&report, now_utc));
                view.notify = self.notices.account_on(&view.account_key);
                view.monogram = desk.monogram_of(&view.account_id);
                view.incident = incident_view(&statuses, view.provider);
                // PRD 0036 RF8: an aggregator carries its connections.
                if l.file.config.accounts[i].provider.is_aggregator() {
                    let (mut connections, summary) = aggregator_views(
                        i,
                        &l.file.config,
                        slot.as_ref(),
                        last_good,
                        |id| {
                            let good = l.cycle.last_good_sub(i, id)?;
                            let source = l.cycle.last_good_report(i).or(slot.as_ref())?;
                            Some(cuotai_core::aggregators::sub_as_report(source, good))
                        },
                        next,
                        self.notices.alerts().pool_mode(&view.account_key)
                            == alerts::PoolAlerts::EachConnection,
                    );
                    for connection in &mut connections {
                        connection.notify = self.notices.account_on(&connection.account_key);
                        connection.incident = incident_view(&statuses, connection.provider);
                        if connection
                            .connection
                            .as_ref()
                            .is_some_and(|c| c.mapped.is_none())
                        {
                            connection.incident = None;
                        }
                        if let Some(report) = slot.as_ref().filter(|r| r.status == Status::Ready) {
                            if let Some(sub) = report.sub_reports.iter().find(|sub| {
                                connection.account_key.ends_with(&format!("/{}", sub.id))
                            }) {
                                let reading = cuotai_core::aggregators::sub_as_report(report, sub);
                                self.mark_usage_alerts(connection, &reading);
                            }
                        }
                    }
                    view.connections = connections;
                    view.aggregator = Some(summary);
                }
                Some(view)
            })
            .collect();
        let mut reports = reports;
        mark_best(&mut reports, &l.file.config, &slot_reports, clock);
        let tray_accounts = reports
            .iter()
            .map(|v| tray_account(v, l.cycle.last_good_report(v.index)))
            .collect();
        let next = self
            .notices
            .next(&l.file.config, &notice_readings(&l.cycle), clock)
            .map(|(event, account)| next_notice_view(&event, &account));
        View {
            config_path: Some(config_label(&l.file)),
            config_source: Some(l.file.source),
            config_exists: l.file.exists,
            other_config: l.other_config.clone(),
            account_count: l.file.config.accounts.len(),
            loaded,
            started_at,
            finished_at,
            reports,
            busy: l
                .tokens
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.is_some().then_some(i))
                .collect(),
            error: None,
            refresh_seconds: l.file.config.refresh_seconds,
            generation: l.generation,
            history_warning: l
                .monitor
                .history_status()
                .and_then(|status| status.warning()),
            history_epoch: self.history_epoch,
            notify: NotifyView {
                enabled: self.notices.prefs().enabled,
                next,
                alerts: self.notices.alerts().enabled,
            },
            status: status_view(
                self.incidents.is_enabled(),
                l.monitor.status_domains(),
                &statuses,
            ),
            serve: self.serve.clone(),
            hooks: self.hooks,
            tray: TrayView::default(),
            tray_accounts,
        }
    }

    /// PRD 0012 RF5.1: what the local server reads, copied from the live
    /// state (the same cycle the app shows); `None` before a configuration
    /// is loaded.
    fn feed(&self) -> Option<Feed> {
        let l = self.loaded.as_ref()?;
        Some(Feed {
            config: l.file.config.clone(),
            identity: l.file.identity().ok()?,
            cycle: l.cycle.clone(),
            incidents: l.monitor.incident_statuses(),
            monitor: l.monitor.clone(),
        })
    }

    /// PRD 0013 RF4: what the hooks watch: the `hooks.toml` beside the
    /// loaded TOML and the `export::v1` of the live cycle at `now` (the
    /// same state the app shows; no query).
    fn hooks_input_with(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        recent: &RecentReader<'_>,
    ) -> Option<(PathBuf, Export, PaceTable)> {
        let l = self.loaded.as_ref()?;
        let export = Export::from_cycle_with_history(
            &l.file.config,
            &l.file.identity().ok()?,
            &l.cycle,
            &l.monitor.incident_statuses(),
            now,
            ExportOptions::default(),
            |index, metrics, at| recent(index, metrics, at),
        );
        // PRD 0024 RF5: the pace input of each account's reading, so the
        // hooks can see a projected exhaustion (no query; the history only).
        let config = &l.file.config;
        let pace = export
            .accounts
            .iter()
            .map(|account| {
                let report = account.report.as_ref()?;
                let source = config.accounts.get(account.index)?;
                Some((
                    Duration::from_secs(source.effective_refresh_seconds(config.refresh_seconds)),
                    recent(account.index, &report.metrics, report.checked_at),
                ))
            })
            .collect();
        Some((
            cuotai_core::hooks::Hooks::path_beside(&l.file.path),
            export,
            pace,
        ))
    }

    /// The `hooks.toml` beside the loaded TOML.
    fn hooks_path(&self) -> Option<PathBuf> {
        self.loaded
            .as_ref()
            .map(|l| cuotai_core::hooks::Hooks::path_beside(&l.file.path))
    }

    /// PRD 0008 RF3.2/RF3.4: while usage alerts are on, each metric that can
    /// alert says whether it is muted and has an active alert, and the
    /// account lists the metrics with one.
    fn mark_usage_alerts(&self, view: &mut ReportView, report: &Report) {
        if !self.notices.alerts().enabled {
            return;
        }
        let missed = self.notices.missed(&view.account_key);
        let active: Vec<&str> = self
            .notices
            .active(report)
            .map(|metric| metric.label.as_str())
            .collect();
        for metric in &mut view.metrics {
            if alerts::eligible(&metric.metric) {
                let label = metric.metric.label.as_str();
                metric.alert = Some(MetricAlertView {
                    muted: self.notices.metric_muted(&view.account_key, label),
                    active: active.contains(&label),
                    missed: missed
                        .iter()
                        .find(|crossing| crossing.label == label)
                        .map(|crossing| crossing.threshold),
                });
            }
        }
        view.usage_alerts = active.into_iter().map(str::to_owned).collect();
    }

    fn install(&mut self, file: ConfigFile) -> Result<(), String> {
        let session = match &self.session {
            Some(s) => Arc::clone(s),
            None => {
                // Development only: plaintext SQLite, refused by release builds (core).
                let session = match std::env::var_os("CUOTAI_DEV_CREDENTIALS_SQLITE") {
                    Some(path) => {
                        eprintln!("WARNING: credentials in UNENCRYPTED SQLite (development only); the system keychain will not be used.");
                        OAuthSession::development_sqlite(std::path::Path::new(&path))
                    }
                    None => OAuthSession::system(),
                };
                let s = Arc::new(session.map_err(|_| crate::desk_text::vault_unavailable())?);
                self.session = Some(Arc::clone(&s));
                s
            }
        };
        let monitor = Monitor::with_oauth(
            file.config.clone(),
            Arc::new(EnvironmentCredentials),
            session,
        )
        .map_err(|e| {
            serde_json::to_string(&cuotai_core::edit_error::config_error_note(&e)).unwrap_or_else(
                |_| {
                    cuotai_core::edit_error::config_error_note(&e)
                        .fallback
                        .into_owned()
                },
            )
        })?
        // PRD 0011 RF4.3: the device's status preference and cached readings
        // survive a reload or a switch to another file.
        .with_incidents(self.incidents.clone());
        // Keyed by `ConfigFile::identity()` in the OS data dir; no data dir
        // leaves model operations as `NoCache` while quotas keep working.
        let mut monitor = match cuotai_core::models::ModelCache::in_data_dir(&file) {
            Ok(cache) => monitor.with_model_cache(cache),
            Err(_) => monitor,
        };
        // PRD 0017 RF5.8: install and reload both rebuild the monitor; the
        // process-wide extension manager (runner, two-process cap, per-id
        // schedules, revocation guard) is shared, never duplicated.
        if let Some(manager) = cuotai_core::extensions::management::Manager::default_shared() {
            monitor = monitor.with_extensions(manager);
        }
        // PRD 0006: the measurement history of this TOML, pruned by the
        // device's retention (`desktop.json`); without a data dir nothing is
        // recorded. Tests never touch the real OS data dir.
        if let Some(store) = (!cfg!(test))
            .then(|| HistoryStore::in_data_dir(&file).ok())
            .flatten()
        {
            monitor =
                monitor.with_history(store.with_retention(crate::settings::history_retention()));
            // RF5.3: a device that switched recording off keeps it off.
            monitor.set_history_recording(crate::settings::history_recording());
        }
        let monitor = Arc::new(monitor);
        // Just loaded from the TOML: drop catalogs of accounts gone from it.
        // A cache failure only keeps stale digests; model ops report it.
        let _ = monitor.prune_models();
        monitor.mark_history_removed();
        self.generation += 1;
        let accounts_len = file.config.accounts.len();
        // `install` replaces the live configuration (initial load or
        // a `cuotai.toml` reload after `Conflict`).
        // Recovery handles minted under the previous configuration
        // reference vault entries that may now be re-used by a fresh
        // account in the new config; the core guard already refuses
        // such retries, but dropping the list here means the UI will
        // not offer a button for a stale handle in the first place.
        // Tests never touch the real OS data dir.
        let other_config = (!cfg!(test))
            .then(|| {
                cuotai_core::last_config::note(cuotai_core::last_config::Frontend::Desktop, &file)
            })
            .flatten()
            .map(|p| p.display().to_string());
        // PRD 0010 RF2.3: the last recorded readings show, stale, until the
        // first query of each account answers.
        let mut cycle = RefreshCycle::new(&file.config, Instant::now());
        let restored = monitor.restored_readings();
        // PRD 0029 RF2.3: bells, mutes, reset notices and alert marks an
        // older release keyed by name move to the account ids, with the
        // configuration and its last readings (which carry the labels) in
        // hand, before any rename or new reading can strand them.
        let _ = self.notices.adopt_config(&file.config);
        self.notices.adopt_reports(restored.iter().flatten());
        cycle.restore_last_good(restored);
        self.loaded = Some(Loaded {
            other_config,
            cycle,
            file,
            monitor,
            generation: self.generation,
            tokens: vec![None; accounts_len],
            pending_recovery: Vec::new(),
        });
        self.load_error = None;
        Ok(())
    }
}

fn config_label(file: &ConfigFile) -> String {
    file.identity()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| file.path.display().to_string())
}

/// PRD 0014 RF3.2: the recent series of each published account's binding
/// metric, so the History widget has data to draw. The span follows the
/// metric's own window (two weeks of samples for a monthly one, a week
/// otherwise); an account with no samples, no binding or no recorded
/// history publishes no series at all.
fn attach_history(
    snapshot: &mut cuotai_core::widget_snapshot::WidgetSnapshot,
    export: &Export,
    store: &HistoryStore,
    accounts: &[cuotai_core::Account],
) {
    let now = chrono::Utc::now();
    let exported: BTreeMap<&str, &cuotai_core::export::v1::AccountExport> = export
        .accounts
        .iter()
        .map(|account| (account.key.as_str(), account))
        .collect();
    for published in &mut snapshot.accounts {
        let Some(binding) = published.binding.as_ref().map(|binding| binding.index) else {
            continue;
        };
        let Some(exported) = exported.get(published.key.as_str()) else {
            continue;
        };
        // The very metrics the core projected: the preserved ones while the
        // account is stale, the current ones otherwise.
        let metric = match &exported.last_good {
            Some(last_good) => last_good.metrics.get(binding).map(|m| &m.metric),
            None => exported
                .report
                .as_ref()
                .and_then(|report| report.metrics.get(binding)),
        };
        let Some(metric) = metric else {
            continue;
        };
        let range = match metric.window {
            Some(cuotai_core::MetricWindow::Monthly) => HistoryRange::Month,
            Some(cuotai_core::MetricWindow::Weekly) => HistoryRange::Week,
            _ => HistoryRange::Day,
        };
        let Some(config_account) = accounts.iter().find(|a| a.state_key() == published.id) else {
            continue;
        };
        let key = cuotai_core::history::metric_key(metric);
        let span = range.since(now)..=now;
        if let Ok(points) = store.series(config_account, &key, span) {
            published.history = widgets::history(&points, metric.limit);
        }
    }
}

impl Inner {
    /// RF1.2: `CUOTAI_CONFIG` > file chosen in settings > OS path
    /// (never `./cuotai.toml` from the `cwd`). Loads through the core and swaps all
    /// per-configuration state (monitor, schedule, tokens, recovery); on error
    /// the current state stays intact.
    fn switch(&mut self, chosen: Option<PathBuf>, locations: &Locations) -> Result<(), String> {
        let file = ConfigFile::load_with(Discovery::Gui { chosen }, true, locations)
            .map_err(|e| edit_error_to_json(&e))?;
        self.install(file)
    }
}

impl Backend {
    /// The app's backend with this device's reset notice state (PRD 0003),
    /// status preference (PRD 0011 RF4.3, off until the user opts in),
    /// local server preferences (PRD 0012 RF5, off by default; started by
    /// [`Self::start_serve`]), hooks switch (PRD 0013 RF4, off by
    /// default; the confirmed rules start with the first state change) and
    /// widget publishing switch (PRD 0014 RF4.1, off by default; the
    /// publisher task writes with the first state change after it).
    pub fn new(
        notices: Notices,
        status_queries: bool,
        serve: ServePrefs,
        hooks_enabled: bool,
        widgets: WidgetPrefs,
    ) -> Self {
        let incidents = IncidentMonitor::new();
        incidents.set_enabled(status_queries);
        let serve = Integration::new(serve, Arc::new(cuotai_core::serve::SystemTokenVault));
        let mut hooks = Hooks::default();
        hooks.set_enabled(hooks_enabled);
        let publisher = widgets::Publisher::new();
        publisher.set_enabled(widgets.enabled);
        Self {
            inner: Mutex::new(Inner {
                notices,
                incidents,
                serve: serve.view(),
                ..Inner::default()
            }),
            serve: Mutex::new(serve),
            hooks: Mutex::new(hooks),
            hooks_on: AtomicBool::new(hooks_enabled),
            widgets: publisher,
            ..Self::default()
        }
    }

    /// PRD 0015 RF4.3: the local server publishes `cost`'s summary as
    /// `local_cost`. Called once, before the server can start.
    pub fn publish_local_cost(&self, cost: Arc<crate::local_cost::LocalCost>) {
        self.feed.attach_cost(cost);
    }

    /// PRD 0018 RF2.4: `true` while a login is in progress. A native login (which owns a
    /// browser/system window and a token rotation) blocks the restart that applies an
    /// update; the interface says so and the app stays open.
    pub async fn login_in_progress(&self) -> bool {
        self.inner.lock().await.login.is_some()
    }

    /// PRD 0018 RF2.4: stop the refresh cycle cleanly before the restart that applies an
    /// update. The loops finish their current pass and return; configuration and
    /// preferences are already persisted (both writers are atomic), so nothing is lost.
    pub fn prepare_for_restart(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        self.notice_nudge.notify_waiters();
        self.status_nudge.notify_waiters();
    }

    /// Every state change ends here: the UI gets the view, the reset
    /// notice timer recomputes (PRD 0003 RF2.1), the tray icon follows
    /// the providers' outages (PRD 0011 RF4.2), the hooks see the new
    /// state (PRD 0013 RF4) and the widget snapshot is marked for its own
    /// task (PRD 0014 RF1.4, which writes without waiting for anybody).
    async fn emit(&self, app: &AppHandle) {
        // The view, the hooks' export and the journal's pace table read the
        // same recorded points: one pass over one history connection off the
        // runtime and before the lock, then one read per reading for all
        // three, and one `hooks_input` shared by the hooks and the journal.
        let hooks_on = self.hooks_on.load(Ordering::Relaxed);
        let now = chrono::Utc::now();
        let wants_input = hooks_on || {
            let g = self.inner.lock().await;
            g.loaded
                .as_ref()
                .is_some_and(|l| l.monitor.history_recording() && l.monitor.history().is_some())
        };
        let seen = self.prefetch_recent(wants_input.then_some(now)).await;
        let (mut view, watched, journaled) = {
            let g = self.inner.lock().await;
            // PRD 0012 RF5.1: the local server serves this same state.
            if self.serving.load(Ordering::Relaxed) {
                self.feed.publish(g.feed());
            }
            let memo = RecentMemo::with(g.loaded.as_ref().map(|l| &*l.monitor), seen);
            let recent = |index: usize, metrics: &[Metric], at| memo.read(index, metrics, at);
            // PRD 0026 RF3.2: the journal sees the state while history records.
            let journal_store = g
                .loaded
                .as_ref()
                .filter(|l| l.monitor.history_recording())
                .and_then(|l| l.monitor.history());
            let input = (hooks_on || journal_store.is_some())
                .then(|| g.hooks_input_with(now, &recent))
                .flatten();
            let journaled = journal_store.and_then(|store| {
                input
                    .as_ref()
                    .map(|(_, export, table)| (store, export.clone(), table.clone()))
            });
            let watched = hooks_on.then_some(input).flatten();
            (g.view_with(&recent), watched, journaled)
        };
        if let Some((store, export, table)) = journaled {
            // PRD 0024 RF5: the journal sees projected exhaustions too.
            let pace = |index: usize, _: &Report| {
                let position = export
                    .accounts
                    .iter()
                    .position(|account| account.index == index)?;
                table.get(position)?.clone()
            };
            let _ =
                self.journal
                    .lock()
                    .await
                    .observe_with(&store, &export, export.generated_at, pace);
        }
        if let Some((path, export, table)) = watched {
            let mut hooks = self.hooks.lock().await;
            let pace = |index: usize, _: &Report| {
                let position = export
                    .accounts
                    .iter()
                    .position(|account| account.index == index)?;
                table.get(position)?.clone()
            };
            if hooks.observe_with(&path, &export, export.generated_at, &pace) {
                let badge = hooks.view(Some(&path)).badge();
                drop(hooks);
                self.inner.lock().await.hooks = badge;
                view.hooks = badge;
            }
        }
        // PRD 0025 RF3: the menu bar follows every state (title, image, badge).
        crate::tray_bar::update(app, std::mem::take(&mut view.tray_accounts));
        view.tray = tray_view();
        let _ = app.emit(EVENT, view);
        self.notice_nudge.notify_one();
        // PRD 0014 RF1.4: the snapshot follows the state changes of its own
        // task, which coalesces them and writes at most once per interval.
        self.widgets.mark_dirty();
    }

    /// PRD 0014 RF4.1/RF2.2: the widget publishing state, the file location
    /// and whether it is the *app group* an extension can read.
    pub async fn widgets_view(&self) -> WidgetsView {
        widgets::view(crate::settings::widget_prefs())
    }

    /// PRD 0014 RF4.1: turn widget publishing on or off (saved in
    /// `desktop.json`, off by default), choose whether identity travels
    /// with it and how many accounts it publishes. Turning it off deletes
    /// the file; turning it on publishes at once. Errors:
    /// `invalid_widget_accounts`, `unwritable`.
    pub async fn set_widgets(
        &self,
        app: &AppHandle,
        enabled: bool,
        include_identity: bool,
        max_accounts: usize,
    ) -> Result<WidgetsView, String> {
        let prefs = crate::settings::widget_prefs()
            .with_choices(enabled, include_identity, max_accounts)
            .map_err(str::to_owned)?;
        crate::settings::save_widgets(&prefs).map_err(str::to_owned)?;
        self.widgets.set_enabled(enabled);
        let path = widgets::snapshot_path();
        if enabled {
            self.widgets.mark_dirty();
        } else {
            // RF2.2: with the feature off no file is written and an
            // existing one is gone.
            self.widgets.reset();
            widgets::delete(path.as_deref());
        }
        self.emit(app).await;
        Ok(widgets::view(prefs))
    }

    /// PRD 0014 RF4.1: "Delete widget data". Removes the file now; while
    /// publishing is on the next state change writes it again.
    pub async fn delete_widget_data(&self, app: &AppHandle) -> Result<(), String> {
        let deleted = widgets::delete(widgets::snapshot_path().as_deref());
        self.widgets.mark_dirty();
        self.emit(app).await;
        if !deleted {
            // Nothing to remove is not a failure; the state still repaints.
            eprintln!("cuotai: there was no widget data to delete");
        }
        Ok(())
    }

    /// PRD 0014 RF1.4/RF3.2: the widget publisher, beside
    /// [`Self::run_scheduler`]. It never queries a provider: it reads the
    /// state the app already shows, projects it (with the recent series of
    /// the binding metric while the history records) and writes the file
    /// off the async runtime. Changes inside the rate-limit window are
    /// coalesced into the next write, and while the switch is off the task
    /// stays quiet and the file stays deleted.
    pub async fn run_widgets(self: Arc<Self>) {
        loop {
            if !self.widgets.is_dirty() {
                self.widgets.wait_nudged().await;
                continue;
            }
            if !self.widgets.is_enabled() {
                // RF2.2: off, nothing is written (and `set_widgets` removed
                // the file); the pending change is dropped.
                self.widgets.reset();
                continue;
            }
            if self.widgets.wait(Instant::now()).is_some() {
                self.widgets.wait_for_window().await;
                continue;
            }
            self.widgets.take_dirty();
            let prefs = crate::settings::widget_prefs();
            if self.write_widget_snapshot(prefs).await {
                self.widgets.wrote(Instant::now());
            }
        }
    }

    /// Writes the current state once. `true` when the file is on disk and
    /// publishing is still on (a change that turned it off while the
    /// payload was being built removes the file instead).
    async fn write_widget_snapshot(&self, prefs: WidgetPrefs) -> bool {
        let Some(path) = widgets::snapshot_path() else {
            return false;
        };
        let payload = {
            let g = self.inner.lock().await;
            let Some(l) = g.loaded.as_ref() else {
                return false;
            };
            let Some(identity) = l.file.identity().ok() else {
                return false;
            };
            let now = chrono::Utc::now();
            let export = Export::from_cycle(
                &l.file.config,
                &identity,
                &l.cycle,
                &l.monitor.incident_statuses(),
                now,
                ExportOptions {
                    include_identity: prefs.include_identity,
                },
            );
            let mut snapshot = cuotai_core::widget_snapshot::WidgetSnapshot::from_export(
                &export,
                prefs.max_accounts,
            );
            // PRD 0014 RF3.2: the binding metric's recent series, only while
            // this device records history.
            if let Some(store) = l.monitor.history() {
                l.monitor.flush_history(HISTORY_FLUSH);
                attach_history(&mut snapshot, &export, &store, &l.file.config.accounts);
            }
            snapshot
        };
        if widgets::publish(payload, path.clone()).await.is_err() {
            return false;
        }
        if crate::settings::widget_prefs().enabled {
            true
        } else {
            // The switch turned off while the payload was being built.
            widgets::delete(Some(&path));
            false
        }
    }

    /// PRD 0013 RF4: tell Settings about each hook run. Called once at
    /// setup, before any state change can start the hooks.
    pub fn notify_hook_runs(&self, app: AppHandle) {
        if let Ok(mut hooks) = self.hooks.try_lock() {
            hooks.set_on_run(Arc::new(move |_, _| {
                let _ = app.emit(HOOKS_EVENT, ());
            }));
        }
    }

    /// Make the hooks match their switch, file and confirmations, then show
    /// the outcome everywhere (the next state change gives newly started
    /// rules their baseline).
    async fn settle_hooks(&self, app: &AppHandle, change: impl FnOnce(&mut Hooks)) -> HooksView {
        let path = self.inner.lock().await.hooks_path();
        let view = {
            let mut hooks = self.hooks.lock().await;
            change(&mut hooks);
            hooks.reconcile(path.as_deref());
            self.hooks_on.store(hooks.is_enabled(), Ordering::Relaxed);
            let view = hooks.view(path.as_deref());
            self.inner.lock().await.hooks = view.badge();
            view
        };
        self.emit(app).await;
        view
    }

    /// PRD 0013 RF4.1: the hooks as Settings shows them.
    pub async fn hooks_view(&self, app: &AppHandle) -> HooksView {
        self.ensure_loaded().await;
        self.settle_hooks(app, |_| {}).await
    }

    /// PRD 0013 RF3.2/RF4: turn the hooks on or off (saved in
    /// `desktop.json`, off by default). On runs only rules already
    /// confirmed; off stops them and kills the running ones. Error:
    /// `unwritable`.
    pub async fn set_hooks_enabled(&self, app: &AppHandle, on: bool) -> Result<HooksView, String> {
        crate::settings::save_hooks_enabled(on).map_err(str::to_owned)?;
        Ok(self.settle_hooks(app, |hooks| hooks.set_enabled(on)).await)
    }

    /// PRD 0013 RF4.2: the user confirmed, in the dialog, the rules listed
    /// under `digest`; they must still be the file's. Error: the core's
    /// `hooks.error.*` text as JSON.
    pub async fn confirm_hooks(
        &self,
        app: &AppHandle,
        digest: String,
    ) -> Result<HooksView, String> {
        self.ensure_loaded().await;
        let path = self.inner.lock().await.hooks_path();
        self.hooks
            .lock()
            .await
            .confirm(path.as_deref(), &digest)
            .map_err(|e| l10n_to_json(&e))?;
        Ok(self.settle_hooks(app, |_| {}).await)
    }

    /// PRD 0013 RF4.1: save the editor's rules into `hooks.toml` (the core
    /// validates them and refuses a file changed since `revision`); the
    /// new rules wait for a confirmation. Errors: `no_config`, the core's
    /// `hooks.error.*` text as JSON.
    pub async fn save_hooks(
        &self,
        app: &AppHandle,
        revision: Option<String>,
        rules: Vec<cuotai_core::hooks::RuleDraft>,
    ) -> Result<HooksView, String> {
        self.ensure_loaded().await;
        let path = self
            .inner
            .lock()
            .await
            .hooks_path()
            .ok_or_else(|| "no_config".to_owned())?;
        cuotai_core::hooks::Document::save(&path, revision.as_deref(), rules)
            .map_err(|e| l10n_to_json(&e))?;
        Ok(self.settle_hooks(app, |_| {}).await)
    }

    /// PRD 0013 RF4.1: run the confirmed rule `name` alone with a sample
    /// event; the run shows in the log (and [`HOOKS_EVENT`]) when it ends.
    /// Errors: `hooks_off`, `hook_unknown`.
    pub async fn test_hook(&self, app: &AppHandle, name: String) -> Result<HooksView, String> {
        let path = self.inner.lock().await.hooks_path();
        self.hooks
            .lock()
            .await
            .test(path.as_deref(), &name)
            .map_err(str::to_owned)?;
        Ok(self.settle_hooks(app, |_| {}).await)
    }

    /// PRD 0012 RF5: the local server's state and choices.
    pub async fn serve_view(&self) -> ServeView {
        self.serve.lock().await.view()
    }

    /// PRD 0012 RF5.1: at launch, start the local server when the device
    /// left it on (off by default: nothing listens otherwise).
    pub async fn start_serve(self: Arc<Self>, app: AppHandle) {
        let mut serve = self.serve.lock().await;
        let prefs = serve.prefs();
        if prefs.enabled {
            self.ensure_loaded().await;
            self.apply_serve(&app, &mut serve, prefs).await;
        }
    }

    /// PRD 0012 RF5.1: turn the local server on or off, change its port or
    /// the "allow browsers" mode (saved in `desktop.json`) and restart it
    /// with them. A port that cannot be bound is the returned state, not an
    /// error. Errors: `invalid_port`, `unwritable`.
    pub async fn set_serve(
        &self,
        app: &AppHandle,
        enabled: bool,
        port: u16,
        allow_browsers: bool,
    ) -> Result<ServeView, String> {
        let mut serve = self.serve.lock().await;
        let prefs = serve
            .prefs()
            .with_choices(enabled, port, allow_browsers)
            .map_err(str::to_owned)?;
        crate::settings::save_serve(&prefs).map_err(str::to_owned)?;
        Ok(self.apply_serve(app, &mut serve, prefs).await)
    }

    /// PRD 0031 RF11.2.5: "Share usage statistics": also serve `/v1/stats`
    /// (saved as `serve.stats` in `desktop.json`), which the server answers
    /// only with a token. Error: `unwritable`.
    pub async fn set_serve_stats(&self, app: &AppHandle, stats: bool) -> Result<ServeView, String> {
        let mut serve = self.serve.lock().await;
        let prefs = ServePrefs {
            stats,
            ..serve.prefs()
        };
        crate::settings::save_serve(&prefs).map_err(str::to_owned)?;
        Ok(self.apply_serve(app, &mut serve, prefs).await)
    }

    /// PRD 0012 RF5.1: generate a token into the OS vault (replacing, and
    /// so invalidating, the previous one) and require it. Errors: the core's
    /// vault error as JSON, `unwritable`.
    pub async fn generate_serve_token(&self, app: &AppHandle) -> Result<ServeView, String> {
        let mut serve = self.serve.lock().await;
        serve
            .store_new_token()
            .await
            .map_err(|e| l10n_to_json(&e.l10n()))?;
        let prefs = ServePrefs {
            token: true,
            ..serve.prefs()
        };
        crate::settings::save_serve(&prefs).map_err(str::to_owned)?;
        Ok(self.apply_serve(app, &mut serve, prefs).await)
    }

    /// PRD 0012 RF5.1: revoke the token: remove it from the vault and serve
    /// without one (loopback programs only, never web pages). Errors: the
    /// core's vault error as JSON, `unwritable`.
    pub async fn revoke_serve_token(&self, app: &AppHandle) -> Result<ServeView, String> {
        let mut serve = self.serve.lock().await;
        serve
            .forget_token()
            .await
            .map_err(|e| l10n_to_json(&e.l10n()))?;
        let prefs = ServePrefs {
            token: false,
            ..serve.prefs()
        };
        crate::settings::save_serve(&prefs).map_err(str::to_owned)?;
        Ok(self.apply_serve(app, &mut serve, prefs).await)
    }

    /// PRD 0012 RF5.1: the stored token, only when the user asks to copy it.
    /// Error: the core's vault error as JSON.
    pub async fn copy_serve_token(&self) -> Result<String, String> {
        let serve = self.serve.lock().await;
        let token = if serve.prefs().token {
            serve.stored_token().await
        } else {
            Err(cuotai_core::serve::ServeError::TokenMissing)
        };
        token
            .map(|token| token.expose().to_owned())
            .map_err(|e| l10n_to_json(&e.l10n()))
    }

    /// Restart the local server with `prefs` over the app's own state and
    /// show the outcome everywhere. The feed is published before the port
    /// opens and, while it is on, on every state change ([`Self::emit`]).
    async fn apply_serve(
        &self,
        app: &AppHandle,
        serve: &mut Integration,
        prefs: ServePrefs,
    ) -> ServeView {
        {
            let g = self.inner.lock().await;
            self.serving.store(prefs.enabled, Ordering::Relaxed);
            self.feed.publish(g.feed().filter(|_| prefs.enabled));
        }
        serve.apply(prefs, &self.feed).await;
        let view = serve.view();
        {
            let mut g = self.inner.lock().await;
            if !serve.is_on() {
                self.serving.store(false, Ordering::Relaxed);
                self.feed.publish(None);
            }
            g.serve = view.clone();
        }
        self.emit(app).await;
        view
    }

    /// PRD 0011 RF4.3: turn the public status queries on or off (saved in
    /// `desktop.json`). On reads the sources at once; off stops every
    /// status request and hides the badges. Error: `unwritable`.
    pub async fn set_status_enabled(&self, app: &AppHandle, on: bool) -> Result<(), String> {
        crate::settings::save_status_queries(on).map_err(str::to_owned)?;
        self.inner.lock().await.incidents.set_enabled(on);
        self.status_nudge.notify_one();
        self.emit(app).await;
        Ok(())
    }

    /// PRD 0011 RF4: the status loop, beside [`Self::run_scheduler`]. While
    /// status queries are on it lets the core read each due source of the
    /// configured providers (cached for five minutes, coalesced per URL,
    /// never touching a quota query) and shows a changed status. While off
    /// it makes no request.
    pub async fn run_incidents(self: Arc<Self>, app: AppHandle) {
        // Compare each pass with what the loop last published, not with a
        // second view of the same cache. That makes the wall-clock-only
        // transition to `expired` observable even when the due retry fails.
        let mut shown = Vec::new();
        loop {
            if self.stopping.load(Ordering::Relaxed) {
                return;
            }
            let monitor = {
                let g = self.inner.lock().await;
                g.loaded
                    .as_ref()
                    .filter(|_| g.incidents.is_enabled())
                    .map(|l| Arc::clone(&l.monitor))
            };
            if let Some(monitor) = monitor {
                let statuses = monitor.refresh_incidents().await;
                if statuses != shown {
                    shown = statuses;
                    self.emit(&app).await;
                }
            } else {
                shown.clear();
            }
            let nudged = self.status_nudge.notified();
            tokio::select! {
                () = tokio::time::sleep(STATUS_TICK) => {}
                () = nudged => {}
            }
        }
    }

    /// PRD 0003 RF2.1: the reset notice timer, beside [`Self::run_scheduler`].
    /// Each pass delivers what is due from the readings already in memory
    /// (never a provider query) and sleeps until the next notice or a nudge
    /// (refresh, configuration or preference change). The sleep is capped
    /// at [`NOTICE_NAP`] while a notice is ahead: the monotonic clock stops
    /// during a system suspension and the wall clock may be changed, so the
    /// next pass sees either soon after.
    pub async fn run_notices(self: Arc<Self>, app: AppHandle) {
        let notifier = SystemNotifier(app.clone());
        loop {
            // PRD 0018 RF2.4: no notice is due after the restart that applies an update.
            if self.stopping.load(Ordering::Relaxed) {
                return;
            }
            let tick = {
                let mut g = self.inner.lock().await;
                let Inner {
                    loaded, notices, ..
                } = &mut *g;
                loaded.as_ref().map(|l| {
                    let readings = notice_readings(&l.cycle);
                    notices.tick(&l.file.config, &readings, chrono::Utc::now(), &notifier)
                })
            };
            if tick.is_some_and(|tick| tick.delivered > 0) {
                // The tray's next notice moved on.
                let view = self.inner.lock().await.view();
                let _ = app.emit(EVENT, view);
            }
            let nudged = self.notice_nudge.notified();
            match tick.and_then(|tick| tick.next) {
                Some(at) => {
                    let wait = (at - chrono::Utc::now())
                        .to_std()
                        .unwrap_or_default()
                        .min(NOTICE_NAP);
                    tokio::select! {
                        () = tokio::time::sleep_until(tokio::time::Instant::now() + wait) => {}
                        () = nudged => {}
                    }
                }
                None => nudged.await,
            }
        }
    }

    /// Change the reset notice state (preferences, a bell, the language),
    /// then show it and recompute the timer. Errors are stable codes.
    pub async fn update_notices(
        &self,
        app: &AppHandle,
        change: impl FnOnce(&mut Notices) -> Result<(), &'static str>,
    ) -> Result<(), String> {
        change(&mut self.inner.lock().await.notices).map_err(str::to_owned)?;
        self.emit(app).await;
        Ok(())
    }

    /// Reset notices or usage alerts are on (PRD 0003 RF3.3, reused by PRD
    /// 0008 RF3.3: nothing reaches the system's notification center, the
    /// test included, before the user turns one of them on).
    pub async fn notices_enabled(&self) -> bool {
        let g = self.inner.lock().await;
        g.notices.prefs().enabled || g.notices.alerts().enabled
    }

    /// Loads the configuration if there is no valid one in memory yet.
    async fn ensure_loaded(&self) {
        let mut g = self.inner.lock().await;
        if g.loaded.is_some() {
            return;
        }
        let chosen = crate::settings::chosen_config();
        if let Err(e) = g.switch(chosen, &Locations::from_process()) {
            g.load_error = Some(e);
        }
    }

    /// Applies the `cuotai.toml` chosen in settings (`None` = OS
    /// default): reloads through the core, clears the previous configuration's state and only
    /// then persists the choice in `desktop.json`.
    pub async fn set_config_file(
        &self,
        app: &AppHandle,
        path: Option<PathBuf>,
    ) -> Result<View, String> {
        let locations = Locations::from_process();
        if locations.env.is_some() {
            return Err("env_locked".into());
        }
        if let Some(p) = &path {
            crate::settings::validate_config_file(p).map_err(str::to_owned)?;
        }
        let mut g = self.inner.lock().await;
        g.switch(path.clone(), &locations)?;
        crate::settings::save_config_file(path.as_deref()).map_err(str::to_owned)?;
        let view = g.view();
        drop(g);
        self.emit(app).await;
        Ok(view)
    }

    /// The configured accounts (for the local cost's plan prices, PRD 0030
    /// RF1); empty before a configuration is loaded.
    pub async fn accounts(&self) -> Vec<cuotai_core::Account> {
        self.ensure_loaded().await;
        self.inner
            .lock()
            .await
            .loaded
            .as_ref()
            .map(|l| l.file.config.accounts.clone())
            .unwrap_or_default()
    }

    pub async fn view(&self) -> View {
        self.ensure_loaded().await;
        let seen = self.prefetch_recent(None).await;
        let g = self.inner.lock().await;
        let memo = RecentMemo::with(g.loaded.as_ref().map(|l| &*l.monitor), seen);
        g.view_with(&|index, metrics, at| memo.read(index, metrics, at))
    }

    /// The recorded points the next view (and, with `export_at`, the hooks
    /// input) will ask for, read in one pass over one history connection
    /// off the async runtime and outside the state lock. A reading the
    /// state gains in between is read on demand by the memo.
    async fn prefetch_recent(
        &self,
        export_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> RecentSeen {
        let wanted = self.inner.lock().await.recent_wanted(export_at);
        let Some((monitor, wanted)) = wanted else {
            return RecentSeen::default();
        };
        if wanted.is_empty() {
            return RecentSeen::default();
        }
        tauri::async_runtime::spawn_blocking(move || {
            let readings: Vec<(usize, &[Metric], chrono::DateTime<chrono::Utc>)> = wanted
                .iter()
                .map(|(index, metrics, at)| (*index, metrics.as_slice(), *at))
                .collect();
            let found = monitor.recent_points_many(&readings);
            wanted
                .iter()
                .zip(found)
                .map(|((index, metrics, at), points)| {
                    ((*index, at.timestamp_millis(), metrics.len()), points)
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    /// RF4 reload path: re-read the config from disk and replace the
    /// live state with the freshly-loaded one. Used after an
    /// `EditError::Conflict` (`save`/`delete`/`move`/`set_refresh_seconds`
    /// refused to write because the in-memory snapshot diverged from
    /// disk) and as the cheap way to pick up external edits detected
    /// via [`Backend::config_changed_on_disk`].
    ///
    /// - same path and source as the live file (a later change of
    ///   `CUOTAI_CONFIG` or of the chosen file cannot silently switch
    ///   files mid-session);
    /// - aborts/clears in-flight refreshes of the previous generation
    ///   by rebuilding monitor/cycle/tokens via `install`;
    /// - drops every RF3 recovery handle (the new config may reference
    ///   the same vault entry under a different account);
    /// - on read failure leaves the live state untouched and surfaces
    ///   the typed error (`EditError::Conflict`/`Validation`/`Io`) so
    ///   the UI can keep the dialog draft and the user can decide.
    ///
    /// The new `View` is returned so the frontend can refresh without
    /// a separate round-trip.
    pub async fn reload_config(&self, app: &AppHandle) -> Result<View, String> {
        let mut g = self.inner.lock().await;
        if g.loaded.is_none() {
            // Nothing to reload yet: behave like the first boot so the
            // dialog draft survives (no "Configuração não carregada"
            // bounce back to the user). The view is rebuilt below.
            drop(g);
            self.ensure_loaded().await;
            g = self.inner.lock().await;
        }
        let Some(l) = g.loaded.as_ref() else {
            // The boot load just failed again: surface its typed error.
            return Err(g.load_error.clone().unwrap_or_default());
        };
        let resolved = ResolvedPath {
            path: l.file.path.clone(),
            source: l.file.source,
        };
        let file = ConfigFile::open(resolved, true).map_err(|e| edit_error_to_json(&e))?;
        // Carry session + load_error reset through `install` (clears
        // them on success, retains the inner state on failure). After
        // a successful `install` the cycle is fresh, the monitor is
        // new and in-flight refreshes of the old generation are
        // rejected by their tokens.
        g.install(file)?;
        let view = g.view();
        drop(g);
        self.emit(app).await;
        Ok(view)
    }

    /// Cheap detector: did the file on disk change since we last
    /// loaded or saved it? Returns `false` when no config is loaded
    /// (the very first sweep races the load) and surfaces an `Io`
    /// error so the UI can keep the dialog open.
    ///
    /// Implemented by comparing the current bytes to the snapshot
    /// the `ConfigFile` already holds (see `ConfigFile::changed_on_disk`),
    /// so a rewrite that keeps the mtime identical (some filesystems
    /// have second-level resolution) is still detected.
    pub async fn config_changed_on_disk(&self) -> Result<bool, String> {
        let g = self.inner.lock().await;
        let Some(l) = g.loaded.as_ref() else {
            return Ok(false);
        };
        l.file.changed_on_disk().map_err(|e| edit_error_to_json(&e))
    }

    /// Full round. `force` ignores the interval ("Refresh all" button).
    pub async fn refresh_all(&self, app: &AppHandle) {
        self.ensure_loaded().await;
        // Claim the sweep token BEFORE spawning the task: any edit
        // that bumps `cycle.generation` while the task is in flight
        // invalidates the token we capture here. `begin_full` returns
        // `None` when at least one slot is already busy with a
        // per-account refresh — in that case the scheduler should have
        // routed the sweep through the per-account path; we simply
        // bail.
        let (monitor, token) = {
            let mut g = self.inner.lock().await;
            let Some(l) = g.loaded.as_mut() else { return };
            let Some(token) = l.cycle.begin_full() else {
                return;
            };
            for slot in l.tokens.iter_mut() {
                *slot = Some(token);
            }
            (Arc::clone(&l.monitor), token)
        };
        self.emit(app).await;
        let snapshot = monitor.refresh().await;
        {
            let mut g = self.inner.lock().await;
            let Inner {
                loaded, notices, ..
            } = &mut *g;
            if let Some(l) = loaded.as_mut() {
                finish_full(
                    &mut l.cycle,
                    notices,
                    (&l.monitor, &l.file.config),
                    snapshot,
                    token,
                    chrono::Utc::now(),
                );
                for slot in l.tokens.iter_mut() {
                    if *slot == Some(token) {
                        *slot = None;
                    }
                }
            }
        }
        self.emit(app).await;
    }

    pub async fn refresh_account(&self, app: &AppHandle, index: usize) -> Result<(), String> {
        self.refresh_account_or_connection(app, index, None).await
    }

    /// PRD 0036 RF4.4: [`Self::refresh_account`], or only the connection
    /// `sub` of an aggregator account when one is named and the account
    /// has a current reading (one usage request instead of the instance).
    pub async fn refresh_account_or_connection(
        &self,
        app: &AppHandle,
        index: usize,
        sub: Option<String>,
    ) -> Result<(), String> {
        // Claim the per-slot token before spawn so an edit that lands
        // mid-flight rejects the eventual result via the generation
        // guard. `begin` returns `None` when the slot is already busy;
        // the caller can retry later.
        let (monitor, token, previous) = {
            let mut g = self.inner.lock().await;
            let l = g
                .loaded
                .as_mut()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            if index >= l.file.config.accounts.len() {
                return Err(crate::desk_text::no_account());
            }
            let previous = sub
                .as_ref()
                .and_then(|_| l.cycle.account_report(index).cloned());
            // Allow refresh whenever the slot is pending OR the cycle
            // already has a snapshot — never bail out just because the
            // public snapshot is hidden by a partial edit.
            let Some(token) = l.cycle.begin(index) else {
                return Ok(());
            };
            l.tokens[index] = Some(token);
            (Arc::clone(&l.monitor), token, previous)
        };
        self.emit(app).await;
        let report = match (sub.as_deref(), previous.as_ref()) {
            (Some(sub), Some(previous)) => monitor.refresh_connection(index, sub, previous).await,
            _ => monitor.refresh_account(index).await,
        };
        {
            let mut g = self.inner.lock().await;
            let Inner {
                loaded, notices, ..
            } = &mut *g;
            if let Some(l) = loaded.as_mut() {
                finish_account(
                    &mut l.cycle,
                    notices,
                    (&l.monitor, &l.file.config),
                    index,
                    report,
                    token,
                    chrono::Utc::now(),
                );
                if let Some(slot) = l.tokens.iter_mut().find(|slot| **slot == Some(token)) {
                    *slot = None;
                }
            }
        }
        self.emit(app).await;
        Ok(())
    }

    /// Background loop: first full round, then each account on its own
    /// interval. After a partial edit, the cycle has a hidden snapshot
    /// (pending slots) and the scheduler must only fire the per-account
    /// refreshes that are actually due — never a full sweep, which would
    /// re-query every account.
    ///
    /// The scheduler respects the cycle's per-slot backoff: a failed
    /// task pushes the deadline forward; the scheduler skips the slot
    /// until the deadline has elapsed (see
    /// [`RefreshCycle::due_indices`] and
    /// [`RefreshCycle::pending_now_indices`]).
    pub async fn run_scheduler(self: Arc<Self>, app: AppHandle) {
        loop {
            // PRD 0018 RF2.4: the restart that applies an update stops the cycle.
            if self.stopping.load(Ordering::Relaxed) {
                return;
            }
            self.ensure_loaded().await;
            let plan: SchedulerPlan = 'plan: {
                let g = self.inner.lock().await;
                let Some(l) = g.loaded.as_ref() else {
                    // No valid configuration: wait for the next tick (no tight loop).
                    break 'plan SchedulerPlan::Idle;
                };
                if l.file.config.accounts.is_empty() {
                    SchedulerPlan::Idle
                } else {
                    let now = Instant::now();
                    let can_full = needs_initial_sweep(&l.cycle, &l.tokens);
                    if can_full {
                        // First round: no data, no full round
                        // attempted and nothing in flight. Run right away, without waiting
                        // for the periodic deadline (which only applies to the next ones).
                        SchedulerPlan::Full
                    } else {
                        // Deadline-aware pending (edit-driven) + periodic
                        // due (Some-slot, deadline elapsed). Both skip
                        // slots that already have an in-flight refresh.
                        let mut all: Vec<usize> = l
                            .cycle
                            .pending_now_indices(now)
                            .into_iter()
                            .chain(l.cycle.due_indices(now))
                            .collect();
                        all.retain(|i| l.tokens.get(*i).and_then(Option::as_ref).is_none());
                        all.sort_unstable();
                        all.dedup();
                        if all.is_empty() {
                            SchedulerPlan::Idle
                        } else {
                            SchedulerPlan::Accounts(all)
                        }
                    }
                }
            };
            match plan {
                SchedulerPlan::Idle => {}
                SchedulerPlan::Full => self.refresh_all(&app).await,
                SchedulerPlan::Accounts(indexes) => {
                    for index in indexes {
                        let (b, a) = (Arc::clone(&self), app.clone());
                        tauri::async_runtime::spawn(async move {
                            let _ = b.refresh_account(&a, index).await;
                        });
                    }
                }
            }
            tokio::time::sleep(TICK).await;
        }
    }

    // ---------- local extensions (PRD 0017 RF5) ----------

    pub async fn extensions(
        &self,
    ) -> Result<Vec<cuotai_core::extensions::management::ExtensionItem>, String> {
        self.ensure_loaded().await;
        let g = self.inner.lock().await;
        let loaded = g.loaded.as_ref().ok_or("config_unavailable")?;
        let manager = loaded
            .monitor
            .extension_manager()
            .ok_or("extensions_unavailable")?;
        Ok(manager.list(&loaded.file.config))
    }

    pub async fn extension_disclosure(
        &self,
        id: &str,
    ) -> Result<cuotai_core::extensions::management::Disclosure, String> {
        self.ensure_loaded().await;
        let g = self.inner.lock().await;
        let loaded = g.loaded.as_ref().ok_or("config_unavailable")?;
        loaded
            .monitor
            .extension_manager()
            .ok_or("extensions_unavailable")?
            .disclosure(id)
            .map_err(|note| l10n_to_json(&note))
    }

    pub async fn enable_extension(
        &self,
        app: &AppHandle,
        candidate: &cuotai_core::extensions::management::Candidate,
    ) -> Result<cuotai_core::extensions::management::ActionResult, String> {
        self.ensure_loaded().await;
        let (action, index) = {
            let mut g = self.inner.lock().await;
            let Inner {
                loaded: Some(loaded),
                generation,
                ..
            } = &mut *g
            else {
                return Err("config_unavailable".into());
            };
            let manager = loaded
                .monitor
                .extension_manager()
                .cloned()
                .ok_or("extensions_unavailable")?;
            let prev = loaded.file.config.clone();
            // RF5.3: the approval binds to the disclosed snapshot, so a file
            // changed in between fails here instead of running unseen.
            let cuotai_core::extensions::management::EnableResult {
                action,
                config,
                consent_rollback,
            } = manager
                .enable(candidate, true, &prev)
                .map_err(|note| l10n_to_json(&note))?;
            let id = action.id.clone();
            // Approval and configuration persistence are one action: core knows
            // exactly what its approval step changed and restores that when
            // persistence loses. A rollback that itself fails (a newer
            // approval landed meanwhile) is reported with the original
            // error, never swallowed.
            // PRD 0028 RF7.1: an override extension creates no account; only
            // the approval changed, and it is already committed.
            if action.account.is_none() {
                manager.confirm_commit(&id);
                drop(g);
                self.emit(app).await;
                return Ok(action);
            }
            let index = action
                .account
                .as_ref()
                .map(|account| account.index)
                .ok_or("extension_account_missing")?;
            let mut cycle = loaded.cycle.clone();
            if let Err(error) =
                cycle.apply_edit(&prev, &config, Edit::Upsert(index), Instant::now())
            {
                let rollback = manager.rollback(consent_rollback);
                return Err(rollback_with(
                    &rollback,
                    refresh_edit_error(&error, prev.accounts.len()),
                ));
            }
            let monitor = match loaded.monitor.with_config(config.clone()).map(Arc::new) {
                Ok(monitor) => monitor,
                Err(error) => {
                    let rollback = manager.rollback(consent_rollback);
                    return Err(rollback_with(&rollback, config_error_note(&error)));
                }
            };
            if let Err(error) = loaded.file.save(config) {
                let rollback = manager.rollback(consent_rollback);
                return Err(rollback_edit(&rollback, error));
            }
            // Persistence succeeded: only now does the newly approved
            // version replace the old one's cached failure and backoff.
            manager.confirm_commit(&id);
            loaded.cycle = cycle;
            loaded.monitor = monitor;
            if loaded.tokens.len() < loaded.file.config.accounts.len() {
                loaded.tokens.extend(
                    (0..loaded.file.config.accounts.len() - loaded.tokens.len()).map(|_| None),
                );
            }
            loaded.generation += 1;
            *generation = loaded.generation;
            (action, index)
        };
        self.emit(app).await;
        self.refresh_account(app, index).await?;
        Ok(action)
    }

    pub async fn disable_extension(
        &self,
        app: &AppHandle,
        id: &str,
    ) -> Result<cuotai_core::extensions::management::ActionResult, String> {
        self.ensure_loaded().await;
        let (action, index) = {
            let g = self.inner.lock().await;
            let loaded = g.loaded.as_ref().ok_or("config_unavailable")?;
            let manager = loaded
                .monitor
                .extension_manager()
                .ok_or("extensions_unavailable")?;
            // PRD 0028 RF7.4: revoking also deletes the extension's vault
            // secrets, restored by core when the revocation cannot be saved.
            let action = match &g.session {
                Some(session) => manager.disable_with_secrets(id, session.token_store().as_ref()),
                None => manager.disable(id),
            }
            .map_err(|note| l10n_to_json(&note))?;
            let index = loaded
                .file
                .config
                .accounts
                .iter()
                .position(|account| account.extension_id() == Some(id));
            (action, index)
        };
        self.emit(app).await;
        if let Some(index) = index {
            self.refresh_account(app, index).await?;
        }
        Ok(action)
    }

    pub async fn test_extension(
        &self,
        app: &AppHandle,
        id: &str,
    ) -> Result<cuotai_core::extensions::output::Checked, String> {
        self.ensure_loaded().await;
        let (manager, store) = {
            let g = self.inner.lock().await;
            let manager = g
                .loaded
                .as_ref()
                .and_then(|loaded| loaded.monitor.extension_manager().cloned())
                .ok_or("extensions_unavailable")?;
            (
                manager,
                g.session.as_ref().map(|session| session.token_store()),
            )
        };
        let checked = manager
            .test_reading_with(id, store)
            .await
            .map(|reading| reading.checked)
            .map_err(|note| l10n_to_json(&note))?;
        self.emit(app).await;
        Ok(checked)
    }

    /// PRD 0028 RF7.1: use (`on`) or stop using the override extension `id`
    /// for the account at `index` ("Use extension override"), saved like any
    /// other account edit.
    pub async fn set_extension_override(
        &self,
        app: &AppHandle,
        index: usize,
        id: &str,
        on: bool,
    ) -> Result<(), String> {
        self.ensure_loaded().await;
        {
            let mut g = self.inner.lock().await;
            let Inner {
                loaded: Some(loaded),
                generation,
                ..
            } = &mut *g
            else {
                return Err("config_unavailable".into());
            };
            let manager = loaded
                .monitor
                .extension_manager()
                .cloned()
                .ok_or("extensions_unavailable")?;
            let prev = loaded.file.config.clone();
            let updated = manager
                .set_override(&prev, index, id, on)
                .map_err(|note| l10n_to_json(&note))?;
            let mut cycle = loaded.cycle.clone();
            cycle
                .apply_edit(&prev, &updated, Edit::Upsert(index), Instant::now())
                .map_err(|error| l10n_to_json(&refresh_edit_error(&error, prev.accounts.len())))?;
            let monitor = loaded
                .monitor
                .with_config(updated.clone())
                .map(Arc::new)
                .map_err(|error| l10n_to_json(&config_error_note(&error)))?;
            loaded
                .file
                .save(updated)
                .map_err(|e| edit_error_to_json(&e))?;
            loaded.cycle = cycle;
            loaded.monitor = monitor;
            loaded.generation += 1;
            *generation = loaded.generation;
        }
        self.emit(app).await;
        self.refresh_account(app, index).await?;
        Ok(())
    }

    /// PRD 0028 RF7.4: the secrets an extension declares, each with whether
    /// the vault holds a value (never the value).
    pub async fn extension_secrets(
        &self,
        id: &str,
    ) -> Result<Vec<cuotai_core::extensions::management::SecretItem>, String> {
        self.ensure_loaded().await;
        let g = self.inner.lock().await;
        let loaded = g.loaded.as_ref().ok_or("config_unavailable")?;
        let manager = loaded
            .monitor
            .extension_manager()
            .ok_or("extensions_unavailable")?;
        let session = g.session.as_ref().ok_or("extensions_unavailable")?;
        manager
            .secrets(id, session.token_store().as_ref())
            .map_err(|note| l10n_to_json(&note))
    }

    /// PRD 0028 RF7.4: keep one secret's value in the vault through core.
    pub async fn set_extension_secret(
        &self,
        id: &str,
        name: &str,
        value: &str,
    ) -> Result<cuotai_core::extensions::management::ActionResult, String> {
        self.ensure_loaded().await;
        let g = self.inner.lock().await;
        let loaded = g.loaded.as_ref().ok_or("config_unavailable")?;
        let manager = loaded
            .monitor
            .extension_manager()
            .ok_or("extensions_unavailable")?;
        let session = g.session.as_ref().ok_or("extensions_unavailable")?;
        manager
            .set_secret(id, name, value, session.token_store().as_ref())
            .map_err(|note| l10n_to_json(&note))
    }

    /// PRD 0028 RF7.4: deletes the vault secrets of extension `id` (offered
    /// when its account is removed); returns how many entries were deleted.
    pub async fn forget_extension_secrets(&self, id: &str) -> Result<usize, String> {
        self.ensure_loaded().await;
        let g = self.inner.lock().await;
        let loaded = g.loaded.as_ref().ok_or("config_unavailable")?;
        let manager = loaded
            .monitor
            .extension_manager()
            .ok_or("extensions_unavailable")?;
        let session = g.session.as_ref().ok_or("extensions_unavailable")?;
        manager
            .forget_secrets(id, session.token_store().as_ref())
            .map_err(|note| l10n_to_json(&note))
    }

    // ---------- account editing ----------

    pub async fn save_account(
        &self,
        app: &AppHandle,
        input: AccountInput,
    ) -> Result<SaveAccountResult, String> {
        self.ensure_loaded().await;
        // PRD 0022 MONO-4: validated before anything is saved.
        let monogram = match input.monogram.as_deref() {
            Some(text) => Some(
                crate::settings::clean_monogram(text)
                    .ok_or_else(crate::desk_text::invalid_monogram)?,
            ),
            None => None,
        };
        let (provider, name) = (input.provider, input.name.trim().to_owned());
        let clearance = self.test_before_save(&input).await?;
        let (refresh_target, cleanup_warning) = self.drive_save_account(input, clearance).await?;
        if let Some(monogram) = monogram {
            self.store_monogram(provider, &name, &monogram).await;
            let _ = app.emit(
                "settings-changed",
                serde_json::json!({ "key": "monograms" }),
            );
        }
        self.emit(app).await;
        if let Some(index) = refresh_target {
            self.refresh_account(app, index).await?;
        }
        Ok(SaveAccountResult { cleanup_warning })
    }

    /// PRD 0022 MONO-4: keeps the monogram of the account just saved under
    /// its stable id. Best effort: the account is already saved, and a
    /// failed write only leaves the monogram derived from the name.
    async fn store_monogram(&self, provider: Provider, name: &str, monogram: &str) {
        let id = {
            let g = self.inner.lock().await;
            g.loaded.as_ref().and_then(|l| {
                l.file
                    .config
                    .accounts
                    .iter()
                    .find(|a| a.provider == provider && a.name.trim() == name)
                    .map(|a| a.id().to_string())
            })
        };
        if let Some(id) = id {
            let _ = crate::settings::save_monogram(&id, monogram);
        }
    }

    /// SES-02: the core's test before save of the dialog's pasted sessions
    /// (one read-only query, outside the lock), `None` without a paste. A
    /// refusal is the core's note: `user_session.save.unreachable` lets the
    /// dialog offer "Save anyway" (`saveAnyway`), any other one blocks.
    pub(crate) async fn test_before_save(
        &self,
        input: &AccountInput,
    ) -> Result<Option<user_session::SaveClearance>, String> {
        let Some((account, monitor, pasted)) = self.session_probe(input).await? else {
            return Ok(None);
        };
        user_session::test_before_save(&monitor, &account, &pasted, input.save_anyway)
            .await
            .map_err(|refused| l10n_to_json(&refused.note))
    }

    /// The dialog's draft account, the monitor to query it and its reduced
    /// pasted sessions; `None` without a paste.
    async fn session_probe(&self, input: &AccountInput) -> Result<Option<SessionProbe>, String> {
        let pasted = reduce_input(input)?;
        if pasted.is_empty() {
            return Ok(None);
        }
        let g = self.inner.lock().await;
        let l = g
            .loaded
            .as_ref()
            .ok_or_else(crate::desk_text::config_not_loaded)?;
        let completed = g
            .login
            .as_ref()
            .filter(|p| {
                input.commit_login
                    && p.provider == input.provider
                    && p.index == input.index
                    && p.attempt == input.login_attempt
            })
            .and_then(|p| p.completed.as_ref());
        let account = input_account(&l.file.config, input, completed)?;
        Ok(Some((account, Arc::clone(&l.monitor), pasted)))
    }

    /// Testable half of [`Backend::save_account`]: the vault (a committed
    /// login, PRD 0016 pasted sessions), the TOML and the staged runtime;
    /// returns the account to refresh and a cleanup warning. SES-02: pasted
    /// sessions are written only with the `clearance` of
    /// [`Backend::test_before_save`].
    pub(crate) async fn drive_save_account(
        &self,
        input: AccountInput,
        clearance: Option<user_session::SaveClearance>,
    ) -> Result<(Option<usize>, Option<cuotai_core::text::Localized>), String> {
        // Everything below runs under ONE inner.lock so a concurrent
        // refresh / scheduler tick cannot observe a half-applied edit.
        // The commit path holds the lock across `commit_login`'s vault
        // write (spawn_blocking against the local vault, brief): the
        // desktop is single-user with a modal dialog, and keeping the
        // lock avoids any half-applied window between the vault and
        // TOML stages.
        let saved: (Option<usize>, Option<cuotai_core::text::Localized>) = {
            let mut g = self.inner.lock().await;
            // Split borrows: pending login, session and loaded config are
            // disjoint fields.
            let Inner {
                login,
                loaded,
                session,
                generation,
                notices,
                aggregator_tested,
                ..
            } = &mut *g;
            let l = loaded
                .as_mut()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            // RF2: a `commit_login` request must find the pending login
            // of THIS account/provider, from the SAME attempt (the `attempt` prevents
            // a stale forwarder from mistaking an already replaced login for
            // another one). The `CompletedLogin` (opaque token) never leaves
            // `PendingLogin`. Modes without a Cuotai credential (aws_sso /
            // bailian_console) are consumed by the save with no vault
            // write.
            let pending_match_ref = login.as_ref().filter(|p| {
                p.provider == input.provider
                    && p.index == input.index
                    && p.attempt == input.login_attempt
            });
            if input.commit_login {
                let Some(p) = pending_match_ref else {
                    return Err(crate::desk_text::pending_missing());
                };
                if !p.done {
                    return Err(crate::desk_text::login_unfinished());
                }
            }
            let completed: Option<&CompletedLogin> = pending_match_ref
                .filter(|_| input.commit_login)
                .and_then(|p| p.completed.as_ref());
            let config = &l.file.config;
            let base = with_named_peer(config, &input)?;
            let account = input_account(&base, &input, completed)?;
            // PRD 0036 RF2.2: an aggregator that is new or reached in
            // another way is saved only after a successful Test connection
            // of exactly this account.
            if account.provider.is_aggregator() {
                let unchanged = input
                    .index
                    .and_then(|index| config.accounts.get(index))
                    .is_some_and(|before| before.same_settings(&account));
                if !unchanged
                    && !aggregator_tested
                        .as_ref()
                        .is_some_and(|ok| ok.same_settings(&account))
                {
                    return Err(l10n_to_json(&cuotai_core::text::Localized::fixed(
                        "desktop.account.aggregator_untested",
                        "Test the connection successfully before saving.",
                    )));
                }
            }
            let updated = base
                .upsert_account(account, input.index)
                .map_err(|e| l10n_to_json(&e.l10n()))?;
            let prev = config.clone();
            let edit_index = input.index.unwrap_or(prev.accounts.len());
            // PRD 0016 RF2/RF3.4: the pasted sessions, reduced by the core,
            // reach the vault first (an existing reference keeps its entry);
            // every later failure takes them out again.
            let vault = session.clone();
            let pasted = reduce_input(&input)?;
            let staged_paste = if pasted.is_empty() {
                None
            } else {
                let vault = vault
                    .as_deref()
                    .ok_or_else(crate::desk_text::oauth_session_missing)?;
                Some(
                    user_session::stage_pasted(
                        vault,
                        &updated,
                        edit_index,
                        &pasted,
                        clearance.as_ref(),
                    )
                    .await
                    .map_err(|note| l10n_to_json(&note))?,
                )
            };
            drop(pasted);
            let updated = staged_paste
                .as_ref()
                .map_or(updated, |staged| staged.config().clone());
            // FIX-06: a provider switch drops the account's session
            // reference; its vault entries go with the save (and come back,
            // with the paste undone, through the same rollback below).
            let staged_paste = match vault.as_deref() {
                Some(vault) => user_session::stage_dropped(vault, &prev, &updated, staged_paste)
                    .await
                    .map_err(|note| l10n_to_json(&note))?,
                None => staged_paste,
            };
            // Stage the new cycle on a copy so a structural-diff rejection
            // (e.g. reorder) leaves the live state untouched.
            let mut staged = l.cycle.clone_state_for_edit();
            // PRD 0029 RF3.3: the named peer first (its reading stays),
            // then the account itself.
            let peer = account_edit::unnamed_peer(&prev, input.provider, input.index);
            let renamed = match peer {
                Some(peer) => staged
                    .apply_edit(&prev, &base, Edit::Rename(peer), Instant::now())
                    .map(|_| &base),
                None => Ok(&prev),
            };
            let due = match renamed.and_then(|before| {
                staged.apply_edit(before, &updated, Edit::Upsert(edit_index), Instant::now())
            }) {
                Ok(due) => due,
                Err(e) => {
                    undo_paste(vault.as_deref(), staged_paste).await;
                    return Err(l10n_to_json(&refresh_edit_error(&e, prev.accounts.len())));
                }
            };
            let refresh_target = due.first().copied();
            // Build the new monitor. A failure here means the config is
            // structurally invalid; we abort before touching disk.
            let new_monitor = match l.monitor.with_config(updated.clone()) {
                Ok(m) => Arc::new(m),
                Err(e) => {
                    undo_paste(vault.as_deref(), staged_paste).await;
                    return Err(l10n_to_json(&cuotai_core::edit_error::config_error_note(
                        &e,
                    )));
                }
            };
            // Persist. Core owns vault→TOML ordering, compensation and the
            // post-save replaced-key cleanup/shared-key guard.
            let mut cleanup_warning = None;
            if let Some(completed) = completed {
                let session = session
                    .as_ref()
                    .map(Arc::as_ref)
                    .ok_or_else(crate::desk_text::oauth_session_missing)?;
                match login::commit_login(&mut l.file, session, completed, updated.clone()).await {
                    login::LoginCommitOutcome::Saved => {}
                    login::LoginCommitOutcome::SavedWithCleanupFailure(failure) => {
                        cleanup_warning = Some(cuotai_core::text::Localized::fixed(
                            "edit_error.login_cleanup_failed",
                            failure.message,
                        ));
                    }
                    login::LoginCommitOutcome::PartialFailure(failure) => {
                        undo_paste(vault.as_deref(), staged_paste).await;
                        return Err(l10n_to_json(&cuotai_core::text::Localized::with_param(
                            "edit_error.login_partial_failure",
                            "detail",
                            failure.message,
                            "{detail} The authorization remains in memory: click Save again to finish without signing in again.",
                        )));
                    }
                }
            } else if let Err(e) = l.file.save(updated.clone()) {
                undo_paste(vault.as_deref(), staged_paste).await;
                return Err(edit_error_to_json(&e));
            }
            // Persisted: the pending login is consumed (committed credential or no-vault
            // mode) and disappears. For `auth_mode_credential` without oauth, the
            // frontend does not send `commitLogin` and the pending login, if any, is
            // simply discarded too.
            if pending_match_ref.is_some() {
                *login = None;
            }
            // Commit: cycle + monitor + bookkeeping + generation
            // change together under the same lock acquisition. Direct bump
            // of `l.generation`; `Inner.generation` mirrors it, so
            // we propagate it last (still under the lock).
            l.cycle = staged;
            l.monitor = new_monitor;
            // Saved: an edited identity (rename, credential, region) loses its entry.
            let _ = l.monitor.prune_models();
            l.monitor.mark_history_removed();
            // PRD 0003 RF3.2: a renamed account loses its bell; best effort
            // like the model cache (an unwritable file keeps a stale key).
            let _ = notices.keep_accounts(&l.file.config);
            if l.tokens.len() < l.file.config.accounts.len() {
                l.tokens
                    .extend((0..l.file.config.accounts.len() - l.tokens.len()).map(|_| None));
            }
            if let Some(slot) = l.tokens.get_mut(edit_index) {
                *slot = None;
            }
            l.generation += 1;
            *generation = l.generation;
            (refresh_target, cleanup_warning)
        };
        Ok(saved)
    }

    /// `purge_credentials`: also delete the OAuth credential from the system vault. The core decides whether it may (a credential
    /// shared with another account is never deleted) and the result says what actually happened.
    pub async fn delete_account(
        &self,
        app: &AppHandle,
        index: usize,
        purge_credentials: bool,
    ) -> Result<DeleteResult, String> {
        self.ensure_loaded().await;
        let result = self.drive_delete_account(index, purge_credentials).await?;
        self.emit(app).await;
        Ok(result)
    }

    /// Testable inner half of [`Backend::delete_account`]: stage the
    /// removal under the inner mutex (snapshot session, validate the
    /// index, write the TOML, mirror cycle + monitor + tokens +
    /// generation), release the mutex, then run the optional vault
    /// purge without holding it. Splitting the work from `emit` lets
    /// the unit tests drive the state machine without standing up a
    /// Tauri runtime.
    pub(crate) async fn drive_delete_account(
        &self,
        index: usize,
        purge_credentials: bool,
    ) -> Result<DeleteResult, String> {
        // RF3 split: stage the removal under the inner mutex (snapshot
        // session, validate the index, write the TOML, mirror cycle +
        // monitor + tokens + generation), release the mutex, then run
        // the optional vault purge without holding it. The desktop UI
        // sends `purge_credentials = true` whenever the user picks
        // "Remover e apagar do cofre" (`dialog.deleteVaultPurge`), so
        // the vault IPC round-trip can take a moment; releasing the
        // mutex here unblocks `view`, `refresh_*` and `login_*` while
        // we wait for the OS keychain.
        let staged = {
            let mut g = self.inner.lock().await;
            let session = match &g.session {
                Some(s) => Arc::clone(s),
                None => return Err(crate::desk_text::oauth_session_missing()),
            };
            let l = g
                .loaded
                .as_mut()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            let prev = l.file.config.clone();
            if index >= prev.accounts.len() {
                return Err(crate::desk_text::no_account());
            }
            // Pre-compute the new config + cycle + monitor so a
            // failure (cycle refusal, monitor refusal, save failure)
            // short-circuits without ever touching the vault.
            let updated = prev
                .without_account(index)
                .map_err(|e| edit_error_to_json(&e))?;
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            staged_cycle
                .apply_edit(&prev, &updated, Edit::Remove(index), Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e, prev.accounts.len())))?;
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            // TOML save via the core; the core still owns the shared-key
            // guard, the partial-failure invariant and (FIX-06) the
            // deletion of the account's vault web sessions, put back when
            // the save fails. On any error here `l.file` is unchanged and
            // the staged cycle + monitor are dropped on return.
            let staged = account_edit::remove_account_save_only(
                &mut l.file,
                session.as_ref(),
                index,
                purge_credentials,
            )
            .await
            .map_err(|error| l10n_to_json(&account_edit::RemoveError::l10n(&error)))?;
            // Mirror the staged runtime into the live state. We
            // snapshot the post-save config BEFORE assigning
            // `g.generation` so the borrows do not overlap
            // (Rust's NLL tracks the lifetime of the cloned value).
            let post_save_config = l.file.config.clone();
            l.cycle = staged_cycle;
            l.monitor = Arc::new(new_monitor);
            let _ = l.monitor.prune_models();
            l.monitor.mark_history_removed();
            if index < l.tokens.len() {
                l.tokens.remove(index);
            }
            l.generation += 1;
            g.generation = l.generation;
            let _ = g.notices.keep_accounts(&post_save_config);
            (staged, session, post_save_config)
        };
        // We are OUT of the inner mutex from here. The vault purge,
        // when applicable, runs without blocking other backend
        // commands.
        let (staged, session, post_save_config) = staged;
        let sessions_removed = match &staged {
            account_edit::StagedRemoval::RemovedNoCredential(account)
            | account_edit::StagedRemoval::Kept { account, .. }
            | account_edit::StagedRemoval::ReadyForPurge { account, .. } => {
                account.sessions_removed
            }
        };
        let (result, pending_recovery) = match staged {
            account_edit::StagedRemoval::RemovedNoCredential(_) => {
                (DeleteResult::new("removed"), None)
            }
            account_edit::StagedRemoval::Kept { account, reason } => {
                let handle = mint_handle();
                let result = match reason {
                    account_edit::KeptReason::Declined => DeleteResult::new("kept"),
                    account_edit::KeptReason::SharedWith { other_account } => DeleteResult {
                        outcome: "kept_shared",
                        other_account: Some(other_account),
                        message: None,
                        sessions_removed: false,
                        recovery_handle: None,
                    },
                };
                (
                    result.with_recovery(handle.clone()),
                    Some((handle, account)),
                )
            }
            account_edit::StagedRemoval::ReadyForPurge {
                account,
                provider,
                oauth,
            } => match account_edit::purge_credential(
                session.as_ref(),
                &post_save_config,
                provider,
                &oauth,
            )
            .await
            {
                Ok(()) => (DeleteResult::new("purged"), None),
                Err(vault_error) => {
                    // Mint a recovery handle so the UI can offer a
                    // retry. The handle references the
                    // `RemovedAccount` we keep in the backend state
                    // below; the core guard still runs against the
                    // *live* config on every retry.
                    let handle = mint_handle();
                    (
                        DeleteResult {
                            outcome: "partial_failure",
                            other_account: None,
                            message: Some(vault_error.to_string()),
                            sessions_removed: false,
                            recovery_handle: Some(handle.clone()),
                        },
                        Some((handle, account)),
                    )
                }
            },
        };
        let result = DeleteResult {
            sessions_removed,
            ..result
        };
        // Re-acquire the mutex briefly to register the recovery
        // handle. The handle MUST NOT survive a config replacement
        // (`install` already clears the list); the mutex section here
        // is just bookkeeping.
        if let Some((handle, account)) = pending_recovery {
            let mut g = self.inner.lock().await;
            if let Some(l) = g.loaded.as_mut() {
                l.pending_recovery.push(RecoveryEntry { handle, account });
            }
        }
        Ok(result)
    }

    /// RF3: re-attempt a vault purge against the *current* configuration
    /// using the [`account_edit::RemovedAccount`] the backend
    /// retained when the original removal could not (or would not)
    /// touch the vault. The shared-key guard runs on every call, so a
    /// retry after the survivor was dropped succeeds and a retry
    /// while a new shared reference exists is refused with the same
    /// honest message the core uses everywhere else. The handle is
    /// consumed on success (no further retry) and left in place on
    /// failure (the UI may retry again).
    pub async fn retry_purge(
        &self,
        app: &AppHandle,
        handle: String,
    ) -> Result<DeleteResult, String> {
        self.ensure_loaded().await;
        let result = self.drive_retry(&handle).await?;
        self.emit(app).await;
        Ok(result)
    }

    /// Testable inner half of [`Backend::retry_purge`]: snapshots
    /// the recovery state under the mutex, releases it, calls the
    /// core's `purge_credential` outside the lock, then re-acquires
    /// the mutex briefly to drop the handle on success. Splitting
    /// the work from `emit` lets the unit tests drive the state
    /// machine without standing up a Tauri runtime.
    pub(crate) async fn drive_retry(&self, handle: &str) -> Result<DeleteResult, String> {
        // Snapshot the session + the live config + the recovery
        // entry under the mutex, then release it before the vault
        // round-trip (same split as `delete_account`).
        let (session, current_config, removed) = {
            let g = self.inner.lock().await;
            let session = match &g.session {
                Some(s) => Arc::clone(s),
                None => return Err(crate::desk_text::oauth_session_missing()),
            };
            let l = g
                .loaded
                .as_ref()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            let pos = l
                .pending_recovery
                .iter()
                .position(|entry| entry.handle == handle)
                .ok_or_else(crate::desk_text::no_pending_credential)?;
            let removed = l.pending_recovery[pos].account.clone();
            let current_config = l.file.config.clone();
            (session, current_config, removed)
        };
        let oauth = match removed.oauth.as_ref() {
            Some(o) => o,
            None => {
                return Err(crate::desk_text::original_without_oauth());
            }
        };
        let provider = removed.provider;
        let outcome =
            account_edit::purge_credential(session.as_ref(), &current_config, provider, oauth)
                .await;
        match outcome {
            Ok(()) => {
                // Success: drop the entry, surface the truthful
                // result. The handle is consumed so a stale UI
                // button cannot retry after we already deleted the
                // vault entry.
                let mut g = self.inner.lock().await;
                if let Some(l) = g.loaded.as_mut() {
                    l.pending_recovery.retain(|entry| entry.handle != handle);
                }
                Ok(DeleteResult::new("purged"))
            }
            Err(vault_error) => {
                // Refused (still shared) or vault failure. Keep the
                // handle so the UI can keep offering the retry; the
                // core guard will continue to refuse while the
                // survivor exists and succeed once it is dropped.
                Ok(DeleteResult {
                    outcome: "partial_failure",
                    other_account: None,
                    message: Some(vault_error.to_string()),
                    sessions_removed: false,
                    recovery_handle: Some(handle.to_string()),
                })
            }
        }
    }

    /// Changes the global refresh interval (`refresh_seconds`). Validates and writes through the core; the cycle is recreated with
    /// the same reports (no new query) and the deadlines of all accounts without their own interval start
    /// following the new value.
    pub async fn set_refresh_seconds(&self, app: &AppHandle, seconds: u64) -> Result<(), String> {
        self.ensure_loaded().await;
        {
            let mut g = self.inner.lock().await;
            let l = g
                .loaded
                .as_mut()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            let mut updated = l.file.config.clone();
            updated.refresh_seconds = seconds;
            updated
                .validate()
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file
                .save(updated.clone())
                .map_err(|e| edit_error_to_json(&e))?;
            l.cycle = rebuild_cycle_keeping_reports(&l.cycle, &updated);
            l.monitor = Arc::new(new_monitor);
            l.tokens = vec![None; updated.accounts.len()];
            l.generation += 1;
            g.generation = l.generation;
        }
        self.emit(app).await;
        Ok(())
    }

    /// PRD 0006 RF4.1: keep `days` of measurement history on this device
    /// (saved in `desktop.json`) and prune by it at once. Errors:
    /// `invalid_history_days`, `unwritable`.
    pub async fn set_history_days(&self, app: &AppHandle, days: u32) -> Result<(), String> {
        let retention = crate::settings::save_history_days(days).map_err(str::to_owned)?;
        if let Some(l) = self.inner.lock().await.loaded.as_ref() {
            l.monitor.set_history_retention(retention);
        }
        self.emit(app).await;
        Ok(())
    }

    /// PRD 0006 RF5.3: record new readings or not on this device (saved in
    /// `desktop.json`), applied at once; switching off keeps what was
    /// recorded until the user erases it. Error: `unwritable`.
    pub async fn set_history_recording(&self, app: &AppHandle, on: bool) -> Result<(), String> {
        crate::settings::save_history_recording(on).map_err(str::to_owned)?;
        {
            let mut g = self.inner.lock().await;
            if let Some(l) = g.loaded.as_ref() {
                l.monitor.set_history_recording(on);
            }
            g.history_epoch += 1;
        }
        self.emit(app).await;
        Ok(())
    }

    /// PRD 0006 RF5.3: bytes the history file takes on this device, measured
    /// once the queued jobs (a prune from a retention change) have run.
    pub async fn history_size(&self) -> Result<u64, String> {
        let (store, _) = self.history_scope(None, None).await?;
        let monitor = self
            .inner
            .lock()
            .await
            .loaded
            .as_ref()
            .map(|l| Arc::clone(&l.monitor));
        tauri::async_runtime::spawn_blocking(move || {
            if let Some(monitor) = monitor {
                monitor.flush_history(HISTORY_FLUSH);
            }
            store.size_on_disk()
        })
        .await
        .map_err(|_| l10n_to_json(&HistoryError::Storage.l10n()))?
        .map_err(|error| l10n_to_json(&error.l10n()))
    }

    /// The history store and the accounts of `index` (every account with
    /// `None`) of the loaded configuration. An index only names an account
    /// in the configuration `generation` it was read from: any other
    /// generation (a move, save, removal or reload since, possibly from
    /// another window while Settings is open) yields [`STALE`], so an export
    /// or an erasure never lands on the account that took its place.
    async fn history_scope(
        &self,
        index: Option<usize>,
        generation: Option<u64>,
    ) -> Result<(HistoryStore, Vec<cuotai_core::Account>), String> {
        self.ensure_loaded().await;
        let g = self.inner.lock().await;
        let l = g
            .loaded
            .as_ref()
            .ok_or_else(crate::desk_text::config_not_loaded)?;
        if index.is_some() && generation != Some(l.generation) {
            return Err(STALE.into());
        }
        let store = l
            .monitor
            .history()
            .ok_or_else(|| l10n_to_json(&cuotai_core::history::HistoryError::Storage.l10n()))?;
        let accounts = match index {
            Some(index) => vec![l
                .file
                .config
                .accounts
                .get(index)
                .cloned()
                .ok_or_else(crate::desk_text::no_account)?],
            None => l.file.config.accounts.clone(),
        };
        Ok((store, accounts))
    }

    /// PRD 0006 RF4.4: write the history of account `index` (every account
    /// with `None`) of the last `days` days (everything with `None`) to the
    /// absolute `path` the user chose, in `format`; the file is private
    /// (0600 on Unix). Returns the rows written.
    pub async fn history_export(
        &self,
        index: Option<usize>,
        generation: Option<u64>,
        days: Option<u32>,
        format: ExportFormat,
        path: PathBuf,
    ) -> Result<usize, String> {
        let export_error = || l10n_to_json(&cuotai_core::history::HistoryError::Export.l10n());
        if !path.is_absolute() {
            return Err(export_error());
        }
        let (store, accounts) = self.history_scope(index, generation).await?;
        let since = days.map(|days| chrono::Utc::now() - chrono::TimeDelta::days(days.into()));
        tauri::async_runtime::spawn_blocking(move || {
            let file =
                private_file(&path).map_err(|_| cuotai_core::history::HistoryError::Export)?;
            let mut out = std::io::BufWriter::new(file);
            let start = since.map_or(Bound::Unbounded, Bound::Included);
            store.export(&accounts, None, (start, Bound::Unbounded), format, &mut out)
        })
        .await
        .map_err(|_| export_error())?
        .map_err(|error| l10n_to_json(&error.l10n()))
    }

    /// PRD 0006 RF4.4: erase the history of account `index` (and the
    /// orphaned series), or the whole history file with `None`; the UI
    /// asks for confirmation first. Returns the samples removed.
    pub async fn history_erase(
        &self,
        app: &AppHandle,
        index: Option<usize>,
        generation: Option<u64>,
    ) -> Result<usize, String> {
        let (store, accounts) = self.history_scope(index, generation).await?;
        let removed = tauri::async_runtime::spawn_blocking(move || match index {
            Some(_) => store.erase_account(&accounts[0]),
            None => store.erase_all(),
        })
        .await
        .map_err(|_| l10n_to_json(&cuotai_core::history::HistoryError::Storage.l10n()))?
        .map_err(|error| l10n_to_json(&error.l10n()))?;
        self.inner.lock().await.history_epoch += 1;
        self.emit(app).await;
        Ok(removed)
    }

    /// PRD 0006 RF5.1: the recorded history of account `index` of the
    /// configuration `generation` over `range` (every metric, or only
    /// `metric_key`), with the chart the core placed. It first waits
    /// briefly for the readings already queued, so the reading on screen is
    /// in it. Another generation before or after the read yields [`STALE`]
    /// (the model catalog's rule: a result of an old identity is never
    /// attributed to the account now at `index`); an unknown index or a
    /// storage failure is the core's typed error.
    pub async fn account_history(
        &self,
        index: usize,
        generation: u64,
        range: HistoryRange,
        metric_key: Option<String>,
    ) -> Result<HistoryView, String> {
        let monitor = {
            let g = self.inner.lock().await;
            let l = g
                .loaded
                .as_ref()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            if l.generation != generation {
                return Err(STALE.into());
            }
            if index >= l.file.config.accounts.len() {
                return Err(l10n_to_json(&ModelError::InvalidIndex.l10n()));
            }
            Arc::clone(&l.monitor)
        };
        let storage = || l10n_to_json(&HistoryError::Storage.l10n());
        if monitor.history().is_none() {
            return Err(storage());
        }
        let to = chrono::Utc::now();
        let from = range.since(to);
        let read = tauri::async_runtime::spawn_blocking(move || {
            monitor.flush_history(HISTORY_FLUSH);
            let recording = monitor.history_recording();
            monitor
                .account_history(index, from..=to)
                .map(|metrics| (metrics, recording))
        })
        .await
        .map_err(|_| storage())?;
        let live = self
            .inner
            .lock()
            .await
            .loaded
            .as_ref()
            .map(|l| l.generation);
        if live != Some(generation) {
            return Err(STALE.into());
        }
        let (metrics, recording) = read.map_err(|error| l10n_to_json(&error.l10n()))?;
        Ok(history_view(
            range,
            from,
            to,
            recording,
            metrics,
            metric_key.as_deref(),
        ))
    }

    /// PRD 0026 RF4.6: the usage statistics of `account` (every account with
    /// `None`) over `range`, with the recent journal events, read from the
    /// history store off the UI thread. A result for another generation
    /// before or after the read is [`STALE`], like `account_history`; an
    /// unknown index or a storage failure is the core's typed error.
    pub async fn stats(
        &self,
        range: StatsRange,
        account: Option<usize>,
        generation: u64,
    ) -> Result<StatsView, String> {
        let (monitor, accounts, indexes) = {
            let g = self.inner.lock().await;
            let l = g
                .loaded
                .as_ref()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            if l.generation != generation {
                return Err(STALE.into());
            }
            let all = &l.file.config.accounts;
            let indexes: Vec<usize> = match account {
                Some(index) => vec![index],
                None => (0..all.len()).collect(),
            };
            let accounts = match account {
                Some(index) => vec![all
                    .get(index)
                    .cloned()
                    .ok_or_else(|| l10n_to_json(&ModelError::InvalidIndex.l10n()))?],
                None => all.clone(),
            };
            (Arc::clone(&l.monitor), accounts, indexes)
        };
        let storage = || l10n_to_json(&HistoryError::Storage.l10n());
        let store = monitor.history().ok_or_else(storage)?;
        let read = tauri::async_runtime::spawn_blocking(move || {
            monitor.flush_history(HISTORY_FLUSH);
            let recording = monitor.history_recording();
            let now = chrono::Utc::now();
            let summary =
                cuotai_core::stats::summarize(&store, &accounts, range, now, &chrono::Local)?;
            let since = now - chrono::TimeDelta::days(i64::from(range.days()));
            let entries = store.journal(since..=now, &accounts, JOURNAL_LIMIT * 4)?;
            Ok::<_, HistoryError>((summary, entries, recording, account.is_some()))
        })
        .await
        .map_err(|_| storage())?;
        let live = self
            .inner
            .lock()
            .await
            .loaded
            .as_ref()
            .map(|l| l.generation);
        if live != Some(generation) {
            return Err(STALE.into());
        }
        let (summary, entries, recording, scoped) =
            read.map_err(|error| l10n_to_json(&error.l10n()))?;
        let events = entries
            .into_iter()
            // A scoped view lists that account only, never entries of removed ones.
            .filter(|entry| !scoped || entry.account_index.is_some())
            .take(JOURNAL_LIMIT)
            .map(|entry| JournalView {
                occurred_at: entry.occurred_at,
                kind: entry.kind.name(),
                account: entry.account,
                parent: entry.parent,
                is_connection: entry.is_connection,
                summary: serde_json::to_value(&entry.summary).unwrap_or_default(),
            })
            .collect();
        Ok(StatsView {
            indexes,
            history_kept: serde_json::to_value(summary.history_kept()).unwrap_or_default(),
            not_enough: serde_json::to_value(StatsSummary::not_enough()).unwrap_or_default(),
            removed_account: serde_json::to_value(cuotai_core::history::removed_account())
                .unwrap_or_default(),
            summary,
            events,
            recording,
        })
    }

    /// PRD 0026 RF5: when each account's tightest limit comes back within
    /// `range`, from the readings the app already holds (no query, no
    /// history), for the configuration `generation`.
    pub async fn reset_timeline(
        &self,
        range: TimelineRange,
        generation: u64,
    ) -> Result<cuotai_core::resets::Timeline, String> {
        let g = self.inner.lock().await;
        let l = g
            .loaded
            .as_ref()
            .ok_or_else(crate::desk_text::config_not_loaded)?;
        if l.generation != generation {
            return Err(STALE.into());
        }
        let readings: Vec<Option<Report>> = (0..l.file.config.accounts.len())
            .map(|index| {
                l.cycle
                    .account_report(index)
                    .filter(|report| report.status == Status::Ready)
                    .or_else(|| l.cycle.last_good_report(index))
                    .cloned()
            })
            .collect();
        Ok(cuotai_core::resets::timeline_of_config(
            &l.file.config,
            &readings,
            range,
            chrono::Utc::now(),
        ))
    }

    /// Persist a shared configuration-order move. `Edit::Move` carries every
    /// report and deadline to the account's new slot and returns no refresh
    /// targets, so this command deliberately never calls a provider.
    pub async fn move_account(
        &self,
        app: &AppHandle,
        from: usize,
        to: usize,
    ) -> Result<(), String> {
        self.ensure_loaded().await;
        {
            let mut g = self.inner.lock().await;
            let l = g
                .loaded
                .as_mut()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            let prev = l.file.config.clone();
            let updated = prev
                .move_account(from, to)
                .map_err(|e| edit_error_to_json(&e))?;
            if from == to {
                return Ok(());
            }
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            let due = staged_cycle
                .apply_edit(&prev, &updated, Edit::Move { from, to }, Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e, prev.accounts.len())))?;
            debug_assert!(due.is_empty(), "a move must not schedule queries");
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file.save(updated).map_err(|e| edit_error_to_json(&e))?;

            l.cycle = staged_cycle;
            l.monitor = Arc::new(new_monitor);
            // Keep busy indicators attached to account identity. Completion
            // clears these mirrored tokens by token value (not old index),
            // while the cycle rejects their now-stale reports safely.
            let token = l.tokens.remove(from);
            l.tokens.insert(to, token);
            l.generation += 1;
            g.generation = l.generation;
        }
        self.emit(app).await;
        Ok(())
    }

    /// PRD 0036 RF8.5: hide one connection of the aggregator at `index`
    /// (`params.hidden`) or keep it beside its direct account
    /// (`params.show_both`), then read the aggregator again. Only these two
    /// list parameters of an aggregator account are writable here.
    pub async fn set_connection_flag(
        &self,
        app: &AppHandle,
        index: usize,
        key: &str,
        sub: &str,
        on: bool,
    ) -> Result<(), String> {
        if !matches!(key, "hidden" | "show_both")
            || sub.is_empty()
            || !sub.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(crate::desk_text::bad_connection_param());
        }
        self.ensure_loaded().await;
        {
            let mut g = self.inner.lock().await;
            let l = g
                .loaded
                .as_mut()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            let prev = l.file.config.clone();
            let account = prev
                .accounts
                .get(index)
                .ok_or_else(crate::desk_text::no_account)?;
            if !account.provider.is_aggregator() {
                return Err(crate::desk_text::no_account());
            }
            let mut updated = prev.clone();
            let params = &mut updated.accounts[index].params;
            match toggle_listed(params.get(key).map(String::as_str), sub, on) {
                Some(list) => params.insert(key.to_owned(), list),
                None => params.remove(key),
            };
            if updated.accounts[index].params == prev.accounts[index].params {
                return Ok(());
            }
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            staged_cycle
                .apply_edit(&prev, &updated, Edit::Upsert(index), Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e, prev.accounts.len())))?;
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file.save(updated).map_err(|e| edit_error_to_json(&e))?;
            l.cycle = staged_cycle;
            l.monitor = Arc::new(new_monitor);
            l.generation += 1;
            g.generation = l.generation;
        }
        self.emit(app).await;
        self.refresh_account(app, index).await
    }

    pub async fn get_account(&self, index: usize) -> Result<AccountForm, String> {
        let g = self.inner.lock().await;
        let l = g
            .loaded
            .as_ref()
            .ok_or_else(crate::desk_text::config_not_loaded)?;
        let a = l
            .file
            .config
            .accounts
            .get(index)
            .ok_or_else(crate::desk_text::no_account)?;
        let credential = account_edit::stored_credential(a);
        // auth_mode + auth_options for prefill. When the account has oauth,
        // the mode id is the one matching the provider: today the core only
        // emits "oauth" for flows with a Cuotai credential; AWS SSO/Bailian
        // do not end up in `oauth` (they have no `Account::oauth`).
        let (auth_mode, auth_options, auth_registration) = if a.oauth.is_some() {
            let request = LoginRequest::for_account(a.provider, login::MODE_OAUTH, Some(a));
            (login::MODE_OAUTH, request.options, request.registration)
        } else {
            ("credential", BTreeMap::new(), BTreeMap::new())
        };
        let native_login = a.oauth.is_some()
            && login::login_mode(a.provider, auth_mode).is_some_and(|mode| {
                mode.credential_store == login::LoginCredentialStore::CuotaiVault
                    && mode.registration.is_empty()
            });
        let fields = field_descriptors(a.provider)
            .iter()
            .map(|d| match d.kind {
                FieldDescriptorKind::Text => FieldValue {
                    key: d.key.into(),
                    text: Some(a.params.get(d.key).cloned().unwrap_or_default()),
                    choice: None,
                },
                FieldDescriptorKind::Choice(c) => FieldValue {
                    key: d.key.into(),
                    text: None,
                    choice: Some(
                        a.params
                            .get(d.key)
                            .and_then(|value| {
                                c.options.iter().position(|option| option.value == value)
                            })
                            .unwrap_or(0),
                    ),
                },
            })
            .collect();
        Ok(AccountForm {
            name: a.name.clone(),
            provider: a.provider,
            credential,
            interval: a.refresh_seconds.map(|s| s.to_string()).unwrap_or_default(),
            group: a.group.clone().unwrap_or_default(),
            uses_oauth: a.oauth.is_some(),
            native_login,
            auth_mode,
            auth_options,
            auth_registration,
            fields,
            session_slots: a
                .session
                .iter()
                .flat_map(|session| session.slots.iter().cloned())
                .collect(),
            session_migration: user_session::migration_candidates(a),
            credential_stored: a
                .session
                .as_ref()
                .is_some_and(|s| s.slots.contains(user_session::CREDENTIAL_SLOT)),
            secret_states: cuotai_core::secret_input::secret_states(a),
            vault_migration: cuotai_core::secret_input::vault_migration_candidates(a)
                .into_iter()
                .map(|c| VaultMigrationView {
                    slot: c.slot,
                    label: c.label,
                    consent: c.consent,
                })
                .collect(),
            account_id: a.id().to_string(),
            monogram: crate::settings::view()
                .monogram_of(a.id().as_str())
                .unwrap_or_default(),
        })
    }

    /// PRD 0016 RF2.1: one read-only query of the dialog's draft with its
    /// pasted sessions, before anything is saved. Nothing reaches the vault,
    /// the TOML or the history.
    pub async fn test_session(&self, input: AccountInput) -> Result<SessionTestView, String> {
        self.ensure_loaded().await;
        let Some((account, monitor, pasted)) = self.session_probe(&input).await? else {
            return Err(crate::desk_text::paste_session_first());
        };
        let report = user_session::test_pasted(&monitor, &account, &pasted).await;
        Ok(SessionTestView::from_report(&report))
    }

    /// PRD 0036 RF8.2: the dashboard address of the aggregator account at
    /// `index`, from the core.
    pub async fn aggregator_dashboard(&self, index: usize) -> Option<String> {
        let g = self.inner.lock().await;
        let account = g.loaded.as_ref()?.file.config.accounts.get(index)?;
        cuotai_core::aggregators::dashboard_url(account)
    }

    /// PRD 0036 RF2.2: the dialog's Test connection of an aggregator draft
    /// (health, version, login, connection list). Read-only; the account
    /// is remembered only so the save can tell it was tested.
    pub async fn test_aggregator(&self, input: AccountInput) -> Result<AggregatorTestView, String> {
        self.ensure_loaded().await;
        let (account, monitor) = {
            let g = self.inner.lock().await;
            let l = g
                .loaded
                .as_ref()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            let account = input_account(&l.file.config, &input, None)?;
            if !account.provider.is_aggregator() {
                return Err(crate::desk_text::not_an_aggregator());
            }
            (account, Arc::clone(&l.monitor))
        };
        // PRD 0036 RF2.1/RF2.6: an unsaved vault password, key or SSO paste
        // is tested in place of the saved one; nothing is stored here.
        let pasted = reduce_input(&input)?;
        let test = monitor.test_nine_router_with(&account, &pasted).await;
        let mut g = self.inner.lock().await;
        g.aggregator_tested = test.is_ok().then(|| account.clone());
        Ok(AggregatorTestView {
            ok: test.is_ok(),
            fingerprint: test.fingerprint().map(str::to_owned),
            message: test.message,
        })
    }

    /// PRD 0016 RF1.2/RF4.2: after the dialog's explicit confirmation, move
    /// the saved account's `session_env`-style variables into the system
    /// vault, or forget its vault session. Returns the account's refreshed
    /// form.
    /// PRD 0029 RF6.2: the vault entries Cuotai created, matched against the
    /// configuration (orphans marked); never a value.
    pub async fn vault_audit(&self) -> Result<Vec<vault_index::AuditEntry>, String> {
        self.ensure_loaded().await;
        let (session, config) = self.vault_inputs().await?;
        Ok(match session.vault_index_path() {
            Some(path) => vault_index::audit(&session, path, &config).await,
            None => Vec::new(),
        })
    }

    /// PRD 0029 RF6.2: delete the orphans of [`Self::vault_audit`]; the
    /// dialog confirmed first. Returns how many were deleted.
    pub async fn vault_prune(&self) -> Result<usize, String> {
        self.ensure_loaded().await;
        let (session, config) = self.vault_inputs().await?;
        match session.vault_index_path() {
            Some(path) => vault_index::prune(&session, path, &config)
                .await
                .map_err(|note| l10n_to_json(&note)),
            None => Ok(0),
        }
    }

    async fn vault_inputs(&self) -> Result<(Arc<OAuthSession>, Config), String> {
        let g = self.inner.lock().await;
        let session = g
            .session
            .clone()
            .ok_or_else(crate::desk_text::oauth_session_missing)?;
        let config = g
            .loaded
            .as_ref()
            .ok_or_else(crate::desk_text::config_not_loaded)?
            .file
            .config
            .clone();
        Ok((session, config))
    }

    pub async fn change_session(
        &self,
        app: &AppHandle,
        index: usize,
        change: SessionChange,
    ) -> Result<AccountForm, String> {
        self.ensure_loaded().await;
        let refresh_target = self.drive_session_change(index, change).await?;
        self.emit(app).await;
        if let Some(target) = refresh_target {
            self.refresh_account(app, target).await?;
        }
        self.get_account(index).await
    }

    /// Testable half of [`Backend::change_session`]: the core changes the
    /// vault, the runtime is staged, then the TOML is saved; any failure
    /// after the vault change undoes it (new entries removed, forgotten
    /// ones back).
    pub(crate) async fn drive_session_change(
        &self,
        index: usize,
        change: SessionChange,
    ) -> Result<Option<usize>, String> {
        let mut g = self.inner.lock().await;
        let Inner {
            loaded,
            session,
            generation,
            notices,
            ..
        } = &mut *g;
        let session = session
            .clone()
            .ok_or_else(crate::desk_text::oauth_session_missing)?;
        let l = loaded
            .as_mut()
            .ok_or_else(crate::desk_text::config_not_loaded)?;
        let prev = l.file.config.clone();
        let staged = match change {
            SessionChange::Migrate => {
                // PRD 0047 G5: sessions and API keys alike.
                user_session::stage_vault_migration(&session, &EnvironmentCredentials, &prev, index)
                    .await
            }
            SessionChange::Forget => user_session::stage_forget(&session, &prev, index).await,
        }
        .map_err(|note| l10n_to_json(&note))?;
        let updated = staged.config().clone();
        let mut staged_cycle = l.cycle.clone_state_for_edit();
        let runtime = staged_cycle
            .apply_edit(&prev, &updated, Edit::Upsert(index), Instant::now())
            .map_err(|e| l10n_to_json(&refresh_edit_error(&e, prev.accounts.len())))
            .and_then(|due| {
                l.monitor
                    .with_config(updated.clone())
                    .map(|monitor| (due, Arc::new(monitor)))
                    .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))
            })
            .and_then(|runtime| {
                l.file
                    .save(updated.clone())
                    .map(|()| runtime)
                    .map_err(|e| edit_error_to_json(&e))
            });
        let (due, new_monitor) = match runtime {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = user_session::rollback(&session, staged).await;
                return Err(error);
            }
        };
        l.cycle = staged_cycle;
        l.monitor = new_monitor;
        let _ = l.monitor.prune_models();
        l.monitor.mark_history_removed();
        let _ = notices.keep_accounts(&l.file.config);
        if let Some(slot) = l.tokens.get_mut(index) {
            *slot = None;
        }
        l.generation += 1;
        *generation = l.generation;
        Ok(due.first().copied())
    }

    // ---------- login (RF2: orchestration in the core; here only state and relay) ----------

    /// Starts a core login flow and relays the typed events to the
    /// frontend through `LOGIN_EVENT`. The driver and the completed credential stay
    /// in the backend; the UI only receives `LoginEvent` (no secrets). Returns the
    /// monotonic `attempt` that the frontend echoes in `save_account` to
    /// make sure the right pending login is consumed.
    pub async fn login_start(
        self: &Arc<Self>,
        app: &AppHandle,
        input: LoginStartInput,
    ) -> Result<u64, String> {
        self.ensure_loaded().await;
        // Session, precondition and `PendingLogin` installation happen
        // under the SAME lock: this avoids the window in which a login_start would cancel
        // a predecessor and install the new one between two `lock().await` calls,
        // a window in which a `login_submit_code` or `save_account` would run
        // against a nonexistent slot and a stale forwarder could mutate the
        // new pending login. The core's `start_login` is synchronous, so the
        // critical section is quick.
        let mut g = self.inner.lock().await;
        if g.login.as_ref().is_some_and(|p| !p.done) {
            return Err(crate::desk_text::login_already_running());
        }
        let session = Arc::clone(
            g.session
                .as_ref()
                .ok_or_else(crate::desk_text::oauth_session_missing)?,
        );
        // RF2.8: the core helper binds the request to the edited account and
        // owns credential_id reuse plus the replaced-key cleanup context.
        let existing = input.index.and_then(|index| {
            g.loaded
                .as_ref()
                .and_then(|loaded| loaded.file.config.accounts.get(index))
        });
        let mut request = LoginRequest::for_account(input.provider, &input.mode, existing);
        request.options = input.options.clone();
        request.registration = input.registration.clone();
        // Validates the NEW request BEFORE taking over the predecessor.
        let driver =
            login::start_login(session, input.provider, request).map_err(|e| e.to_string())?;
        // New validated request: only now discard the terminal predecessor
        // (its forwarder is cancelled so it cannot confuse the new slot).
        if let Some(mut old) = g.login.take() {
            if let Some(cancel) = old.cancel_tx.take() {
                let _ = cancel.send(());
            }
        }
        // Capacity-1 queue: one code at a time waiting to be relayed to the
        // driver; the core's immediate verdict comes back through the oneshot (the
        // validation itself arrives later through LoginEvent).
        let (code_tx, mut code_rx) =
            mpsc::channel::<(String, oneshot::Sender<Result<(), String>>)>(1);
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let attempt = g.next_login_attempt;
        g.next_login_attempt = g.next_login_attempt.wrapping_add(1);
        g.login = Some(PendingLogin {
            attempt,
            provider: input.provider,
            index: input.index,
            code_tx,
            cancel_tx: Some(cancel_tx),
            done: false,
            completed: None,
        });
        let backend = Arc::clone(self);
        let app = app.clone();
        let mut driver = driver;
        let mut cancel_rx = cancel_rx;
        tauri::async_runtime::spawn(async move {
            loop {
                let event = tokio::select! {
                    // Cancelling tears down the driver here (Drop aborts): not a single
                    // byte reaches the vault before the commit (RF2.4).
                    _ = &mut cancel_rx => break,
                    code = code_rx.recv() => {
                        match code {
                            Some((code, reply)) => {
                                // Relays the core's immediate verdict:
                                // Full/Closed/Unsupported go back to the UI with the
                                // code intact in the field; validation of the
                                // accepted code arrives through LoginEvent.
                                let result = match driver.submit_code(code) {
                                    Ok(()) => Ok(()),
                                    Err(login::SubmitCodeError::Unsupported) => Err(crate::desk_text::no_manual_code()),
                                    Err(login::SubmitCodeError::Full(_)) => Err(crate::desk_text::code_pending()),
                                    Err(login::SubmitCodeError::Closed) => Err(crate::desk_text::login_ended()),
                                };
                                let _ = reply.send(result);
                                continue;
                            }
                            // All senders are gone (cancel/cleanup):
                            // treat as a cancellation.
                            None => break,
                        }
                    }
                    event = driver.next_event() => match event {
                        Some(event) => event,
                        None => break,
                    },
                };
                let terminal = matches!(
                    event,
                    LoginEvent::Finished { .. } | LoginEvent::Failed { .. }
                );
                if matches!(event, LoginEvent::Finished { .. }) {
                    // After Finished, `finish` returns immediately with the
                    // result; the completed credential stays in the backend.
                    if let Ok(login::LoginResult::Authorized(completed)) = driver.finish().await {
                        let mut g = backend.inner.lock().await;
                        // Guard against a stale forwarder: only mutate the slot if
                        // the attempt is still its own.
                        if let Some(p) = g.login.as_mut() {
                            if p.attempt == attempt {
                                p.completed = Some(completed);
                            }
                        }
                    }
                }
                // Mark `done` and invalidate `cancel_tx` BEFORE emitting the
                // terminal event: the frontend may call `saveAccount`
                // right after receiving `Finished` and the `p.done` check
                // must pass on the first try.
                if terminal {
                    let mut g = backend.inner.lock().await;
                    if let Some(p) = g.login.as_mut() {
                        if p.attempt == attempt {
                            p.done = true;
                            p.cancel_tx = None;
                        }
                    }
                }
                // Carries the current attempt's `attempt`: the frontend
                // drops envelopes whose attempt does not match the active one.
                let attempt_now = {
                    let g = backend.inner.lock().await;
                    g.login
                        .as_ref()
                        .filter(|p| p.attempt == attempt)
                        .map(|p| p.attempt)
                        .unwrap_or(attempt)
                };
                let _ = app.emit(
                    LOGIN_EVENT,
                    LoginEnvelope {
                        attempt: attempt_now,
                        event: &event,
                    },
                );
                if terminal {
                    break;
                }
            }
            // Exit by cancel or closed channel: mark `done` so the
            // submit/cancel path knows the slot has expired.
            let mut g = backend.inner.lock().await;
            if let Some(p) = g.login.as_mut() {
                if p.attempt == attempt {
                    p.done = true;
                    p.cancel_tx = None;
                }
            }
        });
        Ok(attempt)
    }

    /// Sends a manually pasted code (MiMo flow). The core's immediate verdict
    /// (accepted for validation / queue full / ended) goes back to the
    /// caller; the validation itself reaches the UI through `LoginEvent`.
    pub async fn login_submit_code(&self, code: String) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let g = self.inner.lock().await;
            let p = g.login.as_ref().ok_or_else(crate::desk_text::no_login)?;
            if p.done {
                return Err(crate::desk_text::login_finished());
            }
            p.code_tx.try_send((code, reply_tx)).map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => crate::desk_text::code_pending(),
                mpsc::error::TrySendError::Closed(_) => crate::desk_text::login_ended(),
            })?;
        }
        // The wait must end with the core's IMMEDIATE verdict (Full/etc.)
        // OR with the forwarder going down (oneshot channel closed). The window between
        // the forwarder's last action and the `done` marking is covered by the
        // timeout: the active session answers in < 1 ms, so 5 s only fires
        // even if the forwarder dies before answering.
        match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => Err(message),
            Ok(Err(_)) => Err(crate::desk_text::login_ended()),
            Err(_) => Err(crate::desk_text::login_silent()),
        }
    }

    /// Cancels the pending login. Cancelling before the commit writes
    /// nothing to the vault; a completed credential dies in memory. When the
    /// frontend already knew a specific `attempt` (passed in
    /// `attempt`), it only cancels if the slot is still that one — this avoids clearing a
    /// new login that already took the previous one's place.
    pub async fn login_cancel(&self, attempt: Option<u64>) {
        let mut g = self.inner.lock().await;
        let matches = match (attempt, g.login.as_ref()) {
            (Some(a), Some(p)) => p.attempt == a,
            (None, _) => true,
            (Some(_), None) => false,
        };
        if !matches {
            return;
        }
        if let Some(mut p) = g.login.take() {
            if let Some(cancel) = p.cancel_tx.take() {
                let _ = cancel.send(());
            }
        }
    }

    // ---------- modelos (PRD 0002 RF6.1) ----------

    /// Runs a model operation of `Monitor` for account `index` of the
    /// configuration `generation` (the token the UI read with its `View`).
    /// Another generation before or after the operation (save, remove, move,
    /// reload, file switch) yields [`STALE`]: the result belongs to the old
    /// identity and is never attributed to the account now at `index`. The
    /// core validates the index; the quota cycle and its tokens are untouched.
    pub async fn models<F, Fut>(
        &self,
        index: usize,
        generation: u64,
        op: F,
    ) -> Result<ModelState, String>
    where
        F: FnOnce(Arc<Monitor>, usize) -> Fut,
        Fut: Future<Output = Result<ModelState, ModelError>>,
    {
        let monitor = {
            let g = self.inner.lock().await;
            let l = g
                .loaded
                .as_ref()
                .ok_or_else(crate::desk_text::config_not_loaded)?;
            if l.generation != generation {
                return Err(STALE.into());
            }
            Arc::clone(&l.monitor)
        };
        let state = op(monitor, index).await;
        let live = self
            .inner
            .lock()
            .await
            .loaded
            .as_ref()
            .map(|l| l.generation);
        if live != Some(generation) {
            return Err(STALE.into());
        }
        state.map_err(|e| l10n_to_json(&e.l10n()))
    }

    /// PRD 0028 RF2.3: the secret-free problem report of account `index`
    /// (`None`: the application facts only, from About). Pure and local:
    /// nothing is queried or sent. A result for another `generation` fails
    /// with [`STALE`].
    pub async fn report_problem(
        &self,
        index: Option<usize>,
        generation: u64,
    ) -> Result<ProblemDraft, String> {
        use cuotai_core::support::{app_draft, issue_draft, AccountView, AppInfo};
        let app = AppInfo::current(crate::updater::detect().as_str());
        let draft = match index {
            None => app_draft(&app),
            Some(index) => {
                let g = self.inner.lock().await;
                let l = g
                    .loaded
                    .as_ref()
                    .ok_or_else(crate::desk_text::config_not_loaded)?;
                if l.generation != generation {
                    return Err(STALE.into());
                }
                let account = l.file.config.accounts.get(index).ok_or(STALE)?;
                let report = match l.cycle.snapshot() {
                    Some(snapshot) => snapshot.reports.get(index).cloned(),
                    None => l.cycle.account_report(index).cloned(),
                }
                .ok_or(STALE)?;
                let last_good_age = l
                    .cycle
                    .last_good_report(index)
                    .and_then(|good| (chrono::Utc::now() - good.checked_at).to_std().ok());
                issue_draft(
                    &AccountView {
                        account,
                        report: &report,
                        last_good_age,
                        incident_active: cuotai_core::incidents::incident_active(
                            &l.monitor.incident_statuses(),
                            account.provider,
                        ),
                    },
                    &app,
                )
            }
        };
        Ok(ProblemDraft {
            title: draft.title,
            body: draft.body,
            url: draft.url,
            copy_only: draft.copy_only,
        })
    }
}

/// The problem report the dialog previews (PRD 0028 RF2.3, DS-CMP-RPT-1).
#[derive(Serialize, Clone)]
pub struct ProblemDraft {
    title: String,
    body: String,
    url: String,
    copy_only: bool,
}

/// Stable error of the model commands: the configuration changed since the UI view.
pub const STALE: &str = "stale";

// ---------- history (PRD 0006 RF5) ----------

/// Time buckets of the details' area chart.
const HISTORY_CHART_CELLS: usize = 96;
/// How long a history read waits for the readings already queued.
const HISTORY_FLUSH: Duration = Duration::from_millis(500);
/// PRD 0026 RF4.4: journal entries on the Stats view.
const JOURNAL_LIMIT: usize = 20;

/// PRD 0026 RF4: the statistics view of one scope. Numbers, account names the
/// TOML already holds, and the core's localized texts: never a credential, a
/// URL or provider text.
#[derive(Serialize, Debug)]
pub struct StatsView {
    summary: StatsSummary,
    /// The configuration index of each row of `summary.accounts` (a scoped
    /// answer has one row).
    indexes: Vec<usize>,
    /// The core's `stats.history_kept` and `stats.not_enough`, localized by
    /// the UI from their ids.
    history_kept: serde_json::Value,
    not_enough: serde_json::Value,
    /// "Removed account", for an event whose account left the configuration.
    removed_account: serde_json::Value,
    /// The newest journal events, newest first.
    events: Vec<JournalView>,
    /// New readings enter the history (and the journal).
    recording: bool,
}

#[derive(Serialize, Debug)]
pub struct JournalView {
    occurred_at: chrono::DateTime<chrono::Utc>,
    kind: &'static str,
    /// The account's current name; `None` once it was removed.
    account: Option<String>,
    /// PRD 0036 RF7.3: the aggregator's current name for an event of one of
    /// its connections; `None` for a direct account or a removed aggregator.
    parent: Option<String>,
    /// The event belongs to an aggregator's connection (even when the
    /// aggregator left the configuration).
    is_connection: bool,
    /// The core's `Event::summary`.
    summary: serde_json::Value,
}

/// The recorded history of one account over a span, as the details draw
/// it. Only numbers, metric labels and times: never a credential, an
/// account name, a URL or provider text.
#[derive(Serialize, Debug)]
pub struct HistoryView {
    range: HistoryRange,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    /// New readings enter the history (Settings → History).
    recording: bool,
    /// Each recorded metric with samples in the span, in its reported order.
    metrics: Vec<MetricHistoryView>,
}

#[derive(Serialize, Debug)]
pub struct MetricHistoryView {
    key: String,
    label: String,
    unit: String,
    kind: MetricKind,
    window: Option<MetricWindow>,
    limit: Option<f64>,
    /// Samples with a measured `used` in the span.
    samples: usize,
    /// Lowest, highest and last `used`; `None` when nothing was measured.
    trend: Option<Trend>,
    /// The area chart of the span (bucket peaks, end point, resets).
    chart: Chart,
    /// Window instances: the table read by screen readers.
    windows: Vec<WindowUsage>,
}

fn history_view(
    range: HistoryRange,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    recording: bool,
    metrics: Vec<MetricHistory>,
    only: Option<&str>,
) -> HistoryView {
    let metrics = metrics
        .into_iter()
        .filter(|metric| only.is_none_or(|key| metric.key == key))
        .map(|metric| MetricHistoryView {
            samples: metric.points.len(),
            trend: metric.trend(),
            chart: metric.chart(from, to, HISTORY_CHART_CELLS),
            key: metric.key,
            label: metric.label,
            unit: metric.unit,
            kind: metric.kind,
            window: metric.window,
            limit: metric.limit,
            windows: metric.windows,
        })
        .collect();
    HistoryView {
        range,
        from,
        to,
        recording,
        metrics,
    }
}

#[derive(Serialize)]
pub struct ModelSupport {
    supported: bool,
    /// Catalog source when `supported`, otherwise the reason (no network).
    text: cuotai_core::text::Localized,
}

/// Guidance for an account that is not saved yet: the same default probe as the
/// TUI wizard (no stored parameters and no network).
pub fn model_support(provider: Provider) -> ModelSupport {
    let probe = cuotai_core::Account {
        id: None,
        provider,
        ..Default::default()
    };
    let (supported, text) = match models::support(&probe) {
        Ok(text) => (true, text),
        Err(text) => (false, text),
    };
    ModelSupport { supported, text }
}

// ---------- form ----------

#[derive(Deserialize, Serialize)]
pub struct FieldValue {
    pub key: String,
    pub text: Option<String>,
    pub choice: Option<usize>,
}

/// The interface sends the fields in camelCase. Login options/results contain
/// only public values; the core applies the selected mode's account binding.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub index: Option<usize>,
    pub provider: Provider,
    pub name: String,
    /// PRD 0029 RF3.3: the name typed for the provider's existing, unnamed
    /// account when this one is added beside it; saved in the same write.
    #[serde(default)]
    pub peer_name: Option<String>,
    pub credential: String,
    pub fields: Vec<FieldValue>,
    pub interval: String,
    /// PRD 0024 RF7.1: the optional group of comparable accounts; absent
    /// keeps the edited account's group, blank clears it.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub commit_login: bool,
    #[serde(default)]
    pub login_attempt: u64,
    #[serde(default = "default_credential_mode")]
    pub auth_mode: String,
    #[serde(default)]
    pub auth_options: BTreeMap<String, String>,
    #[serde(default)]
    pub auth_params: BTreeMap<String, String>,
    /// PRD 0016 RF2: sessions pasted in the dialog, by slot. The core
    /// reduces them before anything else; the text is never logged or kept.
    #[serde(default)]
    pub sessions: Vec<SessionPasteInput>,
    /// PRD 0047 G2: secrets typed in the dialog (API key, token, secret),
    /// by `secret_inputs` slot. Written to the vault, never to the TOML;
    /// never logged, overwritten when dropped. A blank value keeps the
    /// stored secret.
    #[serde(default)]
    pub secrets: Vec<cuotai_core::secret_input::SecretValue>,
    /// SES-02: the user explicitly chose "Save anyway" after the test before
    /// save could not reach the provider (never after a rejection).
    #[serde(default)]
    pub save_anyway: bool,
    /// PRD 0022 MONO-4: the monogram override (at most two characters);
    /// `None` keeps the stored one, blank clears it. A display preference in
    /// `desktop.json`, never written to the TOML.
    #[serde(default)]
    pub monogram: Option<String>,
}

/// One pasted session of the dialog; the text is overwritten when dropped.
#[derive(Deserialize)]
pub struct SessionPasteInput {
    pub slot: String,
    pub paste: String,
}

impl Drop for SessionPasteInput {
    fn drop(&mut self) {
        user_session::wipe(&mut self.paste);
    }
}

#[derive(Serialize)]
pub struct SaveAccountResult {
    pub cleanup_warning: Option<cuotai_core::text::Localized>,
}

fn default_credential_mode() -> String {
    "credential".to_string()
}

/// `login_start` input: chosen mode + non-secret options/registration.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartInput {
    pub provider: Provider,
    pub index: Option<usize>,
    pub mode: String,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub registration: BTreeMap<String, String>,
}

/// PRD 0047 G5: one secret the migration would move, without the
/// variable name.
#[derive(Serialize)]
pub struct VaultMigrationView {
    slot: &'static str,
    label: cuotai_core::text::Localized,
    consent: cuotai_core::text::Localized,
}

#[derive(Serialize)]
pub struct AccountForm {
    name: String,
    provider: Provider,
    credential: String,
    interval: String,
    /// PRD 0024 RF7.1: the account's group, blank when it has none.
    group: String,
    uses_oauth: bool,
    native_login: bool,
    auth_mode: &'static str,
    auth_options: BTreeMap<String, String>,
    auth_registration: BTreeMap<String, String>,
    fields: Vec<FieldValue>,
    /// PRD 0016 RF1: session slots this account keeps in the vault.
    session_slots: Vec<String>,
    /// PRD 0016 RF1.2: sessions still read from a variable, offered for
    /// migration to the vault.
    session_migration: Vec<user_session::MigrationCandidate>,
    /// PRD 0047 G4: whether the main credential is kept in the vault (the
    /// dialog shows "saved" and a Replace action, never a value).
    credential_stored: bool,
    /// PRD 0047 G4/G5: where each secret input comes from (vault, terminal
    /// environment, unset); never a value or a variable name.
    secret_states: Vec<cuotai_core::secret_input::SecretState>,
    /// PRD 0047 G5: the secrets the "Move into Cuotai" action would copy
    /// from the terminal environment into the vault (`change_session`
    /// with `migrate`). No variable name.
    vault_migration: Vec<VaultMigrationView>,
    /// PRD 0022 MONO-4: the account's stable id and its monogram override
    /// (blank = derived from the name).
    account_id: String,
    monogram: String,
}

/// PRD 0016: what a confirmed "Web session" button changes in the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionChange {
    /// RF1.2: move the `session_env`-style variables into the vault.
    Migrate,
    /// RF4.2: forget the session, deleting its vault entries.
    Forget,
}

#[derive(Serialize)]
pub struct ProviderInfo {
    id: Provider,
    label: &'static str,
    /// Whether the "Add account" picker offers it: the core's
    /// `access::has_usage_query`. The access level stays out of the UI.
    pickable: bool,
}

/// The providers the account editor lists. PRD 0005/0027: the pickable ones
/// come first in the core's own order (`account_edit::picker_providers`, the
/// order the TUI shows); the rest (kept so an existing account can still be
/// edited) follow, sorted the same way. The UI never re-sorts.
pub fn providers() -> Vec<ProviderInfo> {
    let mut ordered = cuotai_core::account_edit::picker_providers(None);
    let mut rest: Vec<Provider> = Provider::ALL
        .iter()
        .copied()
        .filter(|p| !ordered.contains(p))
        .collect();
    rest.sort_by_key(|p| p.label().to_lowercase());
    ordered.extend(rest);
    ordered
        .into_iter()
        .map(|id| ProviderInfo {
            id,
            label: id.label(),
            pickable: cuotai_core::access::has_usage_query(id),
        })
        .collect()
}

/// PRD 0027 RF3.1: one picker tile, from a core catalog entry. Carries no
/// integration-status level (owner rule: no such label in the UI).
#[derive(Serialize)]
pub struct PickerEntry {
    provider: Provider,
    label: String,
    /// PRD 0027 RF1.3: the extension id of an "Extension: {name}" tile;
    /// choosing it opens the extension's consent flow, never a form.
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    /// `auth_mode` the tile opens (`None`: the provider's only mode).
    mode: Option<&'static str>,
    method: cuotai_core::account_edit::SignInMethod,
    method_label: cuotai_core::text::Localized,
    hint: cuotai_core::text::Localized,
    /// Every query is billed ("Billed per query" chip, DS-CMP-BADGE-2).
    metered: bool,
    recommended: bool,
}

impl From<cuotai_core::account_edit::CatalogEntry> for PickerEntry {
    fn from(entry: cuotai_core::account_edit::CatalogEntry) -> Self {
        Self {
            provider: entry.provider,
            label: entry.provider.label().to_owned(),
            extension: entry.extension,
            mode: entry.mode,
            method: entry.method,
            method_label: entry.method.label(),
            hint: entry.hint,
            metered: entry.metered,
            recommended: entry.recommended,
        }
    }
}

/// PRD 0027 RF2/RF3: the picker's results for `query`, ranked and grouped
/// by the core, with (RF1.3) the discovered extensions that can still be
/// added, labelled "Extension: {name}" in `locale`.
fn search_providers_with(
    query: &str,
    locale: &str,
    extensions: &[cuotai_core::extensions::CatalogEntry],
) -> Vec<PickerEntry> {
    account_edit::picker_search_with_extensions(query, locale, extensions)
        .into_iter()
        .map(|m| {
            let name = m
                .entry
                .extension
                .is_some()
                .then(|| m.entry.display_name.resolve(locale));
            let mut entry = PickerEntry::from(m.entry);
            if let Some(name) = name {
                entry.label = name;
            }
            entry
        })
        .collect()
}

impl Backend {
    /// PRD 0027 RF1.3: the picker's results with the extensions of the
    /// loaded configuration's manager.
    pub async fn search_providers(&self, query: &str, locale: &str) -> Vec<PickerEntry> {
        self.ensure_loaded().await;
        let extensions = {
            let g = self.inner.lock().await;
            g.loaded
                .as_ref()
                .and_then(|loaded| {
                    loaded
                        .monitor
                        .extension_manager()
                        .map(|manager| manager.picker_entries(&loaded.file.config))
                })
                .unwrap_or_default()
        };
        search_providers_with(query, locale, &extensions)
    }
}

/// PRD 0027 RF5.1: every way to connect `provider`, recommended first.
pub fn provider_modes(provider: Provider) -> Vec<PickerEntry> {
    let mut entries: Vec<PickerEntry> = account_edit::catalog()
        .into_iter()
        .filter(|entry| entry.provider == provider)
        .map(Into::into)
        .collect();
    entries.sort_by_key(|entry| !entry.recommended);
    entries
}

impl Backend {
    /// PRD 0027 RF6.2: the providers that already have an account, which
    /// detection skips.
    pub async fn configured_providers(&self) -> Vec<Provider> {
        let g = self.inner.lock().await;
        g.loaded
            .as_ref()
            .map(|l| l.file.config.accounts.iter().map(|a| a.provider).collect())
            .unwrap_or_default()
    }
}

/// PRD 0016 RF2.2: the dialog's non-empty pastes reduced by the core to the
/// cookies each slot accepts; the first one it refuses is the error (it
/// names cookies, never values).
/// PRD 0016 pasted sessions plus PRD 0047 typed secrets of the dialog,
/// reduced by the core; one slot may come only once.
fn reduce_input(input: &AccountInput) -> Result<Vec<user_session::ReducedSession>, String> {
    let mut reduced = reduce_pastes(input.provider, &input.sessions)?;
    let typed = cuotai_core::secret_input::reduce_secrets(input.provider, &input.secrets)
        .map_err(|note| l10n_to_json(&note))?;
    for secret in typed {
        if reduced.iter().any(|r| r.slot() == secret.slot()) {
            return Err(l10n_to_json(&cuotai_core::text::Localized::fixed(
                "secret_input.duplicate_slot",
                "The same secret was sent twice; reopen the account form.",
            )));
        }
        reduced.push(secret);
    }
    Ok(reduced)
}

/// The slots `input` writes to the vault (non-blank pastes and secrets).
fn typed_slots(input: &AccountInput) -> Vec<&str> {
    input
        .sessions
        .iter()
        .filter(|paste| !paste.paste.trim().is_empty())
        .map(|paste| paste.slot.as_str())
        .chain(
            input
                .secrets
                .iter()
                .filter(|secret| !secret.value.trim().is_empty())
                .map(|secret| secret.slot.as_str()),
        )
        .collect()
}

fn reduce_pastes(
    provider: Provider,
    pastes: &[SessionPasteInput],
) -> Result<Vec<user_session::ReducedSession>, String> {
    pastes
        .iter()
        .filter(|paste| !paste.paste.trim().is_empty())
        .map(|paste| {
            user_session::reduce(provider, &paste.slot, &paste.paste, chrono::Utc::now())
                .map_err(|note| l10n_to_json(&note))
        })
        .collect()
}

/// Take a staged paste back out of the vault after the save failed.
async fn undo_paste(vault: Option<&OAuthSession>, staged: Option<user_session::StagedSession>) {
    if let (Some(vault), Some(staged)) = (vault, staged) {
        let _ = user_session::rollback(vault, staged).await;
    }
}

/// The account the dialog describes, built by the core against `config`
/// (`completed`: the login this save commits, if any).
/// PRD 0029 RF3.3: `config` with the provider's unnamed account named
/// from the dialog's `peer_name`, when the dialog adds an account beside
/// one; unchanged otherwise.
fn with_named_peer(config: &Config, input: &AccountInput) -> Result<Config, String> {
    match account_edit::unnamed_peer(config, input.provider, input.index) {
        Some(peer) => config
            .rename_account(peer, input.peer_name.as_deref().unwrap_or_default())
            .map_err(|e| l10n_to_json(&e.l10n())),
        None => Ok(config.clone()),
    }
}

fn input_account(
    config: &Config,
    input: &AccountInput,
    completed: Option<&CompletedLogin>,
) -> Result<cuotai_core::Account, String> {
    let existing = input.index.and_then(|i| config.accounts.get(i));
    let had_oauth = existing
        .filter(|a| a.provider == input.provider)
        .is_some_and(|a| a.oauth.is_some());
    let selected_mode = if input.auth_mode == "credential" {
        None
    } else {
        Some(
            login::login_mode(input.provider, &input.auth_mode)
                .ok_or_else(crate::desk_text::auth_mode_unavailable)?,
        )
    };
    let vault_mode = selected_mode
        .as_ref()
        .is_some_and(|mode| mode.credential_store == login::LoginCredentialStore::CuotaiVault);
    let oauth_existing = if vault_mode && had_oauth {
        existing
            .filter(|a| a.provider == input.provider)
            .and_then(|a| a.oauth.as_ref())
            .cloned()
    } else {
        None
    };
    let oauth_mode = vault_mode && (completed.is_some() || oauth_existing.is_some());
    if login_needed_before_save(selected_mode.as_ref(), oauth_mode) {
        return Err(crate::desk_text::finish_login_first());
    }
    let fields = draft_fields(input.provider, &input.fields);
    let oauth_login_params = completed.map(|c| c.params.clone()).unwrap_or_default();
    let (credential, credential_params) = if let Some(mode) = &selected_mode {
        login::account_login_inputs(
            input.provider,
            &mode.id,
            &input.credential,
            &input.auth_options,
            &input.auth_params,
            input.commit_login,
        )
        .map_err(|error| error.to_string())?
    } else {
        (input.credential.clone(), None)
    };
    let draft = AccountDraft {
        provider: input.provider,
        name: &input.name,
        credential: &credential,
        auth_mode: &input.auth_mode,
        oauth_mode,
        oauth_existing: oauth_existing.as_ref(),
        oauth_authorized: completed.map(|c| c.config.clone()),
        oauth_login_params,
        // The saved account's params (PRD 0028 RF7.3: its extension
        // override and other params no field shows survive the edit;
        // `build_account` keeps them only for the same provider).
        existing_params: input
            .index
            .and_then(|index| config.accounts.get(index))
            .map(|account| &account.params),
        credential_params,
        fields: &fields,
        interval_text: &input.interval,
        group_text: input.group.as_deref(),
    };
    // PRD 0047: the typed secrets go to the vault, so their variable
    // fields are neither required nor written.
    account_edit::build_account_with_secrets(&draft, config, input.index, &typed_slots(input))
        .map_err(|e| l10n_to_json(&e.l10n()))
}

/// PRD 0016 RF2.1: what the dialog shows of a paste before it is saved:
/// the kept names, the discarded count and the expiry, never a value.
#[derive(Serialize)]
pub struct SessionPreview {
    summary: cuotai_core::text::Localized,
    kept: Vec<&'static str>,
    discarded: usize,
    expiry: Option<user_session::SessionExpiry>,
}

pub fn preview_session(
    provider: Provider,
    slot: &str,
    mut paste: String,
) -> Result<SessionPreview, String> {
    let now = chrono::Utc::now();
    let reduced = user_session::reduce(provider, slot, &paste, now);
    user_session::wipe(&mut paste);
    let reduced = reduced.map_err(|note| l10n_to_json(&note))?;
    Ok(SessionPreview {
        summary: reduced.summary(now),
        kept: reduced.kept().to_vec(),
        discarded: reduced.discarded(),
        expiry: reduced.expires_at().map(|at| user_session::expiry(at, now)),
    })
}

/// The dialog's draft account, the monitor that queries it and its reduced
/// pasted sessions (PRD 0016 test, SES-02 test before save).
type SessionProbe = (
    cuotai_core::Account,
    Arc<Monitor>,
    Vec<user_session::ReducedSession>,
);

/// PRD 0036 RF2.2: the outcome of the dialog's Test connection: one core
/// sentence, never a body, URL or token.
#[derive(Serialize)]
pub struct AggregatorTestView {
    pub ok: bool,
    pub message: cuotai_core::text::Localized,
    /// PRD 0036 RF2.3: the certificate fingerprint offered for trust on
    /// first use (untrusted or changed); the dialog asks before pinning it.
    pub fingerprint: Option<String>,
}

/// PRD 0016 RF2.1: the outcome of the dialog's read-only session test.
#[derive(Serialize)]
pub struct SessionTestView {
    ok: bool,
    status: cuotai_core::text::Localized,
    note: Option<cuotai_core::text::Localized>,
    metrics: usize,
    expiry: Option<user_session::SessionExpiry>,
}

impl SessionTestView {
    fn from_report(report: &Report) -> Self {
        Self {
            ok: report.status == Status::Ready,
            status: cuotai_core::text::status_l10n(report.status),
            note: report.notes_l10n.first().cloned(),
            metrics: report.metrics.len(),
            expiry: user_session::report_expiry(report, chrono::Utc::now()),
        }
    }
}

/// PRD 0016 RF2.1: one session slot the dialog offers to paste: what it is,
/// where to copy it from and the only hosts that receive it.
#[derive(Serialize)]
pub struct SessionSlotSchema {
    slot: &'static str,
    label: cuotai_core::text::Localized,
    instructions: cuotai_core::text::Localized,
    hosts: &'static [&'static str],
    /// PRD 0036 RF2.1/RF10.1: the paste is a password or key (a masked
    /// one-line field), not a cookie list.
    secret: bool,
}

/// The dialog's values in the core draft shape; unknown keys and values of
/// the wrong kind are dropped (the core validates the rest).
fn draft_fields(provider: Provider, fields: &[FieldValue]) -> Vec<DraftField> {
    field_descriptors(provider)
        .iter()
        .filter_map(|d| {
            let f = fields.iter().find(|f| f.key == d.key)?;
            let value = match d.kind {
                FieldDescriptorKind::Text => DraftValue::Text(f.text.clone()?),
                FieldDescriptorKind::Choice(_) => DraftValue::Choice(f.choice?),
            };
            Some(DraftField { key: d.key, value })
        })
        .collect()
}

/// PRD 0005 RF3: what the dialog shows about how the draft's usage is read.
#[derive(Serialize)]
pub struct AccessView {
    level: cuotai_core::text::Localized,
    summary: cuotai_core::text::Localized,
    notes: Vec<cuotai_core::text::Localized>,
    cost_advisory: Option<cuotai_core::text::Localized>,
    save_confirmation: Option<cuotai_core::text::Localized>,
}

pub fn draft_access(
    provider: Provider,
    auth_mode: &str,
    fields: &[FieldValue],
    session_slots: &[String],
) -> AccessView {
    let access = account_edit::draft_access(
        provider,
        auth_mode,
        &draft_fields(provider, fields),
        session_slots,
    );
    AccessView {
        level: access.level.label(),
        notes: access.notes(),
        cost_advisory: access.cost_advisory(),
        save_confirmation: access.save_confirmation(),
        summary: access.summary,
    }
}

#[derive(Serialize)]
pub struct ChoiceSchema {
    id: String,
    index: usize,
    value: String,
    label: cuotai_core::text::Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct FieldSchema {
    id: String,
    key: &'static str,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    required: bool,
    options: Option<Vec<ChoiceSchema>>,
}

#[derive(Serialize)]
pub struct FieldChoiceSchema {
    key: &'static str,
    options: Vec<ChoiceSchema>,
    choice: usize,
    notice: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct LoginOptionSchema {
    id: String,
    key: String,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    choices: Option<Vec<ChoiceSchema>>,
    required: bool,
    validation: login::LoginOptionValidation,
}

#[derive(Serialize)]
pub struct LoginModeSchema {
    id: String,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    completion_notice: cuotai_core::text::Localized,
    credential_store: login::LoginCredentialStore,
    account_binding: login::LoginAccountBinding,
    registration: Vec<LoginOptionSchema>,
    options: Vec<LoginOptionSchema>,
    unavailable_reason: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct ProviderLoginSchema {
    modes: Vec<LoginModeSchema>,
    unsupported_reason: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct FormSchema {
    credential_kind: CredentialKind,
    default_env: &'static str,
    credential_help: cuotai_core::text::Localized,
    fields: Vec<FieldSchema>,
    login: ProviderLoginSchema,
    /// PRD 0016 RF2.1: the sessions the provider accepts (empty for most).
    sessions: Vec<SessionSlotSchema>,
    /// PRD 0016 RF4.1: the `UserSession` level and risk warning of the
    /// paste box; `None` without sessions.
    session_notice: Option<user_session::SessionNotice>,
    /// PRD 0036 RF2.2: an aggregator account, saved only after a
    /// successful Test connection.
    aggregator: bool,
    /// PRD 0047 G1/G2: the secrets the dialog asks for (typed into
    /// `AccountInput::secrets`); replaces the variable-name credential row.
    secret_inputs: Vec<cuotai_core::secret_input::SecretInput>,
    /// PRD 0047 G1: keys of `fields` that name a variable; the desktop
    /// never renders them (the TUI does).
    terminal_only_fields: Vec<&'static str>,
}

fn login_option_schema(
    provider: Provider,
    mode_id: &str,
    option: login::LoginOption,
    registration: bool,
) -> LoginOptionSchema {
    let slug = provider.slug();
    let key = option.key.clone();
    // RF8: the option descriptor already carries stable Localized ids for
    // label/help/choices (built by `LoginOption::text`/`from_native`).
    // Wrap it in a small schema envelope so the UI can still group the
    // option under the parent mode (registration vs native flow) without
    // re-deriving the ids.
    let id = if registration {
        format!("login.{slug}.{mode_id}.registration.{key}")
    } else {
        format!("native.{slug}.{key}")
    };
    let choices = option.choices.map(|choices| {
        choices
            .into_iter()
            .enumerate()
            .map(|(index, choice)| ChoiceSchema {
                id: choice.label.id.clone().into_owned(),
                index,
                value: choice.value,
                label: choice.label,
                auth_modes: choice.auth_modes,
                unavailable_reason: choice.unavailable_reason,
            })
            .collect()
    });
    LoginOptionSchema {
        id,
        key: option.key,
        label: option.label,
        help: option.help,
        choices,
        required: option.required,
        validation: option.validation,
    }
}

pub fn form_schema(provider: Provider) -> FormSchema {
    let slug = provider.slug();
    let login = login::login_modes(provider);
    FormSchema {
        credential_kind: account_edit::credential_kind(provider),
        default_env: default_env(provider),
        credential_help: credential_help(provider),
        sessions: user_session::descriptors(provider)
            .iter()
            .map(|descriptor| SessionSlotSchema {
                slot: descriptor.slot,
                label: descriptor.label(),
                instructions: descriptor.instructions(),
                hosts: descriptor.hosts,
                secret: descriptor.delivery == user_session::Delivery::Secret,
            })
            .collect(),
        session_notice: user_session::paste_notice(provider),
        aggregator: provider.is_aggregator(),
        secret_inputs: cuotai_core::secret_input::secret_inputs(provider),
        terminal_only_fields: field_descriptors(provider)
            .iter()
            .map(|d| d.key)
            .filter(|key| account_edit::is_terminal_only_field(provider, key))
            .collect(),
        login: ProviderLoginSchema {
            modes: login
                .modes
                .into_iter()
                .map(|mode| {
                    let mode_id = mode.id.clone();
                    LoginModeSchema {
                        id: mode.id,
                        label: mode.label,
                        help: mode.help,
                        completion_notice: mode.completion_notice,
                        credential_store: mode.credential_store,
                        account_binding: mode.account_binding,
                        registration: mode
                            .registration
                            .into_iter()
                            .map(|option| login_option_schema(provider, &mode_id, option, true))
                            .collect(),
                        options: mode
                            .options
                            .into_iter()
                            .map(|option| login_option_schema(provider, &mode_id, option, false))
                            .collect(),
                        unavailable_reason: mode.unavailable_reason,
                    }
                })
                .collect(),
            unsupported_reason: login.unsupported_reason,
        },
        fields: field_descriptors(provider)
            .iter()
            .map(|d| FieldSchema {
                id: format!("field.{slug}.{}", d.key),
                key: d.key,
                label: cuotai_core::text::Localized::fixed(
                    account_edit::field_label_id(provider, d.key),
                    d.label,
                ),
                help: cuotai_core::text::Localized::fixed(
                    account_edit::field_help_id(provider, d.key),
                    d.help,
                ),
                required: d.required,
                options: match d.kind {
                    FieldDescriptorKind::Text => None,
                    FieldDescriptorKind::Choice(c) => Some(
                        c.options
                            .iter()
                            .enumerate()
                            .map(|(index, option)| {
                                let id =
                                    account_edit::field_option_id(provider, d.key, option.value);
                                ChoiceSchema {
                                    id: id.clone(),
                                    index,
                                    value: option.value.to_owned(),
                                    label: cuotai_core::text::Localized::fixed(id, option.label),
                                    auth_modes: option.auth_modes.map(|modes| {
                                        modes.iter().map(|mode| (*mode).to_string()).collect()
                                    }),
                                    unavailable_reason: account_edit::field_option_unavailability(
                                        provider,
                                        d.key,
                                        option.value,
                                    ),
                                }
                            })
                            .collect(),
                    ),
                },
            })
            .collect(),
    }
}

/// Resolve every provider-owned choice for an authentication mode. The UI
/// round-trips full descriptor indices; filtering and substitution stay in the
/// core contract rather than being reimplemented in JavaScript.
pub fn field_choices(
    provider: Provider,
    choices: BTreeMap<String, usize>,
    auth_mode: &str,
) -> Vec<FieldChoiceSchema> {
    field_descriptors(provider)
        .iter()
        .filter_map(|descriptor| {
            let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
                return None;
            };
            let current = choices
                .get(descriptor.key)
                .and_then(|index| choice.options.get(*index))
                .map(|option| option.value);
            let resolved =
                account_edit::resolve_field_choice(provider, descriptor.key, auth_mode, current)?;
            let mode_options = account_edit::field_options(provider, descriptor.key, auth_mode)?;
            let options = mode_options
                .options
                .into_iter()
                .map(|field_option| {
                    let option = &choice.options[field_option.index];
                    let id = account_edit::field_option_id(provider, descriptor.key, option.value);
                    ChoiceSchema {
                        id: id.clone(),
                        index: field_option.index,
                        value: option.value.to_string(),
                        label: cuotai_core::text::Localized::fixed(id, option.label),
                        auth_modes: option
                            .auth_modes
                            .map(|modes| modes.iter().map(|mode| (*mode).to_string()).collect()),
                        unavailable_reason: account_edit::field_option_unavailability(
                            provider,
                            descriptor.key,
                            option.value,
                        ),
                    }
                })
                .collect();
            Some(FieldChoiceSchema {
                key: descriptor.key,
                options,
                choice: resolved.option.index,
                notice: resolved.notice,
            })
        })
        .collect()
}

/// Result of removing an account, so the UI tells the truth about the vault.
#[derive(Debug, Serialize)]
pub struct DeleteResult {
    /// "removed" (no credential in the vault), "purged" (deleted from the vault), "kept" (kept on request),
    /// "kept_shared" (kept because another account uses it) or "partial_failure" (TOML saved, vault not cleared).
    outcome: &'static str,
    other_account: Option<String>,
    message: Option<String>,
    /// PRD 0029 RF5.3: the account's stored web sessions were deleted with
    /// it (reported apart from the OAuth credential).
    sessions_removed: bool,
    /// RF3 handle round-tripped to `retry_purge`. Present when the
    /// credential is still in the vault (`kept`, `kept_shared`,
    /// `partial_failure`) and the UI should offer a retry button.
    /// Opaque: the UI never inspects the value, only echoes it back.
    /// Absent for `removed` and `purged` (nothing to retry).
    recovery_handle: Option<String>,
}

impl DeleteResult {
    fn new(outcome: &'static str) -> Self {
        Self {
            outcome,
            other_account: None,
            message: None,
            sessions_removed: false,
            recovery_handle: None,
        }
    }

    fn with_recovery(mut self, handle: String) -> Self {
        self.recovery_handle = Some(handle);
        self
    }
}

/// Mint an opaque, monotonically increasing recovery handle. The
/// value is never inspected — the UI round-trips it verbatim — but a
/// fresh id per removal makes stale handles self-evident in the logs
/// and prevents the UI from replaying an old `retry_purge` after a
/// config replacement cleared the list (the lookup misses cleanly
/// instead of deleting the wrong credential).
fn mint_handle() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    // Process-global counter; the value is opaque to the UI, only the
    // uniqueness and the monotonicity matter (a stale handle after
    // config replacement just misses the lookup).
    COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Build a cycle for `config` while retaining reports and last valid readings from the previous cycle
/// without querying providers. Deadlines restart from now using the new effective intervals.
fn rebuild_cycle_keeping_reports(old: &RefreshCycle, config: &Config) -> RefreshCycle {
    let mut cycle = RefreshCycle::new(config, Instant::now());
    cycle.keep_last_good_from(old);
    if let Some(snapshot) = old.snapshot().cloned() {
        if snapshot.reports.len() == config.accounts.len() {
            if let Some(token) = cycle.begin_full() {
                cycle.finish_full(Some(snapshot), token, Instant::now());
            }
        }
    }
    cycle
}

/// Cuotai-vault modes require an authorized/existing credential. External
/// CLI modes never inherit this requirement.
fn login_needed_before_save(mode: Option<&login::LoginMode>, oauth_mode: bool) -> bool {
    mode.is_some_and(|mode| mode.credential_store == login::LoginCredentialStore::CuotaiVault)
        && !oauth_mode
}

/// Keys of the fields visible for the current choices (core rule). `auth_mode` is the mode chosen in the
/// dialog ("credential" or the id of a `LoginMode`): native login hides the identity fields
/// that the login itself fills in, like the `build_account` rule (OAuth mode, no client registration).
pub fn visible_keys(
    provider: Provider,
    choices: BTreeMap<String, usize>,
    auth_mode: &str,
    login: &BTreeMap<String, String>,
) -> Vec<&'static str> {
    let mut vis = FieldVisibility::new(auth_mode, login, &choices);
    vis.native_login = login::login_mode(provider, auth_mode).is_some_and(|mode| {
        mode.credential_store == login::LoginCredentialStore::CuotaiVault
            && mode.registration.is_empty()
    });
    let d = field_descriptors(provider);
    visible_field_indices(provider, &vis)
        .into_iter()
        .map(|i| d[i].key)
        .collect()
}

/// PRD 0025 RF2.3: the stored menu-bar choice, resolved against the current
/// accounts.
pub fn tray_view() -> TrayView {
    let prefs = crate::settings::tray_prefs();
    let accounts = crate::tray_bar::accounts();
    let picked = prefs.selection().pick(&accounts);
    let cap = match (prefs.accounts.is_none(), prefs.style) {
        (_, tray::TrayStyle::Meters) => tray::AUTO_METERS,
        (true, tray::TrayStyle::LogosPercent) => tray::AUTO_LOGOS,
        _ => usize::MAX,
    };
    TrayView {
        style: prefs.style,
        selected: picked.iter().take(cap).map(|a| a.key.clone()).collect(),
        popover_view: prefs.popover_view.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    /// PRD 0005/0027: the desktop provider list is in the core's picker
    /// order, the one the TUI wizard uses, so the two clients agree.
    #[test]
    fn desktop_provider_order_matches_the_core_picker_order() {
        let all = super::providers();
        let pickable: Vec<_> = all.iter().filter(|p| p.pickable).map(|p| p.id).collect();
        assert_eq!(
            pickable,
            cuotai_core::account_edit::picker_providers(None),
            "pickable providers must follow picker_providers"
        );
        // Pickable first, then the rest; nothing is dropped.
        let first_other = all.iter().position(|p| !p.pickable).unwrap_or(all.len());
        assert!(all[first_other..].iter().all(|p| !p.pickable));
        assert_eq!(all.len(), Provider::ALL.len());
    }

    #[test]
    fn toggle_listed_adds_and_removes_one_sub_id() {
        assert_eq!(
            super::toggle_listed(None, "AB12", true).as_deref(),
            Some("ab12")
        );
        assert_eq!(
            super::toggle_listed(Some("x, ab12"), "ab12", true).as_deref(),
            Some("x,ab12")
        );
        assert_eq!(
            super::toggle_listed(Some("x,AB12"), "ab12", false).as_deref(),
            Some("x")
        );
        assert_eq!(super::toggle_listed(Some("ab12"), "ab12", false), None);
    }

    use super::*;

    /// PRD 0027 RF1.3: a discovered extension is an "Extension: {name}"
    /// tile carrying its id, resolved in the picker's locale.
    #[test]
    fn the_picker_lists_discovered_extensions_by_name() {
        let entry = cuotai_core::extensions::CatalogEntry {
            id: "team-quota".into(),
            label: cuotai_core::text::Localized::with_param(
                "extensions.catalog.label",
                "name",
                "Team",
                "Extension: {name}",
            ),
            level: cuotai_core::access::Level::Extension,
            warning: cuotai_core::text::Localized::fixed("access.risk.extension", "x"),
            hint: cuotai_core::text::Localized::fixed("extensions.catalog.hint_none", "x"),
        };
        let tiles = search_providers_with("team", "en-US", &[entry]);
        let value = serde_json::to_value(&tiles[0]).unwrap();
        assert_eq!(value["extension"], "team-quota");
        assert_eq!(value["label"], "Extension: Team");
        let builtin = search_providers_with("codex", "en-US", &[]);
        assert!(serde_json::to_value(&builtin[0]).unwrap()["extension"].is_null());
    }
    use cuotai_core::Account;

    /// PRD 0033 RF2.1: a preserved metric whose window renewed reaches the
    /// UI collapsed and flagged `expired`, so it says "Renewed at ..." in
    /// local time instead of showing an exhausted value.
    #[test]
    fn last_good_view_flags_and_collapses_a_renewed_window() {
        use cuotai_core::{MetricKind, MetricWindow};
        let now = chrono::Utc::now();
        let metric = |label: &str, resets_in: i64| Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used: Some(100.0),
            limit: Some(100.0),
            remaining: Some(0.0),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Weekly),
            period: None,
            resets_at: Some(now + chrono::Duration::hours(resets_in)),
        };
        let report = Report {
            overage_hint: Default::default(),
            account_id: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            account: "a".into(),
            provider: Provider::Claude,
            checked_at: now - chrono::Duration::hours(4),
            status: Status::Ready,
            metrics: vec![metric("Week", -1), metric("5h", 2)],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            as_of: None,
        };
        let view =
            serde_json::to_value(last_good_view(&report.collapse_expired(now), now)).unwrap();
        assert_eq!(view["metrics"][0]["expired"], true);
        assert!(view["metrics"][0]["used"].is_null());
        assert!(view["metrics"][1].get("expired").is_none());
        assert_eq!(view["metrics"][1]["used"], 100.0);
    }

    #[test]
    fn global_interval_change_keeps_reports_and_reschedules_deadlines() {
        let mut config = Config::default();
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let token = cycle.begin_full().expect("rodada completa");
        let reports = config
            .accounts
            .iter()
            .map(|a| Report {
                account_id: None,
                account: a.name.clone(),
                provider: a.provider,
                checked_at: chrono::Utc::now(),
                status: Status::Ready,
                metrics: vec![],
                notes: vec![],
                notes_l10n: vec![],
                source: "teste".into(),
                session_expires_at: None,
                diagnostic: None,
                sub_reports: Vec::new(),
                as_of: None,
                overage_hint: None,
            })
            .collect();
        let snapshot = cuotai_core::Snapshot {
            started_at: chrono::Utc::now(),
            finished_at: chrono::Utc::now(),
            reports,
        };
        cycle.finish_full(Some(snapshot), token, Instant::now());
        assert_eq!(
            cycle.snapshot().map(|s| s.reports.len()),
            Some(config.accounts.len())
        );

        config.refresh_seconds = 3600;
        let rebuilt = rebuild_cycle_keeping_reports(&cycle, &config);
        // reports preserved, no account needs a new query
        assert_eq!(
            rebuilt.snapshot().map(|s| s.reports.len()),
            Some(config.accounts.len())
        );
        assert!(rebuilt.pending_indices().is_empty());
        // accounts without their own interval start following the new global value (≈ 1 h)
        let next = rebuilt.next_refresh_in(0, Instant::now());
        assert!(
            next > Duration::from_secs(3000),
            "prazo não foi reagendado: {next:?}"
        );
    }

    fn quota(label: &str, used: f64, window: cuotai_core::MetricWindow, resets_h: i64) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: Some(100.0 - used),
            unit: "%".into(),
            kind: cuotai_core::MetricKind::Quota,
            window: Some(window),
            period: None,
            resets_at: Some(chrono::Utc::now() + chrono::Duration::hours(resets_h)),
        }
    }

    fn ready(name: &str, metrics: Vec<Metric>) -> Report {
        Report {
            account_id: None,
            account: name.into(),
            provider: Provider::AlibabaCloud,
            checked_at: chrono::Utc::now(),
            status: Status::Ready,
            metrics,
            notes: vec![],
            notes_l10n: vec![],
            source: "teste".into(),
            session_expires_at: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    /// PRD 0023 RF7: the view carries the core's overage reading and texts,
    /// and an account without overage data carries none.
    #[test]
    fn view_carries_the_overage_state_and_its_texts() {
        use cuotai_core::MetricWindow::{Monthly, Weekly, Window5h};
        let config = Config {
            accounts: vec![plain_account("work"), plain_account("plain")],
            ..Config::default()
        };
        let mut blocked = ready(
            "work",
            vec![
                quota("5h", 0.0, Window5h, 2),
                quota("week", 100.0, Weekly, 30),
            ],
        );
        let mut extra = Metric::usage(
            "Extra",
            6.25,
            "USD",
            cuotai_core::MetricKind::Spend,
            Some(Monthly),
        )
        .overage();
        extra.limit = Some(50.0);
        blocked.metrics.push(extra);
        let view =
            serde_json::to_value(report_view(0, &blocked, &config, None, None, 0, &[])).unwrap();
        let overage = &view["overage"];
        assert_eq!(overage["state"], "active");
        assert_eq!(overage["spent"], 6.25);
        assert_eq!(overage["currency"], "USD");
        assert_eq!(overage["cap"], 50.0);
        // The account is also at its limit: the chip reads the combined text.
        assert_eq!(overage["summary"]["id"], "overage.limit_hit_on_extra");
        assert_eq!(overage["text"]["id"], "overage.on_extra");
        assert!(overage["caveat"].is_null());
        // The overage meter never binds: the exhausted week does.
        assert_eq!(view["binding"]["window"], "weekly");

        let plain = ready("plain", vec![quota("5h", 40.0, Window5h, 2)]);
        let view =
            serde_json::to_value(report_view(1, &plain, &config, None, None, 0, &[])).unwrap();
        assert!(view["overage"].is_null());
    }

    /// PRD 0036 RF8: an aggregator's view carries its connections (grouped
    /// by upstream, each with its own key, the mapped provider and the
    /// aggregator's index) and the instance summary; a connection whose
    /// query failed offers its preserved reading; while the instance failed
    /// every connection is stale.
    #[test]
    fn an_aggregator_view_carries_its_connections_and_pools() {
        use cuotai_core::{MetricWindow::Weekly, SubReport};
        let mut router = plain_account("home");
        router.provider = Provider::NineRouter;
        router
            .params
            .insert("base_url".into(), "http://127.0.0.1:20128".into());
        let config = Config {
            accounts: vec![router],
            ..Config::default()
        };
        let sub = |id: &str, upstream: &str, mapped, status, used: f64| SubReport {
            id: id.into(),
            upstream: upstream.into(),
            provider: mapped,
            label: id.into(),
            status,
            metrics: vec![Metric {
                limit: Some(100.0),
                ..Metric::usage("weekly (7d)", used, "%", MetricKind::Quota, Some(Weekly))
            }],
            notes_l10n: Vec::new(),
            checked_at: chrono::Utc::now(),
            as_of: None,
            enabled_upstream: Some(true),
            duplicate_of: None,
            plan: None,
            via: None,
            identity: None,
        };
        let mut instance = ready("home", vec![]);
        instance.provider = Provider::NineRouter;
        instance.sub_reports = vec![
            sub("a1", "claude", Some(Provider::Claude), Status::Ready, 40.0),
            sub("g7", "glm", None, Status::Error, 0.0),
            sub("b2", "claude", Some(Provider::Claude), Status::Ready, 100.0),
        ];
        let good = sub("g7", "glm", None, Status::Ready, 10.0);
        let (views, summary) = aggregator_views(
            0,
            &config,
            Some(&instance),
            None,
            |id| (id == "g7").then(|| cuotai_core::aggregators::sub_as_report(&instance, &good)),
            None,
            false,
        );
        let keys: Vec<_> = views.iter().map(|v| v.account_key.as_str()).collect();
        // Grouped by upstream, in the order of first appearance.
        // PRD 0029: each connection is keyed by the aggregator's id.
        let id = config.accounts[0].state_key();
        assert_eq!(
            keys,
            [format!("{id}/a1"), format!("{id}/b2"), format!("{id}/g7")]
        );
        assert!(views.iter().all(|v| v.index == 0));
        assert_eq!(views[0].provider, Provider::Claude);
        assert_eq!(views[0].class, AccountClass::Available);
        assert_eq!(views[1].class, AccountClass::Exhausted);
        // An unknown upstream keeps the aggregator as provider and says so.
        assert_eq!(views[2].provider, Provider::NineRouter);
        assert!(views[2].connection.as_ref().unwrap().mapped.is_none());
        assert!(
            views[2].last_good.is_some(),
            "the failed connection keeps its reading"
        );
        assert!(views[0].last_good.is_none());
        // Its usage access is the aggregator's.
        let json = serde_json::to_value(&views[0]).unwrap();
        assert_eq!(json["access"]["level"], "undocumented");
        assert_eq!(json["connection"]["upstream"], "claude");
        assert_eq!(
            (summary.available, summary.total, summary.connections),
            (1, 3, 3)
        );
        assert_eq!(summary.host.as_deref(), Some("127.0.0.1:20128"));
        assert!(summary.dashboard);
        // A plain account's JSON has neither block.
        let plain = serde_json::to_value(report_view(
            0,
            &ready("x", vec![]),
            &Config {
                accounts: vec![plain_account("x")],
                ..Config::default()
            },
            None,
            None,
            0,
            &[],
        ))
        .unwrap();
        assert!(plain.get("connections").is_none() && plain.get("aggregator").is_none());

        // The instance failed: connections come from its last good reading,
        // all stale with no current metrics.
        let mut failed = ready("home", vec![]);
        failed.provider = Provider::NineRouter;
        failed.status = Status::Error;
        let (stale, summary) = aggregator_views(
            0,
            &config,
            Some(&failed),
            Some(&instance),
            |_| None,
            None,
            true,
        );
        assert_eq!(stale.len(), 3);
        assert!(stale.iter().all(|v| v.pending));
        assert!(stale[0].last_good.is_some() && stale[2].last_good.is_none());
        assert!(summary.alert_each);
    }

    #[test]
    fn view_marks_the_exhausted_week_as_binding_and_ranks_it_first() {
        use cuotai_core::MetricWindow::{Weekly, Window5h};
        let config = Config {
            accounts: vec![plain_account("free"), plain_account("blocked")],
            ..Config::default()
        };
        let free = ready("free", vec![quota("5h", 40.0, Window5h, 2)]);
        let blocked = ready(
            "blocked",
            vec![
                quota("5h", 0.0, Window5h, 2),
                quota("week", 100.0, Weekly, 30),
            ],
        );
        let slots = vec![Some(free), Some(blocked), None];
        // Blocked first, then the 60%-left account; a missing report stays last.
        assert_eq!(binding_ranks(&slots), vec![1, 0, 2]);

        let report = slots[1].as_ref().unwrap();
        let view =
            serde_json::to_value(report_view(1, report, &config, None, None, 0, &[])).unwrap();
        assert_eq!(view["binding"]["index"], 1);
        assert_eq!(view["binding"]["window"], "weekly");
        assert_eq!(view["binding"]["reason"], "exhausted");
        let until = report.metrics[1].resets_at.unwrap();
        assert_eq!(
            view["binding"]["blocked_until"],
            serde_json::to_value(until).unwrap()
        );
    }

    fn provider_status(
        provider: Provider,
        state: cuotai_core::incidents::IncidentState,
    ) -> ProviderStatus {
        ProviderStatus {
            provider,
            state,
            incident: state
                .is_incident()
                .then(|| cuotai_core::incidents::Incident {
                    title: "Elevated errors".into(),
                    started_at: None,
                    url: "https://status.claude.com/incidents/abc".into(),
                }),
            page_url: "https://status.claude.com/".into(),
            read_at: Some(chrono::Utc::now()),
            stale: false,
            expired: false,
        }
    }

    /// PRD 0011 RF4.1/RF4.2: an account shows its own provider's status
    /// (never another's, never one for a provider without a reading) and
    /// the tray indicator needs a partial outage or worse.
    #[test]
    fn accounts_show_their_providers_status_and_outages_mark_the_tray() {
        use cuotai_core::incidents::IncidentState::*;
        let statuses = [
            provider_status(Provider::Claude, PartialOutage),
            provider_status(Provider::Groq, Degraded),
        ];
        let claude =
            serde_json::to_value(incident_view(&statuses, Provider::Claude).unwrap()).unwrap();
        assert_eq!(claude["provider"], "claude");
        assert_eq!(claude["state"], "partial_outage");
        assert_eq!(claude["label"]["id"], "incident.state.partial_outage");
        assert_eq!(
            claude["incident"]["url"],
            "https://status.claude.com/incidents/abc"
        );
        assert_eq!(claude["stale"], false);
        assert_eq!(claude["expired"], false);
        assert!(incident_view(&statuses, Provider::Kiro).is_none());

        assert!(status_view(true, vec![], &statuses).outage);
        let mut stale = provider_status(Provider::Claude, PartialOutage);
        stale.stale = true;
        assert!(
            !status_view(true, vec![], &[stale]).outage,
            "the tray must not present a failed query's old outage as current"
        );
        for calm in [Degraded, Maintenance, Unknown, Operational] {
            let only = [provider_status(Provider::Claude, calm)];
            assert!(!status_view(true, vec![], &only).outage, "{calm:?}");
        }
        assert!(!status_view(false, vec![], &[]).outage);
    }

    /// PRD 0011 RF4.3: off by default with the domains already listed, and
    /// the preference reaches every monitor the backend installs.
    #[tokio::test(flavor = "current_thread")]
    async fn status_queries_start_off_list_domains_and_survive_a_reload() {
        let backend = build_backend(Arc::new(MemoryStore::default()), vec![]).await;
        let groq = Account {
            provider: Provider::Groq,
            ..plain_account("groq")
        };
        let config = Config {
            accounts: vec![groq, plain_account("alibaba")],
            ..Config::default()
        };
        backend.reinstall_for_test(config.clone()).await.unwrap();
        let view = serde_json::to_value(backend.inner.inner.lock().await.view()).unwrap();
        assert_eq!(
            view["status"],
            serde_json::json!({ "enabled": false, "domains": ["groqstatus.com"], "outage": false })
        );

        backend.inner.inner.lock().await.incidents.set_enabled(true);
        backend.reinstall_for_test(config).await.unwrap();
        let g = backend.inner.inner.lock().await;
        let monitor = &g.loaded.as_ref().unwrap().monitor;
        assert!(monitor.incidents().is_enabled());
        // Before any reading the provider is unknown, never operational.
        let statuses = monitor.incident_statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0].state,
            cuotai_core::incidents::IncidentState::Unknown
        );
        assert!(g.view().status.enabled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn view_offers_the_last_good_reading_only_while_the_current_report_failed() {
        let backend =
            build_backend(Arc::new(MemoryStore::default()), vec![plain_account("a")]).await;
        let mut g = backend.inner.inner.lock().await;
        let finish = |g: &mut Inner, report: Report| {
            let cycle = &mut g.loaded.as_mut().unwrap().cycle;
            let token = cycle.begin(0).unwrap();
            cycle.finish_account(0, Some(report), token, Instant::now());
        };
        let mut good = ready(
            "a",
            vec![quota("5h", 40.0, cuotai_core::MetricWindow::Window5h, 2)],
        );
        good.checked_at = chrono::Utc::now() - chrono::Duration::minutes(42);
        finish(&mut g, good.clone());
        let view = serde_json::to_value(g.view()).unwrap();
        // 42 min against a 300 s interval is past 3×: stale, and no second copy.
        assert_eq!(view["reports"][0]["freshness"], "stale");
        assert!(view["reports"][0]["last_good"].is_null());

        let mut failed = ready("a", vec![]);
        failed.status = Status::Error;
        finish(&mut g, failed);
        let view = serde_json::to_value(g.view()).unwrap();
        let r = &view["reports"][0];
        // The current report keeps the failure; the old metrics keep their age.
        assert_eq!(r["status"], "error");
        assert_eq!(r["metrics"].as_array().unwrap().len(), 0);
        assert_eq!(r["freshness"], "fresh");
        assert_eq!(
            r["last_good"]["checked_at"],
            serde_json::to_value(good.checked_at).unwrap()
        );
        assert_eq!(r["last_good"]["metrics"][0]["label"], "5h");

        // The next success removes the preserved reading (the badge goes away).
        finish(&mut g, ready("a", vec![]));
        let view = serde_json::to_value(g.view()).unwrap();
        assert!(view["reports"][0]["last_good"].is_null());
        assert_eq!(view["reports"][0]["freshness"], "fresh");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn usage_alerts_follow_only_the_reports_the_cycle_applies() {
        use cuotai_core::MetricWindow::Window5h;
        struct Titles(std::cell::RefCell<Vec<String>>);
        impl crate::notify::Notifier for Titles {
            fn deliver(&self, title: &str, _: &str) -> Result<(), String> {
                self.0.borrow_mut().push(title.into());
                Ok(())
            }
        }
        let backend =
            build_backend(Arc::new(MemoryStore::default()), vec![plain_account("a")]).await;
        let mut g = backend.inner.inner.lock().await;
        g.notices.switch_alerts("granted").unwrap();
        let now = chrono::Utc::now();
        // The monitor labels every report with its account's id.
        let id = g.loaded.as_ref().unwrap().file.config.accounts[0].id();
        let reading = |used, minutes_ago| Report {
            checked_at: now - chrono::Duration::minutes(minutes_ago),
            account_id: Some(id.clone()),
            ..ready("a", vec![quota("5h", used, Window5h, 2)])
        };
        let titles = Titles(Default::default());
        {
            let Inner {
                loaded, notices, ..
            } = &mut *g;
            let l = loaded.as_mut().unwrap();
            let first = l.cycle.begin(0).unwrap();
            finish_account(
                &mut l.cycle,
                notices,
                (&l.monitor, &l.file.config),
                0,
                reading(50.0, 10),
                first,
                now,
            );
            // A late result of a superseded query is dropped by the cycle:
            // its crossing is neither delivered nor remembered.
            finish_account(
                &mut l.cycle,
                notices,
                (&l.monitor, &l.file.config),
                0,
                reading(95.0, 5),
                first,
                now,
            );
            assert_eq!(notices.tick(&l.file.config, &[], now, &titles).delivered, 0);
            let token = l.cycle.begin(0).unwrap();
            finish_account(
                &mut l.cycle,
                notices,
                (&l.monitor, &l.file.config),
                0,
                reading(92.0, 1),
                token,
                now,
            );
            assert_eq!(notices.tick(&l.file.config, &[], now, &titles).delivered, 1);
            assert!(titles.0.borrow()[0].contains("90%"), "{:?}", titles.0);
            // A full sweep is judged the same way.
            let sweep = l.cycle.begin_full().unwrap();
            let snapshot = cuotai_core::Snapshot {
                started_at: now,
                finished_at: now,
                reports: vec![reading(100.0, 0)],
            };
            finish_full(
                &mut l.cycle,
                notices,
                (&l.monitor, &l.file.config),
                snapshot,
                sweep,
                now,
            );
            let later = now + cuotai_core::alerts::NOTICE_SPACING;
            assert_eq!(
                notices.tick(&l.file.config, &[], later, &titles).delivered,
                1
            );
        }
        assert_eq!(titles.0.borrow().len(), 2);
        let view = serde_json::to_value(g.view()).unwrap();
        assert_eq!(view["notify"]["alerts"], true);
        let r = &view["reports"][0];
        assert_eq!(r["usage_alerts"], serde_json::json!(["5h"]));
        assert_eq!(
            r["metrics"][0]["alert"],
            serde_json::json!({ "muted": false, "active": true })
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn view_shows_a_restored_reading_as_pending_and_stale_until_the_first_answer() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("a"), plain_account("b")],
        )
        .await;
        let mut g = backend.inner.inner.lock().await;
        let mut stored = ready(
            "a",
            vec![quota("5h", 40.0, cuotai_core::MetricWindow::Window5h, 2)],
        );
        stored.checked_at = chrono::Utc::now() - chrono::Duration::hours(3);
        stored.source = String::new();
        let cycle = &mut g.loaded.as_mut().unwrap().cycle;
        cycle.restore_last_good(vec![Some(stored.clone()), None]);

        let view = serde_json::to_value(g.view()).unwrap();
        assert_eq!(view["loaded"], false);
        let reports = view["reports"].as_array().unwrap();
        assert_eq!(
            reports.len(),
            1,
            "b, with nothing recorded, stays a skeleton"
        );
        let r = &reports[0];
        assert_eq!(
            (r["index"].as_u64(), r["pending"].as_bool()),
            (Some(0), Some(true))
        );
        assert!(
            r["metrics"].as_array().unwrap().is_empty()
                && r["notes"].as_array().unwrap().is_empty()
        );
        assert_eq!(r["class"], "unmeasured");
        assert!(r["binding"].is_null() && r["pace_index"].is_null());
        assert_eq!(
            r["last_good"]["checked_at"],
            serde_json::to_value(stored.checked_at).unwrap()
        );
        assert_eq!(r["last_good"]["metrics"][0]["label"], "5h");
        assert!(
            r["last_good"]["metrics"][0]["pace"].is_null(),
            "no pace from an old reading"
        );

        // The first answer replaces it with a current report.
        let cycle = &mut g.loaded.as_mut().unwrap().cycle;
        let token = cycle.begin(0).unwrap();
        cycle.finish_account(0, Some(ready("a", vec![])), token, Instant::now());
        let view = serde_json::to_value(g.view()).unwrap();
        assert_eq!(view["reports"][0]["pending"], false);
        assert!(view["reports"][0]["last_good"].is_null());
        assert!(view["history_warning"].is_null());
    }

    #[test]
    fn view_carries_each_metric_pace_and_the_tightest_metric_with_one() {
        use cuotai_core::MetricWindow::{Other, Weekly, Window5h};
        let config = Config {
            accounts: vec![plain_account("a")],
            ..Config::default()
        };
        let report = ready(
            "a",
            vec![
                quota("5h", 40.0, Window5h, 2),
                quota("week", 88.0, Weekly, 30),
                quota("credits", 95.0, Other, 30),
                quota("fresh", 50.0, Window5h, 5),
            ],
        );
        let view =
            serde_json::to_value(report_view(0, &report, &config, None, None, 0, &[])).unwrap();
        let m = &view["metrics"];
        // 3 h of 5 h gone, 40% used: 20 points in reserve, lasts.
        assert_eq!(m[0]["pace"]["stage"], "reserve");
        assert_eq!(m[0]["pace"]["label"]["params"]["delta"], "20");
        assert_eq!(m[0]["pace"]["outlook"]["id"], "pace.lasts_until_reset");
        // 138 h of 168 h gone, 88% used: ahead, runs out ~11 h before the reset.
        assert_eq!(m[1]["pace"]["stage"], "ahead");
        assert_eq!(m[1]["pace"]["outlook"]["id"], "pace.runs_out_at");
        assert_eq!(m[1]["pace"]["outlook"]["params"]["margin"], "11h 10m");
        // No uniform window, and a freshly opened one: no invented pace.
        assert!(m[2]["pace"].is_null());
        assert!(m[3]["pace"].is_null());
        // The binding is the 95% "credits"; the card's pace line uses the week.
        assert_eq!(view["binding"]["index"], 2);
        assert_eq!(view["pace_index"], 1);
    }

    /// PRD 0024 FIX-11: the pace is read at the reading's `checked_at` and
    /// is absent once the reading is not fresh.
    #[test]
    fn view_reads_the_pace_at_the_reading_time_and_only_while_fresh() {
        use cuotai_core::MetricWindow::Window5h;
        let config = |refresh_seconds| Config {
            accounts: vec![plain_account("a")],
            refresh_seconds,
            ..Config::default()
        };
        // 3 h of 5 h gone now, 50% used; the reading is 80 minutes old, when
        // only 100 of 300 minutes had gone (about 17 points above the pace).
        let report = Report {
            checked_at: chrono::Utc::now() - chrono::Duration::minutes(80),
            ..ready("a", vec![quota("5h", 50.0, Window5h, 2)])
        };
        let view = |refresh_seconds| {
            serde_json::to_value(report_view(
                0,
                &report,
                &config(refresh_seconds),
                None,
                None,
                0,
                &[],
            ))
            .unwrap()
        };
        // Fresh at a 1-hour interval: the stage at the reading's time
        // ("ahead"), not at the wall clock ("reserve").
        let fresh = view(3600);
        assert_eq!(fresh["freshness"], "fresh");
        assert_eq!(fresh["metrics"][0]["pace"]["stage"], "ahead");
        assert_eq!(fresh["pace_index"], 0);
        // Stale at a 5-minute interval: the age chip only, no pace.
        let stale = view(300);
        assert_eq!(stale["freshness"], "stale");
        assert!(stale["metrics"][0]["pace"].is_null());
        assert!(stale["pace_index"].is_null());
    }

    /// PRD 0024 RF1.3/RF1.4: the pace carries the core's chip in both
    /// units (the interface only picks one) and the metric the core's
    /// "Resets {at} · {in}" pair; a past reset has neither.
    #[test]
    fn view_carries_the_core_chips_in_both_units_and_the_reset_pair() {
        use cuotai_core::MetricWindow::Window5h;
        let config = Config {
            accounts: vec![plain_account("a")],
            refresh_seconds: 3600,
            ..Config::default()
        };
        let report = Report {
            checked_at: chrono::Utc::now() - chrono::Duration::minutes(2),
            ..ready("a", vec![quota("5h", 50.0, Window5h, 2)])
        };
        let view =
            serde_json::to_value(report_view(0, &report, &config, None, None, 0, &[])).unwrap();
        let pace = &view["metrics"][0]["pace"];
        assert!(pace["chips"]["points"]["text"]["id"]
            .as_str()
            .unwrap()
            .starts_with("pace.chip."));
        assert_ne!(
            pace["chips"]["points"]["text"]["id"], pace["chips"]["percent"]["text"]["id"],
            "the percent form names the comparison"
        );
        let pair = &view["metrics"][0]["resets_label"];
        assert_eq!(pair["id"], "resets.pair");
        assert_eq!(pair["params"]["in"]["id"], "text.countdown");
        let past = Report {
            checked_at: chrono::Utc::now(),
            ..ready("a", vec![quota("5h", 50.0, Window5h, -1)])
        };
        let view =
            serde_json::to_value(report_view(0, &past, &config, None, None, 0, &[])).unwrap();
        assert!(view["metrics"][0]["resets_label"].is_null());
    }

    /// PRD 0024 RF7.3: the best account of a group carries the core's chip;
    /// an exhausted one says what the best has left; nothing else is added.
    #[test]
    fn view_marks_the_best_account_and_advises_an_exhausted_one() {
        use cuotai_core::MetricWindow::Window5h;
        let mut accounts = vec![plain_account("a"), plain_account("b"), plain_account("c")];
        for account in &mut accounts {
            account.group = Some("coding".into());
        }
        let config = Config {
            accounts,
            groups: vec![cuotai_core::GroupDef {
                id: "coding".into(),
                label: Some("Coding".into()),
                order: None,
                members: Vec::new(),
            }],
            ..Config::default()
        };
        let now = chrono::Utc::now();
        let reading = |name: &str, used: f64| Report {
            checked_at: now - chrono::Duration::minutes(1),
            ..ready(name, vec![quota("5h", used, Window5h, 3)])
        };
        let readings = vec![
            Some(reading("a", 100.0)),
            Some(reading("b", 40.0)),
            Some(reading("c", 70.0)),
        ];
        let mut views: Vec<ReportView> = readings
            .iter()
            .enumerate()
            .map(|(i, r)| report_view(i, r.as_ref().unwrap(), &config, None, None, 0, &[]))
            .collect();
        mark_best(&mut views, &config, &readings, now);
        let json = serde_json::to_value(&views).unwrap();
        assert_eq!(json[0]["group"], "coding");
        assert_eq!(json[0]["group_label"], "Coding");
        assert!(json[0]["best_now"].is_null());
        assert_eq!(json[1]["best_now"]["id"], "advice.best");
        assert!(json[2]["best_now"].is_null());
        // Exhausted: "a is exhausted · b has 60% left"; the others (no
        // forecast, not exhausted) get nothing.
        assert_eq!(json[0]["advice"]["text"]["id"], "advice.switch.exhausted");
        assert_eq!(json[0]["advice"]["text"]["params"]["pct"], "60");
        assert_eq!(json[0]["advice"]["best_index"], 1);
        assert!(json[1]["advice"].is_null() && json[2]["advice"].is_null());
        // Without a second candidate nobody is marked.
        let mut alone = vec![views.remove(0)];
        mark_best(&mut alone, &config, &readings[..1], now);
        assert!(serde_json::to_value(&alone).unwrap()[0]["best_now"].is_null());
    }

    /// PRD 0005 RF4: each report carries its account's access descriptor,
    /// the level's text and the core's explanation of the failure.
    #[test]
    fn view_carries_the_access_and_explains_unavailable_or_changed_interfaces() {
        let failed = |name: &str, provider, status, id: &'static str| Report {
            provider,
            status,
            notes: vec!["x".into()],
            notes_l10n: vec![cuotai_core::text::Localized::fixed(id, "x")],
            ..ready(name, Vec::new())
        };
        let config = Config {
            accounts: vec![
                Account {
                    provider: Provider::Poolside,
                    ..plain_account("pool")
                },
                Account {
                    provider: Provider::Zed,
                    ..plain_account("zed")
                },
                plain_account("cloud"),
            ],
            ..Config::default()
        };
        let reports = [
            failed(
                "pool",
                Provider::Poolside,
                Status::Unsupported,
                "query.inference.poolside_no_public_usage",
            ),
            failed("zed", Provider::Zed, Status::Error, "query.schema"),
            ready("cloud", Vec::new()),
        ];
        let view = |i: usize| {
            serde_json::to_value(report_view(i, &reports[i], &config, None, None, 0, &[])).unwrap()
        };
        let pool = view(0);
        assert_eq!(pool["access"]["level"], "unavailable");
        // PRD 0028 RF6.2: the details' "Verified N months ago" chip is the core's text.
        assert!(pool["access"]["verification"]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("access.verif")));
        assert!(pool["access"]["verification_stale"].is_boolean());
        assert_eq!(pool["access"]["label"]["id"], "access.level.unavailable");
        assert_eq!(pool["access"]["summary"]["id"], "access.summary.poolside");
        assert_eq!(pool["access"]["explanation"], pool["access"]["reason"]);
        assert_eq!(pool["access"]["reason"]["id"], "access.reason.poolside");
        let zed = view(1);
        assert_eq!(zed["access"]["level"], "undocumented");
        assert_eq!(
            zed["access"]["explanation"]["id"],
            "access.interface_changed"
        );
        assert_eq!(
            zed["access"]["explanation"]["params"]["evidence"],
            zed["access"]["evidence"]["url"]
        );
        let cloud = view(2);
        assert_eq!(cloud["access"]["level"], "official");
        assert_eq!(cloud["access"]["scope"], "whole_cloud");
        assert!(cloud["access"]["explanation"].is_null());
    }

    #[test]
    fn login_start_input_reads_the_camel_case_fields_the_ui_sends() {
        let input: LoginStartInput = serde_json::from_value(serde_json::json!({
            "provider": "gemini_api", "index": null, "mode": "oauth", "options": {},
            "registration": {"client_id": "meu-client", "client_secret_env": "MEU_SEGREDO"}
        }))
        .expect("pedido de login");
        assert_eq!(
            input.registration.get("client_id").map(String::as_str),
            Some("meu-client"),
            "registration client_id foi ignorado"
        );
        assert_eq!(
            input
                .registration
                .get("client_secret_env")
                .map(String::as_str),
            Some("MEU_SEGREDO")
        );
    }

    #[test]
    fn account_input_reads_the_camel_case_fields_the_ui_sends() {
        // Exactly the object that the dialog's `saveAccount` builds when saving after a completed login.
        let input: AccountInput = serde_json::from_value(serde_json::json!({
            "index": null, "provider": "kiro", "name": "Kiro Pessoal", "credential": "",
            "fields": [], "interval": "",
            "commitLogin": true, "loginAttempt": 7, "authMode": "oauth",
            "authOptions": {"region": "us-east-1"}, "authParams": {}
        }))
        .expect("pedido do diálogo");
        assert!(input.commit_login, "commitLogin foi ignorado");
        assert_eq!(input.login_attempt, 7);
        assert_eq!(input.auth_mode, "oauth");
        assert_eq!(
            input.auth_options.get("region").map(String::as_str),
            Some("us-east-1")
        );
    }

    #[test]
    fn oauth_mode_without_login_is_reported_as_missing_login_not_missing_env_var() {
        let oauth_mode = login::login_mode(Provider::GeminiApi, login::MODE_OAUTH);
        let aws_mode = login::login_mode(Provider::AwsBedrock, login::MODE_AWS_SSO);
        let bailian_mode =
            login::login_mode(Provider::AlibabaTokenPlan, login::MODE_BAILIAN_CONSOLE);
        assert!(login_needed_before_save(oauth_mode.as_ref(), false));
        assert!(!login_needed_before_save(oauth_mode.as_ref(), true));
        assert!(!login_needed_before_save(None, false));
        assert!(!login_needed_before_save(aws_mode.as_ref(), false));
        assert!(!login_needed_before_save(bailian_mode.as_ref(), false));
    }

    #[test]
    fn native_login_hides_the_identity_fields_the_login_fills_in() {
        let none = BTreeMap::new();
        let keys = |mode: &str| visible_keys(Provider::Kiro, BTreeMap::new(), mode, &none);
        // manual credential: the authentication method (token origin) is an editable field
        assert!(keys("credential").contains(&"auth_method"));
        // browser login: the login itself defines the method; the field must not appear duplicated
        let native = keys(login::MODE_OAUTH);
        assert!(!native.contains(&"auth_method"));
        // the ARN stays editable until the login pins it
        assert!(native.contains(&"profile_arn"));
    }

    #[test]
    fn first_sweep_does_not_wait_for_the_periodic_deadline() {
        // Deadlines of a freshly created cycle are in the future (now + interval).
        let config = Config::default();
        let cycle = RefreshCycle::new(&config, Instant::now());
        assert!(cycle.pending_now_indices(Instant::now()).is_empty());
        let tokens = vec![None; config.accounts.len()];
        assert!(needs_initial_sweep(&cycle, &tokens));
    }

    #[test]
    fn no_sweep_while_a_slot_is_busy() {
        let config = Config::default();
        let cycle = RefreshCycle::new(&config, Instant::now());
        let mut tokens = vec![None; config.accounts.len()];
        tokens[0] = cycle_token();
        assert!(!needs_initial_sweep(&cycle, &tokens));
    }

    fn cycle_token() -> Option<cuotai_core::refresh::Generation> {
        let config = Config::default();
        RefreshCycle::new(&config, Instant::now()).begin(0)
    }

    // -------------------------------------------------------------------
    // RF3 desktop recovery: `delete_account` registers an opaque
    // handle on `kept` / `partial_failure`, drops it on config
    // replacement, and `retry_purge` honours the core's shared-key
    // guard so a retry after the survivor drops succeeds and a retry
    // while the survivor remains is refused with the same honest
    // message the core uses everywhere else.
    // -------------------------------------------------------------------

    use cuotai_core::oauth::{OAuthConfig, OAuthToken, TokenStore};
    use parking_lot::Mutex;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// In-memory `TokenStore` mirroring the core's RF3 test fixture.
    #[derive(Default)]
    struct MemoryStore {
        entries: Mutex<HashMap<String, OAuthToken>>,
        delete_calls: Mutex<Vec<String>>,
    }

    impl TokenStore for MemoryStore {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, cuotai_core::oauth::OAuthError> {
            Ok(self.entries.lock().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), cuotai_core::oauth::OAuthError> {
            self.entries.lock().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), cuotai_core::oauth::OAuthError> {
            self.delete_calls.lock().push(id.into());
            self.entries.lock().remove(id);
            Ok(())
        }
    }

    struct NoSecrets;
    impl cuotai_core::Credentials for NoSecrets {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn oauth_config_for(provider: Provider) -> OAuthConfig {
        let mut config = OAuthConfig::new(String::new());
        config.credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087".into();
        config.validate(provider).unwrap();
        config
    }

    fn oauth_account(name: &str, provider: Provider) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider,
            credential_env: None,
            session: None,
            oauth: Some(oauth_config_for(provider)),
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    async fn save_token_for(
        store: &MemoryStore,
        provider: Provider,
        oauth: &OAuthConfig,
    ) -> String {
        let key = account_edit::account_storage_key(&Account {
            id: None,
            name: "probe".into(),
            provider,
            credential_env: None,
            session: None,
            oauth: Some(oauth.clone()),
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        })
        .expect("OAuth config is valid; the storage key is derivable");
        store.entries.lock().insert(
            key.clone(),
            OAuthToken {
                access_token: format!("access-for-{}", oauth.credential_id),
                refresh_token: None,
                expires_at: None,
                metadata: BTreeMap::new(),
            },
        );
        key
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delete_account_registers_handle_on_kept_and_drops_after_install() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(
            store.clone(),
            vec![oauth_account("solo", Provider::ChatGpt)],
        )
        .await;
        let oauth = backend.loaded_config().await.accounts[0]
            .oauth
            .clone()
            .unwrap();
        let key = save_token_for(&store, Provider::ChatGpt, &oauth).await;

        // `purge_credentials = true` + unique key → the staged
        // outcome is `ReadyForPurge`, the vault delete succeeds, and
        // no recovery handle is registered (nothing left to retry).
        let result = backend
            .drive_delete_account(0, true)
            .await
            .expect("delete_account must succeed");
        assert_eq!(result.outcome, "purged");
        assert!(result.recovery_handle.is_none());
        assert!(backend.pending_recovery().await.is_empty());
        assert!(store.entries.lock().get(&key).is_none());

        // `install` (config replacement) must NOT carry stale
        // recovery entries into the next session; this is the
        // invariant that protects against "retry on a credential
        // now referenced again". The test injects a fake entry,
        // reinstalls, and checks the list is empty.
        {
            let mut g = backend.inner.inner.lock().await;
            g.loaded
                .as_mut()
                .unwrap()
                .pending_recovery
                .push(RecoveryEntry {
                    handle: "stale-handle".into(),
                    account: account_edit::RemovedAccount {
                        provider: Provider::ChatGpt,
                        oauth: Some(oauth.clone()),
                        storage_key: Some(key.clone()),
                        sessions_removed: false,
                    },
                });
        }
        backend
            .reinstall_for_test(Config::default())
            .await
            .expect("reinstall");
        assert!(
            backend.pending_recovery().await.is_empty(),
            "install must drop recovery handles"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_purge_succeeds_after_survivor_drops_and_refuses_while_shared() {
        // Two accounts sharing the same OAuth config (same
        // credential_id → same storage key). The first removal
        // returns `kept_shared` with a recovery handle; a retry
        // must refuse while the survivor exists, then succeed once
        // the survivor is also removed.
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(
            store.clone(),
            vec![
                oauth_account("first", Provider::ChatGpt),
                oauth_account("second", Provider::ChatGpt),
            ],
        )
        .await;
        let oauth = backend.loaded_config().await.accounts[0]
            .oauth
            .clone()
            .unwrap();
        let key = save_token_for(&store, Provider::ChatGpt, &oauth).await;

        let result = backend
            .drive_delete_account(0, true)
            .await
            .expect("delete_account must succeed");
        let handle = result
            .recovery_handle
            .expect("kept_shared must register a recovery handle");
        assert_eq!(result.outcome, "kept_shared");
        assert_eq!(backend.pending_recovery().await.len(), 1);
        // Credential still in the vault (shared-key guard).
        assert!(store.entries.lock().contains_key(&key));

        // Retry while the survivor still shares the key: refused.
        let refused = backend
            .drive_retry(&handle)
            .await
            .expect("retry must return a DeleteResult, not error");
        assert_eq!(refused.outcome, "partial_failure");
        assert!(refused.recovery_handle.is_some());
        assert!(
            refused
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("sobrevive")
                || refused
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("compartilh")
                || refused
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("survivor"),
            "refusal message must mention the survivor: {:?}",
            refused.message,
        );
        // Handle is preserved so the UI can keep offering the retry.
        assert_eq!(backend.pending_recovery().await.len(), 1);

        // Drop the survivor (same OAuth config, the config still
        // has one account after we removed it manually via a fresh
        // delete). The shared-key guard must now let the retry
        // through.
        let second_result = backend
            .drive_delete_account(0, false /* declined */)
            .await
            .expect("second removal must succeed");
        assert_eq!(second_result.outcome, "kept");
        assert!(second_result.recovery_handle.is_some());
        assert!(store.entries.lock().contains_key(&key));

        // Retry against the now-unique config must purge.
        let purged = backend
            .drive_retry(&handle)
            .await
            .expect("retry must succeed");
        assert_eq!(purged.outcome, "purged");
        assert!(purged.recovery_handle.is_none());
        assert!(store.entries.lock().get(&key).is_none());
        // Only the retried handle is consumed; the second removal's
        // handle is still in the list (it references a different
        // credential that is now alone and not shared).
        let remaining = backend.pending_recovery().await;
        assert_eq!(remaining.len(), 1);
        assert_ne!(remaining[0], handle, "retried handle must be dropped");
    }

    /// PRD 0034 RF7: the refresh-cycle errors carry the params their catalog texts use.
    #[test]
    fn refresh_cycle_errors_carry_the_params_of_their_catalog_texts() {
        let diff = refresh_edit_error(&cuotai_core::refresh::EditError::UnexpectedDiff, 3);
        assert_eq!(diff.id, "edit_error.refresh_cycle_diff");
        assert!(diff.params.contains_key("detail"));
        assert!(!diff.fallback.contains('{'));
        let index = refresh_edit_error(&cuotai_core::refresh::EditError::Index(7), 3);
        assert_eq!(index.fallback, "Index 7 out of range (3).");
        assert!(index.params.contains_key("count"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_purge_rejects_unknown_handle() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(
            store.clone(),
            vec![oauth_account("solo", Provider::ChatGpt)],
        )
        .await;
        let oauth = backend.loaded_config().await.accounts[0]
            .oauth
            .clone()
            .unwrap();
        save_token_for(&store, Provider::ChatGpt, &oauth).await;

        // No removal happened: no handles exist. A retry for an
        // unknown id must error (not silently succeed and not
        // accidentally delete the survivor's credential).
        let err = backend.drive_retry("never-issued").await.unwrap_err();
        assert!(
            err.contains("desktop.error.no_pending_credential"),
            "unexpected error message: {err}"
        );
        assert!(
            store.entries.lock().contains_key(
                &account_edit::account_storage_key(&Account {
                    id: None,
                    name: "solo".into(),
                    provider: Provider::ChatGpt,
                    credential_env: None,
                    session: None,
                    oauth: Some(oauth.clone()),
                    params: BTreeMap::new(),
                    refresh_seconds: None,
                    group: None,
                })
                .unwrap()
            ),
            "an unknown-handle retry must not touch the vault"
        );
    }

    // --- test helpers (kept local; do not export) ---

    /// Build a `Backend` whose `Inner.loaded` already contains the
    /// given accounts (no `Monitor::refresh`, no OS discovery).
    async fn build_backend(store: Arc<MemoryStore>, accounts: Vec<Account>) -> TestBackend {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
        let mut file = ConfigFile::load(Some(path), false).unwrap();
        let config = Config {
            refresh_seconds: 300,
            concurrency: 4,
            timeout_seconds: 30,
            accounts,
            groups: Vec::new(),
        };
        file.save(config.clone()).unwrap();
        let session = Arc::new(
            cuotai_core::oauth::OAuthSession::new(
                store as Arc<dyn TokenStore>,
                Arc::new(NoSecrets),
            )
            .unwrap(),
        );
        let monitor = Arc::new(
            Monitor::with_oauth(
                config.clone(),
                Arc::new(cuotai_core::EnvironmentCredentials),
                Arc::clone(&session),
            )
            .unwrap(),
        );
        let backend = Backend::default();
        let mut g = backend.inner.lock().await;
        g.generation = 1;
        g.session = Some(session);
        g.loaded = Some(Loaded {
            other_config: None,
            file,
            monitor,
            cycle: RefreshCycle::new(&config, Instant::now()),
            generation: 1,
            tokens: vec![None; config.accounts.len()],
            pending_recovery: Vec::new(),
        });
        drop(g);
        TestBackend {
            inner: backend,
            _dir: dir,
        }
    }

    /// Wrapper that lets tests reach `Backend::inner.lock().await`
    /// directly through a thin facade, avoiding the need to spin up
    /// a `tauri::AppHandle`.
    struct TestBackend {
        inner: Backend,
        _dir: TempDir,
    }

    impl TestBackend {
        async fn pending_recovery(&self) -> Vec<String> {
            let g = self.inner.inner.lock().await;
            g.loaded
                .as_ref()
                .map(|l| {
                    l.pending_recovery
                        .iter()
                        .map(|e| e.handle.clone())
                        .collect()
                })
                .unwrap_or_default()
        }
        async fn loaded_config(&self) -> Config {
            let g = self.inner.inner.lock().await;
            g.loaded.as_ref().unwrap().file.config.clone()
        }
    }

    // Trait-bridge so the test helpers above can call the
    // `pub(crate)` methods that drive the public Tauri commands
    // without a `tauri::AppHandle`.
    impl TestBackend {
        pub(crate) async fn drive_delete_account(
            &self,
            index: usize,
            purge: bool,
        ) -> Result<DeleteResult, String> {
            self.inner.drive_delete_account(index, purge).await
        }
        pub(crate) async fn drive_retry(&self, handle: &str) -> Result<DeleteResult, String> {
            self.inner.drive_retry(handle).await
        }
        pub(crate) async fn reinstall_for_test(&self, config: Config) -> Result<(), String> {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cuotai.toml");
            std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
            let mut file = ConfigFile::load(Some(path), false).unwrap();
            file.save(config).unwrap();
            let mut g = self.inner.inner.lock().await;
            g.install(file)
        }
        /// File path the live `ConfigFile` was loaded from; `reload`
        /// needs a valid path or it returns the in-memory state.
        async fn file_path(&self) -> PathBuf {
            let g = self.inner.inner.lock().await;
            g.loaded.as_ref().unwrap().file.path.clone()
        }
        /// RF4 reload path: equivalent to `Backend::reload_config`
        /// but operating on the test backend. The function ignores
        /// the `AppHandle` emission (the shell isn't required to
        /// prove the reload semantics) and returns the post-reload
        /// config so the test can assert the live state.
        async fn reload_for_test(&self) -> Config {
            // Mirrors `reload_config` without a Tauri runtime: re-open the
            // live path/source, then rebuild monitor/cycle via `install`.
            let mut g = self.inner.inner.lock().await;
            let l = g.loaded.as_ref().unwrap();
            let resolved = ResolvedPath {
                path: l.file.path.clone(),
                source: l.file.source,
            };
            let file = ConfigFile::open(resolved, true)
                .expect("reload must parse the freshly-edited file");
            let new_config = file.config.clone();
            g.install(file).expect("install");
            new_config
        }
    }

    /// A MiMo account reading its platform session from `env`.
    fn session_env_account(env: &str) -> Account {
        Account {
            id: None,
            name: "MiMo".into(),
            provider: Provider::XiaomiMimo,
            params: BTreeMap::from([("platform_session_env".to_string(), env.to_string())]),
            ..Default::default()
        }
    }

    // PRD 0016 RF1.2: the dialog's confirmed migration moves the variable's
    // value into the vault and leaves only the reference in the TOML.
    #[tokio::test(flavor = "current_thread")]
    async fn migrate_session_moves_the_variable_into_the_vault() {
        let env = "CUOTAI_TEST_DESKTOP_MIGRATE_SESSION";
        std::env::set_var(env, "api-platform_serviceToken=DESKTOP-SECRET; userId=1");
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![session_env_account(env)]).await;
        let before = backend.inner.get_account(0).await.unwrap();
        assert_eq!(before.session_migration.len(), 1);

        backend
            .inner
            .drive_session_change(0, SessionChange::Migrate)
            .await
            .unwrap();
        std::env::remove_var(env);

        let values: Vec<_> = store
            .entries
            .lock()
            .values()
            .map(|token| token.access_token.clone())
            .collect();
        assert_eq!(
            values,
            ["api-platform_serviceToken=DESKTOP-SECRET; userId=1"]
        );
        let text = std::fs::read_to_string(backend.file_path().await).unwrap();
        assert!(
            !text.contains("DESKTOP-SECRET") && !text.contains(env),
            "{text}"
        );
        let after = backend.inner.get_account(0).await.unwrap();
        assert!(after.session_migration.is_empty());
        assert_eq!(after.session_slots, ["platform_session_env"]);
    }

    // A save refused after the vault write (the file changed on disk) leaves
    // neither a vault entry nor a changed configuration.
    #[tokio::test(flavor = "current_thread")]
    async fn migrate_session_undoes_the_vault_write_when_the_save_fails() {
        let env = "CUOTAI_TEST_DESKTOP_MIGRATE_CONFLICT";
        std::env::set_var(env, "api-platform_serviceToken=x; userId=1");
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![session_env_account(env)]).await;
        let path = backend.file_path().await;
        let external = format!(
            "{}\n# edited elsewhere\n",
            std::fs::read_to_string(&path).unwrap()
        );
        std::fs::write(&path, external).unwrap();

        let error = backend
            .inner
            .drive_session_change(0, SessionChange::Migrate)
            .await
            .unwrap_err();
        std::env::remove_var(env);

        assert!(error.contains("edit_error.conflict"), "{error}");
        assert!(store.entries.lock().is_empty());
        let config = backend.loaded_config().await;
        assert!(config.accounts[0].session.is_none());
        assert_eq!(
            config.accounts[0]
                .params
                .get("platform_session_env")
                .map(String::as_str),
            Some(env)
        );
    }

    /// PRD 0036 RF2.2: a new 9router account is saved only after a
    /// successful Test connection of exactly that account.
    #[tokio::test(flavor = "current_thread")]
    async fn an_aggregator_is_saved_only_after_its_connection_test() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store, Vec::new()).await;
        let input = || AccountInput {
            group: Default::default(),
            peer_name: Default::default(),
            index: None,
            provider: Provider::NineRouter,
            name: "home".into(),
            credential: String::new(),
            fields: vec![FieldValue {
                key: "base_url".into(),
                text: Some("http://127.0.0.1:20128".into()),
                choice: None,
            }],
            interval: String::new(),
            commit_login: false,
            login_attempt: 0,
            auth_mode: "credential".into(),
            auth_options: BTreeMap::new(),
            auth_params: BTreeMap::new(),
            sessions: Vec::new(),
            secrets: Vec::new(),
            save_anyway: false,
            monogram: None,
        };
        let error = backend
            .inner
            .drive_save_account(input(), None)
            .await
            .unwrap_err();
        assert!(
            error.contains("desktop.account.aggregator_untested"),
            "{error}"
        );
        assert!(backend.loaded_config().await.accounts.is_empty());
        // The test of another account does not count.
        let tested = {
            let mut g = backend.inner.inner.lock().await;
            let config = g.loaded.as_ref().unwrap().file.config.clone();
            let mut other = input();
            other.name = "office".into();
            g.aggregator_tested = Some(input_account(&config, &other, None).unwrap());
            input_account(&config, &input(), None).unwrap()
        };
        assert!(backend
            .inner
            .drive_save_account(input(), None)
            .await
            .is_err());
        backend.inner.inner.lock().await.aggregator_tested = Some(tested);
        backend
            .inner
            .drive_save_account(input(), None)
            .await
            .expect("a tested account saves");
        assert_eq!(backend.loaded_config().await.accounts[0].name, "home");
    }

    /// PRD 0028 RF7.3: saving the dialog keeps an account's extension
    /// override, which no field of the form shows.
    #[test]
    fn a_dialog_save_keeps_the_extension_override() {
        let mut account = plain_account("Work");
        account.params.insert(
            cuotai_core::extensions::management::EXTENSION_OVERRIDE_PARAM.into(),
            "proxy".into(),
        );
        let config = Config {
            accounts: vec![account.clone()],
            ..Config::default()
        };
        let input = AccountInput {
            index: Some(0),
            provider: account.provider,
            name: "Renamed".into(),
            peer_name: None,
            credential: account.credential_env.clone().unwrap_or_default(),
            fields: Vec::new(),
            interval: String::new(),
            group: None,
            commit_login: false,
            login_attempt: 0,
            auth_mode: "credential".into(),
            auth_options: BTreeMap::new(),
            auth_params: BTreeMap::new(),
            sessions: Vec::new(),
            secrets: Vec::new(),
            save_anyway: false,
            monogram: None,
        };
        let saved = input_account(&config, &input, None).unwrap();
        assert_eq!(saved.extension_override(), Some("proxy"));
    }

    /// The dialog's save of the MiMo account at 0 with `paste` in its
    /// platform session slot.
    fn pasted_input(paste: &str) -> AccountInput {
        AccountInput {
            index: Some(0),
            provider: Provider::XiaomiMimo,
            name: "MiMo".into(),
            peer_name: None,
            credential: "XIAOMI_MIMO_API_KEY".into(),
            fields: Vec::new(),
            interval: String::new(),
            group: None,
            commit_login: false,
            login_attempt: 0,
            auth_mode: "credential".into(),
            auth_options: BTreeMap::new(),
            auth_params: BTreeMap::new(),
            sessions: vec![SessionPasteInput {
                slot: "platform_session_env".into(),
                paste: paste.into(),
            }],
            secrets: Vec::new(),
            save_anyway: false,
            monogram: None,
        }
    }

    const TYPED_KEY: &str = "gsk_DESKTOP-TYPED-SECRET";

    /// PRD 0047 G2: a new Groq account whose key the dialog typed.
    fn typed_key_input(key: &str) -> AccountInput {
        let mut input = pasted_input("");
        input.sessions.clear();
        input.index = None;
        input.provider = Provider::Groq;
        input.name = String::new();
        input.credential = String::new();
        input.secrets = vec![cuotai_core::secret_input::SecretValue {
            slot: user_session::CREDENTIAL_SLOT.into(),
            value: key.into(),
        }];
        input
    }

    /// PRD 0047 G2/G4: the typed key reaches only the vault; the TOML, the
    /// account form and the schema carry no value and no variable name.
    #[tokio::test(flavor = "current_thread")]
    async fn a_typed_key_is_saved_in_the_vault_and_never_in_the_toml() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), Vec::new()).await;
        // Without the clearance of the test before save nothing is stored.
        let refused = backend
            .inner
            .drive_save_account(typed_key_input(TYPED_KEY), None)
            .await
            .unwrap_err();
        assert!(refused.contains("user_session.save.untested"), "{refused}");
        assert!(!refused.contains(TYPED_KEY));
        assert!(store.entries.lock().is_empty());
        assert!(backend.loaded_config().await.accounts.is_empty());

        backend
            .inner
            .drive_save_account(typed_key_input(TYPED_KEY), cleared())
            .await
            .unwrap();
        let values: Vec<_> = store
            .entries
            .lock()
            .values()
            .map(|token| token.access_token.clone())
            .collect();
        assert_eq!(values, [TYPED_KEY]);
        let text = std::fs::read_to_string(backend.file_path().await).unwrap();
        assert!(!text.contains(TYPED_KEY), "{text}");
        assert!(!text.contains("credential_env ="), "{text}");
        let config = backend.loaded_config().await;
        assert_eq!(config.accounts[0].credential_env, None);

        let form = serde_json::to_string(&backend.inner.get_account(0).await.unwrap()).unwrap();
        assert!(!form.contains(TYPED_KEY), "{form}");
        let form: serde_json::Value = serde_json::from_str(&form).unwrap();
        assert_eq!(form["credential_stored"], true);
        assert_eq!(form["secret_states"][0]["source"], "vault");
        assert!(form["vault_migration"].as_array().unwrap().is_empty());

        // Editing without retyping keeps the vault key.
        let mut edit = typed_key_input("");
        edit.index = Some(0);
        backend.inner.drive_save_account(edit, None).await.unwrap();
        assert_eq!(store.entries.lock().len(), 1);
        assert_eq!(
            backend.loaded_config().await.accounts[0].credential_env,
            None
        );
    }

    /// PRD 0047 G1: the schema lists the secret inputs, marks the variable
    /// fields as terminal-only, and no GUI label names a variable.
    #[test]
    fn the_form_schema_asks_for_secrets_not_variables() {
        let schema = serde_json::to_value(form_schema(Provider::AlibabaCloud)).unwrap();
        let slots: Vec<_> = schema["secret_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["slot"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            slots,
            ["credential_env", "secret_env", "security_token_env"]
        );
        assert_eq!(
            schema["terminal_only_fields"],
            serde_json::json!(["secret_env", "security_token_env"])
        );
        for input in schema["secret_inputs"].as_array().unwrap() {
            let label = input["label"]["fallback"].as_str().unwrap().to_lowercase();
            assert!(!label.contains("variable"), "{label}");
        }
    }

    /// PRD 0047 G5: an account set up in the terminal shows as such, without
    /// its variable name, and the move into the vault copies its key.
    #[tokio::test(flavor = "current_thread")]
    async fn an_environment_key_moves_into_the_vault_on_request() {
        let env = "CUOTAI_TEST_DESKTOP_MIGRATE_KEY";
        std::env::set_var(env, TYPED_KEY);
        let store = Arc::new(MemoryStore::default());
        let account = Account {
            name: "groq".into(),
            provider: Provider::Groq,
            credential_env: Some(env.into()),
            ..Default::default()
        };
        let backend = build_backend(store.clone(), vec![account]).await;
        let before = serde_json::to_value(backend.inner.get_account(0).await.unwrap()).unwrap();
        assert_eq!(before["secret_states"][0]["source"], "environment");
        assert_eq!(before["credential_stored"], false);
        assert!(!before["vault_migration"].to_string().contains(env));
        assert_eq!(before["vault_migration"].as_array().unwrap().len(), 1);

        let moved = backend
            .inner
            .drive_session_change(0, SessionChange::Migrate)
            .await;
        std::env::remove_var(env);
        moved.unwrap();
        let values: Vec<_> = store
            .entries
            .lock()
            .values()
            .map(|token| token.access_token.clone())
            .collect();
        assert_eq!(values, [TYPED_KEY]);
        let text = std::fs::read_to_string(backend.file_path().await).unwrap();
        assert!(!text.contains(env) && !text.contains(TYPED_KEY), "{text}");
        let after = serde_json::to_value(backend.inner.get_account(0).await.unwrap()).unwrap();
        assert_eq!(after["credential_stored"], true);
    }

    /// PRD 0036 RF2.1/RF2.6: Test connection of a 9router draft reads the
    /// pasted slots (an unsaved SSO session is reduced, and refused when it
    /// lacks its cookie) instead of testing only the vault ones; the dialog
    /// schema marks the vault password as a masked secret paste.
    #[tokio::test(flavor = "current_thread")]
    async fn aggregator_test_uses_the_pasted_sessions() {
        let backend = build_backend(Arc::new(MemoryStore::default()), Vec::new()).await;
        let mut input = pasted_input("theme=dark");
        input.index = None;
        input.provider = Provider::NineRouter;
        input.name = "Home".into();
        input.credential = String::new();
        input.fields = vec![FieldValue {
            key: "base_url".into(),
            text: Some("http://127.0.0.1:9".into()),
            choice: None,
        }];
        input.sessions[0].slot = user_session::NINE_ROUTER_SSO_SLOT.into();
        let error = backend
            .inner
            .test_aggregator(input)
            .await
            .err()
            .unwrap_or_default();
        assert!(error.contains("auth_token"), "{error}");

        let schema = serde_json::to_value(form_schema(Provider::NineRouter)).unwrap();
        let slots = schema["sessions"].as_array().unwrap();
        let secret = |slot: &str| {
            slots
                .iter()
                .find(|s| s["slot"] == slot)
                .map(|s| s["secret"].clone())
        };
        assert_eq!(
            secret(user_session::CREDENTIAL_SLOT),
            Some(serde_json::json!(true))
        );
        assert_eq!(
            secret(user_session::NINE_ROUTER_SSO_SLOT),
            Some(serde_json::json!(false))
        );
    }

    /// PRD 0022 MONO-4: a monogram override in `desktop.json` reaches the
    /// tiles through the report view; without one the key is absent and
    /// the letters are derived from the name.
    #[test]
    fn the_report_view_carries_the_monogram_override() {
        let config = Config {
            accounts: vec![plain_account("work")],
            ..Config::default()
        };
        let mut view = report_view(0, &ready("work", Vec::new()), &config, None, None, 0, &[]);
        assert!(serde_json::to_value(&view)
            .unwrap()
            .get("monogram")
            .is_none());
        view.monogram = Some("Wk".into());
        assert_eq!(serde_json::to_value(&view).unwrap()["monogram"], "Wk");
    }

    /// PRD 0029 RF3.3: adding a second MiMo account beside an unnamed one
    /// names both in one save; without the peer's name nothing is written.
    #[tokio::test(flavor = "current_thread")]
    async fn adding_beside_an_unnamed_account_names_it_in_the_same_save() {
        let store = Arc::new(MemoryStore::default());
        let mut unnamed = session_env_account("MIMO_COOKIE");
        unnamed.name.clear();
        let backend = build_backend(store, vec![unnamed]).await;
        let id = backend.loaded_config().await.accounts[0].id();
        let mut input = pasted_input("");
        input.sessions.clear();
        input.index = None;
        input.name = "Work".into();
        let error = backend
            .inner
            .drive_save_account(input, None)
            .await
            .unwrap_err();
        assert!(error.contains("edit_error.name_required"), "{error}");
        assert_eq!(backend.loaded_config().await.accounts.len(), 1);

        let mut input = pasted_input("");
        input.sessions.clear();
        input.index = None;
        input.name = "Work".into();
        input.peer_name = Some("Personal".into());
        backend.inner.drive_save_account(input, None).await.unwrap();
        let config = backend.loaded_config().await;
        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[0].name, "Personal");
        assert_eq!(config.accounts[0].id(), id);
        assert_eq!(config.accounts[1].name, "Work");
    }

    /// SES-02: the core's clearance of a passed test before save, for the
    /// tests of the vault and TOML half of the save.
    fn cleared() -> Option<user_session::SaveClearance> {
        let mut report = ready("MiMo", Vec::new());
        report.provider = Provider::XiaomiMimo;
        Some(user_session::clear_for_save(&report, false).expect("a passed test clears"))
    }

    // SES-02: a paste reaches the vault only with the clearance of the test
    // before save; without it nothing is written and the TOML is unchanged.
    #[tokio::test(flavor = "current_thread")]
    async fn a_paste_without_the_test_before_save_never_reaches_the_vault() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![session_env_account("MIMO_COOKIE")]).await;
        let error = backend
            .inner
            .drive_save_account(
                pasted_input("api-platform_serviceToken=DESKTOP-SECRET; userId=1"),
                None,
            )
            .await
            .unwrap_err();
        assert!(error.contains("user_session.save.untested"), "{error}");
        assert!(!error.contains("DESKTOP-SECRET"), "{error}");
        assert!(store.entries.lock().is_empty());
        assert!(backend.loaded_config().await.accounts[0].session.is_none());
        // Without a paste there is nothing to test and no query is made.
        let mut plain = pasted_input("");
        plain.sessions.clear();
        assert!(backend
            .inner
            .test_before_save(&plain)
            .await
            .unwrap()
            .is_none());
        // A paste the core cannot reduce is refused before any query.
        let error = backend
            .inner
            .test_before_save(&pasted_input("userId=1; tracking=DESKTOP-SECRET"))
            .await
            .expect_err("unreducible paste");
        assert!(error.contains("user_session.paste.missing"), "{error}");
    }

    // SES-01: the card and the details offer "Paste a new session" exactly
    // when the core does (a rejected vault session), never otherwise.
    #[test]
    fn the_view_offers_a_new_paste_only_for_a_rejected_session() {
        let config = Config {
            accounts: vec![session_env_account("MIMO_COOKIE")],
            ..Config::default()
        };
        let mut report = ready("MiMo", Vec::new());
        report.provider = Provider::XiaomiMimo;
        let offered = |report: &Report| {
            serde_json::to_value(report_view(0, report, &config, None, None, 0, &[])).unwrap()
                ["paste_session"]
                .as_bool()
                .unwrap()
        };
        assert!(!offered(&report));
        report.status = Status::AuthenticationError;
        report.notes_l10n = vec![cuotai_core::text::Localized::fixed(
            "user_session.rejected",
            "Sessão expirada ou revogada; cole uma nova no assistente da conta.",
        )];
        report.notes = vec![report.notes_l10n[0].fallback.to_string()];
        assert!(offered(&report));
        report.status = Status::Error;
        report.notes_l10n = vec![cuotai_core::text::Localized::fixed("query.transport", "x")];
        assert!(!offered(&report));
    }

    // PRD 0016 RF2/RF3.4: the dialog's paste reaches the vault reduced to the
    // accepted cookies, and a second paste replaces it in the same entry.
    #[tokio::test(flavor = "current_thread")]
    async fn save_account_stores_only_the_reduced_paste_and_replaces_it_in_place() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![session_env_account("MIMO_COOKIE")]).await;
        backend
            .inner
            .drive_save_account(
                pasted_input(
                    "Cookie: api-platform_serviceToken=DESKTOP-SECRET; userId=1; _ga=TRACKER",
                ),
                cleared(),
            )
            .await
            .unwrap();
        let values = || -> Vec<String> {
            store
                .entries
                .lock()
                .values()
                .map(|token| token.access_token.clone())
                .collect()
        };
        assert_eq!(
            values(),
            ["api-platform_serviceToken=DESKTOP-SECRET; userId=1"]
        );
        let text = std::fs::read_to_string(backend.file_path().await).unwrap();
        for absent in ["DESKTOP-SECRET", "TRACKER", "MIMO_COOKIE"] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
        let reference = backend.loaded_config().await.accounts[0].session.clone();
        assert!(reference.is_some());

        backend
            .inner
            .drive_save_account(pasted_input(
                "curl 'https://platform.xiaomimimo.com/' -b 'userId=1; api-platform_serviceToken=RENEWED'",
            ), cleared())
            .await
            .unwrap();
        assert_eq!(values(), ["api-platform_serviceToken=RENEWED; userId=1"]);
        let config = backend.loaded_config().await;
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].session, reference);
    }

    // A paste the core refuses writes nothing; a save refused after the vault
    // write (the file changed on disk) takes the entry out again.
    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_paste_or_save_leaves_the_vault_untouched() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![session_env_account("MIMO_COOKIE")]).await;
        let error = backend
            .inner
            .drive_save_account(pasted_input("userId=1; tracking=DESKTOP-SECRET"), cleared())
            .await
            .unwrap_err();
        assert!(error.contains("user_session.paste.missing"), "{error}");
        assert!(!error.contains("DESKTOP-SECRET"), "{error}");

        let path = backend.file_path().await;
        let external = format!(
            "{}\n# edited elsewhere\n",
            std::fs::read_to_string(&path).unwrap()
        );
        std::fs::write(&path, external).unwrap();
        let error = backend
            .inner
            .drive_save_account(
                pasted_input("api-platform_serviceToken=DESKTOP-SECRET; userId=1"),
                cleared(),
            )
            .await
            .unwrap_err();
        assert!(error.contains("edit_error.conflict"), "{error}");
        assert!(store.entries.lock().is_empty());
        assert!(backend.loaded_config().await.accounts[0].session.is_none());
    }

    // PRD 0016 RF4.2: "Forget this session" deletes the account's vault entry
    // and its reference; a save refused afterwards puts the session back.
    #[tokio::test(flavor = "current_thread")]
    async fn forget_session_deletes_the_vault_entry_or_restores_it_when_the_save_fails() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![session_env_account("MIMO_COOKIE")]).await;
        let paste = "api-platform_serviceToken=DESKTOP-SECRET; userId=1";
        backend
            .inner
            .drive_save_account(pasted_input(paste), cleared())
            .await
            .unwrap();

        let path = backend.file_path().await;
        let saved = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{saved}\n# edited elsewhere\n")).unwrap();
        let error = backend
            .inner
            .drive_session_change(0, SessionChange::Forget)
            .await
            .unwrap_err();
        assert!(error.contains("edit_error.conflict"), "{error}");
        assert_eq!(store.entries.lock().len(), 1);
        assert!(backend.loaded_config().await.accounts[0].session.is_some());

        std::fs::write(&path, saved).unwrap();
        backend
            .inner
            .drive_session_change(0, SessionChange::Forget)
            .await
            .unwrap();
        assert!(store.entries.lock().is_empty());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("credential_ref"), "{text}");
        let after = backend.inner.get_account(0).await.unwrap();
        assert!(after.session_slots.is_empty());
    }

    // RF1.2: desktop precedence env > chosen > OS, never `./cuotai.toml`;
    // switching reloads accounts and drops per-config state.
    #[tokio::test(flavor = "current_thread")]
    async fn desktop_switch_follows_env_chosen_os_precedence() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha")],
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, account: &str| {
            let path = dir.path().join(name);
            std::fs::write(
                &path,
                format!("refresh_seconds = 300\n\n[[accounts]]\nname = \"{account}\"\nprovider = \"chat_gpt\"\ncredential_env = \"K\"\n"),
            )
            .unwrap();
            path
        };
        let env = write("env.toml", "from-env");
        let chosen = write("chosen.toml", "from-chosen");
        write("cuotai.toml", "from-cwd");
        let mut loc = Locations {
            env: Some(env.clone()),
            cwd: dir.path().to_path_buf(),
            config_dir: Some(dir.path().join("os")),
        };
        let mut g = backend.inner.inner.lock().await;
        let generation = g.generation;
        let live = |g: &Inner| {
            let file = &g.loaded.as_ref().unwrap().file;
            (file.path.clone(), file.source, file.config.accounts.len())
        };
        g.switch(Some(chosen.clone()), &loc).unwrap();
        assert_eq!(live(&g), (env, PathSource::Env, 1));
        assert!(g.generation > generation, "per-config state rebuilt");
        loc.env = None;
        g.switch(Some(chosen.clone()), &loc).unwrap();
        assert_eq!(live(&g), (chosen, PathSource::Chosen, 1));
        assert_eq!(
            g.loaded.as_ref().unwrap().file.config.accounts[0].name,
            "from-chosen"
        );
        g.switch(None, &loc).unwrap();
        assert_eq!(
            live(&g),
            (
                dir.path().join("os/cuotai/config.toml"),
                PathSource::System,
                0
            )
        );
    }

    // PRD 0002 RF6.1: model commands are bound to the generation of the
    // view the UI read; a result computed for an old identity is refused.
    #[tokio::test(flavor = "current_thread")]
    async fn model_commands_validate_index_and_reject_stale_generation() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha"), plain_account("beta")],
        )
        .await;
        let b = &backend.inner;
        let state = |m: Arc<Monitor>, i: usize| async move { m.model_state(i) };
        // Current generation: the core answers (Alibaba Cloud lists nothing, no network).
        assert!(matches!(
            b.models(1, 1, state).await,
            Ok(ModelState::Unsupported { .. })
        ));
        // Invalid index is the core's typed error, never a panic.
        let err = b.models(9, 1, state).await.unwrap_err();
        assert!(err.contains("models.error.invalid_index"), "{err}");
        // Unknown/old token is refused before running anything.
        assert_eq!(b.models(0, 0, state).await.unwrap_err(), STALE);

        // Removal while the query runs: "beta" moves to index 0, and the
        // result computed for the old generation must not be returned.
        let raced = b
            .models(1, 1, |m, i| async move {
                let result = m.model_state(i);
                b.drive_delete_account(0, false).await.unwrap();
                result
            })
            .await;
        assert_eq!(raced.unwrap_err(), STALE);
        // The old token stays stale; the new view's token works.
        assert_eq!(b.models(0, 1, state).await.unwrap_err(), STALE);
        let live = b.view().await.generation;
        assert!(live > 1);
        assert!(b.models(0, live, state).await.is_ok());
        assert!(
            b.models(1, live, state).await.is_err(),
            "only one account left"
        );
    }

    // PRD 0006 RF5.1: the details' history comes from the core, carries no
    // account identity and, like the model catalog, is refused for a view
    // of another generation.
    #[tokio::test(flavor = "current_thread")]
    async fn account_history_serves_the_core_chart_for_the_live_generation_only() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha"), plain_account("beta")],
        )
        .await;
        let b = &backend.inner;
        let day = HistoryRange::Day;
        // Without a history store: a typed storage error, never "no data".
        let err = b.account_history(0, 1, day, None).await.unwrap_err();
        assert!(err.contains("history.error.storage"), "{err}");

        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new(
            dir.path().join("history.sqlite3"),
            &cuotai_core::config_file::ConfigIdentity::Explicit("one".into()),
        );
        let alpha = {
            let mut g = b.inner.lock().await;
            let l = g.loaded.as_mut().unwrap();
            let monitor = l.monitor.with_config(l.file.config.clone()).unwrap();
            l.monitor = Arc::new(monitor.with_history(store.clone()));
            l.file.config.accounts[0].clone()
        };
        let now = chrono::Utc::now();
        for (hours, used) in [(72, 40.0), (3, 10.0), (2, 60.0), (1, 5.0)] {
            let at = now - chrono::Duration::hours(hours);
            store
                .record(
                    &alpha,
                    at,
                    &[quota("Sessão", used, MetricWindow::Window5h, 1)],
                )
                .unwrap();
        }

        let view = b.account_history(0, 1, day, None).await.unwrap();
        assert!(view.recording, "on by default");
        assert_eq!(view.to - view.from, chrono::Duration::hours(24));
        let [session] = &view.metrics[..] else {
            panic!("expected exactly one metric");
        };
        assert_eq!((session.label.as_str(), session.samples), ("Sessão", 3));
        assert_eq!(session.trend.map(|t| t.last.used), Some(5.0));
        assert_eq!(session.chart.heights.len(), HISTORY_CHART_CELLS);
        assert_eq!(session.chart.resets.len(), 1, "the drop to 5 of 100");
        assert_eq!(session.chart.last.map(|p| p.y), Some(0.05));
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("alpha") && !json.contains("ALPHA_KEY"),
            "{json}"
        );
        // The week reaches the reading of three days ago.
        let week = b.account_history(0, 1, HistoryRange::Week, None);
        assert_eq!(week.await.unwrap().metrics[0].samples, 4);
        // Only the metric asked for; another account has none.
        let key = session.key.clone();
        let only = b.account_history(0, 1, day, Some(key)).await.unwrap();
        assert_eq!(only.metrics.len(), 1);
        let other = b.account_history(0, 1, day, Some("x".into())).await;
        assert!(other.unwrap().metrics.is_empty());
        assert!(b
            .account_history(1, 1, day, None)
            .await
            .unwrap()
            .metrics
            .is_empty());
        // Switched off: what was recorded still shows, flagged.
        b.inner
            .lock()
            .await
            .loaded
            .as_ref()
            .unwrap()
            .monitor
            .set_history_recording(false);
        let off = b.account_history(0, 1, day, None).await.unwrap();
        assert!(!off.recording);
        assert_eq!(off.metrics[0].samples, 3);

        let err = b.account_history(9, 1, day, None).await.unwrap_err();
        assert!(err.contains("models.error.invalid_index"), "{err}");
        assert_eq!(b.account_history(0, 0, day, None).await.unwrap_err(), STALE);
        // After a removal the old view's token is stale.
        b.drive_delete_account(1, false).await.unwrap();
        assert_eq!(b.account_history(0, 1, day, None).await.unwrap_err(), STALE);
        let live = b.view().await.generation;
        assert_eq!(
            b.account_history(0, live, day, None)
                .await
                .unwrap()
                .metrics
                .len(),
            1
        );
    }

    // PRD 0026 RF4.6/RF5: statistics and the reset timeline are served for
    // the live generation only, from the stored history and the held
    // readings; scoped to one account the journal lists that account only.
    #[tokio::test(flavor = "current_thread")]
    async fn stats_and_timeline_are_served_for_the_live_generation_only() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha"), plain_account("beta")],
        )
        .await;
        let b = &backend.inner;
        // Without a history store: a typed storage error, never "no data".
        let err = b.stats(StatsRange::Days7, None, 1).await.unwrap_err();
        assert!(err.contains("history.error.storage"), "{err}");

        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new(
            dir.path().join("history.sqlite3"),
            &cuotai_core::config_file::ConfigIdentity::Explicit("stats".into()),
        );
        let (alpha, beta) = {
            let mut g = b.inner.lock().await;
            let l = g.loaded.as_mut().unwrap();
            let monitor = l.monitor.with_config(l.file.config.clone()).unwrap();
            l.monitor = Arc::new(monitor.with_history(store.clone()));
            (
                l.file.config.accounts[0].clone(),
                l.file.config.accounts[1].clone(),
            )
        };
        let now = chrono::Utc::now();
        for (hours, used) in [(30, 40.0), (3, 100.0), (1, 60.0)] {
            store
                .record(
                    &alpha,
                    now - chrono::Duration::hours(hours),
                    &[quota("Sessão", used, MetricWindow::Window5h, 1)],
                )
                .unwrap();
        }
        let event = |account: &cuotai_core::Account, kind| cuotai_core::events::Event {
            parent: Default::default(),
            schema: cuotai_core::events::SCHEMA,
            event: kind,
            occurred_at: now - chrono::Duration::hours(2),
            account_key: account.key(),
            account_id: account.state_key(),
            account_name: account.name.clone(),
            provider: account.provider,
            accounts: Vec::new(),
            metric: None,
            current: Default::default(),
            previous: None,
            sample: false,
        };
        store
            .journal_record(&[
                event(&alpha, cuotai_core::events::EventKind::RefreshFailed),
                event(&beta, cuotai_core::events::EventKind::AuthRequired),
            ])
            .unwrap();

        let all = b.stats(StatsRange::Days7, None, 1).await.unwrap();
        assert_eq!(all.summary.accounts.len(), 2);
        assert_eq!(all.summary.hits, 1, "the 100% window");
        assert_eq!(all.events.len(), 2);
        assert!(all.recording);
        let only = b.stats(StatsRange::Days30, Some(0), 1).await.unwrap();
        assert_eq!(only.summary.accounts.len(), 1);
        assert_eq!(
            (all.indexes.as_slice(), only.indexes.as_slice()),
            (&[0, 1][..], &[0][..])
        );
        assert_eq!(only.events.len(), 1);
        assert_eq!(only.events[0].kind, "refresh_failed");
        // No credential or provider text crosses the IPC.
        let json = serde_json::to_string(&all).unwrap();
        assert!(!json.contains("ALPHA_KEY"), "{json}");
        let err = b.stats(StatsRange::Days7, Some(9), 1).await.unwrap_err();
        assert!(err.contains("models.error.invalid_index"), "{err}");
        assert_eq!(
            b.stats(StatsRange::Days7, None, 0).await.unwrap_err(),
            STALE
        );

        // The timeline reads the held readings (none yet) for this generation.
        let timeline = b
            .reset_timeline(cuotai_core::resets::TimelineRange::Hours24, 1)
            .await
            .unwrap();
        assert!(timeline.lanes.is_empty() && timeline.unknown.is_empty());
        assert_eq!(
            b.reset_timeline(cuotai_core::resets::TimelineRange::Days7, 0)
                .await
                .unwrap_err(),
            STALE
        );
        // After a removal the old view's token is stale.
        b.drive_delete_account(1, false).await.unwrap();
        assert_eq!(
            b.stats(StatsRange::Days7, None, 1).await.unwrap_err(),
            STALE
        );
    }

    // PRD 0006 RF4.4 with PRD 0020 RF1: Settings is not modal, so the accounts
    // can move under its history selector. An export or erasure of one
    // account is refused for any other configuration generation, so it never
    // lands on the account now at that index; "every account" needs none.
    #[tokio::test(flavor = "current_thread")]
    async fn history_scope_of_one_account_is_refused_for_another_generation() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha"), plain_account("beta")],
        )
        .await;
        let b = &backend.inner;
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new(
            dir.path().join("history.sqlite3"),
            &cuotai_core::config_file::ConfigIdentity::Explicit("one".into()),
        );
        {
            let mut g = b.inner.lock().await;
            let l = g.loaded.as_mut().unwrap();
            let monitor = l.monitor.with_config(l.file.config.clone()).unwrap();
            l.monitor = Arc::new(monitor.with_history(store));
        }
        let names =
            |accounts: Vec<Account>| accounts.into_iter().map(|a| a.name).collect::<Vec<_>>();
        let (_, one) = b.history_scope(Some(1), Some(1)).await.unwrap();
        assert_eq!(names(one), ["beta"]);
        let (_, all) = b.history_scope(None, None).await.unwrap();
        assert_eq!(names(all), ["alpha", "beta"]);
        assert_eq!(b.history_scope(Some(1), None).await.err().unwrap(), STALE);
        assert_eq!(
            b.history_scope(Some(1), Some(0)).await.err().unwrap(),
            STALE
        );
        // After a removal, index 0 of the old view is refused; the live one works.
        b.drive_delete_account(0, false).await.unwrap();
        assert_eq!(
            b.history_scope(Some(0), Some(1)).await.err().unwrap(),
            STALE
        );
        let live = b.view().await.generation;
        let (_, one) = b.history_scope(Some(0), Some(live)).await.unwrap();
        assert_eq!(names(one), ["beta"]);
    }

    // -------------------------------------------------------------------
    // RF4 desktop reload: external edit → reload → next save succeeds.
    // Mirrors the TUI's `reload_config_after_conflict`, but driven from
    // the test backend (no `tauri::AppHandle` available).
    // -------------------------------------------------------------------

    /// Account helper that exercises only the schema fields relevant
    /// to the reload test (no OAuth, no params): a reload pick-up is a
    /// pure-config operation that does not touch the vault.
    fn plain_account(name: &str) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider: Provider::AlibabaCloud,
            credential_env: Some(format!("{}_KEY", name.to_uppercase())),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    /// Write directly to the on-disk file the backend currently
    /// holds, simulating an out-of-process editor.
    async fn externally_edit(backend: &TestBackend, file_text: &str) {
        let path = backend.file_path().await;
        std::fs::write(&path, file_text).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reload_after_external_edit_makes_next_save_succeed() {
        // Start with one account; the backend loads it cleanly.
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha")],
        )
        .await;
        assert_eq!(backend.loaded_config().await.accounts.len(), 1);

        // Out-of-process editor rewrites the file (added + renamed).
        externally_edit(
            &backend,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\nname = \"beta\"\nprovider = \"chat_gpt\"\ncredential_env = \"BETA_KEY\"\n",
        )
        .await;

        // Backend still believes the old snapshot is on disk: any
        // save against it would now return `Conflict`.
        assert!(
            backend.inner.config_changed_on_disk().await.unwrap(),
            "changed_on_disk must fire after the external rewrite"
        );

        // Reload rebuilds the live state. No `AppHandle` here: use
        // `reload_for_test`, which mirrors the public path.
        let reloaded = backend.reload_for_test().await;
        assert_eq!(reloaded.accounts.len(), 1);
        assert_eq!(reloaded.accounts[0].name, "beta");
        assert!(
            !backend.inner.config_changed_on_disk().await.unwrap(),
            "snapshot was refreshed, so the change detector must rest"
        );

        // Saving against the fresh snapshot succeeds (no further
        // `Conflict` from the core). We exercise the same code path
        // the dialog uses: load again, mutate, save.
        let path = backend.file_path().await;
        let mut file = ConfigFile::load(Some(path.clone()), false).unwrap();
        let mut cfg = file.config.clone();
        cfg.accounts.push(plain_account("gamma"));
        file.save(cfg.clone()).unwrap();
        assert_eq!(file.config.accounts.len(), 2, "gamma now persists");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reload_clears_stale_recovery_and_pending_refresh_tokens() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![plain_account("alpha")]).await;

        // Snapshot the generation we will be bumping from.
        let initial_generation = {
            let g = backend.inner.inner.lock().await;
            g.generation
        };

        // Inject a stale recovery entry and a fake busy slot to
        // prove the RF3 invariant survives the reload (the entry
        // references the previous configuration's vault key).
        {
            let mut g = backend.inner.inner.lock().await;
            let l = g.loaded.as_mut().unwrap();
            l.pending_recovery.push(RecoveryEntry {
                handle: "stale-after-reload".into(),
                account: account_edit::RemovedAccount {
                    provider: Provider::ChatGpt,
                    oauth: Some(oauth_config_for(Provider::ChatGpt)),
                    storage_key: Some("stale-key".into()),
                    sessions_removed: false,
                },
            });
            if !l.tokens.is_empty() {
                l.tokens[0] = Some(cuotai_core::refresh::Generation(99));
            }
        }

        backend.reload_for_test().await;

        let g = backend.inner.inner.lock().await;
        let l = g.loaded.as_ref().unwrap();
        assert!(
            l.pending_recovery.is_empty(),
            "RF3 recovery entries must NOT survive a config replacement"
        );
        assert!(
            l.tokens.iter().all(Option::is_none),
            "in-flight tokens of the previous generation must be cleared"
        );
        // `install` always bumps the generation counter: the
        // back-end's own dummy token (`Generation(99)`) is now
        // provably stale because the cycle's generation guard
        // discards results that don't match the live token table.
        assert_eq!(
            l.generation,
            initial_generation + 1,
            "exactly one install ran (the original load's install plus this reload)"
        );
    }
    #[test]
    fn desktop_field_choices_use_full_indices_and_report_kiro_substitution() {
        let fresh = field_choices(Provider::Kiro, BTreeMap::new(), "credential");
        let auth_method = fresh
            .iter()
            .find(|field| field.key == "auth_method")
            .expect("Kiro auth_method schema");
        assert_eq!(
            auth_method
                .options
                .iter()
                .map(|option| (option.index, option.value.as_str()))
                .collect::<Vec<_>>(),
            [
                (2, "google"),
                (3, "github"),
                (4, "import"),
                (5, "api_key"),
                (6, "external_idp")
            ]
        );
        assert_eq!(auth_method.choice, 2);
        assert!(auth_method.notice.is_none());

        let reconciled = field_choices(
            Provider::Kiro,
            BTreeMap::from([("auth_method".to_string(), 0)]),
            "credential",
        );
        let auth_method = reconciled
            .iter()
            .find(|field| field.key == "auth_method")
            .expect("Kiro auth_method schema");
        assert_eq!(auth_method.choice, 2);
        assert_eq!(
            auth_method.notice.as_ref().map(|notice| notice.id.as_ref()),
            Some("edit_notice.option_replaced_for_auth_mode")
        );
    }
}
