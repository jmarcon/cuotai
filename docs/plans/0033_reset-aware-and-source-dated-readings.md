# PRD 0033 — Reset-aware and source-dated readings

| Field | Value |
|---|---|
| ID | 0033 |
| Status | In progress |
| Owner | `cuotai-core` (`RefreshCycle`, `resets`, `freshness`, `Report`, metered cadence), desktop and TUI |
| Consumers | `apps/desktop` (cards, details, tray), `crates/cuotai-tui` (cards, `--once`), `--watch`/`--serve`, `export::v1` consumers |
| Depends on | PRD 0001 (`RefreshCycle` deadlines), PRD 0003 (RF1: reset planner), PRD 0006 (RF2: restored readings), PRD 0007 (pace line), PRD 0009 (binding chip), PRD 0010 (RF1, RF2: freshness, last valid reading) |
| Benefits from | PRD 0029 (RF1: account `id` for the metered lock file), PRD 0031 (RF13: same pace in the export) |
| Extends | PRD 0003 §4 (supersedes the non-goal "querying the provider at the reset moment" for non-metered providers), PRD 0010 RF1/RF2 (`as_of`, expired metrics in preserved readings), PRD 0009 RF2.2 and PRD 0007 RF3.2 (labelled chip and pace line), PRD 0001 (metered cadence) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **preserved reading** is PRD 0010's last valid reading shown while the current query fails or has not run; a **restored reading** is one loaded from the history at start-up (PRD 0006). A **metered** provider charges per query (`Provider::is_metered`, today only AWS Bedrock through Cost Explorer). Code references are to `origin/main` at `0ba9c9b`.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | — | Proposed. |
| RF2 | — | Proposed. |
| RF3 | — | Proposed. |
| RF4 | — | Proposed. |
| RF5 | 0.80.0 | RF5.1–RF5.2 delivered: metered accounts use a 3600 s default; compatible 300–899 s overrides run at 900 s and add `note.metered.interval_raised` to the account report; smaller overrides retain the field- and account-specific rejection. RF5.3–RF5.4 remain proposed. |
| FIX-10 | 0.80.0 | History rejects a reading more than five minutes ahead of the UTC wall clock before changing the series, retains legitimate older readings without a lower age bound, and excludes pre-existing out-of-bound future rows from `latest_in`. |
| FIX-09 | 0.80.0 | Provider-status readings expire after 30 minutes (six five-minute cache windows) against the UTC wall clock: the retained cache is not erased, but its presented state becomes `unknown`, its incident is absent, and its original check time remains visible. Desktop cards and tray content and TUI/`--once` name the last-check age; the desktop outage dot ignores every stale reading. `export::v1.incident.expired` makes the transition additive and explicit. |
| RF6 | — | Proposed. |

## 1. Context

Cuotai shows what the last query said, for as long as nothing replaces it. Around resets that is wrong in visible ways:

- **The card stays "Blocked until 14:30" after 14:30** (RST-01). PRD 0003 §4 ruled out querying at the reset moment, so the new window only appears at the next scheduled refresh, up to the account's interval later. `RefreshCycle` has no way to bring one account's deadline forward (`crates/cuotai-core/src/refresh.rs:250-310` lists `due_indices`, `next_refresh_in`; only `set_deadline_for_test` moves a deadline, `:960`).
- **Restored and preserved readings keep expired metrics** (RES-01): `restore_last_good` accepts any `Ready` reading from the history, with no age cap (`refresh.rs:213-222`), and `last_good_report` serves it while the current report is missing or failed (`:177-182`). A weekly metric "100 %, resets Monday 14:30" restored on Tuesday still reads exhausted.
- **Freshness measures the query, not the data** (0010 open item): `freshness::assess` uses `checked_at` (`freshness.rs:25-37`); billing APIs (OpenAI costs, AWS Cost Explorer) lag hours behind, but a fresh query reads "Fresh". `Report` has no data timestamp (`model.rs:503-524`).
- **Chip and pace line can name different metrics** (0009 open item): the card's chip uses the binding metric (PRD 0009 RF2.2), while the pace line uses "the most restrictive metric that has a pace" (`apps/desktop/ui/js/main.js:143-146`, `lib.js:1457-1460`); an exhausted weekly window (no pace) beside a 5-hour pace line reads as if the pace were the week's.
- **Metered polling is expensive and duplicated** (0001 open item): the metered floor is 300 s (`model.rs:145-149`) and the global default is also 300 s (`model.rs:364-366`), about US$ 86 a month per Bedrock account at Cost Explorer prices; the desktop, the TUI and `--serve`/`--watch` each query on their own schedule.

## 2. Problem

- After a reset, Cuotai keeps saying the account is blocked or exhausted.
- After a restart or a failure, old exhausted windows look current.
- "Fresh" is claimed for data that is hours old at the source.
- The card's pace line can be mistaken for the binding window's pace.
- Bedrock monitoring costs more than it should and multiplies with every open front end.

