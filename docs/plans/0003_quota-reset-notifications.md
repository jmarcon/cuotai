# PRD 0003 — Quota reset notifications

| Field | Value |
|---|---|
| ID | 0003 |
| Status | Awaiting owner verification (every RF delivered; only the owner smoke listed in `0000_control.md` §2.1 remains; status introduced on 2026-10-23; re-validation fixes in 0.84.3, see "Re-validation") |
| Owner | `cuotai-core` and desktop (TUI optional) |
| Consumers | `apps/desktop`, `crates/cuotai-tui` |
| Extended by | PRD 0029 (`Account::key` → stable account id for the bell and `notified.json`), PRD 0031 (`quota_reset_soon` event), PRD 0032 (hide account names on the lock screen; cross-process dedup through the shared `notified.json` ledger, `--notify` kept), PRD 0033 (one refresh 30 s after a reset notice), PRD 0051 (ledger scoped by config identity; a corrupt `notified.json` quarantined without overwriting the previous one and with a visible marker, fix requirement COMP-02) |
| Date | 2026-10-19 |

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.24.0 | `cuotai_core::resets::upcoming(config, reports, now, &Prefs) -> Vec<ResetEvent>`, pure, sorted by `notify_at`. `Prefs { lead, when: NotifyWhen (OnlyIfUsed default, OnlyIfExhausted, Always), include_other, disabled }` (the set of disabled account keys). `ResetEvent` carries `account_index`, `account_key`, `metrics` (label, window, `resets_at`), the group's earliest `resets_at` and `notify_at = resets_at − lead`. The stable account key is `Account::key()` = `<provider slug>:<name>` (also used by PRD 0012 RF1.1); a report whose provider or name no longer matches its account is skipped. Exhaustion follows `metrics::classify_metric`; a missing value is neither used nor exhausted. `ResetKey` (account key + label + `resets_at`) with `same_reset` applies the 2-minute `DRIFT_TOLERANCE`; a postponement beyond it is a new key, so RF2 cancels the old notice. Metrics of one account resetting within one minute of the earliest become one event. Table tests with a fixed clock. No visible change. |
| RF2 | 0.41.0 | The core adds `resets::plan(config, readings, now, &Prefs, &Ledger) -> Plan { due, next }`: `upcoming` (which now also takes one optional reading per account) minus what the `Ledger` already notified, `due` once `notify_at` passed while the reset is at most `MISSED_GRACE` (10 min) old, merged into at most one event per account (suspension, clock jump), and `next`, the earliest notice ahead; `ResetEvent::notice` writes the `Localized` title ("Claude: 5-hour quota reset" / "… resets in 15 min", minutes counted from the delivery) and body (account name, metric labels, "per the time the provider reported; the new balance shows on the next refresh"), with `resets.*` ids in the core catalogs. `Ledger` keeps SHA-256 digests of account key + label with `resets_at` and the notice date (drift-tolerant), pruned after `LEDGER_KEEP` (8 days), saved atomically to `<OS local data dir>/cuotai/notified.json`. The desktop backend (`notify.rs`) holds the device's preferences and ledger behind `Notices::tick(config, readings, now, &dyn Notifier)`; `Backend::run_notices` runs beside `run_scheduler`, reads each account's current report or its preserved/restored reading (so a reset passed while closed is known), and sleeps with `tokio::time::sleep_until` until the next notice (capped at 60 s, since the monotonic clock stops during suspension) or a nudge sent by every view emission (refresh, configuration change) and preference change; never a provider query. Delivery through `tauri-plugin-notification` 2.7.0 (`notification:default` capability at the time; since the 0.84.3 re-validation the webview holds only `notification:allow-is-permission-granted` and Rust delivers) in the language the interface reports (`set_locale`, stored in `desktop.json`). Scheduler tests with a fake clock and a fake `Notifier` cover the notice at the time, the lead time, cancellation on postponement and drift, restarting before and after the time, opening within and beyond the 10-minute window, a long suspension, a removed account, a bell turned off and notices turned off midway. |
| RF3 | 0.41.0 (partial) | **Settings → Notifications**: "Notify quota reset" (off by default; owner decision), lead time at the time/5/15/60 min, filter only if used/only if exhausted/always, include "other" windows and "Send test notification", stored leniently under `notify` in `desktop.json` with the atomic `settings.rs` writes. Turning it on asks the plugin for permission and keeps it off unless `granted`, showing the refusal and how to allow Cuotai in the system settings (on desktop the plugin 2.x always answers `granted`; the system itself decides on the first notice, which the test button checks, so the test is only available, and only accepted by the backend, while notices are on). Each account's bell (card and details, shown while notices are on, `aria-pressed` and an accessible label) is keyed by `Account::key` (`off_accounts`); a rename turns it back on and saving or removing accounts prunes the keys of accounts gone. The tray menu (a first line), its tooltip (a second line) and the tray popover show "Next notice: Claude 5h · today, 14:30" (`View.notify.next`, the notice time). Texts in pt-BR, en-US and es at the time; all 14 locales since PRD 0035. **Missing (owner):** the smoke on the packaged macOS app with a human granting and denying the system permission and a real notice arriving with the window hidden, and the same on Windows and Linux. |
| RF4 | 0.44.0 | `cuotai --notify` (off by default; `--once`, `--json` and `--history` reject it, so their output is unchanged; `--json-v1`, `--serve` and `--watch` also reject it today): `crates/cuotai-tui/src/notify.rs` runs `resets::plan` on the 1 s tick of the loop over each account's current reading or, while it failed, its last valid one (the desktop's `notice_readings`), with the planner's defaults (at the reset, only if used, no `other`) and a session-only `Ledger`. What is due becomes a yellow header line (`dashboard::View::banner`, one extra header row) with the local time and each `ResetEvent::notice` title plus the account name, in `CUOTAI_LANG`, kept for an hour; one `BEL` per pass; and, only when `TERM_PROGRAM` is `iTerm.app`, `WezTerm` or `ghostty` and neither `TMUX` nor `STY` is set, one OSC 9 (`ESC ] 9 ; title — body ST`) per notice. Control characters are stripped from every text. Tests: once at the reset (silent before, after it and on drift), banner expiry, an unused quota and a reset before the session never announced, two accounts due after a suspension sharing one bell with one OSC 9 each, OSC 9 detection, the banner's header row. tmux smoke with a fake `bl` CLI (Alibaba Token Plan) whose 5 h quota reset 70 s after opening: banner and exactly one `BEL` at the reset, none after a manual refresh, OSC 9 bytes only with `TERM_PROGRAM=ghostty` and a clean screen, nothing without `--notify`. |
| Re-validation | 0.84.3 | The fixes of the 2026-11-08 re-validation, listed below. |

