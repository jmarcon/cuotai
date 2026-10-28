# PRD 0006 — Local measurement history

| Field | Value |
|---|---|
| ID | 0006 |
| Status | Implemented |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (details), `crates/cuotai-tui` (details and `--history`), PRDs 0007, 0008, 0012 and 0014 |
| Depends on | none |
| Extended by | PRD 0026 (statistics over stored windows, event journal), PRD 0028 (RF11: per-adapter label snapshot and alias table so series survive label changes), PRD 0029 (stable account ids replace the identity digest as the series key), PRD 0033 (reset-aware restore of the last reading; samples more than 5 minutes in the future rejected), PRD 0051 (same-major downgrade read, typed newer-major failure, `.bak` migration and history relocation when the TOML moves; fix requirement COMP-03), PRD 0052 (RF5: TUI/CLI history export and erase parity) |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.37.0 | `cuotai_core::history::HistoryStore`: `new(path, &ConfigIdentity)` and `in_data_dir(&ConfigFile)` (`<OS local data dir>/cuotai/history/history.sqlite3`, its own directory, `0700`/`0600` on Unix even when they already existed, sharing `models::create_private_file`); the file is opened per operation in WAL mode with a 5 s busy timeout (a journal-mode switch racing another first open is retried within it). Rows are keyed by `ConfigIdentity::digest()` and the model catalog's account digest (`models::account_key`, now shared), so a move keeps the series and a rename or a new credential, product or region starts another. `record(account, taken_at, &[Metric])` writes one sample per metric in one `BEGIN IMMEDIATE` transaction, `INSERT OR IGNORE` on `(config, account, metric_key, taken_at)` to the millisecond, and returns how many were new; non-finite values become `NULL`, never zero. `mark_removed(&saved_config, now)` sets `removed_at` on the series of accounts gone from the saved TOML (kept until RF4 prunes them) and clears it when the identity comes back or is recorded again; it never creates the file. Schema v1 through an append-only forward migration list (`PRAGMA user_version`, applied inside `BEGIN IMMEDIATE`; a newer version is rejected): `samples` (`account_digest`, `metric_key`, `taken_at`, `used`, `limit`, `remaining`, `unit`, `kind`, `window`, `resets_at`, plus `config_key`; times in UTC unix ms; primary key on `(config_key, account_digest, metric_key, taken_at)`, an index on `(config_key, taken_at)`) and `series` (label, position and `seen_at` of the newest reading carrying the metric, `removed_at`). `history::metric_key(&Metric)` is the SHA-256 of kind, window, label and unit. Every open, read or write failure is `HistoryError::Storage` (`history.error.storage`, en-US/es). Only recording API: nothing calls it before RF2. |
| RF3 | 0.37.0 | Network-free queries on `HistoryStore`, each a `Result` (storage errors are never an empty answer): `latest(account) -> Option<StoredReport { checked_at, metrics: Vec<StoredMetric { key, taken_at, metric }> }>`, the metrics of the last recorded reading in their order, each with its own last sample and original time (`checked_at` is that reading's time, aged with `freshness::age`; `period` is not stored); `series(account, metric_key, impl RangeBounds<DateTime<Utc>>) -> Vec<pace::Point>` in time order, without samples lacking a finite `used`; `windows(account, metric_key, range) -> Vec<WindowUsage { started_at, ended_at, resets_at, limit, initial_used, final_used, peak_used, peak_at, samples }>`, where a window ends when `used` drops by more than 20% of a finite positive `limit` or when `resets_at` moves beyond `resets::DRIFT_TOLERANCE` (2 minutes) from the last one seen in the window (slow drift stays one window; a metric without a limit splits only on `resets_at`; non-finite values are ignored); `bounds() -> Option<Bounds { first, last }>` over this configuration. Table tests cover increasing use, a reset drop, a drop within 20%, drift within and beyond the tolerance, gaps of days, a metric with no limit and non-finite values; a local smoke read 30 days of one metric at a 5-minute cadence (8,640 samples) in about 6 ms on a debug build. |
| RF2 | 0.38.0 | `Monitor::with_history(HistoryStore)` (kept by `with_config`; without it nothing changes) spawns a `history::HistoryRecorder`: one worker thread fed by a bounded queue (64), so `refresh_account` only enqueues and a full queue drops the reading (`HistoryStatus::dropped`, a visible warning). Only `Status::Ready` reports are recorded, with their metrics that carry a finite `used` or `remaining`, at the report's `checked_at`. `HistoryStore::record` skips a sample identical (used, limit, remaining) to the previous one of its metric taken less than 15 minutes before unless `resets_at` moved beyond the 2-minute tolerance; the series still dates the reading, so `latest` ages it right. Every storage failure lands in `HistoryStatus::error` and `HistoryStatus::warning()` (`history.error.*`, `history.warning.*`, en-US/es) shown by the TUI status line, the desktop banner and `--once`/`--json` on stderr; `flush_history` lets the CLI wait (5 s) before exiting. Wired: TUI, desktop (`desktop.json` retention) and `--once`/`--json`; `mark_history_removed` runs where `prune_models` does. PRD 0010 RF2.3: `Monitor::restored_readings` (`HistoryStore::latest_reports`) feeds `RefreshCycle::restore_last_good`, so on opening each account shows its last recorded reading as stale (TUI card "Aguardando · (desatualizada)", desktop `ReportView.pending` with `last_good`) until the first query answers. PRD 0007 RF1.5: `Monitor::recent_points` / `HistoryStore::recent` read the span `pace::recent_since` names, so the pace projects from the recent slope on the TUI, the desktop and `--once`. `wiremock` tests with a fixed clock: a `Ready` query records one sample per metric, a 401 records nothing, a repeat 10 minutes later is omitted, a new window records, and with the file write-locked five refreshes each return in under 2 s while the capacity-1 queue drops and the rest lands once the lock goes. |
| RF4 | 0.38.0 | `history::Retention` (90 days by default, 7 to 365, `RetentionError::Bounds`; size ceiling 64 MiB of live pages) on the store (`with_retention`), from `--history-days` on the CLI/TUI and `history_days` in `desktop.json` (Settings → History, applied at once through `Monitor::set_history_retention`). `HistoryStore::prune(now)`, run by the recorder on open and then every 24 hours, file-wide in 2,000-row `BEGIN IMMEDIATE` batches (WAL keeps reads going): series marked removed over 24 hours ago, samples older than the retention, samples older than 7 days folded to one per hour and window instance (the highest `used`, first on ties, so each window's peak stays; progress in the new `meta` table), then, above the ceiling, the oldest samples until 10% below it, reported as `PruneReport::ceiling` and shown as a warning; orphan series are dropped. Schema v2 adds the `samples_by_age` index and `meta`. `export(accounts, metric_key, range, CSV|JSON, out)` writes account, provider, metric, key, unit, kind, window, times and values (empty/`null` when missing, spreadsheet formulas neutralized); `erase_account` (plus this configuration's orphaned series, `secure_delete`) and `erase_all` (the whole file, then `VACUUM`). The desktop exports through the native save dialog to a `0600` file and erases after an in-dialog confirmation (account or everything; the scope is fixed when the confirmation opens). Tests: pruning by age and of removed accounts, the size ceiling (oldest first, reported, then quiet), reduction keeping both peaks of an hour with a reset and idempotent, export with missing values and a refusing writer, partial and total erasure; browser smoke of Settings → History in pt-BR, en-US and es. |
| RF6 | 0.39.0 | Core: `history::HistoryRange` (`Day`/`Week`/`Month`: 24 h, 7 d, 30 d; `since(now)`), `HistoryStore::account_history(account, range) -> Vec<MetricHistory { key, label, unit, kind, window, limit, points, windows }>` (every recorded metric of the account with samples in the span, in its last reported order, one query, never creates the file; a storage failure is an error) and `Monitor::account_history(index, range)` (also noted in `HistoryStatus`); `MetricHistory::trend()` (lowest, highest and last `used`) and `levels(start, end, cells, levels)` (per equal time bucket the peak `used`, scaled from zero to the limit, or from the lowest to the highest value without a limit; `None` for an empty bucket, never zero); `pace::Point`, `WindowUsage` and `MetricHistory` serialize. TUI: the details add `HISTÓRICO [1 24 h] 2 7 d 3 30 d` (keys `1`/`2`/`3`, footer hint `1/2/3 histórico`) and, under each metric, `24 h  ▁▂▃…  mín · máx · último <unit>` with blanks for gaps (the span label leads so a leading gap is never trimmed), `histórico 24 h: sem medições registradas` for a metric without samples, the text summary alone when fewer than 8 cells fit (narrow terminals), and a storage failure once on the heading; read from the core only when the account, span or reading changes, or every 30 s while open (so the reading the recorder writes after the refresh shows). CLI: `--history [CONTA]` (conflicts with `--once`; `--json` emits `{from, to, days, accounts: [{account, provider, metrics}]}`) prints, per account, each metric's sample count, summary, a 60-cell sparkline and its window instances (start → end, initial → final, peak and time, reset), without querying providers or creating the file; `--history-days` gives the days printed (default 7, same 7–365 bounds). Exit 0 when every listed account has data, 2 when one (or the configuration) has none (it prints "Sem medições registradas nos últimos N dias."), 1 for local errors (unknown account, invalid days, file, storage). Tests: `TestBackend` renderings (exact 24 h and 7 d sparklines with gaps, span highlight, no-sample line, narrow text fallback, storage failure, another account's history ignored), core tables (ranges, trend, levels: bucket peak, gaps, no limit, over the limit, flat, out of span) and `account_history` on a temporary store, and the CLI text/JSON/exit codes over a temporary store; smoke with a seeded sample database (10 days every 20 minutes with a one-day gap) through `--history`, `--json`, an unknown account, invalid days and the TUI details in tmux at 130 and 45 columns. |
| RF5 | 0.40.0 | Core: `MetricHistory::chart(start, end, cells) -> Chart { floor, ceiling, heights, last: ChartPoint { taken_at, used, x, y }, resets: [ChartReset { at, x }] }`, already placed (x and heights from 0 to 1, the axis of `levels`): each bucket's peak, an empty bucket between samples at most an hour apart carries the earlier value (compaction skips unchanged readings for 15 minutes) and a longer silence stays a gap, the end point is the last measured sample and a reset marker sits where each window instance after the first began; `levels` keeps per-cell gaps. `HistoryRecorder::set_recording`/`Monitor::set_history_recording` (on by default; off ignores new readings, never counted as dropped, and keeps the data, still pruned by the retention) and `HistoryStore::size_on_disk()` (file, WAL and shared memory; 0 without a file, never created; an unreadable path is `HistoryError::Storage`). Desktop: the Tauri command `account_history(index, generation, range, metric_key)` (`day`/`week`/`month`, `metric_key` `None` = every metric) flushes the queued readings (≤ 500 ms), reads `Monitor::account_history` and returns `HistoryView { range, from, to, recording, metrics: [{ key, label, unit, kind, window, limit, samples, trend, chart (96 buckets), windows }] }` with no account name, credential or provider text; another generation before or after the read is `stale` and an unknown index or a missing store a typed error (the model catalog's pattern). The details dialog adds a History section (24 h / 7 days / 30 days, `aria-pressed`) with, per metric, the label, `lowest · highest · last`, an SVG area chart with the highlighted end point and dashed reset markers (legend "reset"), hidden from assistive technology, plus a visually hidden table of the window instances (start, last reading, initial, final, peak and its time) captioned with the summary and the reset count; it reads again when the reading on screen, the span or the view's `history_epoch` (bumped by erasure and the recording switch) changes and discards answers of an old identity. An account without samples in the span explains that readings are recorded on this computer and the chart fills in; recording off is noted. The there is no entrance animation: chart entrances are forbidden by `docs/design-system.md` DS-MOT-5 (PRD 0022 RF12), so a chart is drawn at its value at once. Settings → History adds "Record readings" (`history_recording` in `desktop.json`, only `false` stored, applied at once with `set_history_recording`) and "On disk: N MiB", refreshed after erasing. Tests: chart placement (peaks, gap, joined hour, end point, span bounds, no-limit axis, empty span) and reset markers from the store, the recording switch shared by clones, size on disk (absent file, WAL counted, shrinks after erasing, unreadable path), the desktop setting default and the Tauri command (storage error without a store, chart and window data, no account identity in the JSON, week span, metric filter, recording flag, invalid index, stale generation before and after a removal); browser smoke of the demo (10 days every 20 minutes with a day missing) in pt-BR, en-US and es: 24 h, 7 d and 30 d charts with gaps and resets, the empty state (an account without history), a metric without measured use, recording off in Settings and in the details, erasing one account and everything (the size drops), reduced motion. |

