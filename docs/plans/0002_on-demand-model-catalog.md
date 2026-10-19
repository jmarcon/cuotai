# PRD 0002 — On-demand model catalog

| Field | Value |
|---|---|
| ID | 0002 |
| Status | Implemented (0.14.x) |
| Owner | `cuotai-core`, TUI and desktop |
| Consumers | `crates/cuotai-tui`, `apps/desktop` |
| Date | 2026-10-16 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-17)

All RFs were implemented in the core, the TUI and the desktop app, one at a time, with validation (`cargo fmt`, `clippy -D warnings`, workspace and desktop tests, TUI and desktop UI smoke tests) before each version.

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.8.0, 0.14.0 | A single resolution in the core (TUI: `--config` > `CUOTAI_CONFIG` > an existing `./cuotai.toml` > OS; desktop: `CUOTAI_CONFIG` > chosen file > OS), TOML choice in the desktop settings, normalized `ConfigIdentity` and a warning when the TUI and the desktop app use different files. |
| RF2 | 0.9.0 | `ModelInfo`/`ModelCatalog`/`ModelState` and `Monitor::{model_state, load_models, sync_models}` reusing the client, OAuth, timeout and semaphore. |
| RF3 | 0.10.0 | `models.sqlite3` in the data directory (0700/0600, digests only), `fetching`/`ready`/`failed`/`interrupted` states, claiming across processes and `prune_models`. |
| RF4 | 0.11.0 | Verified adapters (Groq, AWS Bedrock, Antigravity, Gemini AI Studio, Kimi, Poolside, Alibaba Model Studio intl), bounded pagination and the matrix of the 36 providers in `docs/providers.md`. |
| RF5 | 0.12.0 | Models section in the TUI (details and wizard with a configured account), local search and manual sync. |
| RF6 | 0.13.0 | Models section in the desktop app (details, popover and account dialog), accessible search, manual sync and discarding of results for an old identity. |

Sections 1 and 2 describe the situation of 2026-10-16, before the implementation. The first set of RF4.1 adapters was extended with Gemini AI Studio, Kimi, Poolside and Alibaba Model Studio (intl), each verified against an official source.

## 1. Context

`Monitor::refresh_account` shares the HTTPS client, OAuth, timeout and concurrency limit, but returns only usage reports (`crates/cuotai-core/src/lib.rs`). `RefreshCycle` keeps reports and deadlines in memory, not a persistent catalog. Groq already queries `GET /openai/v1/models` to read rate-limit headers (`providers/router_keys.rs`); Antigravity uses `fetchAvailableModels` for quota on paid plans (`providers/antigravity.rs`). That is not a searchable catalog and does not authorize automatically querying model lists during a refresh.

The TUI and the desktop app can load different TOMLs: the TUI prefers an existing `./cuotai.toml` after `--config`, while the desktop app uses `CUOTAI_CONFIG` or the OS configuration (`config_file.rs::resolve_path`, `crates/cuotai-tui/src/main.rs`, `apps/desktop/src-tauri/src/backend.rs::ensure_loaded`). There is no standalone provider page: the wizard selects providers, the cards open account details, and the desktop app uses a form and a details dialog/popover. In this PRD, **provider page** means selecting/editing the provider with a Models section; **details** are those of the configured account.

## 2. Problem

It is not possible to discover and search the models available to an account in the TUI or the desktop app. A query per screen opening, quota refresh or typed search would be unnecessary and could cost money; a memory-only or `localStorage` cache would not survive a restart nor be shared. A per-`Provider` cache would confuse distinct accounts, regions and credentials. Even a shared cache does not solve the fact that the front ends can use different configuration files.

## 3. Goals

- Query the models **of the configured account** only on the first explicit opening of Models and again only when the user asks for **Sync models**.
- Persist and share results between the TUI and the desktop app that point to the same TOML; a local search does not query the provider.
- Preserve the TUI's choice of `./cuotai.toml`, and also let the desktop app explicitly pick that same file.
- Show source, scope, date and state so the catalog is not confused with quota, inference permission or instantaneous availability.

## 4. Non-goals

- Querying all 36 providers without a verified endpoint, permission and credential; inventing models from metrics or static tables.
- Using inference requests, TTL/timer synchronization, prefetch, background revalidation or per-character remote search.
- Automatically migrating/merging TOMLs, storing tokens in the TOML/cache, or changing `Report`, `Snapshot` and quota scheduling.

## 5. Functional requirements

### RF1 — Shareable configuration path (high priority; prerequisite)