**Decisions (2026-10-20, 0.44.0):** the TUI has no notice preferences (the lead time, filter and per-account bell are the desktop's, per device in `desktop.json`), so it uses the planner's defaults. "No persistence" is read as per session: a reset whose notice time came before the TUI opened (a reading restored from the history, PRD 0010 RF2.3) is recorded as seen and never announced, so reopening the TUI does not repeat it. OSC 9 goes out automatically on a whitelist of terminals known to show it, never through a multiplexer, rather than behind a second flag. The banner lasts an hour so it is still there when the user comes back to the terminal.

> **PRD 0032 (0.81.0):** the open items on duplicates and lock-screen names are closed there: `cuotai --notify` and the desktop share `notified.json` under `notified.lock` (one notice per event across processes), notification texts hide account names by default (`notify.private`), and quiet hours drop a reset that happens inside the window (recorded as `skipped`).

### Re-validation (2026-11-08)

A serial re-validation of the delivered plan (visual, security and review passes against v0.84.2) found seven medium and thirteen low findings. Every medium one is fixed in 0.84.3, and so is every low one except the items listed after the table:

| ID | Severity | RF | Fix |
|---|---|---|---|
| S-01, R-01, R-02 | medium, medium, low | — (refresh) | An out-of-range Groq reset header, a JetBrains AI token lifetime near `u64::MAX` and a plan-limit reset near chrono's maximum no longer panic: the reset time is dropped, the lifetime is clamped to one year, no reset query is armed. |
| R-03 | medium | — (refresh) | `panic_guard`: each account query runs under `catch_unwind`; a panic fails only that account (incompatible-answer note, no payload text), and `panic_isolated()` keeps the TUI's terminal and skips the default printer in the logging hook. |
| V-04 | medium | RF3.4 | The tray, menu and tooltip "Next notice" skips a reset quiet hours drop and shows a held one at the window's end (`quiet::reset_shown_at`, `resets::next_shown`). |
| V-02, V-07 | medium, low | RF3.4 | The tray line keeps its time in a non-shrinking element with the full sentence as tooltip; the time follows DS-MSG-8 with translated joins (`time.weekdayTime`, `time.dateTime`). |
| V-01, S-05 | medium, low | RF2.2 | Notice bodies, `other` window titles and the tray line show metric labels through `metric_text` in the interface language, joined by a translated list pattern; with private texts an `other` window gets a generic title. |
| V-05 | low | RF2.2, RF3.2 | An unnamed account is named by its provider in the body and on the bell. |
| S-04 | low | RF2.2 | `text::notice_safe`: every desktop notice (resets, alerts, digests, channel failures) and the TUI's OSC 9 text lose controls and bidi overrides and are capped at 64/160 characters, with markup escaped on Linux; the TUI banner filter drops bidi overrides too. |
| V-09, V-06 | low, low | RF2.2 | Shorter titles and caveat in every core catalog, within DS-CMP-NOTIF-1's 50/110 budget for a short provider name (a test checks all locales); grammar fixed in ru, tr, hi-IN, de and spacing in ja, zh-CN. |
| V-03 | medium | RF3.1 | A Settings row with a segmented control drops it below the label when both do not fit; ui-shots `check.mjs` checks label/control overlap. |
| S-06 | low | RF2.2 | The webview's notification grant is only `notification:allow-is-permission-granted` (PRD 0050 notes it). |
| S-02, S-03, S-07 | low | RF2.3 | A frozen (later-major) or unreadable `notified.json` is never overwritten and keeps the process's own records, so no notice repeats; `notified.lock` treats a future mtime as stale, breaks a stale lock without removing a fresh one, and a holder removes only its own lock. |
| V-08, V-07 | low | RF4 | The `--notify` banner continues on a second header row in narrow terminals; its time is in UTC and labelled like the header's clock. |
| R-04 | low | — | This plan's status text (capability, locales, `--notify` conflicts) is current. |

Open items: the ledger claim still runs under the desktop's `inner` mutex, so a held lock can delay a view emission by up to 2 s (S-07, optional part); the Linux daemon's markup rendering and the webview console with the narrowed grant were not observed in a packaged app (they join the RF3 owner smoke of `0000_control.md` §2.1); the browser demo's next-notice mirror ignores the ledger and quiet hours (demo only).

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

Each `Metric` can carry `resets_at: Option<DateTime<Utc>>` and `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other`) in `crates/cuotai-core/src/model.rs`. The cards already show "in 4h 10m", but nothing warns when the moment arrives. On the desktop, closing the window only hides it (`lib.rs`: `CloseRequested` → `prevent_close` + `hide`) and `Backend::run_scheduler` keeps running with the tray icon. In other words, the process stays alive to fire notices. Today the desktop app depends only on `tauri-plugin-dialog`, has no notification permission or plugin (`capabilities/default.json`), and its preferences live in `desktop.json` (`settings.rs`: `user_dir`, `config_file`).

