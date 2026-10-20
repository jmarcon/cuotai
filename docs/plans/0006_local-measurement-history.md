# PRD 0006 — Local measurement history

| Field | Value |
|---|---|
| ID | 0006 |
| Status | In progress |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (details), `crates/cuotai-tui` (details and `--history`), PRDs 0007, 0008, 0012 and 0014 |
| Depends on | none |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.37.0 | `cuotai_core::history::HistoryStore`: `new(path, &ConfigIdentity)` and `in_data_dir(&ConfigFile)` (`<OS local data dir>/cuotai/history/history.sqlite3`, its own directory, `0700`/`0600` on Unix even when they already existed, sharing `models::create_private_file`); the file is opened per operation in WAL mode with a 5 s busy timeout (a journal-mode switch racing another first open is retried within it). Rows are keyed by `ConfigIdentity::digest()` and the model catalog's account digest (`models::account_key`, now shared), so a move keeps the series and a rename or a new credential, product or region starts another. `record(account, taken_at, &[Metric])` writes one sample per metric in one `BEGIN IMMEDIATE` transaction, `INSERT OR IGNORE` on `(config, account, metric_key, taken_at)` to the millisecond, and returns how many were new; non-finite values become `NULL`, never zero. `mark_removed(&saved_config, now)` sets `removed_at` on the series of accounts gone from the saved TOML (kept until RF4 prunes them) and clears it when the identity comes back or is recorded again; it never creates the file. Schema v1 through an append-only forward migration list (`PRAGMA user_version`, applied inside `BEGIN IMMEDIATE`; a newer version is rejected): `samples` (`account_digest`, `metric_key`, `taken_at`, `used`, `limit`, `remaining`, `unit`, `kind`, `window`, `resets_at`, plus `config_key`; times in UTC unix ms; primary key on `(config_key, account_digest, metric_key, taken_at)`, an index on `(config_key, taken_at)`) and `series` (label, position and `seen_at` of the newest reading carrying the metric, `removed_at`). `history::metric_key(&Metric)` is the SHA-256 of kind, window, label and unit. Every open, read or write failure is `HistoryError::Storage` (`history.error.storage`, en-US/es). Only recording API: nothing calls it before RF2. |
| RF3 | 0.37.0 | Network-free queries on `HistoryStore`, each a `Result` (storage errors are never an empty answer): `latest(account) -> Option<StoredReport { checked_at, metrics: Vec<StoredMetric { key, taken_at, metric }> }>`, the metrics of the last recorded reading in their order, each with its own last sample and original time (`checked_at` is that reading's time, aged with `freshness::age`; `period` is not stored); `series(account, metric_key, impl RangeBounds<DateTime<Utc>>) -> Vec<pace::Point>` in time order, without samples lacking a finite `used`; `windows(account, metric_key, range) -> Vec<WindowUsage { started_at, ended_at, resets_at, limit, initial_used, final_used, peak_used, peak_at, samples }>`, where a window ends when `used` drops by more than 20% of a finite positive `limit` or when `resets_at` moves beyond `resets::DRIFT_TOLERANCE` (2 minutes) from the last one seen in the window (slow drift stays one window; a metric without a limit splits only on `resets_at`; non-finite values are ignored); `bounds() -> Option<Bounds { first, last }>` over this configuration. Table tests cover increasing use, a reset drop, a drop within 20%, drift within and beyond the tolerance, gaps of days, a metric with no limit and non-finite values; a local smoke read 30 days of one metric at a 5-minute cadence (8,640 samples) in about 6 ms on a debug build. |
| RF2, RF4 to RF6 | — | Not delivered. |

**Decisions (2026-10-20):** the `series` table keeps the metric label in clear text (adapter metric names, already part of `metric_key`) so `latest` can name what it restores; `period` (adapter descriptive text) is not stored. Account names, credential references, params, the TOML path, notes and provider bodies never enter the file.

## 1. Context

`RefreshCycle` (`crates/cuotai-core/src/refresh.rs`) keeps only the last `Report` of each account in memory (`slots[index] = Some(report)`) and the `Snapshot` assembled from them. When the app closes nothing survives: on the next opening every card stays empty until the first query finishes. There is no time series, no trend chart and no way to know how much of a quota was consumed yesterday.

The core already persists local data in SQLite (`rusqlite` with `bundled`): the model catalog lives in `<OS local data dir>/cuotai/models/models.sqlite3`, with restricted permissions, isolation by configuration and account digest, and persistence errors exposed (PRD 0002, RF3).

The 2026-10-19 market analysis showed history and trend in CodexBar (charts and burn-down from recorded snapshots), OpenQuota (today, yesterday and 30 days) and OpenUsage (30-day spend blocks).

## 2. Problem

- You cannot see the trend of a quota, nor whether this week's consumption is above normal.
- The card is empty on every opening of the app, even if the last reading was taken minutes before.
- Future features depend on recorded data: pace with a recent slope (PRD 0007), alerts that recognize the same window across restarts (PRD 0008) and widgets with a chart (PRD 0014).

## 3. Goals

- Record, per account and per metric, the successful readings in a local database shared between the TUI, the desktop app and the CLI pointing at the same TOML.
- Expose series, last-reading and window queries in the core, with no business logic in the screens.
- Show a trend in the account details and allow exporting and erasing the data.
- Keep retention and size bounded and predictable.

## 4. Non-goals

- History of errors, free-form notes or any provider text: only numeric values and window metadata.
- Synchronization between machines, the cloud or user accounts.
- Retroactive billing: history starts at the first recorded reading; the past is never reconstructed.
- Changing `Report`, `Snapshot`, scheduling or the current `--json` output.

## 5. Functional requirements

### RF1 — Storage (high priority; prerequisite)

- **RF1.1** Create `history::HistoryStore` in `cuotai-core`, in a SQLite file separate from the others, at `<OS local data dir>/cuotai/history/history.sqlite3`, with `0700` permissions on the folder and `0600` on the file on Unix. An open or write error is exposed, never treated as success. No secret, HTTP body, URL or note enters the database.
- **RF1.2** Isolate by the digest of the normalized TOML path and by the digest of the account identity, with the same derivation as the model catalog (PRD 0002, RF3.2). Moving an account preserves its series; renaming it or changing credential, product or region opens a new series; removing the account marks the series for pruning.
- **RF1.3** Versioned schema (`PRAGMA user_version`) with forward migration. `samples` table: `account_digest`, `metric_key`, `taken_at` (UTC), `used`, `limit`, `remaining`, `unit`, `kind`, `window`, `resets_at`. `metric_key` is the digest of `kind + window + label + unit`. Missing values are `NULL`, never zero.
- **RF1.4** The database uses `WAL` and `busy_timeout`; two instances (TUI and desktop) write without corruption. Writes are idempotent by `(account_digest, metric_key, taken_at)`.
- **Acceptance criterion:** tests with a temporary database cover creation, migration, permissions, two instances writing, missing values, a moved, renamed and removed account, and a simulated disk failure (error exposed).

### RF2 — Recording from the refresh (high priority)

- **RF2.1** `Monitor::with_history(HistoryStore)` follows the `with_model_cache` pattern: with no store configured, nothing changes in current behavior.
- **RF2.2** After each `Report` with `Status::Ready`, record one sample per eligible metric. Error, authentication, rate-limit and `NeedsConfiguration` reports record nothing.
- **RF2.3** Compaction on input: do not record a sample identical to the previous one of the same metric less than 15 minutes after it, unless `resets_at` changed beyond the 2-minute tolerance (a new window). This limits growth when the query interval is short.
- **RF2.4** Recording runs off the refresh's critical path (a separate task with a bounded queue); a slow disk does not delay the cycle or the account's deadline.
- **Acceptance criterion:** `wiremock` tests with a fixed clock: a `Ready` query records N samples; a query with an error records nothing; a repeated sample within 15 minutes is omitted; a window change records; a full queue drops without blocking.

### RF3 — Queries in the core (high priority)

- **RF3.1** A pure API with no network:
  - `latest(account) -> Option<StoredReport>`: the last reading per metric, with the original age (input for PRD 0010);
  - `series(account, metric_key, range) -> Vec<Point>`;
  - `windows(account, metric_key, range) -> Vec<WindowUsage>`: groups samples by window instance (same `resets_at` within the 2-minute tolerance) and reports initial use, final use and peak;
  - `bounds()` with the first and last recorded dates.
- **RF3.2** Reset detection: a drop in `used` greater than 20% of `limit`, or an advance in `resets_at` beyond the tolerance, ends the current window. Non-finite values are ignored.
- **Acceptance criterion:** table tests with synthetic series (increasing, reset, `resets_at` drift, gaps of days, a metric with no limit).

### RF4 — Retention, export and cleanup (high priority)

- **RF4.1** Default retention of 90 days, configurable from 7 to 365 days in `desktop.json` (a per-device preference) and through `--history-days` on the CLI. Pruning on open and then every 24 hours, in batches, without blocking reads.
- **RF4.2** Resolution reduction: samples older than 7 days are reduced to one per hour (the one with the highest `used`), preserving each window's peak.
- **RF4.3** Size ceiling (default 64 MiB). When exceeded, prune the oldest samples first and record a visible warning, never silently.
- **RF4.4** Export to CSV and JSON (account, metric, range) and erase one account's history or everything, with confirmation in the interface. Erasing also removes orphaned series.
- **Acceptance criterion:** tests for pruning by age and by size, resolution reduction preserving the peak, export with missing values, and partial and total erasure.

### RF5 — Display on the desktop (medium priority)

- **RF5.1** The Tauri command `account_history(index, metric_key, range)` returns the series computed by the core, with no secret in the IPC; discarding responses for an old identity follows the model catalog's pattern.
- **RF5.2** In the account details, each metric with history shows a compact area chart (24 h, 7 days, 30 days) with a highlighted end point and reset markers. Accessible: an equivalent table hidden for screen readers, and `prefers-reduced-motion` respected (PRD 0004).
- **RF5.3** In **Settings → History**: turn recording on and off (on by default), retention, size on disk, export and erase. Turning it off stops recording and keeps the data until the user erases it.
- **Acceptance criterion:** a smoke test in three languages with several days of history, a new account with no history (an explained empty state), recording turned off and erasure.

### RF6 — Display in the TUI and CLI (medium priority)

- **RF6.1** In the TUI account details, a line with a Unicode-block sparkline (`▁▂▃▄▅▆▇█`) per metric over the chosen range (keys for 24 h, 7 d and 30 d). A narrow width degrades to text with minimum, maximum and last value.
- **RF6.2** `cuotai --history [ACCOUNT] [--history-days N] [--json]` prints the series and exits with 0, 2 or 1 like the other non-interactive outputs. With no history, it says there is no data instead of failing.
- **Acceptance criterion:** `TestBackend` rendering tests and a CLI smoke test with a sample database.

## 6. Non-functional requirements

- **Privacy:** the data is numeric and stays on the user's computer. The history file never enters the TOML or logs; export is explicit.
- **Performance:** batched writes, with an index on `(account_digest, metric_key, taken_at)`; reading 30 days of one metric under 50 ms on ordinary hardware.
- **Compatibility:** without changing `Report`, `Snapshot` or `--json`; accounts without history keep working.
- **Risks:** the `label` of the metrics may change between adapter versions and break series continuity; this is documented and the new series starts clean.

## 7. Order of execution and verification

1. **RF1 and RF3:** storage and queries with table tests; no visible change.
2. **RF2:** recording wired into the `Monitor`; a test that the cycle is not delayed.
3. **RF4:** retention, export and cleanup.
4. **RF5 and RF6:** desktop, TUI and CLI, one at a time, with a smoke test.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when, after closing and reopening the app, the series of the last days is available on the three surfaces, the file's growth respects the retention and the ceiling, erasing the history removes it entirely, and no error report or secret is recorded.
