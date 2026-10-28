# PRD 0032 — Notification delivery policy

| Field | Value |
|---|---|
| ID | 0032 |
| Status | Awaiting owner verification |
| Owner | `cuotai-core` (`notify_prefs`, quiet-hours decision, digest, `AlertState`, `resets::Ledger`), desktop and TUI |
| Consumers | `apps/desktop` (Settings → Notifications, delivery, cards and details), `crates/cuotai-tui` (`--notify`), `--watch` hooks, PRDs 0023 and 0024 (their alerts follow the same policy) |
| Depends on | PRD 0003 (RF1–RF4: reset planner, ledger, delivery), PRD 0008 (RF1–RF4: `alerts::evaluate`, `AlertState`, preferences), PRD 0013 (RF3: hooks); PRD 0029 (RF2: state keyed by account `id`) for RF1.3 |
| Benefits from | PRD 0020 (Settings rows), PRD 0022 (UI refresh: marker visuals), PRD 0025 (RF6: recommended defaults) |
| Extends | PRD 0003 RF2/RF4 (quiet hours, cross-process ledger, private texts), PRD 0008 RF1/RF2/RF3/RF4 (deferred marks, missed-crossing marker, recovery without reset, single preference source), PRD 0013 RF3 (`quiet` filter) |
| Extended by | PRD 0051 (state schema and backup policy beside RF4's `LedgerLock`; COMP-02 strengthens the fixed `.corrupt` set-aside), PRD 0054 (RF2: hook claims beside RF4's ledger claims; RF4.2 must cover every `notified.json` write path) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. **Quiet hours** are the person's do-not-disturb intervals. A **digest** is the single notification sent when quiet hours end. A notice **breaks through** when it is delivered during quiet hours. Code references are to `origin/main` at `0ba9c9b`.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | `cuotai_core::notify_prefs` owns `NotifyPrefs`, `AlertSettings` and the `quiet` schedule with their lenient codecs (the desktop's `settings.rs` re-exports them and reads and writes the sections through them), `settings_path()`, `read()` and a read-only `Source` that re-reads `desktop.json` when its modification time changes; a missing file reads as `Loaded::absent()` (the recommended defaults: resets and alerts on, 75% and 90%, exhaustion and recovery, nothing muted, private texts, no quiet hours). Bells and mutes are keyed by `Account::state_key` (PRD 0029), so every reader shares them. Tests: one fixture document builds the same `AlertPrefs` and planner for every reader, a lenient read with unknown and invalid fields, the source following an edit. |
| RF2 | vNEXT | `cuotai_core::quiet`: `QuietSchedule` (`enabled`, up to 3 `Interval`s per weekday, `digest`; lenient `from_value`, strict `from_value_strict` for the interface, `to_value`) and the pure `window_at(schedule, now, tz)`, tested across midnight, the week wrap, adjacent intervals merged into one window, and spring-forward and fall-back changes with a test zone (a skipped hour shortens, a repeated hour lengthens). Settings → Alerts → **Quiet hours** (a sub-page opened from one row with the schedule in a line): the switch, "Send a summary when quiet hours end" (on by default), one row per weekday with labelled `HH:MM` fields (up to three intervals, add and remove) and "Same every day" (copies Monday's), saved at once through `set_quiet_hours` (a time edit saves without redrawing the editor, so typing `HH:MM` from the keyboard is never cut short); the schedule survives a restart (`Notices` round-trip test). Keyboard-only browser test `ui-shots/keyboard.mjs` (Tab, Enter, Space, typed digits), blocking in the CI `ui-checks` job. The weekday editor is keyboard-operable, which `ui-shots/keyboard.mjs` proves in the `ui checks` CI job. |
| RF3 | vNEXT | `quiet::decide(Subject, window)` returns `Deliver` for `Exhausted`, `Defer(ends_at)` for thresholds, recoveries, session expiries and projections, and `Drop` for a reset inside the window (a lead notice whose reset comes after the window is deferred, not rung inside it). `AlertState::update_with` records a held crossing as a mark with `deferred_until` (the `Deferred { until }` state, persisted in `notified.json`, older releases ignore the unknown field) and `take_deferred` settles the due ones: with the digest on, only what still holds (same window instance, still at or above the level, recovery or session expiry still standing) comes back as one event per metric with its current state; with it off, or when nothing holds, they are only marked seen. `Ledger::record_skipped` stores dropped resets as `skipped`; `MISSED_GRACE` is unchanged. `quiet::digest` lists at most 5 lines plus "and N more" (lock-screen-safe titles when private) and the notifier sends it under the one-per-minute spacing. Scheduler tests with a fake clock: a 90% crossing at 23:00 with quiet 22:00–08:00 gives one digest at 08:00 across two processes, exhaustion at 23:00 notifies at once, a reset at 03:00 is never notified, a crossing whose window resets at 05:00 is not in the digest, a restart at 02:00 keeps the deferred item, and with the digest off nothing is sent. The deferral of PRD 0023's overage alert and the persistence of a held PRD 0024 projection (the TUI keeps it in memory and re-judges after a restart) join with those alerts. Overage (PRD 0023) and projected-exhaustion (PRD 0024) notices held by quiet hours are deferred marks in `notified.json` too (the projection mark keeps what it projected), re-judged when delivered, so both reach the digest, even after a restart; the TUI keeps no in-memory hold. |
| RF4 | vNEXT | `resets::LedgerLock` (`notified.lock` created exclusively beside `notified.json`, waits up to 2 s, breaks a lock older than 10 s, never blocks past that) and `Ledger::claim`, which re-reads the file under the lock, applies the change and writes it back; `claim_resets`, `claim_alerts` and `claim_digest` (wrapped by `SharedLedger`, which also re-reads the file when its modification time changed) drop what another process already recorded, so two `Notices` over one directory deliver each reset, crossing and digest once. `cuotai --notify` now loads and saves the shared ledger. A corrupt `notified.json` is set aside as `notified.json.corrupt`. Tests: two processes, a stale lock, a waiting lock, a missing directory, the digest leaving through one process, the TUI and the desktop `Notices` over one fixture directory. Owner: a TUI smoke with the desktop open. |
| RF5 | vNEXT | `hooks.toml` rules accept `quiet = "run" | "skip"` (default `run`; the field is left out of the confirmation digest while `run`, so existing confirmations stay valid); `Runner::submit_in(event, in_quiet)` skips the `skip` rules for events inside a window, `cuotai --watch --hooks` and the desktop's hooks evaluate the window at the event's time from `desktop.json`, there is no digest for hooks and the NDJSON stream is unchanged. The consent listing shows `quiet = "skip"` and the confirmation changes with it; the desktop editor has a "Do not run during quiet hours" box per rule (`quiet = "skip"` written only when chosen). |
| RF6 | vNEXT | `cuotai --notify` reads the thresholds, mutes and bells from `desktop.json` (re-read on change), holds the banner, `BEL` and OSC 9 during quiet hours except exhaustion, drops the resets that happen inside a window, writes one digest line when the window ends and shares the ledger (RF4). Tests with a fixture `desktop.json` and a fake clock: recommended defaults without the file, a threshold list and a muted bell from the file, an edit picked up without a restart, the quiet-hours digest, exhaustion breaking through, a night reset never announced, two terminals announcing once. Owner: the tmux smoke. |
| RF7 | vNEXT | `notify.private` (on by default; an absent key reads as on) selects `alerts::notice_with` and `ResetEvent::notice_with`: titles keep the provider, the window and the percentage, bodies drop the account name, the metric and the values beyond the percentage (`alerts.body.private`, `alerts.body.details.private`, `alerts.title.projected.private`, `resets.body.private`, …; en-US, es and it catalogs, Portuguese fallbacks); the digest follows it. `--notify` applies it to OSC 9 texts only (the header banner is the terminal's own screen). Text tests in the four languages assert that no account name or metric label appears with the switch on; Settings → Alerts → More options has "Hide account names in notifications". Owner: the lock-screen smoke on a packaged app. |
| RF8 | vNEXT | `AlertState::missed(prev, next, prefs)` lists the highest level per metric that its window instance reached without a notice, a waiting digest or an earlier acknowledgement (nothing without a previous reading of the account); the desktop's `Notices` computes it on the first reading of an account in the process and keeps it until the window instance ends or `acknowledge_missed` (called when the details open) records it as seen in `notified.json`. The card gets a "Crossed while closed" chip and the details a line "Passed 90% while Cuotai was closed" (`MetricAlertView.missed`). Tests: closed at 70% and reopened at 92% marks without a notice, a reopening after the window reset or a notified crossing shows nothing, the acknowledgement survives a restart. |
| RF9 | vNEXT | A metric without `resets_at` whose exhaustion was notified recovers (`Recovered`) once consumption is at most 80%: the 5-point hysteresis still re-arms the exhaustion in between (the exhausted mark is kept as `lapsed` until the drop), 100 → 85 → 79% recovers once at 79%, 100 → 79 → 100% notifies the exhaustion again, and a metric with `resets_at` keeps the window-instance rule. The thresholds below stay governed by the existing hysteresis. |
| RF10 | vNEXT | `README.md` (quiet hours, the summary, private texts, one announcement per profile), `docs/gui-features.md` (the whole policy), `docs/tui.md` (`--notify` reads `desktop.json`, quiet hours, the shared ledger, private OSC 9) and `docs/events.md` (the hooks `quiet` key); notes in PRDs 0003 and 0008. |

## 1. Context

Three paths notify today: the desktop (reset notices, PRD 0003; usage alerts, PRD 0008), `cuotai --notify` in the TUI, and hooks under `--watch` (PRD 0013). They decide independently:

- **Preferences live in three places** (0008 open item): `desktop.json` `alerts`/`notify` (`apps/desktop/src-tauri/src/settings.rs:146-166`, `:252-271`; the file is `<OS config dir>/cuotai/desktop.json`, `:670-676`), fixed defaults in the TUI (`AlertPrefs::default()` with `enabled = true`, `crates/cuotai-tui/src/notify.rs:48-62`), and `EventState::new` with `DEFAULT_THRESHOLDS` (`crates/cuotai-core/src/events.rs:430-448`, `alerts.rs:27`).
- **No quiet hours** (DEC-03). Two code facts make a digest impossible today: `AlertState::update` records every level a reading reached as a mark, delivered or not (`alerts.rs:336-433`, `notified: notified(kind, threshold)`), so a crossing held back is never sent later; and the reset planner drops a reset more than `MISSED_GRACE` (10 minutes, `resets.rs:27`, used at `:277`) old.
- **Duplicates across processes** (0003 open item): the desktop persists `notified.json` (`resets.rs:343-366`), but `--notify` keeps its ledger and alert state in memory, "never saved" (`crates/cuotai-tui/src/notify.rs:30-36`). With the app and the TUI open, both notify the same crossing.
- **Account names on the lock screen** (0003 open item): the reset body names the account (`resets.rs:176-186`), as do the usage alerts.
- **Crossings while closed are invisible** (ALR-01): the first-reading rule (PRD 0008 RF1.7, `alerts.rs:240-245`, `CROSSING_GRACE` 10 minutes) rightly suppresses a stale notification, but nothing tells the person "this passed 90 % while Cuotai was closed".
- **No recovery without `resets_at`** (0008 open item): `awaiting_recovery` returns `None` when the metric has no `resets_at` (`alerts.rs:516-528`), so a balance-style quota that was exhausted never "recovers".

## 2. Problem

- Notifications arrive at night and on weekends with no way to hold them.
- The TUI and the desktop disagree on thresholds and mutes and both notify.
- Notification texts can reveal account names on a locked screen.
- A person opening the app after a crossing gets neither a notice nor a hint.
- An exhausted metric without a reset date stays "exhausted" in the alert state forever.

## 3. Goals

- One core-owned source of notification preferences that every notifying process reads.
- Quiet hours per weekday, with a single digest at the end, exhaustion breaking through and stale reset notices dropped.
- One delivery per event across processes on one profile, keeping `--notify`.
- An option for lock-screen-safe texts without account names.
- A visible marker for crossings that happened while Cuotai was closed, without a notification.
- Recovery for metrics without a reset date.

## 4. Non-goals

- Per-window thresholds (for example 90 % for the 5-hour window, 75 % for the week); a later extension of RF1's format.
- Calendar or OS focus-mode integration (macOS Focus, Windows Focus Assist); the system already silences banners there and Cuotai's own schedule is independent.
- Changing the `--watch` event stream: events are never held or dropped by quiet hours (only hook execution may be, RF5).
- Retiring `cuotai --notify`.

## 5. Functional requirements

### RF1 — Single preference source in the core (high priority; prerequisite)

- **RF1.1** `cuotai_core::notify_prefs` owns the type, the lenient codec and the path of the `notify`, `alerts` and new `quiet` sections of `desktop.json`; the desktop's `settings.rs` delegates to it for those sections.
- **RF1.2** Readers: the desktop (read/write), `cuotai --notify` and `--watch` (read-only, re-read when the file's modification time changes); when the file is absent, the core recommended defaults (`AlertPrefs::recommended()`, PRD 0024 RF2.7) apply, so a TUI-only user keeps today's behaviour.
- **RF1.3** Bells and mutes are keyed by account `id` (PRD 0029 RF2) and therefore shared by every reader.
- **Acceptance criterion:** tests that the TUI and `EventState` built from one `desktop.json` fixture produce the same `AlertPrefs` as the desktop; a lenient-read test with unknown and invalid fields.

### RF2 — Quiet hours schedule (high priority; DEC-03)

- **RF2.1** `quiet: { enabled, days: { mon..sun: [ { from: "HH:MM", to: "HH:MM" } ] }, digest: bool }` in `desktop.json`, at most 3 intervals per day; an interval whose `to` ≤ `from` crosses midnight and belongs to its start day. Evaluated in the system's local time zone, DST-safe (a skipped hour shortens, a repeated hour lengthens).
- **RF2.2** `quiet::window_at(schedule, now, tz) -> Option<QuietWindow { ends_at }>` is pure and tested across midnight, week wrap and DST changes.
- **RF2.3** Settings → Notifications: a "Quiet hours" switch, a per-weekday editor (DS-CMP rows of PRD 0020) with a "Same every day" shortcut and "Send a summary when quiet hours end" (on by default).
- **Acceptance criterion:** pure tests above; a desktop smoke editing a 22:00–08:00 weekday schedule that survives a restart.

### RF3 — Delivery decision and digest (high priority; DEC-03)

- **RF3.1** `quiet::decide(notice, window) -> Deliver | Defer(until) | Drop`:
  - `Exhausted` usage alerts break through (`Deliver`);
  - `Threshold`, `Recovered`, `SessionExpiring`, PRD 0023 overage and PRD 0024 `ProjectedExhaustion` are deferred to `ends_at`;
  - reset notices (PRD 0003) whose reset happens during the window are dropped; a reset notice due after `ends_at` is unaffected.
- **RF3.2** **Deferred marks:** `AlertState::update` gains the `Deferred { until }` state for a crossing held by quiet hours, persisted in `notified.json`, instead of recording it as seen-without-notice; a restart during quiet hours keeps it. The reset ledger records dropped notices as `skipped` so they are never sent at the end; `MISSED_GRACE` keeps its meaning for the closed-app case.
- **RF3.3** **Digest:** at `ends_at`, one notification lists the deferred items that **still hold** (the metric is still above the threshold in the same window instance; a projection still projects before the reset), newest state per metric, at most 5 lines plus "and N more"; items that no longer hold are dropped. `digest = false` drops deferred items instead.
- **RF3.4** Grouping and the one-per-minute spacing of PRD 0008 apply to the digest as one notice.
- **Acceptance criterion:** scheduler tests with a fake clock: a 90 % crossing at 23:00 with quiet 22:00–08:00 yields one digest at 08:00; exhaustion at 23:00 notifies at once; a reset at 03:00 is never notified; a crossing at 23:00 whose window resets at 05:00 is not in the digest; restart at 02:00 keeps the deferred item.

### RF4 — Cross-process de-duplication (high priority; 0003 open item)

- **RF4.1** `cuotai --notify` loads and saves the shared `notified.json` (reset ledger and `AlertState`) like the desktop, instead of keeping them in memory.
- **RF4.2** Delivery is a claim: under an advisory lock (`notified.lock` next to the file), the process re-reads the ledger, skips what another process already recorded, records its own items and releases the lock, then delivers. The lock is held for milliseconds and never across a network call; a stale lock older than 10 seconds is broken.
- **RF4.3** Quiet hours and the digest are claimed the same way, so only one process sends the digest.
- **Acceptance criterion:** a test with two `Notices` instances sharing a temporary directory delivers each event once; a stale lock test; a TUI smoke with the desktop open.

### RF5 — Hooks `quiet` filter (medium priority; DEC-03)

- **RF5.1** `hooks.toml` rules accept `quiet = "run" | "skip"` (default `run`, today's behaviour); `skip` does not run the rule for events occurring inside a quiet window; no digest for hooks. The `--watch` NDJSON stream is unchanged.
- **RF5.2** The hooks editor in the desktop (PRD 0013 RF4) shows the option; the consent listing shows it.
- **Acceptance criterion:** hook tests inside and outside a window; the consent digest changes when `quiet` changes.

### RF6 — TUI `--notify` follows the policy (medium priority)

- **RF6.1** `--notify` uses RF1 preferences (thresholds, mutes, bells), RF3 decisions (no banner, `BEL` or OSC 9 during quiet hours except exhaustion; digest banner at the end) and RF4's shared ledger.
- **Acceptance criterion:** TUI tests with a fixture `desktop.json` and a fake clock; a tmux smoke.

### RF7 — Lock-screen-safe texts (medium priority; 0003 open item)

- **RF7.1** `notify.private` in `desktop.json` ("Hide account names in notifications"), **on by default** (owner decision 2026-10-23; an absent key reads as on): titles keep provider and window, bodies omit the account name and metric values beyond the percentage (`resets.body.private`, `alerts.*.private` text ids); the notification click still opens the right account.
- **RF7.2** `--notify` follows the same switch (OSC 9 texts can reach the system notification centre).
- **Acceptance criterion:** text tests in the four languages that no account name appears with the switch on; a desktop smoke on the lock screen.

### RF8 — "Crossed while closed" marker (medium priority; ALR-01)

- **RF8.1** On the first reading of an account in a process, `AlertState::missed(report) -> Vec<MissedCrossing>` lists levels reached in the current window instance that were never notified (no mark, or a mark with `notified = false` from an earlier process). No notification is sent (PRD 0008 RF1.7 still applies).
- **RF8.2** The card shows a small marker and the details a line `alerts.missed` ("Passed 90 % while Cuotai was closed"), until the window instance ends or the person opens the details; then the mark is recorded as seen.
- **Acceptance criterion:** tests: closed at 70 %, reopened at 92 % → marker, no notice; reopened after the window reset → nothing; a notified crossing never shows the marker.

### RF9 — Recovery without `resets_at` (medium priority; 0008 open item)

- **RF9.1** For a metric without `resets_at`, an `Exhausted` mark is followed by `Recovered` when consumption falls **at least 20 points** below 100 % (≤ 80 %); the drop also re-arms the thresholds below it (the existing 5-point hysteresis keeps working above).
- **RF9.2** Metrics with `resets_at` keep the window-instance rule.
- **Acceptance criterion:** tests: 100 → 85 → 79 % recovers once at 79; 100 → 79 → 100 notifies exhaustion again; a metric with `resets_at` is unchanged.

### RF10 — Documentation (low priority)

- **RF10.1** `README.md` (quiet hours, digest, private texts, `--notify` behaviour with the app open), `docs/events.md` (`quiet` filter), `docs/tui.md` (`--notify` reads `desktop.json`), PRD 0003/0008 notes.
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

## 6. Non-functional requirements

- **Honesty:** a digest never reports a condition that no longer holds; dropped reset notices are not "missed" (the card already shows the new window).
- **Determinism:** `window_at`, `decide` and the digest builder are pure over their inputs and the injected clock and time zone.
- **Robustness:** the advisory lock never blocks the UI thread and never deadlocks (timeouts, stale-lock breaking); a corrupt `notified.json` is set aside and recreated, as today.
- **Privacy:** private texts carry provider, window and percentage only; quiet-hours schedules stay local.
- **Accessibility and i18n:** the weekday editor is keyboard-operable with labelled time inputs; day names and times follow the locale; pt-BR, en-US, es and it (plus PRD 0035 languages).

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `desktop.json` | `quiet`, `notify.private`; sections owned by the core codec | Additive, lenient read |
| `notified.json` | `Deferred` marks, `skipped` reset entries, missed-seen marks; written by `--notify` too | Additive; unknown states ignored by older releases' lenient read |
| `notified.lock` | new file | New |
| `hooks.toml` | `quiet` per rule | Additive |
| Events, `export::v1`, TOML | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | `notify_prefs`, quiet schedule and decision, digest, deferred and missed marks, recovery rule, shared-ledger claim, private texts |
| Desktop | Settings rows, digest delivery, marker on card and details |
| TUI/CLI | `--notify` reading the shared preferences and ledger; hooks `quiet` filter in `--watch` |

## 9. Dependencies and sequencing

RF1 first (every other RF reads it). RF4 can ship with RF1 as the fix for duplicates. RF2 and RF3 together. RF8 and RF9 are independent of quiet hours. Account-keyed preferences should land after PRD 0029 RF2 to avoid a second key migration. PRD 0023 and 0024 alerts adopt RF3 when they ship. Wave 5e of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| A held notice turns out to be important | exhaustion always breaks through; the digest is on by default |
| Lock contention or a stuck lock | short critical section, timeout, stale-lock breaking |
| The TUI now depends on a desktop file | read-only, absent file means recommended defaults |
| Time-zone and DST mistakes | pure tests across transitions in several zones |

## 11. Order of execution and verification

1. **RF1** and **RF4**.
2. **RF2** and **RF3**; **RF6**; **RF5**.
3. **RF7**, **RF8**, **RF9**.
4. **RF10** with each step.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke with the desktop and `cuotai --notify` open together. The requirement is only delivered when a 90 % crossing at 23:00 with quiet hours 22:00–08:00 produces exactly one digest at 08:00 across both processes and an exhaustion at 23:00 is notified at once.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Should quiet hours also hold reset notices due right after they end | No; only resets that happen inside the window are dropped. |
| Per-window thresholds | Later extension of the RF1 format (`thresholds_by_window`), not in this PRD. |
| Should `--watch` events carry a `quiet: true` flag | Not now; consumers can apply their own schedule. |

## 13. Decisions (owner, 2026-10-23)

| Topic | Decision |
|---|---|
| Account names in notifications | **Hidden by default** (`notify.private` on, RF7.1); the person can turn names back on. |
