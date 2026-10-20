# PRD 0010 — Measurement age and stale reading

| Field | Value |
|---|---|
| ID | 0010 |
| Status | In progress |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (cards, details, tray), `crates/cuotai-tui`, PRDs 0012 and 0014 |
| Depends on | PRD 0006 (RF3: `latest`) to survive a restart; the rest works without it |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.21.0 | `cuotai_core::freshness`: `age(checked_at, now)`, `assess(checked_at, interval, now) -> Freshness` (`Fresh` ≤ 1.5× the effective interval, `Aging` ≤ 3×, `Stale` above, compared in whole durations) and `clock_adjusted(checked_at, now)` for a clock that went backwards (age reads zero, never an error). Pure, injected clock; table tests at 30 s, 300 s and 3600 s. Core only: front ends consume it from RF3 and RF5. |
| RF2 | 0.25.0 | `RefreshCycle` keeps `last_good` per account beside the current report; `last_good_report(index)` returns it only while the current report is missing or not `Ready`. `Ready` replaces it, `AuthenticationError` discards it, `RateLimited`/`Error`/others keep it; `Edit::Upsert` discards the touched account's reading, `Remove`/`Move` carry the rest; `keep_last_good_from` carries it across the desktop's cycle rebuild for a global interval change. `slots`, `snapshot()` and `--json` are unchanged, so alerts and history never see it. RF2.3 holds per session only: surviving a restart waits for PRD 0006 RF2/RF3 (`latest`). Front ends display it from RF3 and RF5. |
| RF3 | 0.28.0 | The desktop `ReportView` carries the core `freshness` (`freshness::assess` against `Account::effective_refresh_seconds`), `clock_adjusted` and `last_good` (`{checked_at, metrics}` from `RefreshCycle::last_good_report`, present only while the current report failed); the UI re-reads the view every 30 s, so age and freshness follow the clock with no new query. Cards, tray tiles and list rows, the details (dialog and popover) and each native tray menu line show "3 min ago" with a Fresh/Aging/Stale icon and text (the absolute time in the tooltip); the tray icon tooltip names the oldest reading. A failure with a preserved reading draws the old metrics dimmed and still (no bar transition, count-up or level flash) under the localized cause, with a "Stale · 42 min ago" badge; the binding, alerts and ordering keep reading the current report only. The next `Ready` removes it; `AuthenticationError` drops it (RF2.4). Browser smoke in pt-BR, en-US and es with healthy, aging, failing-with-reading and failing-without-reading accounts. |
| RF5 | 0.29.0 | The TUI `dashboard::View` takes the injected `now` and `RefreshCycle::last_good_report` per account; `ui::age_text` renders the core `freshness::age` as `agora`/`há N s`/`há N min`/`há N h`/`há N d` (a clock adjusted backwards reads `agora`). Each card's status line ends with the age; a failure with a preserved reading shows the old metrics still (no bar transition) with `(desatualizada) há 42 min` in place of the class, using the reading's own `checked_at`. The details add the age and the time in UTC and the local time zone to `consultado`, and a `LEITURA (desatualizada) última válida …` line over the preserved metrics while the current report's notes keep the failure cause. `--once` account headings end with `consultado há N min`; `--json`, `slots` and `snapshot()` are unchanged. Rendering tests with a fixed clock. |
| RF4 | — | Not delivered. |

## 1. Context

Each `Report` has `checked_at` (`crates/cuotai-core/src/model.rs`). Today:
- the TUI shows `checked_at` only as an absolute time in UTC in the details (`dashboard.rs`);
- the desktop app shows the measurement age nowhere;
- `RefreshCycle::finish_account` replaces the account's report with the new one **even when it is a failure** (`slots[index] = Some(report)`). A query with a network, authentication or rate-limit error erases the previous good metrics from the screen and, on the desktop, the card goes empty exactly when the user most needs the last known value.

In the 2026-10-19 market analysis: Quotum shows how long ago each account was measured and groups accounts with an exhausted weekly quota in a "Recharging" block with the return time; CodexBar keeps the last valid reading with its original age when a query fails and never borrows data from another account; QuotaTempo states that refreshing does not bypass the provider's waits.

## 2. Problem

- You cannot tell how old the displayed number is: a card can show a value from 40 minutes ago with no indication.
- A transient failure erases the last useful value, and the user cannot tell "the account is at zero" from "the query failed".
- Exhausted accounts mix with the others, although all that matters about them is when they come back.

## 3. Goals

- Show the measurement age per account, in every interface, computed by the core.
- Preserve the last valid reading when a query fails, marked as stale and with its original age, without ever promoting it to "fresh".
- Group exhausted accounts with the return time.