- **RF1.1** Centralize resolution in `ConfigFile`: in the TUI, `--config` > `CUOTAI_CONFIG` > an **existing** `./cuotai.toml` > `<OS config dir>/cuotai/config.toml`. `--config ./cuotai.toml` still allows creating the local file explicitly. With neither a local file nor an override, the wizard creates the file at the OS path. Preserve the TUI's current preference for an existing local file.
- **RF1.2** The desktop app uses `CUOTAI_CONFIG`, a file explicitly chosen in the application settings, or the OS path; it does not infer `./cuotai.toml` from its `cwd`. Allow choosing, in the desktop app, the same local TOML as the TUI. Show the effective path in both interfaces and report when the paths differ; different configs imply isolated caches. Never migrate, merge or overwrite local files automatically.
- **RF1.3** Normalize relative paths and symlinks to the identity of the same file, including before it is created (existing parent directory plus name). The resolved `ConfigFile::path` feeds the cache identity; integrators with no file supply a stable identity explicitly. Update the CLI help, `docs/tui.md` and `apps/desktop/README.md` when implementing.
- **Acceptance criterion:** with no `./cuotai.toml`, both with no overrides see the OS path/accounts, even with a different `cwd`. With an existing local file, the TUI keeps using it; selecting the same file in the desktop app makes both see the same accounts after a reload and share the cache. Test precedence, explicit local creation, symlink/relative paths and distinct files.

### RF2 — Per-account model contract in the core (high priority)

- **RF2.1** Expose `ModelInfo` (real ID, optional name/description), `ModelCatalog` (models, source/scope, `fetched_at`) and typed states/errors; the texts presented to the front ends use `text::Localized` with a Portuguese fallback. Missing values stay missing; a catalog does not prove authorization for inference, price or current availability.
- **RF2.2** Expose reading the state without network, and explicit operations `load_models(account_index)` and `sync_models(account_index)`. `load_models`: with no entry it makes the first query; with a success, an empty list or a recorded failure it only reads the state. `sync_models`: the only operation that queries again after the first attempt, preserving the previous catalog if it fails. `Unsupported` makes no network request. An invalid index is a typed error.
- **RF2.3** Reuse the `Monitor`'s client, `Context`, shared OAuth, timeout and concurrency limit; `Monitor::with_config` keeps access to the same cache. The cache location/identity is configured explicitly from `ConfigFile::path`, without guessing `cwd` and without requiring a cache for the existing `Monitor::refresh`. These operations do not change `RefreshCycle`, `Report`, `Snapshot` or deadlines.
- **Acceptance criterion:** two accounts of the same provider with distinct credentials/regions return isolated catalogs; the first opening queries once; reopening, restarting, searching or refreshing quota produce no **additional catalog query**. A successful empty result differs from the absence of a query.

### RF3 — Persistence and manual sync (high priority)

- **RF3.1** Persist the catalog in a SQLite file separate from the TOML/vault/debug-credential SQLite (`rusqlite` is already available), in the OS **persistent data directory**, not in a folder subject to cleanup. The TUI and the desktop app use the same database when they point to the same file. A persistence error is exposed, never treated as success. Restrict permissions on Unix and do not store secrets, HTTP bodies, URLs with credentials or sensitive parameters.
- **RF3.2** Isolate by the digest of the normalized TOML path and the digest of the account identity (provider, name, credential reference and relevant parameters), not by index or by provider alone. Moving an account preserves the entry; editing the credential/product/region or renaming conservatively invalidates; removing the account discards its entry. Do not store the raw, potentially sensitive identity.
- **RF3.3** Store the attempt state (`fetching`, `ready`, `failed`, `interrupted`), the date of the last success and the full catalog only after all pages succeed. An initial failure is recorded and not retried automatically. A sync failure keeps the last searchable catalog with the earlier date and a visible error. A valid response with zero models is `ready` with a date. Cancellation or a terminated process does not publish a partial list.
- **RF3.4** Serialize the claim per account in SQLite: two simultaneous windows/processes do not trigger two first queries. Coalesce concurrent syncs; an old completion does not overwrite a newer result. An abandoned claim becomes `interrupted`, visible after recovery, without an automatic query; the manual action allows trying again. Searching and reading the cache make no network request.
- **Acceptance criterion:** tests for miss → 1 call, hit after restart → 0, empty → 0, initial failure after restart → 0, manual sync → 1, sync error preserving the last result, two simultaneous instances → 1, crash/interruption → recoverable state without a refetch, edited/moved/removed identity, and shared/isolated paths.

### RF4 — Verified adapters and limitations (high priority)

