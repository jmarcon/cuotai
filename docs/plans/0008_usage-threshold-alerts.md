# PRD 0008 — Usage threshold alerts

| Field | Value |
|---|---|
| ID | 0008 |
| Status | In progress |
| Owner | `cuotai-core`, desktop (TUI optional) |
| Consumers | `apps/desktop`, `crates/cuotai-tui`, PRD 0013 (events) |
| Depends on | PRD 0003 (RF2 and RF3: `Notifier` deliverer, permission, `notified.json` and preferences); PRD 0006 is optional |
| Date | 2026-10-19 |

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.42.0 | `cuotai_core::alerts::evaluate(prev, next, &AlertPrefs, &AlertState, now) -> Vec<AlertEvent>`, pure, with an injected clock and no query. `AlertEvent` carries `account_key` (`Report::account_key`, the same `<provider slug>:<name>` as `Account::key`), `label`, `window`, `kind` (`Threshold`, `Exhausted`, `Recovered`), `threshold` (percent; 100 for `Exhausted` and `Recovered`), `consumed` (fraction) and `resets_at`. Eligible: a `Quota` that `metrics::classify_metric` reads as available with a cap or exhausted with a positive cap; uncapped metrics, the other kinds and missing or non-finite values are skipped. `AlertPrefs::new(enabled, thresholds)` rejects values outside 10–99 (`ThresholdError`); the default is off with 75 and 90, and exhaustion is always evaluated while enabled. The window instance is `resets_at` within `resets::DRIFT_TOLERANCE` (2 min, an unknown `resets_at` being the same instance); in one instance a level fires once and a query crossing several emits the highest above every level already reached; a level re-arms after falling `HYSTERESIS_POINTS` (5) below it or in a new instance. `Recovered` fires once, on the first reading with slack in a later instance, only when that metric's exhaustion was notified. RF1.7: without a usable `prev` (absent, not `Ready`, another account, or not older), or for a reading older than `CROSSING_GRACE` (10 min) at `now`, nothing fires; on an account's first reading in the process, `prev` (the last reading PRD 0006 recorded, which the caller passes when present) must be at most 10 minutes older than `next`, and only levels `prev` had not reached yet in the same window instance count as crossed. Design: the evaluator stays pure and the caller then calls `AlertState::update(next, &prefs, &delivered, now)`, which records every level the reading reached (notified or not, so a crossing seen without a notice, on a first reading or with alerts off, never fires later), the seen recoveries, the re-armed levels and `resets_at` drift (followed reading by reading, so gradual drift stays one instance); `retain_accounts(keys)` forgets removed accounts. The state keeps SHA-256 digests of the account key and of account key + label, thresholds and dates; the first-reading set lives only in memory. Persisting it in `notified.json` is RF2. |
| RF2 | 0.43.0 | `AlertState` is now serialized (marks only: SHA-256 digests of the account key and of account key + label, kind, threshold, `resets_at`, notified flag and date; the first-reading set stays in memory) inside the PRD 0003 `Ledger` (`Ledger::alerts`, `alerts_mut`; the `alerts` key of `<OS local data dir>/cuotai/notified.json`, omitted while empty), and `Ledger::prune` also drops marks whose window and date are more than `LEDGER_KEEP` (8 days) old (`AlertState::prune`). `AlertState::update` and `retain_accounts` say whether the persisted part changed, so the file is only rewritten then. `RefreshCycle::finish_account`/`finish_full` now return whether the result was accepted. The desktop backend (`backend.rs` `finish_account`/`finish_full`) judges each report against the reading it replaces (`last_reading`: the current report or the preserved/restored one, so the PRD 0006 restored reading is the `prev` of the first reading after opening) with `Notices::assess` before the cycle applies it, and calls `Notices::record` (core `AlertState::update`, ledger save, queue) only when the cycle accepted it; a stale result is neither delivered nor remembered, and nothing is recorded while alerts are off. `Notices::tick` (the PRD 0003 timer, `run_notices`, nudged by every view emission) delivers the queue through the PRD 0003 `Notifier`: one account's events become one notification, at most one notification per `alerts::NOTICE_SPACING` (1 min, §6), the excess grouped into the next ("Cuotai: 2 usage alerts"), dropping what is no longer wanted (alerts off, bell or metric muted, account removed); a refused delivery is not retried. `alerts::notice(groups, now, tz)` writes the `Localized` text: "Claude: 90% of the 5-hour window used" with "Work: Session (5h) · resets 14:30" (local time, with the date when not today), "… quota exhausted", "… quota available again", the metric label when there is no known window, "Claude: 2 usage alerts" with one item per metric, and the cross-account form; only provider, account name, metric labels and thresholds, `alerts.*` ids in the core `en-US`/`es` catalogs with Portuguese fallbacks. Scheduler tests with a fake clock and fake deliverers cover one notice per threshold and window with oscillation, a restart that does not resend (and that would without the persisted state), grouping by account and the one-per-minute spacing, alerts turned off midway, a muted bell or metric, permission denied and a refused delivery, and a removed account. |
| RF3 | 0.43.0 (partial) | **Settings → Notifications** gains **Usage alerts** (off by default), the thresholds as editable chips (click to remove, add from a 10–99 field; saved at once, a refused value is put back), **Notify exhaustion** and **Notify recovery**, stored leniently under `alerts` in `desktop.json` (`settings::AlertSettings`; never in `cuotai.toml`); the core `AlertPrefs` gains `exhaustion` (off: reaching 100% notifies the highest threshold instead), `recovery`, `muted_accounts`, `muted_metrics` and `wants`. The switch reuses the PRD 0003 permission request (`Notices::switch_alerts`, on only when the system answers `granted`), its refusal guidance and its test button (available, and accepted by the backend, while reset notices or usage alerts are on). The PRD 0003 account bell (shown while either is on) also mutes the account's alerts; each eligible metric (`alerts::eligible`) gets its own bell in the details (`set_metric_alert`, `muted` keyed by `Account::key` and label); saving or removing accounts prunes both and forgets the account's alert state and pending alerts. The view carries `notify.alerts`, `usage_alerts` (labels with an active alert, `AlertState::active`: a level notified in the current window and not re-armed) and per metric `alert { muted, active }`; the card (`usage-alert` beside `exhausted`), its "Usage alert: …" chip, the tray popover rows, tiles and footer, the tray menu ("▲") and the details' metric highlight the account. Texts in pt-BR, en-US and es. **Missing (owner):** the smoke on the packaged macOS app with a human granting and denying the system permission, a real alert arriving with the window hidden, custom thresholds and preferences surviving a restart there, and the same on Windows and Linux. |
| RF4 | — | Not delivered. |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

