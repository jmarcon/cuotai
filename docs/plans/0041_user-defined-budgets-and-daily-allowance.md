# PRD 0041 — User-defined budgets, balance floors and daily allowance

| Field | Value |
|---|---|
| ID | 0041 |
| Status | Planned (next version) |
| Owner | `cuotai-core` (`budget` module, synthetic metrics, allowance, validation), `cuotai-tui` (wizard field, details line, `--once`), desktop (account dialog, card chip, details) |
| Consumers | Every front end; `export::v1`, `--watch` and `/v1/*` consumers; PRD 0026 statistics |
| Depends on | PRD 0029 (budgets keyed by the stable account id), PRD 0023 RF1 (`Metric::billing`, so a budget on overage spend is possible), PRD 0033 (`measured_at`, for the cycle boundary) |
| Benefits from | PRD 0006 (history: the cycle start value), PRD 0007 (pace works on the synthetic quota), PRD 0008 and 0024 (alerts and projection work on it), PRD 0009 (binding), PRD 0030 (plan value: the plan price can prefill a budget), PRD 0036 and 0038 (aggregator spend and OpenRouter credits are typical targets) |
| Extends | PRD 0008 (alerts reach spend, balance and credits through the budget), PRD 0007 (a new `allowance` figure) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **budget** is a limit the person sets on a metric the provider reports without one (spend, usage, credits); a **floor** is the balance level below which the person wants to be warned; the **allowance** is how much of a quota can be used per day (or per hour) and still reach the reset.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1–RF7 | — | Proposed. |

## 1. Context

Cuotai alerts, paces and ranks **quotas**: metrics with a limit and a reset. Half of the catalog reports something else: a monthly spend in dollars with no cap (`open_ai_api`, `anthropic_api`, `aws_bedrock`, `azure_open_ai`, `mistral`, `grok`), a prepaid balance that only goes down (`kimi`, `cline`, `open_router` credits, `nine_router` upstreams), or a usage count with no plan (`groq`, `gemini_api`). `docs/gui-features.md` states the consequence plainly: "Balances, spend, rate limits and missing values never alert."

