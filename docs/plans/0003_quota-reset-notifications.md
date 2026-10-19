# PRD 0003 — Quota reset notifications

| Field | Value |
|---|---|
| ID | 0003 |
| Status | Proposed |
| Owner | `cuotai-core` and desktop (TUI optional) |
| Consumers | `apps/desktop`, `crates/cuotai-tui` |
| Date | 2026-10-19 |

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

- Querying the provider at the reset moment to confirm the new balance. There is a cost per query (AWS Cost Explorer) and there are rate limits; the normal refresh cycle stays the same.
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
- **RF3.2** Per-account on/off with a bell icon on the card and in the details. The state lives in `desktop.json`, keyed by the account's stable key (provider + name). Renaming the account loses the preference, and this is documented. Removed accounts have their preference pruned when the configuration is saved. The icon has `aria-pressed` and an accessible label.
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