**Decisions (2026-10-20, 0.40.0):** the recording switch is a per-device preference of the desktop app (`desktop.json`); the TUI and the CLI, which do not read it, keep recording into the shared file. Turning it off keeps the retention pruning, so the data stays bounded by the days chosen until erased. The screen-reader table lists window instances (initial, final and peak use), the information the chart conveys, rather than one row per bucket. The chart joins samples up to an hour apart so compaction and hourly refreshes do not look like gaps; the TUI sparkline keeps per-cell gaps.

**Decisions (2026-10-20, 0.39.0):** the TUI spans use `1`, `2` and `3` in the details (24 h by default, per session); `--history` reuses `--history-days` for the days printed with the retention's 7–365 bounds and a 7-day default (24 h stays a TUI span); an account without data in the span makes `--history` exit 2 (a partial result, like `--once`) while still saying there is no data; sparkline buckets show their peak, as the resolution reduction keeps it.

**Decisions (2026-10-20, 0.38.0):** retention and the size ceiling are per device and file-wide (the history file is shared by every configuration on the computer), so when front ends with different retentions share it the shortest wins; "erase everything" erases the whole file. A series of an account removed from the TOML is pruned 24 hours after the mark, so a brief rename back keeps it. Readings dropped by a full queue are counted and shown, never retried.

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

- History of errors, free-form notes or any provider text: only numeric values and window metadata. *Narrowed by PRD 0026 RF3: an event journal stores event names and failure categories only.*
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