PRD 0003 deals with the **reset** notice of a quota and explicitly leaves "consumption notices (below 20%)" to their own PRD. Today the only indication of approaching the limit is visual: `metrics::classify_report` marks an account as `Exhausted` and the cards get the `exhausted` class on the desktop. Nothing warns before the quota runs out, nor while the app is hidden in the tray.

PRD 0003 prepares the reusable infrastructure: `Notifier` behind a trait, a notification plugin with permission, a scheduler in the Tauri backend, anti-duplication persistence in `notified.json` and the **Settings → Notifications** section. In the 2026-10-19 market analysis, several competitors notify by threshold: Usagebar at 50/75/90% and QuoPeek with configurable thresholds and repetition.

## 2. Problem

People working with 5-hour and weekly windows find out the quota ran out when the work stops. Alerting naively creates noise:
- a metric oscillates around the threshold on every query;
- several metrics of the same account cross thresholds together;
- restarting the app would resend old notices;
- metrics without a cap (balance, spend) have no "percentage" and cannot fire.

## 3. Goals

- Notify, once per window and per threshold, when the consumption of a metric with a cap crosses a threshold chosen by the user.
- Notify when the quota is exhausted and when it has slack again in the next window, without repeating after an app restart.
- Reuse the deliverer, the permission and the storage of PRD 0003, with no new provider query.

## 4. Non-goals

- The reset notice (PRD 0003) and a pace-based notice (PRD 0007), which may become a future option.
- E-mail, remote push, webhooks (PRD 0013) and notices with the app closed.
- Alerting about balance, accumulated spend or short rate limits.
- A "do not disturb" schedule mode in this version.

## 5. Functional requirements

### RF1 — Threshold evaluator in the core (high priority; prerequisite)

