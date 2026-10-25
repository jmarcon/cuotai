# PRD 0026 — Usage statistics, reset timeline and event journal

| Field | Value |
|---|---|
| ID | 0026 |
| Status | In progress |
| Owner | `cuotai-core` (`stats`, `resets::timeline`, event journal in the history store), desktop and TUI |
| Consumers | `apps/desktop` (Stats view, tray popover timeline), `crates/cuotai-tui` (stats screen, `--stats`, `--timeline`), PRD 0025 (popover) |
| Depends on | PRD 0006 (RF1–RF4: `HistoryStore`, `windows`, retention, erase); PRD 0013 (RF1: `events::diff`) and PRD 0029 (stable account identity) for RF3 |
| Benefits from | PRD 0009 (binding metric), PRD 0023 (RF3: overage amounts for "extra spent"), PRD 0024 (RF5: projected events in the journal), PRD 0028 (RF11: label aliases keep series continuous), PRD 0031 (`/v1/stats` serves RF1), PRD 0022 (UI refresh: KPI cards, charts and timeline visuals) |
| Extends | PRD 0006 RF3 (aggregates over stored windows; journal table in the same store), PRD 0009 RF1 (binding return per account on a timeline), PRD 0013 RF1 (events persisted for display) |
| Date | 2026-10-22; amended 2026-10-23 (owner decision P: `/v1/stats` lives in PRD 0031; journal keyed by PRD 0029 ids; Stats placement per DS-CMP-STATS-1) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **limit hit** is a window instance of a plan limit whose recorded peak reached its limit. The **tightest limit** of an account is its binding metric (PRD 0009).

## Implementation status (2026-10-22)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Core `stats::summarize` (hits, average peak, daily peaks, coverage, retention note) and RF1.5 "extra spent" (`Metric.billing == Overage`, stored per series since schema version 3, summed per unit). |
| RF2 | vNEXT | Core `resets::timeline` (lanes, later, unknown, share level) and `timeline_of_config`. |
| RF3 | vNEXT | `journal` table (schema version 4), `HistoryStore::journal_record`/`journal`, written by the desktop, the interactive TUI and `--watch`/`--serve` through `watch::JournalTap`; retention, erase and the JSON export. The CSV export has no journal section. `quota_exhaustion_projected` is not journaled by the front ends yet (they pass no pace input), and `spent`/`currency` columns exist but no event fills them until PRD 0023/0024 events do. |
| RF4 | vNEXT | Desktop Stats view in the main window and the compact popover drill-in; Tauri `stats` command guarded by the generation. |
| RF5 | vNEXT | Reset timeline in the Stats view and under the popover accounts; a lane opens its account. Following the PRD 0025 menu-bar selection waits for 0025. |
| RF6 | vNEXT | TUI screen (`S`) and `--stats`, `--timeline`, `--events-log`. |
| RF7 | vNEXT | `docs/tui.md`, `docs/events.md`, `docs/gui-features.md` and the README. |

## 1. Context

