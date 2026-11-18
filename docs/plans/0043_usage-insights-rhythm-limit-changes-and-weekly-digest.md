# PRD 0043 — Usage insights: hourly rhythm, limit changes and the weekly digest

| Field | Value |
|---|---|
| ID | 0043 |
| Status | Awaiting owner verification (2026-11-18 re-validation fixes in vNEXT) |
| Owner | `cuotai-core` (`stats::rhythm`, `stats::limit_changes`, `digest::weekly`, events), desktop (Stats view additions, menu-bar strip, digest notification), `cuotai-tui` (`--stats --rhythm`, `--digest`) |
| Consumers | Every front end; `/v1/stats` (additive); hooks (`limit_changed`, `weekly_digest`) |
| Depends on | PRD 0006 (history), PRD 0026 (stats, journal, Stats view), PRD 0029 (account id) |
| Benefits from | PRD 0025 (menu-bar styles: the strip is one more style), PRD 0032 (digest delivery and quiet hours), PRD 0031 (`/v1/stats`), PRD 0022 (Stats layout), PRD 0044 (digest to a channel) |
| Extends | PRD 0026 RF1/RF4 (new sections), PRD 0013 (two events) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. **Rhythm** is how use spreads over the hours of a day and the days of a week; a **limit change** is a change in a metric's `limit` between two readings of the same window (a plan change, a promotion, a cap edit); the **digest** is a once-a-week summary the person opted in to.

## Implementation status (2026-11-07)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.83.0 | `insights::rhythm` (re-exported as `stats::rhythm`): 24 hourly buckets, 7 daily buckets with days off, typical day over 30 days (3-day minimum, sample counts), gaps as `None`, resets marked, local zone (DST tested). `notify.days_off` in the shared preferences. |
| RF2 | 0.83.0 | `insights::limit_changes` (0.5% / one-unit threshold, collapsed renewals skipped, budgets excluded) as `AlertKind::LimitChanged` (opt-in `notify.limit_changes`) and the `limit_changed` event (journal row). The 48-hour card note reads the changes back from the history (`HistoryStore::limit_changes`), so no new storage; the journal row's summary has no values. |
| RF3 | 0.83.0 | `digest::weekly` (hits, average peak, days at the limit, extra spent, limit changes, heaviest and lightest day, busiest hours, budgets, allowance), short (≤ 8 lines, no account names) and Markdown texts in every catalog, `digest::due_slot` with the 48-hour catch-up and `Ledger::claim_weekly_digest` (once across processes), `cuotai --digest [--week] [--json|--markdown]` (exit 0; 2 when nothing was recorded). The desktop delivers it; `cuotai --notify` does not. |
| RF4 | 0.83.0, 0.84.0 | Desktop Stats **Rhythm** section with tables, **Insights** settings (limit changes, days off, weekly digest, test digest), limit-change notes on the details, digest notification (0.83.0). 0.84.0: RF4.2, the **24-hour strip** menu-bar style (`tray::TrayStyle::HourlyStrip`, macOS only: a monochrome template image of 24 hourly bars of the tightest chosen account from `tray::strip_account`/`strip_bars` over `insights::rhythm`, recomputed at most once a minute, gaps faint and resets dotted, the level color in the colored variant, a Settings card and preview offered on macOS only) and the popover strip under the account rows on every system (the Stats **Last 24 hours** chart with its table, in a disclosure whose state is kept per device), both through the `hourly_strip` command; RF4.3, the digest's open-Stats action: the notification plugin offers no click action on the desktop, so a digest posted within the last 30 minutes opens the Stats view once when the main window gains focus (a click on the notification activates the app). Tests: the core summary and bars, the image (scale, gaps, resets, colors, macOS only), the 30-minute rule and the Settings preview. DS-CHT-12 in `docs/design-system.md`. |
| RF5 | 0.83.0 | `--stats --rhythm [ACCOUNT] [--range 24h\|7d\|typical] [--json]`, TUI Stats `r` key, `--digest`, `rhythm`/`limit_changes` in `--stats --json` and `/v1/stats` (additive). |
| RF6 | 0.83.0, 0.84.0 | `docs/gui-features.md`, `docs/tui.md`, `docs/advanced.md`, `docs/events.md`, `docs/local-server.md`, `docs/export-v1.schema.json`; 0.84.0: the strip style and the popover strip (`docs/gui-features.md`, `docs/platforms.md`) and the digest's action. |
| Deviation | 0.84.0 | RF4.3 asks for "an action that opens the Stats view" on the notification. `tauri-plugin-notification` 2.7 has no click callback or action buttons on the desktop, so the action is the focus rule above (decision of 2026-11-07, the most conservative way without a new native dependency). |
| Fix | 0.86.5 | The weekly digest notification keeps its short lines as separate lines: the desktop delivers them through `text::notice_lines` (each line cleaned on its own, whole lines within 400 characters, "and N more" kept), as the quiet-hours digest does (PRD 0032 re-validation C-01). |
| 2026-11-18 re-validation | vNEXT | Fixes of RF1.1, RF1.2, RF2.2, RF3.1, RF3.2, RF4.2, RF4.3 and RF5 (3 high, 1 medium, 10 low findings); see "2026-11-18 re-validation" below. |
| Owner verification | — | The strip style on the macOS menu bar in light and dark (and colored), the popover strip on Windows and Linux, a weekly digest clicked on each system opening Stats, and the §11 smoke after a week of readings (§2.1 of `0000_control.md`). |