## 4. Non-goals

- Querying faster because the data is old: the scheduling and the 300 s floor of AWS Bedrock stay.
- Inventing values or borrowing the reading of another account or product.
- Changing the format of `Report` or the current `--json` (the stale reading enters through the versioned contract of PRD 0012).

## 5. Functional requirements

### RF1 — Age and freshness in the core (high priority; prerequisite)

- **RF1.1** Create `freshness::assess(checked_at, interval, now) -> Freshness` (`Fresh`, `Aging`, `Stale`) and `age(checked_at, now) -> Duration`. Thresholds derived from the account's effective interval (`Account::effective_refresh_seconds`, already with the floor for providers billed per query): `Fresh` up to 1.5 times the interval, `Aging` up to 3 times, `Stale` above that. Injected clock, no network.
- **RF1.2** A negative age (the system clock went backwards) is treated as zero and marked "clock adjusted", never as an error.
- **Acceptance criterion:** table tests with intervals of 30 s, 300 s and 3600 s and a fixed clock.

### RF2 — Last valid reading (high priority)

- **RF2.1** `RefreshCycle` starts keeping, per account, the last `Ready` reading (`last_good: Option<Report>`) **alongside** the current report, without changing what `snapshot()` and `account_report()` already return. A new API (`last_good_report(index)`) exposes that reading. Removal, identity edits and reordering follow the existing `Edit` rules (the reading follows the moved account and is discarded when the identity changes).
- **RF2.2** The preserved reading is only offered when the current report has a `Status` other than `Ready`. It never replaces the current report, never updates `checked_at` and is never used to decide alerts (PRD 0008) or history (PRD 0006).
- **RF2.3** With PRD 0006's history, the last valid reading survives an app restart and is loaded on opening (`latest`), shown as stale until the first query finishes. Without history, it holds only during the session.
- **RF2.4** An authentication failure invalidates the account's preserved reading (the data belongs to a credential that is no longer valid); `RateLimited` and network errors keep it.
- **Acceptance criterion:** cycle tests: a failure after a success keeps the reading with the original age; the next success replaces it; a credential edit discards it; moving the account preserves it; an authentication failure discards it; the current report and `snapshot()` stay unchanged.

### RF3 — Display on the desktop (high priority)

- **RF3.1** Each card shows "3 min ago" (relative text updated every minute by the front end, computed from the `checked_at` delivered by the backend) and a freshness indicator (`Fresh`, `Aging`, `Stale`) with an icon and text, not color alone.
- **RF3.2** When there is a `last_good` and the current report failed, the card shows the old metrics with a "Stale · 42 min ago" badge and the cause of the failure (the existing localized message). Bars are dimmed and not animated.
- **RF3.3** Tray and popover repeat the age and the badge. The icon tooltip indicates the age of the oldest account.
- **Acceptance criterion:** a smoke test in three languages with a healthy account, an aging one, one failing with a preserved reading and one failing without a reading; the badge disappears on the next successful query.

### RF4 — "Recharging" group (medium priority)

- **RF4.1** Accounts with `AccountClass::Exhausted` (or blocked according to PRD 0009, when it exists) form a collapsible "Recharging" group with the return time (`blocked_until`). Accounts with no known `resets_at` stay in the group with "return unknown".
- **RF4.2** Grouping is an option in **Settings → Appearance** (off by default), preserves the manual order inside each group and never changes the order saved in `cuotai.toml`.
- **Acceptance criterion:** a smoke test with two exhausted accounts and a free one; the group collapses and expands by keyboard and mouse; the manual order persists.

### RF5 — TUI and CLI (medium priority)

- **RF5.1** The dashboard and details show the relative age; accounts failing with a preserved reading are marked "(stale)". The absolute time remains available in the details, now also in the local time zone.
- **RF5.2** `--once` prints the age; `--json` stays the same.
- **Acceptance criterion:** rendering tests with a fixed clock.

## 6. Non-functional requirements

- **Honesty:** old data is always labelled as old. A preserved reading never becomes `Ready`, never recomputes `checked_at` and never appears in the current JSON as if it were new.
- **Privacy:** the reading preserved on disk (RF2.3) follows the rules of PRD 0006.
- **Cost:** no new query nor change of schedule.

## 7. Order of execution and verification

1. **RF1:** pure freshness in the core, with tests.
2. **RF2:** `last_good` in the cycle, with edit, removal and reordering tests.
3. **RF3:** desktop with a smoke test.
4. **RF5 and RF4:** TUI, CLI and grouping.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a query with an error after a success shows the previous metrics with their original age and the cause of the failure, a successful query removes the badge, an authentication failure discards the old value, and no current report, snapshot or `--json` output changes shape.
