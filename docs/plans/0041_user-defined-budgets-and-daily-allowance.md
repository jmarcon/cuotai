# PRD 0041 — User-defined budgets, balance floors and daily allowance

| Field | Value |
|---|---|
| ID | 0041 |
| Status | Awaiting owner verification (2026-11-17 re-validation fixes in 0.86.10) |
| Owner | `cuotai-core` (`budget` module, synthetic metrics, allowance, validation), `cuotai-tui` (wizard field, details line, `--once`), desktop (account dialog, card chip, details) |
| Consumers | Every front end; `export::v1`, `--watch` and `/v1/*` consumers; PRD 0026 statistics |
| Depends on | PRD 0029 (budgets keyed by the stable account id), PRD 0023 RF1 (`Metric::billing`, so a budget on overage spend is possible), PRD 0033 (`measured_at`, for the cycle boundary) |
| Benefits from | PRD 0006 (history: the cycle start value), PRD 0007 (pace works on the synthetic quota), PRD 0008 and 0024 (alerts and projection work on it), PRD 0009 (binding), PRD 0030 (plan value: the plan price can prefill a budget), PRD 0036 and 0038 (aggregator spend and OpenRouter credits are typical targets) |
| Extends | PRD 0008 (alerts reach spend, balance and credits through the budget), PRD 0007 (a new `allowance` figure) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **budget** is a limit the person sets on a metric the provider reports without one (spend, usage, credits); a **floor** is the balance level below which the person wants to be warned; the **allowance** is how much of a quota can be used per day (or per hour) and still reach the reset.