### 2026-11-18 re-validation (vNEXT)

The re-validation (Visual, Security, Review) found 3 high, 1 medium and 10 low findings; every one is fixed in vNEXT, one commit per group:

| Group | Findings | RF | Delivery (vNEXT) |
|---|---|---|---|
| G1 | V-01 (high) | RF4.2 | The menu-bar popover's 24-hour strip takes the Stats view's chart tokens, so the bars of the hours with use are drawn (only the gaps showed before); the popover drops the repeated caption and sits on the row gutter. The UI check flags a drawn rhythm column with no paint. |
| G2 | R-01 (high) | RF3.1, RF3.2 | `stats::summarize_between` takes explicit local dates; `digest::weekly` uses it for both weeks, so `--digest --week current` counts only this week's limit hits, extra spending and peaks (it included the days before Monday). |
| G3 | S-01 (high, privacy) | RF3.2 (PRD 0044 RF6.1) | A channel whose filter names accounts receives the weekly digest of those accounts only, with the totals recomputed from them (`Digest::retain_accounts`, one `overall` for both paths); none left means nothing is sent. `--digest --json` gains `accounts[].account_id` (additive). |
| G4 | V-02 (medium), R-02, R-06 | RF1.1, RF1.2 | Use is counted only between readings in the same or the adjacent clock hour; across a longer gap it is unknown and goes to no bucket, in the Rhythm charts, `--stats --rhythm` and the digest's busiest hours. The typical day labels its buckets from the hour index, so a DST day keeps hours 0–23. Each read starts one hour before its span, so the first reading has its predecessor. |
| G5 | S-02 | RF2.2, RF3.1 | A history CSV label with hidden direction characters is refused like a control character; `--digest` prints through the terminal sanitizer and channel texts drop the marks. |
| G6 | V-03, V-04, V-05, R-03, R-04 | RF2.2, RF3.1, RF5 | Limit-change texts name the metric on its own ("Key limit: limit changed …"); the note's date is the day and month in the viewer's language (`LimitChange::note_local`); the digest's busiest hours and the digest-hour picker follow the language's clock; the text Rhythm chart pads by display width and its "off" and day-count marks are translated (`insights.rhythm.day_off`, `insights.rhythm.samples`). |
| G7 | V-06, R-05 | RF4.3 | **Send a test digest** is unavailable, with the reason, while every notice is off, like **Send a test notification**; it reads the history outside the backend lock. |

