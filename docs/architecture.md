# Architecture, Rust Library and Verification

## Core / TUI / Desktop split

```text
crates/cuotai-core/       Rust library: accounts and persistence, credentials, providers, metrics, queries
crates/cuotai-tui/        cuotai binary: CLI, sign-in wizard, Ratatui, keyboard/mouse
apps/desktop/             Tauri 2 app: main window, tray popover and native menu
```

`cuotai-core` is independent of any UI (it depends on neither Ratatui, Crossterm nor a graphical loop):

- **`access`**: `describe(account)` returns, with no network and without reading credentials, how the account's usage is read: `level` (`Official`: an interface the vendor documents for that credential, always with a documentation URL; `Undocumented`: internal or inferred, may change without notice; `Derived`: inferred from Cloud Monitoring metrics or rate-limit headers; `Unavailable`: no query, always with a `reason`), `credential` class, optional `plan_requirement`, `scope` (`Account`, `Organization`, `WholeCloud`, `Product`), `metered` (`Provider::is_metered`), `evidence` (documentation URL or 9router/community-client file) with `verified_on`, and localized `summary`/`credential_hint`/`risk` (`access.*` ids). The `match` on `Provider` is exhaustive with no wildcard, so a new provider does not compile without declaring its access; `product`/`query_mode`/`mode`, the native login and `session_env` select the variant. `UsageAccess::explain(&report)` is what cards and details show in place of a failed report's generic note: the reason of an `Unavailable` account, or `access.interface_changed` (with the evidence URL) for an `Undocumented` account whose report is `Error` with `query.schema`; `None` otherwise. `UsageAccess` serializes with snake_case slugs (`level`, `credential`, `scope`) and `evidence` as `{ "kind": "documentation" | "reference", "url" }`, the shape `export::v1` (PRD 0012) embeds per account; it never enters `Report`, `Snapshot` or `--json`.
- **`account_edit`**: `field_descriptors` and `native_option_descriptors` define fields, options and which of them are required per provider; `visible_field_indices` decides which fields apply for a given product, mode and login; `build_account` validates a draft against that schema; `draft_access` describes the draft with the `access` rules (the editors' level, notes from `UsageAccess::notes`, cost warning from `cost_advisory` and "Save anyway" text from `save_confirmation`); `Config::upsert_account`/`without_account` create, edit or remove accounts.
- **`config_file::ConfigFile`**: discovers, loads and atomically writes the TOML, preserving comments and rejecting concurrent external edits.
- **`export::v1`**: `Export::from_cycle(config, identity, cycle, now, options)` builds the versioned export contract ([`export-v1.md`](export-v1.md)) from the state a `RefreshCycle` already holds, without querying: per account its `key`, `class`, the current `report`, `age_seconds`/`freshness`/`clock_adjusted`, the preserved `last_good` reading with `stale`, `binding`, `pace` per metric and the `access` descriptor. Notes and `source` are redacted (credential variable names, URL credentials and queries, and e-mails unless `Options::include_identity`); missing values stay `null`. A reference-file test and a JSON Schema test guard the contract. `ConfigIdentity::digest` is the configuration key it publishes (and `models` stores).
- **`history`**: local measurement history (PRD 0006). `Monitor::with_history(store)` records every `Ready` report through `HistoryRecorder` (a worker thread behind a bounded queue: the refresh only enqueues, a full queue drops and counts the reading; `HistoryStatus::warning` carries storage failures, ceiling pruning and drops to the front ends), which also prunes by `Retention` on open and every 24 hours (age, hourly reduction after 7 days keeping window peaks, 64 MiB ceiling); `restored_readings` feeds `RefreshCycle::restore_last_good` on opening and `recent_points` the pace's recent slope; `export`, `erase_account` and `erase_all` serve the desktop's Settings → History. `HistoryStore::in_data_dir(&file)` uses `<OS local data dir>/cuotai/history/history.sqlite3`, a file of its own (directory `0700`, file `0600` on Unix, even if they already existed), opened per operation in WAL mode with a busy timeout so the TUI, the desktop app and the CLI on the same TOML write to it at once. Rows are keyed by the digests `models` uses (configuration and account identity: a move keeps the series, a rename or a new credential, product or region starts another) and hold only numbers and window metadata plus the metric label; never secrets, account names, URLs or notes, and a missing or non-finite value is `NULL`. `record` is idempotent per account, metric (`metric_key`: digest of kind, window, label and unit) and time; `mark_removed(&saved, now)` marks the series of accounts gone from the TOML for pruning. `latest` (the last reading per metric with its original time), `series` (`pace::Point`), `windows` (a window ends on a drop above 20% of `limit` or a `resets_at` move beyond 2 minutes) and `bounds` never touch the network. The schema is versioned (`PRAGMA user_version`, forward migrations; a newer file is rejected) and every storage failure is `HistoryError::Storage`.
  - **Views (RF6):** `HistoryStore::account_history(account, range)` (also `Monitor::account_history(index, range)`) returns every recorded metric of an account with samples in the span as `MetricHistory` (points with a finite `used`, window instances, last limit) in one query, without creating the file; `MetricHistory::trend()` gives the lowest, highest and last value and `levels(start, end, cells, levels)` the sparkline levels (bucket peaks, zero-to-limit scale or lowest-to-highest without a limit, `None` for gaps), so the TUI details (`HistoryRange` 24 h / 7 d / 30 d) and `--history` only map them to glyphs and text.
- **`Monitor`**: `refresh` and `refresh_account` query providers and normalize reports; the internal semaphore limits full and single-account queries together (1–32), applies a per-account deadline (1–300 s), preserves the configured order and isolates failures per account.
- **`models`**: per-account model catalog, outside the quota refresh. `Monitor::with_model_cache(ModelCache::in_data_dir(&file))` writes to `<OS persistent data dir>/cuotai/models/models.sqlite3` (outside any cache folder, separate from the TOML, the vault and the debug credential SQLite; directory `0700` and file `0600` on Unix, even if they already existed). Each row stores digests only: SHA-256 of `ConfigFile::identity()` and of the account identity (provider, name, `credential_env`/`oauth` and `params`, except `models` and `billing_cycle`, which only filter or date the quota); never secrets, HTTP bodies or URLs. Moving an account keeps its entry; renaming it or changing its credential, product or region creates another one. `Monitor::prune_models` deletes configuration entries that are absent from the freshly loaded or saved TOML (the TUI and the desktop app call it on open, on saving an edit and on removal), without touching live accounts of another window. States: `not_fetched`, `fetching`, `ready`, `failed`, `interrupted`, `unsupported`. `model_state` only reads; `load_models` queries only when no attempt is recorded (success, empty list, failure or interruption are stored and not repeated); `sync_models` queries and, if it fails, keeps the previous catalog with its date. Every query needs a per-account claim (owner token + lease = account deadline + 30 s) taken in `BEGIN IMMEDIATE`: another window or process with a live claim does not query and instead waits for the stored result (100 ms polling, bounded by the lease), so simultaneous syncs become a single request; only the owner publishes, so a stale completion never overwrites a newer result. Cancellation expires the lease immediately and a killed process lets the lease run out: in both cases the row becomes `interrupted`, with no partial list and no automatic retry, until a manual `sync_models`. A persistence error is `ModelError::Storage`. Today every provider except Alibaba Cloud tries to list models, either live with the account's credential or from a built-in list that is labelled as not being a query of the account; see the matrix in [`providers.md`](providers.md). `Unsupported` makes no network request.
- **`refresh::RefreshCycle`**: computes the effective deadline of each account (including a 300 s floor for providers billed per query, such as AWS Bedrock) and assembles the `Snapshot` incrementally. After `RateLimited`, `AuthenticationError` or a task failure, the account's interval doubles on each consecutive failure up to 1 h (configured intervals above 1 h are kept, never shortened); a result with any other status restores the configured interval. Manual refreshes remain available.
- **`transport`**: a guard every credential-bearing request goes through (`providers::send`, `oauth::send_secure`): HTTPS is always allowed, plain HTTP only to loopback hosts (local OAuth callbacks and the contract tests' `wiremock` servers). Anything else fails closed with `query.insecure_transport` / `oauth_error.insecure_endpoint` before a byte is sent, and the URL is never reported.
- **`metrics`**: `classify_report` and `classify_metric` derive availability and consumption (`AccountClass` and `MetricState`). `binding(report)` picks the account's binding metric: among `Quota` metrics with a finite positive cap (`RateLimit` only when none qualifies), the exhausted one with the latest `resets_at` when the account is blocked (`blocked_until`; unknown when a return is missing), otherwise the smallest remaining fraction, ties to the shortest window then adapter order; non-`Ready` or unmeasured reports return `None`. `binding_ratio`/`cmp_binding` sort accounts with no value last, and `Binding::summary` emits the localized line (`binding.*`, nested `metric_window.*`).
- **`freshness`**: `age(checked_at, now)` and `assess(checked_at, interval, now)` classify a measurement as `Fresh` (up to 1.5× the account's effective interval), `Aging` (up to 3×) or `Stale`, with an injected clock; a measurement newer than `now` (clock went backwards) reads as age zero and `clock_adjusted` reports it.
- **`pace`**: `assess(metric, now, recent)` compares a `Quota` metric in a `window_5h`/`daily`/`weekly`/`monthly` window with a uniform pace until `resets_at` (calendar month for `monthly`; an RFC 3339 start in `period` wins when coherent): elapsed and expected fractions, `delta_points`, stage `Reserve`/`OnPace`/`Ahead`/`Exhausted` (±5-point band) with a `good`/`neutral`/`warn`/`bad` tone, and a linear projection (`source` `Average` since the start, or `Recent` slope over ≥ 3 `Point { taken_at, used }` samples in the last 2 hours or 10% of the window). Missing or non-finite data, a past reset and a window under 5% (or 10 minutes) old return `None`. `stage_label`/`outlook_label` emit `pace.*` texts.
- **`resets`**: `upcoming(config, reports, now, &prefs)` plans quota reset notices from the readings already taken, with an injected clock: a `Quota` metric with a future `resets_at` in a `window_5h`/`daily`/`weekly`/`monthly` window (`other` with `include_other`), filtered by `NotifyWhen` (`OnlyIfUsed` by default, `OnlyIfExhausted`, `Always`; a missing value is neither used nor exhausted), skipping accounts in `prefs.disabled`. Metrics of one account resetting within the same minute form one `ResetEvent` with `notify_at = resets_at − lead`. `ResetKey::same_reset` treats `resets_at` within 2 minutes as the same reset. Accounts are identified by `Account::key()` (`<provider slug>:<name>`); renaming an account gives it a new key.
- **`oauth` and `login`**: `OAuthSession`, `OAuthConfig`, `OAuthLogin` and `TokenStore` manage credentials and renewal; `cuotai_core::login` exposes a cancellable, UI-independent driver with prompt/progress events and a staged commit between the vault and the TOML.

### Adding a provider

A new provider touches only the core; the TUI and the desktop app render its descriptors and reports generically.

1. Add the variant to `Provider` (`model.rs`: `label`, `slug`, `ALL`), the adapter under `providers/` and its dispatch in `lib.rs`.
2. **Declare its access descriptor (mandatory).** Add the arm to the exhaustive `match` in `access::classify` (level, credential class, scope, plan requirement, public evidence and the summary/reason texts), one arm per `params` value or login that changes the queried interface, and the `access.summary.<slug>` (plus `access.reason.<slug>` for `Unavailable`) ids in `locales/en-US.json` and `es.json`. The build fails without the arm, and the `access` tests fail without the texts or when `Unavailable` lacks a reason or `Official` lacks a documentation URL.
3. Regenerate the usage access matrix of [`providers.md`](providers.md) with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix`; the same test without the variable fails while the document differs from `access::describe`. A new interface-switching param also goes into the `MODES` list of the `access` tests, which the matrix and the invariant tests walk.
4. Add the account fields in `account_edit.rs`, the credential help, an example to `cuotai.example.toml` and the `wiremock` contract tests.

### Using it as a Rust library

```rust
use cuotai_core::{Config, Monitor};

async fn query(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let monitor = Monitor::new(config)?;
    let snapshot = monitor.refresh().await;
    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}
```

- `AccountClass` serializes as `"available"`, `"exhausted"` or `"unmeasured"`.
- `MetricState` serializes with the `state` tag: `{"state":"unmeasured"}`, `{"state":"exhausted","no_cap":true}` or `{"state":"available","ratio":0.25,"uncapped":false}` (when `uncapped` is `true`, `ratio` does not represent availability).
- Each `Metric` carries `kind` (`"quota"`, `"rate_limit"`, `"balance"`, `"credits"`, `"spend"` or `"usage"`) and `window` (`"window_5h"`, `"daily"`, `"weekly"`, `"monthly"`, `"other"` or `null`). The adapters assign these fields from the provider's contract; front ends must not infer them from `label` or `period`. The fields are additive to the `Snapshot` and to the CLI JSON.
- `account_edit::CredentialKind` serializes as `"env"`, `"profile"` or `"none"`.
- `Monitor::with_credentials(config, Arc<dyn Credentials>)` lets you inject a custom secret lookup (`Credentials::get` must be fast and non-interactive). `Monitor::with_oauth` shares the `OAuthSession` across queries.

### Internationalization (`Localized`)

Structured texts emitted by the core carry `Localized { id, params, fallback }`:

- The **core owns the catalog of the ids it emits**:
  `crates/cuotai-core/locales/{en-US,es}.json` (loaded through
  `include_str!` and `serde_json` behind a `LazyLock`). Every core id
  (field.*, native.*, login.*, credential_help.*, oauth.*,
  access.*, edit_error.*, query.*, note.*, remove_error.*,
  remove.kept.*, status.*, provider.*, binding.*, metric_window.*, pace.*) lives in `en-US.json` and `es.json`
  with the same set of keys; `pt-BR` does not consult a catalog and uses
  the Portuguese `fallback` embedded in each `Localized`. Resolution
  (`Localized::resolve`, `cuotai_core::text::core_catalog`) follows the order
  exact locale → same language → `en-US` → `fallback`.
- The desktop app receives the core catalog through the Tauri command
  `core_catalog(locale)`, and `js/i18n.js` merges it under the UI catalog
  (`apps/desktop/ui/locales/*.json`); user packs (in
  `<user dir>/locales/<code>.json`) override both. Front ends **never match
  on text**: `t()` and `tl()` look the id up in the catalog.
- The TUI resolves the core's `Localized` values through `Localized::resolve` in the
  active locale (`CUOTAI_LANG`, default `pt-BR`); Ratatui's own captions
  (shortcuts, frames, ...) are outside the scope of RF8 and remain in
  Portuguese.
- `Report.notes` keeps Portuguese text for compatibility with CLI/JSON
  consumers; `Report.notes_l10n` accompanies the notes in the same order
  with structured ids and parameters (never containing tokens). Adding a
  language only means adding a catalog.

## Development and verification commands

Run from the workspace root:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo fmt --all -- --check
```

For the desktop app (Tauri):

```sh
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Dependency licenses, advisories and sources are checked with `cargo deny` (see [`CONTRIBUTING.md`](../CONTRIBUTING.md)).

### Continuous integration

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every push and pull request (PRD 0019, RF2). On Linux, macOS and Windows it runs the commands above for both workspaces: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and `cargo test`, with `--locked`. A separate Linux job runs `cargo deny` on both workspaces. Each job has a timeout, the token is read-only (`contents: read`), every action is pinned by commit hash, the workflow uses no secrets, and pull requests (forks included) run with `pull_request`, never `pull_request_target`. The `target/` cache is only written from `main`.

Platform coverage of the test suite:

| Area | Linux | macOS | Windows |
|---|---|---|---|
| Contract, OAuth, persistence, TUI and desktop logic tests | yes | yes | yes |
| Owner-only permissions (`0600`/`0700`) and symlink handling in `ConfigFile`, `last_config`, the model cache and the dev SQLite store | yes | yes | no (`#[cfg(unix)]`: Unix permission bits and symlinks) |
| Fake `aws` and `bl` command-line tools (`#!/bin/sh` scripts) | yes | yes | no (`#[cfg(unix)]`) |
| OS credential vault (Keychain, Credential Manager, Secret Service) | not tested | not tested | not tested |
| Tray, notifications and opening the system browser | not tested | not tested | not tested |

Tests never touch the real vault, tray, notifications or browser: OAuth and account tests use an in-memory `TokenStore`, and the desktop tests exercise the backend without starting Tauri. Those surfaces need a packaged app and a manual check (PRD 0019, RF5.2). On Linux the desktop build needs WebKitGTK (`libwebkit2gtk-4.1-dev`). On Windows, `build.rs` embeds Tauri's Common Controls v6 manifest into every artifact so the test executables can load (tauri-apps/tauri#13419), and the bundle lists `icons/icon.ico`, which the Windows resource step requires.

### Test and verification scope

- **Contract tests (`wiremock`):** exercise pagination, cents/currency, overflow, incomplete schemas, billing filters, gross versus net consumption, balance versus spend, and missing or invalid credentials, without depending on an external network.
- **OAuth and vault tests:** cover callback/CSRF, denial, polling/`slow_down`/expiry, RSA/X25519/AES-GCM encryption, refresh-token rotation, per-account/provider isolation (for example, multiple Kiro accounts) and the staged commit.
- **Persistence and TUI tests:** validate editing and reordering in `ConfigFile` while preserving comments, `0600` permissions, rejection of symlinks and concurrent edits, and rendering of the dashboard and wizard.
- Simulated HTTP responses validate contracts and error handling, but they do not replace authorization and querying against real production accounts.