Cuotai records every ready reading in `history.sqlite3` (PRD 0006: `series` and `samples` tables, 90-day retention configurable from 7 to 365, hourly reduction after 7 days that keeps each window's peak). `HistoryStore::windows` already returns `WindowUsage { started_at, ended_at, resets_at, limit, initial_used, final_used, peak_used, peak_at, samples }` per window instance (`crates/cuotai-core/src/history.rs`), and the desktop details draw one metric at a time over 24 h, 7 d or 30 d. Nothing summarizes across metrics or accounts.

The event engine (PRD 0013) derives edge events (`quota_threshold_crossed`, `quota_exhausted`, `quota_reset`, `refresh_failed`, …) but keeps `EventState` in memory and only hands events to `--watch` and hooks; the hooks `runs.log` stores runs without payloads. There is no "what happened recently" list anywhere.

The binding metric (PRD 0009) knows each account's return time (`resets_at` or `blocked_until`), and PRD 0010 RF4's "Recharging" group lists blocked accounts, but no view answers "when do my limits come back" across accounts on a time axis.

**Market expectation.** A paid macOS monitor (WhatsMyUsage) shows a reset timeline ("limits come back", 24 h / 7 d, the tightest limit per account) and a Stats view: limit hits in 7 days, average peak share of the tightest limit, extra spent, peak usage per day over 7 or 30 days with today highlighted, per-account average and hits, recent events, and a note "history kept N days". `docs/design-system.md` already specifies the visuals (DS-CHT-6 reset timeline, DS-CHT-5 daily bars, DS-CHT-7 sparkline mini-bars, DS-CMP-KPI).

## 2. Problem

- People cannot see whether they hit limits often, which account is under pressure, or whether a plan is oversized.
- The return times of several accounts are scattered across cards.
- Events that mattered (an exhaustion, an authentication failure) vanish once the state changes.
- Any summary risks lying: windows partly outside the range, compacted samples, missing days and unknown limits must not become zeros.

## 3. Goals

- One core aggregate per range (7 or 30 days) from the stored history, with explicit "not enough history" states.
- One core timeline of each account's tightest-limit return within 24 hours or 7 days.
- A persisted, secret-free event journal with the history's retention, feeding "recent events".
- Desktop and TUI presentations of the three, with no new provider query.

## 4. Non-goals

- Cost forecasts or plan recommendations ("downgrade to Pro").
- Synchronizing statistics across devices (PRD 0006 §4).
- Exposing statistics over the local server or `export::v1` in this PRD: **decided on 2026-10-23 (decision P)**, `/v1/stats` is served from `stats::summarize` by PRD 0031, with its security rules (token required, loopback only, no account secrets, rate limit, the export's redaction); this PRD keeps `StatsSummary` serializable for it.
- A guaranteed-delivery event queue for hooks: the journal is for display; PRD 0013 §4 still applies.
- This PRD narrows the PRD 0006 §4 non-goal "History of errors, free-form notes or any provider text": the journal (RF3) stores event names and PRD 0013's failure **categories** only, never messages, notes or provider text.

## 5. Functional requirements

### RF1 — `stats::summarize` in the core (high priority; prerequisite)

- **RF1.1** Pure over stored data: `stats::summarize(store: &HistoryStore, accounts: &[Account], range: StatsRange, now, tz) -> StatsSummary`, with `StatsRange::{Days7, Days30}`. Only plan limits count (PRD 0023's `metrics::is_plan_limit`; `kind == Quota` until PRD 0023 RF1 lands) with a finite positive `limit`.
- **RF1.2** **Limit hits:** window instances (`HistoryStore::windows`) that end inside the range with `peak_used ≥ limit`, counted per account and in total; a window that started before the range but ended inside it counts once.
- **RF1.3** **Average peak of the tightest limit:** for each account and each local day with samples, the highest `peak_used / limit` among its plan limits that day; the account average is the mean over days with data; the total is the mean over accounts. Days without samples are gaps, not zeros.
- **RF1.4** **Peak usage per day:** a series of `(day, Option<peak_share>)` per account and the maximum across accounts, in the caller's time zone, with `is_today` on the current day; 7 or 30 entries.
- **RF1.5** **Extra spent (after PRD 0023):** per currency, the sum over billing cycles of the highest recorded overage amount in the range; provider units are reported separately; unknown stays `None`.
- **RF1.6** **Coverage:** `retention_days` (the configured retention), `first_sample_at` and `coverage` (`Full`, `Partial { days_with_data }`, `NotEnough` when fewer than two days have data); texts `stats.history_kept` ("History kept {days} days on this device") and `stats.not_enough` in the core catalogs.
- **RF1.7** Compaction safety: results are the same before and after PRD 0006's hourly reduction (peaks are kept), proved by a test that compacts a fixture and compares.
- **Acceptance criterion:** table tests with a fixed clock over an in-memory store: no history, one day, a window straddling the range start, a hit exactly at the limit, an unknown limit, a gap day, DST changes in the time zone, compaction parity, several accounts, overage in two currencies.

### RF2 — `resets::timeline` in the core (high priority)

- **RF2.1** Pure `resets::timeline(accounts: &[AccountView], range: TimelineRange, now) -> Timeline` with `TimelineRange::{Hours24, Days7}`.
- **RF2.2** One lane per account for its binding metric (PRD 0009 `metrics::binding`): `returns_at` (`blocked_until` when blocked, else the binding metric's `resets_at`), the level of the binding remaining share (DS-ID-5), `exhausted`, `stale` and the binding label; lanes sorted by `returns_at`, ties by configuration order.
- **RF2.3** Accounts whose return falls after the range go to `later` with the time; accounts with no known return go to `unknown` ("Return unknown", never plotted at an edge, DS-CHT-6); overage metrics never form a lane.
- **Acceptance criterion:** table tests: blocked versus resetting accounts, return after the range, unknown, stale, ties, several accounts of one provider.

### RF3 — Persisted event journal (medium priority)

- **RF3.1** A `journal` table in `history.sqlite3` (a forward migration of PRD 0006's versioned schema): `occurred_at`, the account's stable id from PRD 0029 (which must land first, so renaming an account keeps its journal; the PRD 0006 identity digest is the fallback only for entries migrated before it), `kind` (PRD 0013 event name), optional metric label digest and label, optional `threshold`, `used_ratio`, `resets_at`, `category` (`FailureCategory` name), and for PRD 0023/0024 events `projected_at`, `spent` and `currency`. No notes, messages, URLs, e-mails or tokens.
- **RF3.2** Written by every front end that records history (desktop backend, TUI, `--watch`) from `events::diff` over the cycle it already runs; recording follows the history switch (`Monitor::set_history_recording`). Duplicates are avoided by a unique index on (account digest, kind, metric, `occurred_at` rounded to the minute), so two processes on one profile do not double entries.
- **RF3.3** Retention and erasure follow the history (PRD 0006 RF4): pruned at the same retention, removed with the per-account or total erase, exported with the CSV/JSON export.
- **RF3.4** `HistoryStore::journal(range, accounts, limit) -> Vec<JournalEntry>` (newest first) with a localized summary per entry from `Event::summary`; account names are resolved from the current configuration by id, and entries of removed accounts are shown as "Removed account".
- **RF3.5** `usage_updated` is never journaled (it is a throttle signal, not an event a person needs).
- **Acceptance criterion:** migration test from the current schema; no-leak test over every kind with credentials, e-mails and URLs in the readings; retention and erase tests; a two-writer test proving no duplicates.

### RF4 — Desktop Stats view (medium priority)

- **RF4.1** A Stats view in the main window (DS-CMP-STATS-1: a topbar entry next to the account grid, `Esc` returns; the popover's "Stats" opens a compact drill-in with "Open full stats"), with the DS-CMP-STATS-2 scope picker and a 7 d / 30 d toggle (DS-CMP-SEG): KPI cards (DS-CMP-KPI) "Limit hits", "Average peak" (of the tightest limit) and "Extra spent" (after PRD 0023), each with "—" and a reason when there is not enough data (DS-CMP-KPI-2).
- **RF4.2** "Peak usage per day": a daily bar chart with today highlighted and labelled "Today" (DS-CHT-5), percent axis 0–100, gaps for days without data (DS-CHT-3), `--bad` only for days at the limit.
- **RF4.3** "By account": rows with tile, name, sparkline mini-bars (DS-CHT-7), average and hits.
- **RF4.4** "Recent events": the last 20 journal entries (DS-CMP-LIST) with time and summary.
- **RF4.5** Footer note "History kept {days} days on this device" with a link to the history settings; every chart has its text alternative and table (DS-CHT-10).
- **RF4.6** A Tauri command `stats(range)` returning the core summary for the current generation, refused as `stale` for another (as `account_history`).
- **Acceptance criterion:** smoke in the four languages and reference themes with seeded history: empty, one day, a week with hits; screen-reader tables present; no zero bars for missing days.

### RF5 — Desktop reset timeline (medium priority)

- **RF5.1** "Limits come back" in the tray popover and the main window: the RF2 timeline drawn per DS-CHT-6 (lanes, "Now" line, dot and time at the return, 24 h / 7 d toggle), `later` and `unknown` listed as text below.
- **RF5.2** Selecting a lane focuses the account's card; the timeline follows the PRD 0025 menu-bar selection in the popover and all accounts in the main window.
- **Acceptance criterion:** smoke with blocked, resetting, later and unknown accounts; reduced motion respected; accessible name per lane ("Claude Work back at 19:30, in 1h 35m").

### RF6 — TUI and CLI (medium priority)

- **RF6.1** TUI: a stats screen (key `S` from the dashboard) with the RF1 figures as text and a block bar per day, and the timeline as a sorted list ("19:30 in 1h 35m · Claude Work · 0% left").
- **RF6.2** `cuotai --stats [--days 7|30] [--json]` and `cuotai --timeline [--range 24h|7d] [--json]` read the store and the last recorded readings without queries; `cuotai --events-log [--days N] [--json]` prints the journal. Exit 0, 1 for local errors (invalid arguments included), 2 when the store cannot be read.
- **Acceptance criterion:** `TestBackend` rendering tests, text snapshots and binary tests for every flag combination.

### RF7 — Documentation (low priority)

- **RF7.1** `docs/tui.md`, `README.md` and `docs/events.md` (the journal, what it stores and how to erase it).
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

## 6. Non-functional requirements

- **Honesty:** missing days are gaps; unknown limits are excluded and counted in a caption; "Not enough history" instead of zeros; averages state their base ("over 5 days with data").
- **Privacy:** the journal follows PRD 0006's digest model and never stores provider text; erasing history erases it.
- **Performance:** `summarize` over 30 days and 20 accounts completes in under 100 ms on the reference machine; it runs off the UI thread; results are cached per generation.
- **Cost:** no provider query.
- **i18n and accessibility:** four languages; charts with tables (DS-CHT-10); day labels in the locale.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `history.sqlite3` | `journal` table, schema version +1 | Forward migration; older versions refuse a newer schema as today |
| History export (CSV/JSON) | journal rows in a separate file/section | Additive |
| CLI | `--stats`, `--timeline`, `--events-log` | New flags |
| `--json`, `export::v1`, TOML, `desktop.json` | none (range toggles remembered in `desktop.json` `stats.range` only) | Additive |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | aggregates, timeline, journal storage and queries, texts |
| Desktop | Stats view, timeline in popover and window, commands |
| TUI/CLI | stats screen, timeline list, three read-only flags |

## 9. Dependencies and sequencing

RF1 and RF2 depend only on delivered PRDs (0006, 0009). RF3 needs PRD 0013 RF1 (delivered) and PRD 0029 (stable ids; critical path in `0000_control.md`). "Extra spent" waits for PRD 0023 RF3; projected events in the journal wait for PRD 0024 RF5. Desktop visuals follow PRD 0022 and the cited `docs/design-system.md` rules; the TUI can ship first.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Misleading averages with sparse history | coverage state and explicit base in captions |
| Journal grows with noisy failures | edge events only, no `usage_updated`, history retention and the 64 MiB ceiling |
| Two writers on one profile | unique index and WAL (PRD 0006) |
| Time-zone and DST errors in daily buckets | caller time zone injected; DST table tests |

## 11. Order of execution and verification

1. **RF1 and RF2:** aggregates and timeline with table tests.
2. **RF6 (CLI part):** read-only flags to validate the numbers early.
3. **RF3:** the journal.
4. **RF4 and RF5:** desktop views; TUI screen.
5. **RF7** documentation.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface. The requirement is only delivered when the figures match a hand-computed fixture, missing data never reads as zero, and erasing history removes the statistics and the journal.

## 12. Open questions

| Question | Recommendation |
|---|---|
| ~~Serve statistics over the local server (`/v1/stats`)~~ | **Decided on 2026-10-23 (decision P): yes, in PRD 0031**, token required, loopback only, redacted, rate-limited. |
| Count a hit per window or per metric when two metrics of one account hit at once | Per metric window; "by account" shows the sum. |
| Journal on by default | Yes, it follows the history switch, which is on by default. |