## Implementation status (2026-11-07)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.83.0 | `budget` module: `[[accounts.budgets]]`/`[[accounts.floors]]` on `Account` (TOML round trips keep comments; `ConfigFile` writes array tables), `budget::validate`/`problems` in `Config::validate` and the editors (`ConfigError::Budget` with a localized note per rule). The kind check knows the shapes of a few providers only (`budget::reportable_kinds`); elsewhere a mismatch shows at reading time. |
| RF2 | 0.83.0 | `budget::apply` in `Monitor::refresh_account` (after the adapter, before history and alerts): derived quota with `derived_from`, local-midnight boundaries (DST-safe, tested in New York, Berlin and Santiago), provider-period, baseline and balance-drop derivations, `note.budget.*` notes, restored readings re-marked. The budget metric's `billing` is always `included` (so every engine treats it as a plan limit); a top-up raises the baseline by its amount instead of resetting it, so use before it still counts; no journal entry for top-ups. |
| RF3 | 0.83.0 | `AlertKind::BalanceBelowFloor`/`BalanceRecovered` (10% hysteresis, top-up re-arm, quiet hours like a threshold, private texts), `budget::floor_statuses` chips and lines, `balance_below_floor`/`balance_recovered` events (floors travel in `AccountExport.floors`). |
| RF4 | 0.83.0 | `pace::allowance` with `vs_recent`, `Allowance::sentence` (`pace.allowance.*`), desktop details, TUI details, `--once` `Allowance:` and `export::v1` `allowance`. |
| RF5 | 0.83.0, 0.84.0 | Desktop **Budgets and floors** section with core validation and the live sentence, **Set a budget…**/**Set a floor…** quick actions (0.83.0). 0.84.0: RF5.2, the TUI wizard's **Budgets** step after **Group** (`a` add a budget, `f` a floor, `e` edit, `x` remove; ←→ choices, typing fills a field and filters the metric picker), drawn from the new core `budget::editor` rows (`BudgetRow`/`FloorRow`, `targets` with type-to-filter, `starts`, the texts and the parsing), checked by `budget::problems` before the step moves on and before the save; the review lists the budgets. Tests: the core rows round trip and the filter, the step's add/edit/remove/limits, and the wizard writing the rows while keeping the rest of the account. |
| RF6 | 0.84.0 | `budget::editor::suggest`: **Set a budget…** asks the desktop command `budget_suggestion`, which prefills the amount and the cycle day from the account's PRD 0030 plan price when it is in the metric's currency (labelled "From your plan price… Check it before saving.") and adds the "Last 30 days: $38.20" reference line from 30 days of history for a spend or usage metric, never as the amount. Nothing is written until the dialog is saved; without an answer the card's own draft is used. Tests: prefill only in the same currency, cycle day, the reference line never becoming the amount. |
| RF7 | 0.83.0 | `cuotai.example.toml`, `docs/advanced.md`, `docs/gui-features.md`, `docs/tui.md`, `docs/export-v1.md` (+ schema), `docs/events.md`. |
| Open question | 0.83.0 | `advisory = true` (keep a budget out of binding and `best`) is not implemented: every budget counts, the recommended default. Decision of 2026-11-07 (owner said not to stop and ask): kept out of this delivery, because `[[accounts.budgets]]` rejects unknown keys, so a new `advisory` key would make the TOML unreadable for 0.83.x; it needs the PRD 0029 RF0 tolerance first, as `budgets` itself had. |
| PRD 0038 re-validation | 0.86.8 | RF2.3 on a real store: a balance budget read its baseline through the history, whose readers dropped balance readings (`used` is null), so in production every credits budget on a balance said `note.budget.no_baseline`; the unit test hid it with a fake store. The readers now return `remaining` for balance-shaped readings (PRD 0006 row), and the budget test reads a real `HistoryStore`, plus a `Monitor`-level test deriving `Drops`. |
| 2026-11-17 re-validation | 0.86.10 | Fixes of RF1.2, RF2.1–RF2.5, RF3.1, RF3.2, RF4.1, RF4.2, RF5.1 and RF5.2 (1 high, 11 medium, 15 low findings); see "2026-11-17 re-validation" below. |
| Owner verification | — | §11 smoke on a real `open_ai_api` account (a $50 monthly budget shows the budget quota on the card, the 75% alert when crossed and the allowance in the details) and one **Set a budget…** with a plan price saved, on the packaged desktop app (§2.1 of `0000_control.md`). |

### 2026-11-17 re-validation (0.86.10)

The re-validation (Visual, Security, Review) found 1 high, 11 medium and 15 low findings; the fixes ship in 0.86.10, one commit per group:

| Group | Findings | RF | Delivery (0.86.10) |
|---|---|---|---|
| G1 | F-01 (high), F-16 | RF2.2, RF2.3, RF4.1 | Reset-aware counting: `HistoryStore::readings` (stored `resets_at`, balances read back as `remaining`, bounded at `now`) and `budget::counted_use` add the use segment by segment, so a counter that restarts at the provider's reset inside the budget period keeps counting (`GAP` = 1 h). A stretch whose use cannot be known (the first reading more than an hour after the period start, no reading within an hour before a reset or a top-up, a counter that went down inside one provider instance) makes the result a lower bound: `derived_from.partial`, `remaining` null, `note.budget.partial`, no allowance, "≥ " before the used value on every surface; alerts still fire. A provider spend below zero is unknown (`note.budget.negative`), never clamped to 0. |
| G2 | F-02, F-03, F-04, F-26 | RF1.2, RF2.1, RF2.4, RF3.1 | Every budget and floor has a derived identity (`budget_key`, `floor_key`; `derived_from.key` in the export) that keys its alert marks and its history series, with lazy migration of unambiguous legacy marks and series. Default labels name the period ("Monthly budget · …", "Weekly budget · …"). New rules: no two budgets on one target and period whatever their unit, no two budgets with the same label, no two equal floors. Floors are tiers (see decisions). A restored reading never shows a removed budget as a provider quota. |
| G3 | F-06, F-09, F-20 | RF2.1, RF2.4, RF2.5 | Budget notes name the budget through its translated label; the period reads "since Tue" / "since 10/01" (`budget.period_since`, `derived_from.period_start` and `period_text`, `Metric::period_text()`) in `--once`, the TUI and the desktop; the binding line names a budget ("most restrictive: … (your budget) at 90%", "used up until …"). |
| G4 | F-07 | RF1.2 | A budget whose unit matches no metric while one of its kind exists in another unit reads "reported in USD; this budget is in EUR" (`note.budget.unit_mismatch_named`); the editors refuse it against the account's last good reading (`BudgetRule::UnitMismatch`). |
| G5 | F-05, F-10, F-11, F-15 (command) | RF1.2, RF5.1, RF5.2 | `text::number::parse_amount`: the editors read amounts the way the language writes them and refuse an ambiguous one such as `1,000`; `--cost-plan` and `--cost-currency` use the same parser. Problems name the field by its label and sit under it (`aria-invalid`, `aria-describedby`); a new row's empty amount stays quiet until typed or saved; new rows prefill the unit. The desktop `check_budgets` is async and takes the account index, text rows and the locale. |
| G6 | F-08, F-12, F-21 | RF2 | The desktop details and the main card keep a budget's own label and tag ("Your budget"); a budget no longer takes a provider's 5-hour or weekly column; the TUI card line starts with "◇ "; the Win98 budget meter outline stays inside its panel. |
| G7 | F-14, F-15, F-19 | RF1.2, RF3.2 | `Monitor::apply_budgets` ends with the report filter, and budget labels, units and filters refuse invisible direction characters (`budget.error.characters`). An invalid budget or floor is left out with `note.budget.ignored` while the account and the other entries keep working; `Config::from_toml` (strict) and `--check-config` still exit 1. A floor with no balance to watch says so (`note.budget.floor_missing`). |
| G8 | F-13, F-17, F-18 | RF4.1, RF4.2 | The allowance says "less than 0.1%/day" or "less than $0.01/day" instead of 0, shows nothing once a budget is used up, and gives a money budget's recent pace in money (the percent `…_recent` ids are retired). |
| G9 | F-23, F-25 | RF1.2, RF5.1 | The demo's `?budget` fixture (budgets, a partial budget, two floors on one balance, a currency-mismatch note, a "less than" allowance), faithful `check_budgets`/`budget_suggestion` stubs, the UI-check pages `details-budget`, `details-floor`, `main-budget`, `dialog-budget` and `dialog-floor`, and one desktop test per editor rule. |

**Decisions (orchestrator, recorded for the owner):**
- **Identity derived, not written:** `Budget` and `Floor` reject unknown keys, so a written `id` would make the TOML unreadable to 0.83–0.86 on a downgrade. The key is derived from what validation already makes unique (kind or scope, folded `label_contains`, period, row; floors also unit and amount), so editing a budget's amount, label, unit or start day keeps its alerts and series. A written id needs PRD 0029 RF0 tolerance inside budget tables one release earlier.
- **Floors are tiers:** each floor has its own mark and 10% hysteresis; one reading sends at most one `BalanceBelowFloor` per balance, naming the lowest floor crossed, and marks every crossed floor; `BalanceRecovered` is sent once, when every marked floor has re-armed. An engine-only `AlertEvent::mark_key` (not serialized) carries the mark through the claim flow.
- **V-06:** a floor with no mark and the balance already below it warns once at the next reading (also after its amount changes), then the usual hysteresis applies.
- **Partial means a lower bound:** `used` is at least that much, `remaining` is null, there is no allowance, and thresholds still fire. A provider `resets_at` that only rolls forward with the clock is not a reset.
- **Ambiguous amounts:** one separator followed by exactly three digits ("1,000", "1.000") is refused in every locale unless spaces already group the digits or the whole part is 0; otherwise the last separator is the decimal. The CLI parses in en-US.
- **Entry-level tolerance (PRD 0051 principle):** an invalid budget or floor is left out with a visible note, the TOML keeps it byte for byte, and strict mode still fails; entries beyond the limits are left out one by one.
- **Binding wording:** a used-up budget reads "used up until …", not "blocked", since going over your own budget does not block the provider.

**Deviations, documented:**
- **RF2.4:** the reset notice of a budget is the generic reset notice with the budget's label; there is no separate "Budget period starts again" text.
- **§7:** the budget history series is keyed by the budget's identity (`digest(("budget", key))`), not `budget:<index>`; the export carries the allowance as an account `allowance[]`, not `Metric.allowance`.
- A budget restored from the history loses `partial` (its stored `remaining` is null, so it still shows no amount left).
- The unit is prefilled from the metric but not locked; the mismatch rule blocks the save instead.

**Open items:**
- **F-22 (low, deferred):** the floor notification body omits the amounts; it needs an `AlertEvent` field across about 25 constructors, the channel payloads and the notification-length budget. The title says "below your floor" and the details show both amounts.
- **F-24 (low, deferred):** the wizard's Budgets step is reachable only through the Credential step; a debug-only `--wizard-step budgets` jump is suggested for PRD 0053-style tooling (the logic is covered by `wizard.rs` tests).
- **Decided (owner, 2026-11-20), delivered in vNEXT:** a partial budget's desktop meter and its share left (the card figure and the details "% available") are dimmed to 50 % (`docs/design-system.md` DS-CMP-HON-5; the stale treatment alone when the reading is also stale).
- **Decided (owner, 2026-11-20), confirmed in vNEXT:** `--cost-currency BRL 5.437` (exactly three decimals) stays refused as ambiguous, with "type 5437 or 5.4370"; `tests/cost_amounts.rs` now covers it.
- **Decided (owner, 2026-11-20), delivered in vNEXT:** two budgets whose default labels are the same (different `label_contains` resolving to the same target and period) get " (2)", " (3)"… in the TOML order (`budget::numbered_labels`, account budgets and each row's budgets apart), so muting by label and event grouping keep them apart; `metric_text` renders the number around the translated label (`metric.label.budget_numbered`), and a restored reading finds its numbered budget again.
- A budget whose target metric is missing is named by its kind slug ("Monthly budget · usage") in every language.
- Native review of the new catalog strings (listed in each catalog's `pending_review`).
- The allowance sentences have no class in the text length budgets (`text/budget_tests.rs`); the new "less than" and money-pace forms are covered by goldens in en, de, ru and ja instead.

**Awaiting owner verification (2026-11-17 checklist):**
1. §11 smoke on a real `open_ai_api` account, packaged desktop app: a $50 monthly budget on the spend shows "Monthly budget · …" with "Your budget" and the outlined meter, one 75% alert when the spend crosses $37.50, and "≈ $…/day left in the budget" in the details.
2. **Set a budget…** with a plan price saved (Settings › Advanced › Local cost): the amount and day come from the plan ("From your plan price…") with the unit filled in; after saving, the TOML has `[[accounts.budgets]]` and its comments are unchanged; a plan in another currency does not prefill.
3. A weekly budget across the 1st on a monthly-spend account with the app running over midnight UTC keeps the week's spend (no drop to $0). With the app closed from the 31st to the 2nd it shows "≥ $…" and "part of this period's use was not recorded".
4. A credits or balance budget on OpenRouter or Kimi counts the drops; a top-up shows the top-up note and does not reduce the use.
5. A $10 floor set while the balance is $3.20: exactly one notification at the next refresh, none afterwards, and one "back above your floor" once the balance passes $11.
6. Floors of $10 and $5 on a balance hovering around $10: one notification; falling below $5 later gives a second one.
7. Desktop editor in en-US: `1,000` is refused under the Amount field naming both readings; `1000` saves `amount = 1000`. In pt-BR, `12,5` saves `amount = 12.5`.
8. An EUR budget on a USD spend in the editor: the save is refused, naming USD and EUR.

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
- **History:** series key `budget:<budget index>` per account id; a removed budget stops recording and its series ages out with the retention. *(2026-11-17 re-validation: the series is keyed by the budget's derived identity, `digest(("budget", key))`, so it survives reordering; the export carries the allowance as an account `allowance[]` rather than `Metric.allowance`.)*

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

- **TOML tolerance for `budgets` and `floors` ships in the current release: yes.** The keys are accepted and ignored by the release that precedes this PRD (PRD 0029 RF0 tolerance list), so the version that delivers budgets is downgrade-safe; delivered in 0.81.0 (the account table accepts and ignores the key).
- Scheduled for the next version, not this release.