- **RF4.1** Enable the list only for a read-only endpoint with a proven credential, product, region, permissions, cost and semantics. The first set of contracts to implement/test: Groq (`/openai/v1/models`, already used for headers), Bedrock by profile/region (`aws bedrock list-foundation-models`, different from Cost Explorer) and Antigravity only where `fetchAvailableModels` returns a catalog. **Before enabling each one**, validate that the configured credential gives access. Do not add a call to the existing quota refresh.
- **RF4.2** Evaluate Gemini AI Studio only with a Gemini API key; `models.list` is paginated and the key is not to be confused with a Cloud Monitoring/CLI token. Do not assume that OpenAI/Anthropic admin keys, Azure or subscription tokens list inference models. For the rest, investigate the endpoint and scope; with no evidence, `Unsupported` with a reason and zero network. Publish the matrix of the 36 `Provider::ALL` with status and source in `docs/providers.md` at implementation.
- **RF4.3** Paginate to completion with a page limit and repeated-token detection; an incomplete schema/page rejects the sync without overwriting the cache. Deduplicate IDs and sort for presentation without changing the real IDs. Errors do not show bodies, headers, tokens or sensitive URLs.
- **Acceptance criterion:** local HTTP/CLI fixtures cover a full list, pagination, an invalid/incomplete page, 401/403/429, distinct region/product/credential and `Unsupported`; no adapter invents a list from metrics. [Gemini models.list](https://ai.google.dev/api/models) and [Bedrock ListFoundationModels](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_ListFoundationModels.html) document their contracts; they do not guarantee access to the user's account.

### RF5 — Search in the TUI selection and details (medium priority)

- **RF5.1** In `ui.rs`/`dashboard.rs`, offer a Models section in the provider selection/editing and in the account details. In the selection, require an explicit choice of an **already configured account** of that provider before the first query; a new account with no credential shows the support status and guidance, not a fabricated list. Do not create an empty standalone page.
- **RF5.2** When Models opens, read the cache/trigger `load_models` only if eligible; a search field filters locally by ID/name, separate from the global account filter. Show status, source/scope, count, `fetched_at`, empty states and the error together with the old result. Keys, focus, scroll, mouse and `Esc` follow the panel. A distinct footer/shortcut **Sync models**; the asynchronous operation does not freeze navigation nor refresh quota.
- **Acceptance criterion:** a TUI smoke test with a supported account/no cache, cache after restart, two accounts of the same provider, search without HTTP, unavailability and a sync error keeping the catalog; `r`, `R`, `--once` and `--json` create no additional model-sync calls.

### RF6 — Search in the desktop selection and details (medium priority)

- **RF6.1** Expose through the Tauri backend read/first query/sync per validated account, with no secret in the IPC. In `account-dialog.js`, search the models of the account being edited or explicitly choose an existing account of the provider when creating another; with no account, show guidance. In `detail.js`/`details-dialog.js`, show a searchable section in the window and popover details. `lib.js` reuses the same calls; `localStorage` is not the source of truth.
- **RF6.2** Local search by ID/name with a labelled `<input type="search">`, keyboard navigation, announced progress (`aria-live`) and a manual sync button disabled during the operation. Render state/source/date/error without discarding the previous catalog; localize in `pt-BR`, `en-US` and `es`. If the configuration changes during a query, discard the old identity's result, never assign it to the new index.
- **Acceptance criterion:** a desktop smoke test in three languages covers the first query, cache after closing/reopening, search without HTTP, manual sync, an error preserving data, a new account with no credential, change/removal/reordering of the account, and access in the popover. The same TOML chosen in the TUI provides the same accounts and cached models.

## 6. Non-functional requirements

- **Security:** never store or serialize tokens/cookies, a raw response or an authenticated URL; model queries are read-only and do not invoke inference. A lack of permission is distinct from an empty list.
- **Compatibility:** the TUI keeps `./cuotai.toml` and `--config`; the desktop app does not infer `cwd`. Distinct configurations/caches stay isolated; the TOML format and the quota output do not change.
- **Cost and honesty:** no catalog query is triggered by a refresh/scheduler, a search or a restart. Groq/Antigravity may already reach their endpoints in a quota refresh: the criterion is not to **add** a sync call on those paths, not to promise zero traffic to the existing endpoints.

## 7. Order of execution and verification

1. **RF1 first:** resolve paths, choose the TOML in the desktop app, update the documentation and prove sharing/isolation.
2. **RF2–RF3:** the core contract and the SQLite cache with concurrency, failure and restart tests; keep `Monitor::refresh` independent.
3. **RF4:** validate contracts/credentials with sources, deterministic fixtures and a capability matrix.
4. **RF5–RF6:** integrate the TUI and the desktop app, visually prove search and manual sync in both; run `cargo test --workspace`, Clippy, formatting, desktop tests/build and CLI, TUI and desktop UI smoke tests.

The requirement is only delivered when a supported account's catalog survives closing and is searchable in both front ends configured for **the same file**, without a new query; another query happens exclusively after **Sync models**. Unsupported providers show a reason without network, and an error/partial page never replaces a valid catalog.