## 3. Goals

- One extra query 30 seconds after a reset, never for metered providers.
- Never show an expired window as current: collapse it to "renewed; waiting for a reading".
- Carry the source's data time where the provider gives it, and say it.
- Label the chip and the pace line with their own metric.
- A metered default of one query per hour, a 15-minute floor and one query per interval across processes.

## 4. Non-goals

- Inventing a reset time or a data time that the provider does not report (PRD 0003 §4 and PRD 0010 §4 still apply).
- Querying at resets for metered providers, or more often than once per reset instance.
- Predicting the post-reset value; the collapsed metric shows no number until a reading arrives.
- Sharing readings between processes other than the metered last-query record (RF5.3).

## 5. Functional requirements

### RF1 — One query after a reset (high priority; RST-01, decision I)

- **RF1.1** `RefreshCycle::bring_forward(index, at)` lowers an account's deadline to `at` (never raises it, no-op while the account is busy or when `at` is within 60 s of the current deadline).
- **RF1.2** The core computes, per account, the earliest future `resets_at` of its plan-limit metrics (`resets::upcoming`, PRD 0023 `is_plan_limit` when present) and calls `bring_forward(index, resets_at + 30 s)`; the front ends' refresh loops already wake on deadlines. At most **one** reset-driven query per account per window instance (`DRIFT_TOLERANCE` identity) and per 5 minutes; resets of several metrics within 2 minutes share one query.
- **RF1.3** Never for `Provider::is_metered` accounts, accounts with an authentication failure, or disabled accounts. Independent of whether reset notices are on.
- **RF1.4** Applies to the desktop, the TUI, `--watch` and `--serve`, since all use `RefreshCycle`.
- **Acceptance criterion:** cycle tests with a fake clock: a block until 14:30 triggers one query at 14:30:30 and the card no longer shows "Blocked until 14:30" after it; two metrics resetting at 14:30 and 14:31 cause one query; a Bedrock account never; a reset whose next scheduled refresh is at 14:30:45 causes no extra query.

### RF2 — Expired metrics in preserved and restored readings (high priority; RES-01)

- **RF2.1** When a preserved or restored reading is shown, every metric whose `resets_at` is before `now` is **collapsed**: no value, no exhaustion, excluded from the binding metric and from `blocked_until`, with the text `metric.renewed_waiting` ("Renewed at 14:30; waiting for a reading").
- **RF2.2** `restore_last_good` ignores restored readings older than **24 hours** (`RESTORE_MAX_AGE`); a preserved reading in the running process keeps PRD 0010's rules but with RF2.1 applied.
- **RF2.3** `export::v1` `last_good.metrics[*]` gains `expired: bool` (additive); the TUI and `--once` show the collapsed text.
- **Acceptance criterion:** tests: a restored weekly exhausted metric after its reset is collapsed and the account is not "Recharging"; a 30-hour-old restored reading is not used; the export flags the metric; snapshot tests of the card in the four languages.

### RF3 — `as_of`: when the data is from (medium priority; 0010 open item)

- **RF3.1** `Report::as_of: Option<DateTime<Utc>>` (additive, skipped when `None`), set **only** by adapters whose provider states the data's time (for example the end of the last complete cost bucket, or a usage "as of" field); every other adapter leaves it `None`. Each adapter that sets it documents the field it reads.
- **RF3.2** `freshness::assess` is unchanged (it measures the query cadence); `freshness::data_age(report, now)` returns the age of `as_of` when present. Cards and details show "Checked 2 min ago · data as of 06:00" (`freshness.data_as_of`) when `as_of` is more than 15 minutes before `checked_at`.
- **RF3.3** `export::v1` `report.as_of` and `data_age_seconds` (additive); `docs/providers.md` lists which adapters set it.
- **Acceptance criterion:** adapter fixture tests for each adapter that sets it; a test that adapters without a source time never set it; rendering tests.

### RF4 — Chip and pace line name their metric (medium priority; 0009 open item)

- **RF4.1** The card's pace line always starts with its metric's window label ("Session (5h): 8 pts under pace"), from a core `Pace::labelled(label)` text, and the binding chip keeps its label ("Week · 100%"); when both refer to the same metric the pace line omits the repeated label.
- **RF4.2** The same rule in the TUI cards and `--once` (`crates/cuotai-tui/src/dashboard.rs:209-260`).
- **Acceptance criterion:** desktop and TUI rendering tests: binding week exhausted with a 5-hour pace line shows both labels; same metric shows one.

### RF5 — Metered reading cadence (high priority; 0001 open item, owner-accepted)