## 2. Problem

People with window-based subscriptions (Claude 5h/week, Codex, Copilot, Kimi…) have to go back to the app to find out whether the quota has already reset. Notifying naively creates noise and false alarms:
- rate-limit windows reset every minute;
- the `resets_at` of rolling windows changes a few seconds or minutes between queries;
- restarting the app could resend old notices;
- many accounts may reset in the same minute.

## 3. Goals

- Turn operating-system notifications about quota resets on and off, globally and per account, on the desktop.
- Notify at the reset time or with a chosen lead time, once per reset, even after restarting the app.
- Use only data that has already been queried: no extra provider call because of a notification.

## 4. Non-goals

- Querying the provider at the reset moment to confirm the new balance. There is a cost per query (AWS Cost Explorer) and there are rate limits; the normal refresh cycle stays the same. *Superseded by PRD 0033 RF1 (owner decision I, 2026-10-23): one query of the account 30 s after a reset notice fires, never for a metered account.*
- Notifying with the app closed, starting with the system, remote push or e-mail.
- Consumption notices ("below 20%"), which are left to their own PRD, and changes to `Report`, `Snapshot` or the `--json` output.
- Inventing a reset time when the provider does not report `resets_at`.

## 5. Functional requirements

### RF1 — Reset planner in the core (high priority; prerequisite)

- **RF1.1** Create in `cuotai-core` a pure function, with no network and no internal clock, of the form `resets::upcoming(config, reports, now, &Prefs) -> Vec<ResetEvent>`. Each `ResetEvent` carries the account index, the account's stable key, the label and window of the metric, `resets_at` and the notice time (`resets_at − lead`). The TUI and the desktop app use the same logic.
- **RF1.2** Eligibility:
  - the metric has a future `resets_at`, `kind = quota` and a `window_5h`, `daily`, `weekly` or `monthly` window;
  - `other` only enters if the user enables it;
  - `rate_limit` and `balance` never enter;
  - "when to notify" filter: **Only if used** (default: `used > 0` or `remaining < limit`), **Only if exhausted** or **Always**;
  - a missing value counts as neither used nor exhausted.
- **RF1.3** Identity and deduplication:
  - the reset key is the account key + metric label + `resets_at`;
  - two `resets_at` of the same metric within 2 minutes of each other are the same reset (rolling-window drift);
  - if a later query postpones `resets_at` beyond the tolerance, the old notice is cancelled and the new one is scheduled;
  - metrics of the same account that reset in the same minute become a single grouped event.
- **Acceptance criterion:** table tests with a fixed clock cover no `resets_at`, a past `resets_at`, a rate limit, the `other` window off and on, the three filters, missing values, drift below and above the tolerance, grouping and a disabled account.

