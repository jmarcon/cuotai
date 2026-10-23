# PRD 0009 — Binding limit indicator

| Field | Value |
|---|---|
| ID | 0009 |
| Status | Implemented |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (tray, cards, popover), `crates/cuotai-tui` (`dashboard.rs`), PRDs 0007, 0012 and 0014 |
| Depends on | none |
| Extended by | PRD 0023 (overage and model-scoped metrics excluded from the binding metric, MET-01), PRD 0026 (reset timeline), PRD 0033 (the card chip and the pace line each name their metric), PRD 0036 (binding per aggregator sub-account) |
| Date | 2026-10-19 |

## Implementation status (2026-10-19)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.22.0 | `cuotai_core::metrics::binding(report) -> Option<Binding>` (`index`, `label`, `window`, `used_ratio`, `remaining`, `resets_at`, `reason` `Exhausted`/`Tightest`, `blocked_until`), eligibility from `classify_metric` with a finite positive cap; `RateLimit` only without an eligible `Quota`; blocked picks the exhausted metric with the latest return (a missing return wins and leaves `blocked_until` unknown); otherwise the smallest remaining fraction, ties to the shortest window then adapter order. `binding_ratio` and `cmp_binding` sort accounts with no value last. Table tests cover every listed case. |
| RF4 | 0.22.0 | `Binding::summary(format_at) -> Localized` with ids `binding.blocked_until`, `binding.blocked` (unknown return) and `binding.tightest`; known windows travel as nested `metric_window.*`, others as the metric label; `en-US` and `es` catalogs, Portuguese fallback; catalog test over every variant. |
| RF2 | 0.26.0 | The desktop `get_view`/`snapshot` `ReportView` carries the core `binding` (`metrics::binding`) and `binding_rank` (stable sort by `cmp_binding`, so ties and accounts without a value keep the manual order). The tray ring, list row, ring tooltip and native tray menu read `binding`; blocked accounts show "Blocked until {time}" (unknown return: "return time unknown", never "now"). The card shows a `Week · 100%` chip next to the state and marks the binding 5 h/week window or summary bar; the details list highlights the binding metric with the same chip and keeps every metric. Settings has an opt-in "Most restrictive first" order (also in the sort menu) stored as the local sort mode only; the TOML order is never rewritten. Strings in `pt-BR`, `en-US` and `es`. A backend test covers a 5 h free / week exhausted account (binding week, blocked until the week's return, ranked first); browser smoke in the three languages confirmed the ring and the chip show the week. |
| RF3 | 0.27.0 | The TUI card leads with the core `binding` metric (the others follow in adapter order; the card still shows two plus the "+N métricas" hint); without a binding the adapter order stays. The details add a `LIMITE` line with `Binding::summary` (local `HH:MM`, date when not today; `CUOTAI_LANG`) and mark the binding metric with "◆ limitante" while listing every metric. `--once` opens with one line per account: `Claude  bloqueada até 14:30 pela janela semanal` / `Work  mais restritiva: janela semanal em 62%` (unknown return: "horário de retorno desconhecido"); accounts without a binding print their status or class. `--json` and the exit codes are unchanged; the binding enters the versioned contract through PRD 0012 RF1.1 (`AccountExport.binding`) when `export::v1` exists. `TestBackend` tests cover the card order, the unchanged order without a binding and the single marker; a text snapshot covers `--once`. |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

An account can have several quota metrics at the same time (a 5-hour window, a week, a month, per-model limits). `metrics::classify_report` already returns `AccountClass::Exhausted` when **any** measurable metric is consumed, and `classify_metric` computes the state of each bar (`MetricState`). But there is no single notion of "which limit is blocking me right now". Each interface chooses what to highlight on its own: the desktop tray shows rings (donuts) and the TUI shows the metric list in the adapter's order.

WhatsMyUsage's README describes the problem concretely: bars that show only the 5-hour window read 0% while the account was blocked by the weekly limit at 100%. The app always shows the worst limit. CodexBar also prioritizes the "most restrictive general quota" in its widgets.

## 2. Problem

- An account's summary can show "free" while the account is blocked by another window.
- Each interface would have to reimplement the choice rule (forbidden by the project guidelines: business rules live in the core).
- Without a single value per account, the tray, ordering and widgets have nothing compact to display.

## 3. Goals

- Define in the core, per account, the **binding** metric (the one that restricts use the most right now) and the instant the account becomes usable again.
- Use the same result in the tray, the cards, the TUI, the CLI and the exports.
- Never hide a blocking metric nor invent slack when data is missing.

## 4. Non-goals

- Changing the existing classification (`AccountClass`, `MetricState`) or what each adapter reports.
- Summing or averaging metrics of different natures.
- Pace (PRD 0007) and alerts (PRD 0008), which consume this result but are not part of it.

## 5. Functional requirements

### RF1 — Choosing the binding metric in the core (high priority; prerequisite)

- **RF1.1** Create `metrics::binding(report: &Report) -> Option<Binding>`, a pure function, using the same eligibility as `classify_metric`. `Binding` is `Serialize` and carries: the metric's index and label, `window`, consumed fraction, `remaining`, `resets_at` and the reason (`Exhausted` or `Tightest`).
- **RF1.2** Eligibility: `kind = Quota` with a finite and positive cap and a finite signal (`used` or `remaining`). `RateLimit` enters only if no `Quota` is eligible in the account. `Balance`, `Credits`, `Spend` and `Usage` never enter.
- **RF1.3** Choice:
  - if some eligible metric is exhausted, the account is **blocked**: the result is the exhausted metric whose return (`resets_at`) is the latest, because the account is only usable again when every exhausted one resets; `blocked_until` receives that instant;
  - otherwise, the metric with the smallest remaining fraction (highest consumption) wins; a tie goes to the shortest window and then to the adapter's order;
  - a missing `resets_at` on an exhausted metric leaves `blocked_until` unknown, never "now".
- **RF1.4** `Unmeasured` accounts, or those with a `Status` other than `Ready`, return `None`; the account's state (error, authentication) remains the main information.
- **RF1.5** When sorting or summarizing several accounts, `binding_ratio(report)` provides the remaining fraction of the binding one; accounts with no value come last.
- **Acceptance criterion:** table tests: 5 h free and week exhausted (result: week, blocked, the week's return); two exhausted with different returns; none exhausted with three windows; a tie; rate limit only; metrics with no cap; missing and non-finite values; `used = 0, limit = 0` as an unknown sentinel.

### RF2 — Desktop (high priority)

- **RF2.1** A Tauri command that returns each account's binding metric together with the current state. The tray, in ring mode, uses the binding metric as each account's main ring value; the list mode shows "blocked until HH:MM" when applicable.
- **RF2.2** The card shows a chip with the label of the binding window ("Week · 100%") next to the state, and highlights that item in the metric list. The other metrics stay visible.
- **RF2.3** An optional "most restrictive first" ordering in Settings; the manual order (drag and drop, synced with the TUI) remains the default and is never overwritten.
- **Acceptance criterion:** a smoke test in three languages with an account blocked by the weekly window and free on the 5 h one; the ring and the chip show the week.

### RF3 — TUI and CLI (medium priority)

- **RF3.1** The account row on the dashboard shows the binding metric instead of the first metric; in the details, the binding one gets a marker.
- **RF3.2** `--once` prints a summary line per account: `Claude  BLOCKED until 14:30 (week 100%)` or `Claude  week 62%`. `--json` stays the same; the binding metric enters the versioned contract of PRD 0012.
- **RF3.3** Exit codes do not change.
- **Acceptance criterion:** `TestBackend` rendering tests and a text snapshot of `--once`.

### RF4 — Texts (medium priority)

- **RF4.1** The core emits `Localized` for the summary ("blocked until {at} by the {window} window", "most restrictive: {window} {used}%"), with stable ids and `en-US` and `es` catalogs.
- **Acceptance criterion:** a complete-catalog test.

## 6. Non-functional requirements

- **Honesty:** the binding metric never hides data; it is an emphasis, not a filter. A missing value does not become zero or "100% free".
- **Consistency:** every surface reads the same result from the core.
- **Cost:** no new query.

## 7. Order of execution and verification

1. **RF1 and RF4:** the function and the texts with table tests.
2. **RF2:** desktop.
3. **RF3:** TUI and CLI.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when an account with the 5-hour window free and the weekly one exhausted appears as blocked, with the correct return time, in the tray, the card, the TUI and `--once`, and no interface contains a choice rule of its own.