- **RF5.1** Metered accounts without `refresh_seconds` use **3600 s** (`Provider::METERED_DEFAULT_REFRESH_SECONDS`) instead of the global interval.
- **RF5.2** The metered floor becomes **900 s** (`METERED_MIN_REFRESH_SECONDS`). An account override between 300 and 899 s is **raised to 900 s with a note** (`note.metered.interval_raised`) rather than rejected, so existing configurations keep loading; values below 300 stay rejected as today. The FIX-08 rule (the global interval no longer has to meet the metered floor when the metered account has its own override; the error names the field) is kept.
- **RF5.3** **Cross-process last-query record:** before a metered query, the process takes an advisory lock on `<OS local data dir>/cuotai/metered/<account id>.json` (`0600`) and reads the last query time and the last redacted `Ready` report; if another process queried within the effective interval, it uses that report (marked with its `checked_at`) instead of querying; otherwise it queries and writes the record. Never a credential, URL or response body in the file.
- **RF5.4** The wizard and the desktop form show the cadence and the cost note ("about one Cost Explorer request per hour").
- **Acceptance criterion:** config tests (default 3600, override 600 raised to 900 with the note, 200 rejected); two `Monitor`s sharing a temporary data directory make one Cost Explorer call per interval (wiremock counter); the record file contains no secrets.

### RF6 — Documentation (low priority)

- **RF6.1** `README.md` (refresh after reset, Bedrock cadence and cost), `docs/providers.md` (`as_of` column), `docs/export-v1.md`, `cuotai.example.toml` (Bedrock comment), PRD 0003 §4 note that RF1 supersedes the non-goal.
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

## 6. Non-functional requirements

- **Cost:** RF1 adds at most one query per account per window instance and never for metered providers; RF5 lowers metered queries to one per hour across all processes by default.
- **Honesty:** a collapsed metric shows no number; `as_of` appears only when the provider states it; "Fresh" keeps meaning "queried on schedule".
- **Regression NFRs (review fixes, delivered with the wave-5a fix commits when they land first, otherwise in this PRD's first delivery):**
  - **FIX-09:** the tray's outage dot ignores stale provider-status readings (today `apps/desktop/src-tauri/src/backend.rs:494` uses `is_outage()` without `stale`), and a status reading older than about 30 minutes expires to `unknown` (today the last reading is kept indefinitely, `incidents/status.rs:255-281`).
  - **FIX-10:** the history rejects samples more than 5 minutes in the future, and `latest_in` never returns them (today it orders by `MAX(taken_at)` with no bound, `history.rs:613-638`).
- **Privacy and security:** the metered record holds only a redacted report and a time; `0600`; locks never held across a network call.
- **i18n and accessibility:** new texts in pt-BR, en-US, es and it (plus PRD 0035 languages); the collapsed state is conveyed in text, not only by dimming.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `Report` (`--json`) | `as_of` | Additive, skipped when `None` |
| `export::v1` | `report.as_of`, `data_age_seconds`, `last_good.metrics[*].expired` | Additive (schema 1) |
| TOML | metered default 3600 s; overrides 300–899 raised to 900 with a note | No file rejected; behaviour change documented |
| `metered/<id>.json` | new file | New |
| `desktop.json`, events | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | `bring_forward`, reset-driven scheduling, collapse rule, restore cap, `as_of`, `data_age`, labelled pace text, metered cadence and record |
| Desktop | collapsed metric rendering, "data as of" line, labelled pace line |
| TUI/CLI | the same in cards and `--once` |

## 9. Dependencies and sequencing

RF2 and RF5 first (correctness and cost). RF1 next. RF3 per adapter, as evidence is collected. RF4 with PRD 0022's card work. RF5.3's file name uses PRD 0029's `id` when available, the legacy key digest before. Wave 5e of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Reset-time queries hit rate limits | one per instance, 5-minute spacing, coalescing, no metered providers |
| A provider updates its counters some seconds after the reset | the 30-second delay; if the reading still shows the old window, the normal cycle corrects it |
| Wrong `as_of` claims | set only from a documented provider field, tested per adapter |
| Raised Bedrock interval surprises users | note on the account and changelog entry |

## 11. Order of execution and verification

1. **RF2**, **RF5** and the FIX-09/FIX-10 regressions.
2. **RF1**.
3. **RF4**, then **RF3** per adapter; **RF6** with each step.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke with a fake provider whose block ends during the session. The requirement is only delivered when "Blocked until 14:30" disappears at 14:30:30 without a second query, and two front ends together make one Cost Explorer request per hour.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Reset-driven query also when the account is not blocked or near its limit | Yes, for any plan-limit reset; the cost is one query per window. |
| Share non-metered readings across processes too | No; only metered queries cost money. |

## 13. Decisions (owner, 2026-10-23)

| Topic | Decision |
|---|---|
| Restored-reading age cap | **24 hours** (`RESTORE_MAX_AGE`, RF2.2). |
| Bedrock overrides of 300–899 s | **Raised to 900 s with a note**, never rejected (RF5.2); no configuration stops loading. |