Competitors close that gap with a person-set number. Fetchy lets API-key accounts "set daily, weekly, or monthly token budgets" next to the remaining balance ([App Store listing, read 2026-10-25](https://apps.apple.com/app/id6786673900)); LiteLLM has soft budgets per key and team with a Slack alert at a threshold ([LiteLLM docs, alerting](https://docs.litellm.ai/docs/proxy/alerting.md)); OpenRouter recommends pairing a per-key credit limit with the Activity dashboard and exposes `usage_daily`, `usage_weekly`, `usage_monthly` and `limit_remaining` per key ([OpenRouter, team spend controls, read 2026-10-25](https://openrouter.ai/blog/tutorials/team-spend-controls-setup/)). WhatsMyUsage shows "an estimated dollar total" per month ([Setapp listing](https://setapp.com/apps/whatsmyusage)).

The second half is the **allowance**. Cuotai says "runs out at 16:40" and "8 points in reserve"; it does not say the number people actually plan with: "you can use 12% a day (about 2 h of coding) and still make the weekly reset".

## 2. Problem

- A person paying per token has no way to be told "you have spent $40 of the $50 you meant to spend this month" by the app whose job is exactly that.
- A prepaid balance that silently reaches zero stops an agent mid-task; today Cuotai shows the balance but never warns.
- Spend with no cap takes part in nothing: no pace, no binding, no stats, no `best`.
- The pace tells people whether they will make it, not **how much they may use per day** to make it, which is the actionable number for a weekly or monthly window.

## 3. Goals

- A budget on any `Spend`, `Usage` or `Credits` metric (or on the whole account's spend) that becomes a **synthetic quota**: `used`, `limit`, `remaining`, `window`, `resets_at`, so history, pace, threshold alerts, exhaustion, projection, binding, stats, groups and events work unchanged.
- A floor on `Balance` and `Credits` metrics that alerts when the remaining amount drops below it, with the same notification policy as the other alerts (PRD 0032).
- A **daily (or hourly) allowance** for every quota with a reset, in the core, shown as one chip and one sentence.
- Budgets are **the person's numbers**, stored in the accounts TOML (shared by TUI and desktop, no secret), validated at the boundary, and never sent anywhere.

## 4. Non-goals

- Enforcing a budget: Cuotai never blocks a key, changes a provider limit or edits a provider setting; it reads and warns.
- Converting currencies to compare a budget with a spend in another currency (PRD 0030's display-only rate stays display-only); a budget must be in the metric's currency.
- Budgets across accounts ("$100 for all my APIs"): a later extension of the `[[groups]]` table (§12).
- Token budgets on local-log cost (PRD 0015): that estimate is not a provider reading; PRD 0042 covers blocks and sessions.
- Guessing a budget from history or from the plan price without the person confirming it (RF6 only prefills).

## 5. Functional requirements

### RF1 — Budget definition and validation (high priority; prerequisite)

- **RF1.1** An account may carry up to **8** `[[accounts.budgets]]` entries: `metric` (a selector: `kind = "spend" | "usage" | "credits"` plus optional `unit` and `label_contains`; or `scope = "account"` for the account's total spend when the adapter reports one), `amount` (positive finite number), `unit` (ISO 4217 code for money, or the metric's unit), `period` (`daily` | `weekly` | `monthly` | `window`), `starts` (`monday`..`sunday` for weekly, `1`..`28` for monthly; the provider's own cycle when `period = "window"` and the metric has `resets_at`), optional `label` (≤ 32 characters).
- **RF1.2** `Config::validate` and the draft validation of `account_edit.rs` reject: a selector that matches no metric kind the provider can report, a unit that is not the metric's unit, an amount ≤ 0 or above 1 000 000 000, more than 8 budgets, two budgets with the same selector and period, and `period = "window"` on a metric that never has `resets_at` (the access descriptor knows the provider's metric shapes; when it does not, the budget is accepted and the mismatch surfaces at reading time as a note, never as zero).
- **RF1.3** Budgets are preserved by `ConfigFile` round trips with comments, keyed in memory by the stable account id (PRD 0029), and a rename never loses them.
- **Acceptance criterion:** TOML round trips, the rejection table above with the localized messages, and a desktop draft validation test for each rule.

### RF2 — Synthetic budget quota in the core (high priority)

- **RF2.1** `budget::apply(report, budgets, history, now) -> report` runs in `refresh.rs` after the adapter and before classification. For each budget it finds the target metric and emits one **additional** metric: `label` = the budget label or "Budget · <metric label>", `kind = Quota`, `unit` = the budget unit, `limit = amount`, `window` from the period, `resets_at` = the next boundary of the period in the **person's local time zone** (midnight; Monday 00:00 for weekly starting Monday; day `starts` 00:00 for monthly; the provider's `resets_at` for `window`), `billing` = the target's billing, `derived = true` (new flag, §7).
- **RF2.2** `used` is the target's value consumed **in the current period**: when the provider's metric already covers the same period (monthly spend for a monthly budget starting on the 1st, UTC month accepted with a note), `used` = the provider's `used`; otherwise `used` = provider's current cumulative value minus the value recorded in the history at the period's start (PRD 0006 `series`, the first sample at or after the boundary). Without a sample at the boundary the metric carries `used = None` and the note `note.budget.no_baseline` ("Budget counting starts with the first reading of this period"), never `0`.
- **RF2.3** A provider value that **decreases** (a prepaid credit balance): `used` = baseline − current, clamped at 0; a top-up (current > baseline) resets the baseline to the new value with the note `note.budget.topup_detected` and a journal event (PRD 0026).
- **RF2.4** The synthetic metric is a plan limit for every generic consumer: `metrics::binding`, `pace::assess`, `alerts::evaluate` (thresholds, exhaustion, recovery), PRD 0024 projection, `resets::upcoming` (reset notice "Budget period starts again"), history (recorded as its own series keyed `budget:<id>`), stats and `best`. The target metric itself is unchanged.
- **RF2.5** When the target metric is missing from a reading, the budget metric is emitted with `used = None` and the note `note.budget.target_missing`; it never alerts and never shows `0`.
- **Acceptance criterion:** pure tests for every period boundary (including DST transitions in two zones), the three `used` derivations, the top-up rule, the missing target, and an end-to-end test that a monthly $50 budget at $45 spend produces the 90% threshold alert and shows "Runs out ≈ 27th" from pace.

### RF3 — Balance floors (high priority)

- **RF3.1** An account may carry up to **4** `[[accounts.floors]]`: `metric` selector on `balance` or `credits`, `amount` (> 0), `unit`. The core emits no extra metric; `alerts::evaluate` gains `AlertKind::BalanceBelowFloor` (once per crossing; re-armed after the balance rises above `amount × 1.1` or a top-up) and `AlertKind::BalanceRecovered`.
- **RF3.2** The card chip reads "Balance low · $3.20 left" (role `warn`; `bad` when ≤ 25% of the floor), the details show the floor line under the balance bar, and the notification follows PRD 0032's policy and private texts ("OpenRouter: balance below your floor").
- **RF3.3** `--watch` emits `balance_below_floor` / `balance_recovered` (additive events); hooks can react.
- **Acceptance criterion:** evaluator tests (crossing, hysteresis, top-up), the chip roles, the event diff, and the quiet-hours interaction (a floor crossing is held like a threshold, not like an exhaustion).

### RF4 — Daily allowance (high priority)

- **RF4.1** `pace::allowance(metric, now) -> Option<Allowance>` for any quota with `limit`, `used` and `resets_at`: `remaining / days_until_reset` (hours when less than 2 days remain), with `unit` and the `per: Day | Hour` granularity, plus `vs_recent: Option<f64>`: the ratio to the recent average use per day (from the history slope PRD 0024 already computes), so the sentence can say "you can use 12% a day · you have been using 18%".
- **RF4.2** Core sentences (DS-CMP-TIME, HON): "≈ 12%/day to the reset", "≈ $3.40/day left in the budget", "≈ 4%/h to the reset" when under 2 days; no sentence when the window is `Window5h`, a rate limit, or `remaining` is unknown.
- **RF4.3** Surfaces: a chip on the desktop details under the pace mark, a line in the TUI details, a `Ritmo:`-style `Allowance:` line in `--once`, and `allowance` in `export::v1` (additive).
- **Acceptance criterion:** pure tests across boundaries (reset in 30 min, 1 day, 6 days, unknown), the hour/day switch, `vs_recent` with and without history, the golden sentences in every catalog.

### RF5 — Editors (medium priority)

- **RF5.1** Desktop account dialog: a **Budgets and floors** section (DS-CMP-LIST, up to 8 + 4 rows) with the metric picker filled from the provider's reportable metric kinds, amount, unit (prefilled from the metric), period, start, label; inline validation from the core; a live sentence "Cuotai will show a Budget quota of $50 a month, starting on the 1st".
- **RF5.2** TUI wizard: the same fields after the account fields, with type-to-filter on the metric picker.
- **RF5.3** Quick action from a card: on a spend or balance metric without a budget, the details show **Set a budget…** / **Set a floor…** that opens the dialog at that row.
- **Acceptance criterion:** desktop and TUI editor tests (add, edit, remove, validation), the TOML written matches RF1, unrelated content preserved.

### RF6 — Prefill from the plan price and from history (medium priority)

- **RF6.1** When PRD 0030 holds a plan price for the account, **Set a budget…** prefills the amount and cycle day from it, labelled "from your plan price"; the person confirms.
- **RF6.2** Otherwise the dialog may suggest "last 30 days: $38.20" from the history as a reference line, never as a prefilled amount.
- **Acceptance criterion:** prefill and suggestion tests; a budget is never written without an explicit save.

### RF7 — Documentation (low priority)

- `cuotai.example.toml` gains a commented `[[accounts.budgets]]` and `[[accounts.floors]]` block; `docs/advanced.md` ("Budgets, floors and allowance"), `docs/gui-features.md`, `docs/tui.md`, `docs/export-v1.md` (`derived`, `allowance`, new events) and `docs/events.md` are updated in the same delivery.

## 6. Non-functional requirements

- **Honesty:** a budget never invents a provider value; `used = None` with a note whenever the baseline or target is unknown. The synthetic metric is labelled as the person's budget (`derived = true`) on every surface and in the export.
- **Determinism:** boundaries in the person's local zone with DST-safe arithmetic (`chrono-tz`), tested in at least three zones.
- **Performance:** `budget::apply` is O(budgets × metrics) per report and reads at most one history sample per budget per period (cached per boundary).
- **Privacy:** budgets live in the TOML (no secret), floors too; nothing is sent anywhere; notification texts follow PRD 0032 private mode.

## 7. Data and contract changes

- **TOML:** `[[accounts.budgets]]` and `[[accounts.floors]]` (additive; older versions reject unknown keys as today, which PRD 0029 RF0's tolerance policy should be extended to cover before release, §12).
- **`Metric`:** new `derived: bool` (default `false`, skipped when false) and, inside the export, `derived_from: { kind, label }`.
- **`export::v1` (additive):** `Metric.derived`, `Metric.allowance { amount, unit, per }`, account `floors[]`; events `balance_below_floor`, `balance_recovered`; the budget metric appears as a quota.
- **History:** series key `budget:<budget index>` per account id; a removed budget stops recording and its series ages out with the retention.

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | `budget` module, `pace::allowance`, alert kinds, validation, export, events |
| Desktop | Account dialog section, card chip, details lines and quick actions |
| TUI/CLI | Wizard fields, details and `--once` lines, `--json-v1` |
| Docs | example TOML, advanced, gui-features, tui, export-v1, events |

## 9. Dependencies and sequencing

RF1 and RF2 first (they are the value); RF4 is independent and can ship with RF2; RF3 with the alert kinds; RF5 after RF1; RF6 after PRD 0030 RF1. One lane; it touches `refresh.rs` and `alerts.rs`, so coordinate with PRD 0024/0032 follow-ups.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Period boundary disagrees with the provider's (UTC month vs local month) | Note on the metric, `period = "window"` to follow the provider, documented |
| Baseline missing after install or history erase | `used = None` with the note; counting starts at the next period or at the first reading |
| People confuse the budget bar with a provider limit | `derived = true` on every surface, "Budget" label, a different meter outline (DS decision) |
| Alert noise from a floor with a bouncing balance | 10% hysteresis and PRD 0032's grouping |
| Config rejected by older versions | Extend PRD 0029 RF0's tolerant keys to `budgets`/`floors` one release earlier |

## 11. Order of execution and verification

1. **RF1**, **RF2**, **RF4** (core), with `export::v1` fields.
2. **RF3** (floors and events).
3. **RF5**, **RF6** (editors), **RF7**.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests; a smoke with an `open_ai_api` account and a $50 monthly budget shows the budget quota on the card, the 75% alert when crossed, and "≈ $1.20/day left in the budget" in the details.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Budgets in the TOML or in `desktop.json`? | TOML: they are shared by the TUI, carry no secret and travel with the accounts file (PRD 0046 backup). |
| Cross-account budgets | Later, as `[[groups]].budget` once per-account budgets have been used for a release. |
| Should a budget's exhaustion count as "blocked" for `best` and the binding indicator? | Yes by default (the person chose that limit), with a per-budget `advisory = true` opt-out that keeps it out of binding and `best`. |
| Allowance granularity | Day above 2 days to the reset, hour below; no week granularity. |
| Tolerant keys for older versions | Add `budgets` and `floors` to the PRD 0029 RF0 tolerance list in the release before this PRD ships. |

### Owner decisions (2026-10-25)

- **TOML tolerance for `budgets` and `floors` ships in the current release: yes.** The keys are accepted and ignored by the release that precedes this PRD (PRD 0029 RF0 tolerance list), so the version that delivers budgets is downgrade-safe; delivered in vNEXT (the account table accepts and ignores the key).
- Scheduled for the next version, not this release.
