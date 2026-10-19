# PRD 0008 — Usage threshold alerts

| Field | Value |
|---|---|
| ID | 0008 |
| Status | Proposed |
| Owner | `cuotai-core`, desktop (TUI optional) |
| Consumers | `apps/desktop`, `crates/cuotai-tui`, PRD 0013 (events) |
| Depends on | PRD 0003 (RF2 and RF3: `Notifier` deliverer, permission, `notified.json` and preferences); PRD 0006 is optional |
| Date | 2026-10-19 |

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