### RF2 — Scheduling and delivery on the desktop (high priority)

- **RF2.1** The Tauri backend schedules the notices, not JavaScript: the hidden webview may be suspended. After each refresh (account or cycle), configuration change and preference change, the backend recomputes the next occurrence and sleeps until it with `tokio::time::sleep_until`, alongside the existing `run_scheduler`. Delivery sits behind a `Notifier` trait for tests with a fake clock and a fake deliverer.
- **RF2.2** Delivery through the 2.x line of `tauri-plugin-notification` (compatible with Tauri 2.12; 3.x is still alpha) and the matching permission in `capabilities/default.json`.
  - Title: "Claude Code: 5h quota reset" (or "resets in 15 min").
  - Body: the account name and, when present, the grouped metrics.
  - Never include e-mail, tokens, URLs or provider notes.
  - Texts in `pt-BR`, `en-US` and `es`, in the language chosen in the app.
- **RF2.3** Anti-duplication persistence:
  - store the already notified keys in `<OS data dir>/cuotai/notified.json` (only digests and dates, pruned after 8 days);
  - on opening the app, a reset that passed more than 10 minutes ago is not notified ("missed while closed"); within the window, it is notified once;
  - changing the system clock or waking from suspension does not generate a burst of notices: at most one grouped event per account.
- **Acceptance criterion:** scheduler tests with a fake clock cover a notice at the time, the lead time, cancellation on postponement, restarting the app before and after the time, a long suspension, a removed account and notifications turned off midway.

### RF3 — Preferences and permission on the desktop (high priority)

- **RF3.1** Create the **Settings → Notifications** section with:
  - **Notify quota reset** (off by default);
  - lead time **At the time / 5 / 15 / 60 min**;
  - filter **Only if used / Only if exhausted / Always**;
  - **Include "other" windows**.

  Everything is stored in `desktop.json` (a per-device preference, never in `cuotai.toml`), with atomic writes like the other `settings.rs` fields.
- **RF3.2** Per-account on/off with a bell icon on the card and in the details. The state lives in `desktop.json`, keyed by the account's stable key (provider + name). Renaming the account loses the preference, and this is documented. *(Superseded by PRD 0029: preferences are keyed by the stable account `id`, so a rename keeps them.)* Removed accounts have their preference pruned when the configuration is saved. The icon has `aria-pressed` and an accessible label.
- **RF3.3** Ask for the operating-system permission only when the user turns the global option on. If it is denied, show the state, guidance on opening the system settings and keep the option off. The **Send test notification** button validates the flow.
- **RF3.4** The tray and the tooltip show the next notified reset ("Next notice: Claude 5h at 14:30") when there is one.
- **Acceptance criterion:** a smoke test on the packaged macOS app (permission granted and denied, a real notice arriving with the window hidden), Windows and Linux when available; preferences survive a restart; in the three languages.

### RF4 — Notice in the TUI (low priority; optional)

- **RF4.1** With `--notify` (off by default), while the TUI is open, reuse the RF1 planner:
  - a header banner ("Claude Code: 5h reset");
  - the terminal bell (`BEL`);
  - optionally, an OSC 9 notification on terminals that support it (iTerm2, WezTerm, Ghostty).

  Unknown sequences must not dirty the screen. No persistence: the notice only holds for the open session.
- **Acceptance criterion:** a tmux smoke test with a test metric resetting in 1 minute; `--once`/`--json` do not change.

## 6. Non-functional requirements

- **Privacy:** the notification content shows in the system's notification center and on the lock screen. Only the provider name, account name and metric label, never credentials, e-mails or responses.
- **Cost and honesty:** a notification does not trigger a query. The text says "reset according to the time reported by the provider"; the new balance only appears on the next refresh.
- **Energy:** a single timer until the next event, with no per-second polling in the backend.
- **Risks to verify before RF2:**
  - on macOS, a notification only has the right name and icon in the packaged app (`dev.cuotai.desktop`); in `tauri dev` it may not appear;
  - clicking the notification to open the card has limited desktop support in the plugin; if it is not reliable, it is out of scope.

## 7. Order of execution and verification

1. **RF1:** a pure planner in the core with table tests; no visible change.
2. **RF2:** the scheduler and deliverer behind the trait, anti-duplication persistence, plugin and permission; tests with a fake clock.
3. **RF3:** preferences, per-account bell, permission, test and tray; a smoke test on the packaged app.
4. **RF4 (optional):** `--notify` in the TUI.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface before the version.

The requirement is only delivered when, with the app hidden in the tray, an account with used quota produces **one** notice at the chosen time (or lead time), no notice repeats after restarting the app, and turning off the global option or the account bell cancels the pending ones, with no extra query to the provider.