- **RF1.1** Create `alerts::evaluate(prev: Option<&Report>, next: &Report, prefs: &AlertPrefs, state: &AlertState, now) -> Vec<AlertEvent>`, a pure function with no internal clock. `AlertEvent` carries the account key, the label and window of the metric, the crossed threshold, the consumed fraction, `resets_at` and the kind (`Threshold`, `Exhausted`, `Recovered`).
- **RF1.2** Eligibility: `kind = Quota` with `MetricState::Available { uncapped: false }` or `Exhausted` with a positive cap. Metrics with no cap, `Balance`, `Credits`, `Spend`, `Usage` and `RateLimit` never fire. Missing or non-finite values are ignored, never treated as zero.
- **RF1.3** Thresholds configurable between 10 and 99% consumption (default 75 and 90). The exhaustion notice (100%) is always evaluated when alerts are on.
- **RF1.4** Identity and deduplication: the key is account + metric + window instance + threshold, with the same 2-minute `resets_at` tolerance as PRD 0003, RF1.3. A threshold fires once per window instance. Crossing several thresholds in a single query emits only the highest one.
- **RF1.5** Hysteresis: a threshold only re-arms when consumption falls 5 points below it or when a new window instance starts. Oscillating around the threshold does not produce repeated notices.
- **RF1.6** `Recovered`: when an exhausted metric gets slack in a new window, it emits a single notice, provided the user received that metric's exhaustion notice.
- **RF1.7** First reading: without `prev`, the evaluator does not emit a notice for a threshold already crossed in the past (avoids a burst when opening the app). It emits only if the reading is new and within 10 minutes of the known crossing (using PRD 0006's history, when present).
- **Acceptance criterion:** table tests with a fixed clock cover a metric with no cap, a missing value, crossing one and several thresholds, oscillation, re-arming by a fall and by a new window, `resets_at` drift, `Recovered`, the first reading and a removed account.

### RF2 — Persistence and delivery (high priority)

- **RF2.1** The state of the notices already emitted (keys and dates) is written to the same `notified.json` as PRD 0003, pruned after 8 days; digests only, never usage values, e-mails or account names.
- **RF2.2** The Tauri backend calls the evaluator after each `Report` applied to the `RefreshCycle`, not JavaScript. Delivery uses the PRD 0003 `Notifier` and groups into a single notification the events of the same account in the same cycle.
- **RF2.3** Content: "Claude: 90% of the 5 h window used · resets 14:30". It never includes e-mail, token, URL or provider notes. Texts in `pt-BR`, `en-US` and `es`.
- **Acceptance criterion:** scheduler tests with a fake clock and a fake deliverer: one notice per threshold and window, no repeated notice after restarting the app, grouping by account, alerts turned off midway and permission denied.

### RF3 — Preferences on the desktop (high priority)

- **RF3.1** In **Settings → Notifications**: a **Usage alerts** switch (off by default), a list of thresholds (editable chips from 10 to 99), **Notify exhaustion** and **Notify recovery**. Preferences in `desktop.json`, never in `cuotai.toml`.
- **RF3.2** Per account and per metric: mute an account (the bell of PRD 0003, RF3.2) and mute a specific metric in the details. Keys by the account's stable identity, pruned on removal.
- **RF3.3** Reuse PRD 0003's permission request and test button; this PRD does not duplicate the flow.
- **RF3.4** The tray and the card highlight the account with an active alert, keeping the current visual `exhausted` state.
- **Acceptance criterion:** a smoke test on the packaged app (macOS and, when available, Windows and Linux) with permission granted and denied, custom thresholds and preferences persisting after a restart.

### RF4 — Notice in the TUI (low priority; optional)

- **RF4.1** With `--notify` (the same flag as PRD 0003, RF4), the TUI shows the notice banner and rings `BEL` for evaluator events, with no persistence between sessions.
- **Acceptance criterion:** a tmux smoke test with a test metric crossing a threshold; `--once` and `--json` do not change.

## 6. Non-functional requirements

- **Privacy:** the notification text shows on the lock screen. Only the provider, account and metric names, never sensitive data.
- **Cost:** no alert triggers a query; notices only react to reports already obtained.
- **Noise:** at most one notification per account per cycle and one per minute in total, with the excess grouped.

## 7. Order of execution and verification

1. **RF1:** a pure evaluator with table tests; no visible change.
2. **RF2:** integration into the backend, shared persistence and grouping.
3. **RF3:** preferences, muting and a smoke test on the packaged app.
4. **RF4 (optional):** TUI.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when, with the app hidden in the tray, a metric that crosses 90% produces **one** notice per window, oscillating around the threshold does not repeat, restarting the app does not resend old notices, turning the alerts off cancels the pending ones, and no extra query is made to the provider.