**Deviations, documented:**
- RF2.2 says "card note"; the note is shown in the account details, as delivered in 0.83.0.
- The digest Markdown dates limit changes in the computer's zone (`Digest::markdown` takes no zone; every caller computes the digest in the local zone).
- On a 24-hour clock the digest-hour picker shows "09", the app's `at.hour` form.

**Open items:**
- The scheduled weekly digest still reads the history inside the timer's locked block (once a week).
- The weekly digest claims its ledger slot before it is computed and posted (once-only delivery by design); a failed history read or post loses that week's digest.
- The digest title and the heaviest and lightest days still print ISO dates.
- Both test buttons are enabled when only the projected alert is on, while the backend answers `notify_off` in that state.
- The new and reworded catalog texts await native review.

## 1. Context

PRD 0026 delivered the Stats view: limit hits, average peak, extra spent, peak per day, recent events and the "limits come back" timeline. Competitors show the **shape of a day and a week**: Fetchy Pro adds "two extra usage strips" beside the menu bar, "one shows the last 24 hours as hourly bars, and the other shows the last 7 days as daily bars", the weekly one "distinguishes work days from days off, which you choose", and offers "monthly usage insights with per-model breakdowns" ([App Store listing, read 2026-10-25](https://apps.apple.com/app/id6786673900)); Claude Pulse lists "peak hours" ([AlternativeTo, Quotum alternatives, read 2026-10-25](https://alternativeto.net/software/quotum/)); OpenUsage shows "a quick sparkline of recent usage" ([OpenUsage v0.7.0 notes](https://newreleases.io/project/github/robinebers/openusage/release/v0.7.0)).

Limits also **move**. Anthropic ran a Claude Code promotion with weekly limits "50% higher" from 2026-05-13, extended more than once ([Claude support article, read 2026-10-25](https://support.claude.com/en/articles/15910845-claude-code-may-july-2026-weekly-limits-promotion)); ElevenLabs' `character_limit` can change with rollovers (PRD 0037 RF4.4); OpenRouter key limits are edited by the person; LiteLLM budgets by an admin. Cuotai records the limit on every reading and says nothing when it changes.

## 2. Problem

- The Stats view says how high the peaks were, not **when** they happen: a person who wants to shift heavy work to the hours after a reset has no view of their own rhythm.
- A limit that grows or shrinks between readings is invisible; a promotion ending looks like "I used more than usual", and a cap someone edited on a shared key is a surprise.
- There is no periodic summary: people open the app when something is red, and never learn "you hit the weekly limit twice this month, both on Thursdays".

## 3. Goals

- An **hourly rhythm** (last 24 hours as hourly bars; typical hour-of-day profile over 30 days) and a **workday-aware weekly strip** (7 daily bars, days off marked) per account and overall, from the local history only.
- **Limit-change detection**: a journal entry, a card note and an opt-in notification when a metric's limit changes within a window ("Weekly limit changed: 100 → 150 points"), plus a `limit_changed` event for hooks.
- An opt-in **weekly digest** (system notification plus a Markdown/JSON text) with hits, peaks, extra spent, limit changes, the best and worst day and the budgets' state (PRD 0041), delivered under PRD 0032's policy.
- Everything additive in `/v1/stats` and `--stats --json`.

## 4. Non-goals

- Reading anything new from a provider: every figure comes from readings already recorded.
- Per-model insights beyond what the recorded metrics carry (model-scoped metrics are included as their own series; no new provider calls).
- A monthly e-mail, cloud sync or sharing of insights (PRD 0044 channels may carry the digest to a webhook the person owns).
- Recommendations that change behaviour automatically; the digest states facts and the allowance (PRD 0041 RF4), never acts.

## 5. Functional requirements

### RF1 — `stats::rhythm` in the core (high priority; prerequisite)

- **RF1.1** `stats::rhythm(account_id | All, range: Last24h | Last7d | Typical30d, now, tz, days_off) -> Rhythm`: for `Last24h`, 24 buckets of **consumption** (the increase of the binding or chosen quota's `used` within the bucket, from the recorded samples; negative deltas at a reset are dropped and the reset marked); for `Last7d`, 7 daily buckets with `day_off: bool`; for `Typical30d`, 24 buckets averaging the consumption per hour of day over the last 30 days with the sample count per bucket (`None` under 3 days of data for that hour).
- **RF1.2** Buckets are in the person's zone; the hour of each **reset** is marked so the chart can draw it; a bucket with no sample is `None` (hatched), never `0`.
- **RF1.3** `days_off` is a 7-flag setting (`notify.days_off`, default Saturday and Sunday, stored in the core-owned preferences of PRD 0032 RF1 so TUI and desktop share it).
- **Acceptance criterion:** pure tests on synthetic series (steady, bursty, with a reset, with gaps, across DST), the `None` rules, the typical profile minimum, and a deterministic golden `Rhythm` for a fixture.

### RF2 — Limit-change detection (high priority)

- **RF2.1** `stats::limit_changes(previous, current) -> Vec<LimitChange>` in the refresh path (next to `events::diff`): for each metric with `limit` in both readings of the **same window instance** (same `resets_at`, or both without one), a change above 0.5% (or 1 unit for integer units) yields `{ metric, from, to, unit, observed_at }`. A change across a reset is not a limit change unless the new window's limit differs from the previous window's limit (compared after the reset settles, PRD 0033).
- **RF2.2** Each change enters the PRD 0026 journal (`limit_changed`), shows a card note for 48 hours ("Weekly limit changed 100 → 150 points on Oct 25"), and emits the `--watch` event `limit_changed` (additive) with `previous`/`current` limits.
- **RF2.3** Opt-in notification **Notify when a limit changes** (off by default, Settings › Alerts), under PRD 0032's policy (held in quiet hours, private text "Claude: a limit changed").
- **Acceptance criterion:** diff tests (increase, decrease, below threshold, across a reset, missing limit), the journal row, the event payload, the notification gate.

### RF3 — Weekly digest (medium priority)

- **RF3.1** `digest::weekly(history, journal, budgets, week, tz) -> Digest`: per account and overall for the past week: limit hits, average peak, days at the limit, extra spent per currency, limit changes, the heaviest and lightest day, the most-used hour band, budgets' used/limit (PRD 0041) and, for the coming week, the allowance per quota with a known reset. Text rendering in the core (every catalog), as plain text (≤ 8 lines for the notification) and Markdown (full).
- **RF3.2** Delivery: opt-in **Weekly digest** (Settings › Alerts; day and hour, default Monday 09:00 local) as a system notification with the short text and an action that opens the Stats view; `cuotai --digest [--week last|current] [--json|--markdown]` prints it; PRD 0044 channels may carry the Markdown. Delivered once per week across processes through PRD 0032's shared ledger; a missed slot (app closed) is delivered on the next start within 48 hours, then dropped.
- **RF3.3** The digest is computed only when due or asked; it never queries a provider.
- **Acceptance criterion:** golden digests for a fixture week in every catalog, the once-per-week ledger behaviour across two processes, the 48-hour catch-up, the `--digest` exit codes (0; 2 when history is off or empty).

### RF4 — Desktop (medium priority)

- **RF4.1** Stats view gains **Rhythm**: the 24-hour bars (resets marked, hatched gaps), the 7-day strip with days off dimmed, and the typical-day profile (DS §5 charts; text alternative and table for screen readers); scope and range follow the existing controls; a **Days off** picker lives in Settings › Alerts (shared with the digest).
- **RF4.2** Menu-bar/tray: a **24-hour strip** style added to PRD 0025's menu-bar styles on macOS (hourly bars of the tightest account, monochrome template) and, in the tray popover, the strip under the account rows (collapsible). Windows and Linux get the popover strip only.
- **RF4.3** Limit changes appear in the journal list and as the card note of RF2.2; the digest preference and preview ("Send a test digest") live in Settings › Alerts.
- **Acceptance criterion:** fixture renders (empty, partial, full; days off), the screen-reader tables, the menu-bar style snapshot, the test digest.

### RF5 — TUI and CLI (medium priority)

- `cuotai --stats --rhythm [--range 24h|7d|typical] [--json]` prints the buckets as a text bar chart; the TUI Stats screen gains a `r` key for the rhythm; `--digest` as RF3.2; `/v1/stats` gains `rhythm` and `limit_changes[]` (additive, same opt-in and rate limit as PRD 0031 RF11).

### RF6 — Documentation (low priority)

- `docs/gui-features.md` (Stats and limits coming back; Alerts), `docs/tui.md`, `docs/advanced.md`, `docs/events.md` (`limit_changed`), `docs/local-server.md` (`/v1/stats` fields), `docs/export-v1.schema.json`.

## 6. Non-functional requirements

- **Honesty:** consumption per bucket comes from recorded deltas; gaps are shown as gaps; the typical profile states its sample count; the digest says "from the readings recorded on this computer".
- **Privacy:** no provider request; the digest text follows PRD 0032's private mode (provider and window, no account name by default) when sent as a notification; the Markdown version includes names because it stays local unless the person sends it through a channel they own.
- **Performance:** rhythm and digest computed from the history in under 200 ms for 90 days × 20 accounts; cached per hour.
- **Accessibility:** every chart has a table alternative (DS §8).

## 7. Data and contract changes

- **Preferences (core-owned, PRD 0032 RF1):** `notify.days_off` (7 flags), `notify.limit_changes` (bool), `notify.digest { enabled, weekday, hour }`.
- **Journal:** kind `limit_changed`.
- **Events (additive):** `limit_changed`.
- **`/v1/stats` and `--stats --json` (additive):** `rhythm { last_24h[], last_7d[], typical[] }`, `limit_changes[]`.
- **CLI:** `--stats --rhythm --range`, `--digest`.

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | `stats::rhythm`, `stats::limit_changes`, `digest::weekly`, event, preferences |
| Desktop | Stats Rhythm section, menu-bar strip style, popover strip, Alerts rows, digest notification |
| TUI/CLI | `--stats --rhythm`, `--digest`, Stats key |
| Docs | gui-features, tui, advanced, events, local-server, schema |

## 9. Dependencies and sequencing

RF1 and RF2 first (core, independent of each other); RF3 after PRD 0041 lands or with budgets omitted until then; RF4/RF5 after RF1–RF3; RF6 with each step. Lane: `stats`, `digest`, the Stats view; no conflict with provider or cost lanes.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Sparse readings (5-minute cadence, hourly after 7 days) make hourly deltas coarse | Buckets state the sample count; `None` under the minimum; 24 h view uses raw samples (kept 7 days) |
| A limit "change" is a provider rounding or a unit flip | 0.5% threshold, same-window rule, PRD 0028 canary notes; the note names both values |
| Digest noise | Opt-in, once a week, quiet-hours aware, short text with an action |
| Menu-bar strip clutter | One more style under PRD 0025, off unless chosen |

## 11. Order of execution and verification

1. **RF1**, **RF2** (core and event).
2. **RF5** (CLI), **RF3** (digest).
3. **RF4**, **RF6**.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests; a smoke: after a week of readings on the owner's machine the Rhythm shows the owner's working hours, a limit change on an OpenRouter key appears as a note and event, and the Monday digest arrives once with the desktop and `cuotai --notify` both running.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Which metric drives the overall rhythm when accounts differ | The binding metric of each account, summed as percentage points; per-account view uses that account's binding metric with a picker for others. |
| Digest default day and hour | Monday 09:00 local; the picker allows any weekday and hour. |
| Should limit changes be on by default as a card note? | Yes as a 48-hour note (silent); the notification stays opt-in. |
| Monthly digest | Later, if people ask; the weekly one first. |
