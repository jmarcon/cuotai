# Architecture, Rust Library and Verification

## Core / TUI / Desktop split

```text
crates/cuotai-core/       Rust library: accounts and persistence, credentials, providers, metrics, queries
crates/cuotai-tui/        cuotai binary: CLI, sign-in wizard, Ratatui, keyboard/mouse
apps/desktop/             Tauri 2 app: main window, tray popover and native menu
```

`cuotai-core` is independent of any UI (it depends on neither Ratatui, Crossterm nor a graphical loop):

- **`account_edit`**: `field_descriptors` and `native_option_descriptors` define fields, options and which of them are required per provider; `visible_field_indices` decides which fields apply for a given product, mode and login; `build_account` validates a draft against that schema; `Config::upsert_account`/`without_account` create, edit or remove accounts.
- **`config_file::ConfigFile`**: discovers, loads and atomically writes the TOML, preserving comments and rejecting concurrent external edits.
- **`Monitor`**: `refresh` and `refresh_account` query providers and normalize reports; the internal semaphore limits full and single-account queries together (1–32), applies a per-account deadline (1–300 s), preserves the configured order and isolates failures per account.
- **`models`**: per-account model catalog, outside the quota refresh. `Monitor::with_model_cache(ModelCache::in_data_dir(&file))` writes to `<OS persistent data dir>/cuotai/models/models.sqlite3` (outside any cache folder, separate from the TOML, the vault and the debug credential SQLite; directory `0700` and file `0600` on Unix, even if they already existed). Each row stores digests only: SHA-256 of `ConfigFile::identity()` and of the account identity (provider, name, `credential_env`/`oauth` and `params`, except `models` and `billing_cycle`, which only filter or date the quota); never secrets, HTTP bodies or URLs. Moving an account keeps its entry; renaming it or changing its credential, product or region creates another one. `Monitor::prune_models` deletes configuration entries that are absent from the freshly loaded or saved TOML (the TUI and the desktop app call it on open, on saving an edit and on removal), without touching live accounts of another window. States: `not_fetched`, `fetching`, `ready`, `failed`, `interrupted`, `unsupported`. `model_state` only reads; `load_models` queries only when no attempt is recorded (success, empty list, failure or interruption are stored and not repeated); `sync_models` queries and, if it fails, keeps the previous catalog with its date. Every query needs a per-account claim (owner token + lease = account deadline + 30 s) taken in `BEGIN IMMEDIATE`: another window or process with a live claim does not query and instead waits for the stored result (100 ms polling, bounded by the lease), so simultaneous syncs become a single request; only the owner publishes, so a stale completion never overwrites a newer result. Cancellation expires the lease immediately and a killed process lets the lease run out: in both cases the row becomes `interrupted`, with no partial list and no automatic retry, until a manual `sync_models`. A persistence error is `ModelError::Storage`. Today every provider except Alibaba Cloud tries to list models, either live with the account's credential or from a built-in list that is labelled as not being a query of the account; see the matrix in [`providers.md`](providers.md). `Unsupported` makes no network request.
- **`refresh::RefreshCycle`**: computes the effective deadline of each account (including a 300 s floor for providers billed per query, such as AWS Bedrock) and assembles the `Snapshot` incrementally. After `RateLimited`, `AuthenticationError` or a task failure, the account's interval doubles on each consecutive failure up to 1 h (configured intervals above 1 h are kept, never shortened); a result with any other status restores the configured interval. Manual refreshes remain available.
- **`transport`**: a guard every credential-bearing request goes through (`providers::send`, `oauth::send_secure`): HTTPS is always allowed, plain HTTP only to loopback hosts (local OAuth callbacks and the contract tests' `wiremock` servers). Anything else fails closed with `query.insecure_transport` / `oauth_error.insecure_endpoint` before a byte is sent, and the URL is never reported.
- **`metrics`**: `classify_report` and `classify_metric` derive availability and consumption (`AccountClass` and `MetricState`).
- **`oauth` and `login`**: `OAuthSession`, `OAuthConfig`, `OAuthLogin` and `TokenStore` manage credentials and renewal; `cuotai_core::login` exposes a cancellable, UI-independent driver with prompt/progress events and a staged commit between the vault and the TOML.

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
  cost_advisory.*, edit_error.*, query.*, note.*, remove_error.*,
  remove.kept.*, status.*, provider.*) lives in `en-US.json` and `es.json`
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

### Test and verification scope

- **Contract tests (`wiremock`):** exercise pagination, cents/currency, overflow, incomplete schemas, billing filters, gross versus net consumption, balance versus spend, and missing or invalid credentials, without depending on an external network.
- **OAuth and vault tests:** cover callback/CSRF, denial, polling/`slow_down`/expiry, RSA/X25519/AES-GCM encryption, refresh-token rotation, per-account/provider isolation (for example, multiple Kiro accounts) and the staged commit.
- **Persistence and TUI tests:** validate editing and reordering in `ConfigFile` while preserving comments, `0600` permissions, rejection of symlinks and concurrent edits, and rendering of the dashboard and wizard.
- Simulated HTTP responses validate contracts and error handling, but they do not replace authorization and querying against real production accounts.
