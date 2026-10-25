# PRD 0023 — Paid extra usage (overage) detection and alerts

| Field | Value |
|---|---|
| ID | 0023 |
| Status | Proposed |
| Owner | `cuotai-core` (model, adapters, alerts, events, export), desktop and TUI |
| Consumers | `apps/desktop` (cards, details, tray, Settings → Alerts), `crates/cuotai-tui` (cards, details, `--once`, `--notify`), `export::v1` and `--watch` consumers, PRDs 0025 and 0026 |
| Depends on | none for RF1–RF3; PRD 0008 (RF1, RF2: `alerts::evaluate`, `AlertState`, delivery) for RF4; PRD 0013 (RF1: `events::diff`) for RF5; PRD 0012 (RF1: `export::v1`) for RF6 |
| Benefits from | PRD 0022 (UI refresh: card, badge and callout visuals), PRD 0009 (binding metric), PRD 0010 RF4 ("Recharging" group, for RF10) |
| Extends | PRD 0008 RF1 (new alert kind), PRD 0009 RF1 (binding excludes overage and model-scoped metrics), PRD 0010 RF4 (scoped exhaustion never sends an account to "Recharging"), PRD 0012 RF1 (additive `overage` object, `billing` and `scope` on metrics), PRD 0013 RF1.2 (two new events) |
| Date | 2026-10-22; amended 2026-10-23 (review item MET-01; owner decisions L and R) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. "Overage" (or "extra usage") means consumption that the provider bills **on top of** the plan's included allowance: Claude "extra usage", Cursor "on-demand", Grok "on-demand", Kiro "overage". A balance of pre-paid credits is not overage.

## Implementation status (2026-10-22)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | `Billing`, `metrics::is_plan_limit`, and the Claude, Grok, Kiro and Cursor overage metrics out of binding, class, pace, alerts and reset notices; extension `billing`. |
| RF2 | vNEXT (partial) | Adapter normalization and fixtures (RF2.1 to RF2.5), Cursor limit 0 and Grok cap 0 read as `disabled`, and the generated Overage column of the providers matrix (RF2.7). Open: the USD conversion pending the RF2.6 scale check (an owner smoke). |
| RF3 | vNEXT | `overage::assess` (state, amount, cap, unit or currency, cycle end, `items`), the `overage.*` texts in the four languages including the combined "Limit hit · on extra" summary, `Report::overage_hint` for providers that state the setting without a metric. |
| RF4 | vNEXT | `AlertKind::OverageStarted` and `OverageCapReached`, once per billing cycle with persisted marks, the first-reading rule, the `AlertPrefs::overage` preference stored as `alerts.overage` in `desktop.json`, and the notice texts. Open: the Settings → Alerts row (RF7.4). |
| RF5 | vNEXT (partial) | `overage_started` and `overage_ended` events with the overage fields, accepted by `--events` and `hooks.toml`; the no-leak test covers them. The `--watch --jsonl` binary test (`crates/cuotai-tui/tests/watch.rs`) drives an approved script extension that reports an overage metric and checks `overage_started` then `overage_ended` on stdout (about two minutes: a real reading is at least 60 s after the previous one). |
| RF6 | vNEXT | `export::v1` `overage` object, `access.overage`, the widget snapshot object (overage kept out of the bars), JSON Schema and reference regenerated. |
| RF7 | vNEXT (partial) | Card chip and textual row, details section, tray chip and tooltip, overage metrics as textual figures, the four-language UI smoke. Open: the Settings → Alerts switch (RF7.4) and the hook editor's explanatory copy, which belong to the Settings lane. |
| RF8 | vNEXT | TUI card line, details section, `--once` line and `--notify` banner. |
| RF9 | vNEXT (partial) | `docs/providers.md` (Overage column, units caveat), `docs/events.md`, `docs/export-v1.md`, `docs/widgets.md`, `docs/tui.md`, `docs/gui-features.md` and `README.md`. The RF2.6 scale record waits for the owner smoke. |
| RF10 | vNEXT | `Scope`, `metrics::limits_account`, scoped exhaustion in binding and class, Antigravity per-model and Claude Opus/Sonnet weekly scopes, extension `scope`, export schema. Consumers delivered: `metrics::scope_blocks`/`ScopeBlock::summary` (`metrics.scope_blocked`, "{model} at its limit until 19:30"), `Overage::summary_with_scope_hit` ("Opus limit hit · on extra $6.25") used by the TUI card, details (`MODELO` line) and `--once`, and by the desktop `overage.summary`; the history stores the scope (schema version 6) so PRD 0026 statistics skip model limits (`MetricHistory::scoped`) and restored readings stay scoped; the timeline, tray summary and widget use `metrics::binding`, which already ignores scoped limits. Open: the PRD 0010 RF4 "Recharging" group and the PRD 0024 RF7 ranking (not on this base), and the desktop card line for `scope_blocked` (UI port). (Added 2026-10-23: `Metric::scope`, review item MET-01). |

## 1. Context

Several subscriptions now keep working after the included quota runs out and bill the excess. Cuotai already reads part of this data, but each adapter normalizes it differently and nothing reacts to it. Verified against `origin/main` at 0.75.0:

| Provider | Where | What it emits today | Consequence |
|---|---|---|---|
| Claude | `providers/subscriptions.rs`, `claude` (the `extra_usage` branch, about line 462) | `Metric::usage("Uso extra — utilizado", used_credits, "unid. provedor", MetricKind::Quota, Monthly)`, with `limit = monthly_limit` and `remaining = limit − used` (not clamped) when `is_enabled = true`. The code comment says the unit (credits or cents) is undocumented. | **Bug.** The extra-usage meter is a capped monthly `Quota`, so it enters `metrics::binding` (`select_kind(report, MetricKind::Quota, …)`, `metrics.rs` line 286), `pace::assess` (`pace.rs` line 83, monthly is eligible), `alerts::evaluate` (`alerts.rs` line 565) and the reset planner as if it were a plan limit. An account whose 5-hour window has 40 % left and whose extra-usage budget has 10 % left shows the **extra budget** as its binding limit, gets a pace line for it and a "90 % of the monthly quota used" alert; a budget past its cap shows a negative remaining value. |
| Cursor | `providers/cursor_personal.rs`, `on_demand_metrics` (about line 436) | "Sob demanda (on-demand)" as `MetricKind::Spend` in USD (cents converted), monthly, `resets_at` = billing-cycle end, `limit` when set, no `remaining`. | Correctly excluded from binding, pace and alerts, but nothing says that paid usage has started. |
| Grok | `providers/router_grok.rs` (`quota_metric`, about line 380) | "Grok CLI: sob demanda" as a capped monthly `Quota` in credits; cap 0 adds the note `note.router_grok.on_demand_exhausted`. | Same misclassification risk as Claude: an on-demand budget can become the binding limit and fire threshold alerts. |
| Kiro | `providers/router_sessions.rs` (about lines 397–490) | Splits plan usage from `currentOveragesWithPrecision`, emits "Kiro: {resource} (excedente)" as a `Quota` with window `other` in credits when > 0, with `overageCapWithPrecision` as the limit only when `overageStatus` is enabled, plus notes for a missing cap and for the charges with currency and rate. | The overage meter is a capped `Quota`, so it can become binding and fire alerts; the charges in money exist only as a note. |
| ChatGPT / Codex | `providers/subscriptions.rs` (about line 360) | Extra Codex credits as a `MetricKind::Credits` balance; `note.subscriptions.chatgpt_codex.no_extra_credits` when there are none. | A pre-paid balance, not overage; no change needed beyond being labelled consistently. |

No alert kind (`AlertKind` has `Threshold`, `Exhausted`, `Recovered`, `SessionExpiring`), event (`docs/events.md`, schema 1) or `export::v1` field tells a person or a script "you are now paying for extra usage, and this much so far".

**Market expectation.** A paid macOS monitor (WhatsMyUsage) advertises that it "notices the moment paid extra usage starts and keeps a $ meter running", and shows a card state such as "Limit hit · on extra $6.25". Cuotai has the data for four providers but buries it.

## 2. Problem

- People find out about overage on the invoice. The moment the included quota runs out and billing starts is the moment that matters, and today it is silent.
- The Claude (and Grok, Kiro) overage meters are treated as plan quotas, which corrupts three delivered features: the binding limit (PRD 0009), the pace (PRD 0007) and the threshold alerts (PRD 0008).
- Overage looks different on each provider (quota, spend, note), so no front end can show it consistently and no script can consume it.
- Per-model limits share the same flaw: Antigravity reports one quota per model, so one exhausted model makes the whole account read "Recharging" with a `blocked_until`, although every other model still works (review item MET-01).
- An unknown amount must never read as "$0 extra": the Claude unit is undocumented, and Cursor omits fields when on-demand is off.

## 3. Goals

- One normalized notion of overage in the core: a metric is either **included** in the plan or **overage**; overage carries a money unit when the provider states one and a provider unit otherwise.
- Overage never takes part in binding, pace, threshold alerts or reset notices; the Claude misclassification is fixed with regression tests.
- A metric limited to one model (or another sub-scope) can be exhausted without blocking the account (RF10).
- A per-account overage state (`disabled`, `armed`, `active`, `cap_reached`, `unknown`) and a running amount, shown on cards, in the tray and in the CLI.
- One notice per billing cycle when overage starts, with the amount; additive events and export fields for scripts.

## 4. Non-goals

- Predicting the end-of-month overage bill or converting provider units to money without a documented rate.
- Changing any provider's overage setting or cap (Cuotai stays read-only).
- Cost from local logs (PRD 0015), which is an estimate at API prices and stays separate from billed overage.
- Pre-paid credit balances (Codex credits, Grok prepaid), which remain `Credits`/`Balance` metrics.
- This PRD supersedes, for overage only, the PRD 0008 §4 non-goal "Alerting about balance, accumulated spend": RF4 adds one "extra usage started" notice per billing cycle; balances and spend in general stay out of the threshold alerts.

## 5. Functional requirements

### RF1 — Normalized overage semantics in the model (high priority; prerequisite)

- **RF1.1** Add `Metric::billing: Billing` with `Billing::Included` (default) and `Billing::Overage`. Serialized as `"billing": "overage"` only for overage metrics (`#[serde(default, skip_serializing_if = "Billing::is_included")]`), so every metric without overage serializes exactly as today in `--json` and `export::v1`. **Decided on 2026-10-23 (decision L):** `billing = included | overage` is additive and non-breaking: it ships in a `feat` commit with a minor bump and no `BREAKING CHANGE` footer.
- **RF1.2** `MetricKind` is unchanged: an overage metric keeps the kind that describes its numbers (`Spend` for money, `Quota` or `Usage` for provider units) and `billing` says it is outside the plan. This avoids a new `kind` enum value in `--json`.
- **RF1.3** Eligibility rule shared by every consumer, in one core predicate `metrics::is_plan_limit(metric)` (`kind == Quota && billing == Included`): `metrics::binding`/`binding_where`, `pace::assess`, `alerts::evaluate` (`Threshold`, `Exhausted`, `Recovered`), `resets::upcoming`/`plan`/`passed` and `metrics::classify_report` use it instead of testing `kind == Quota` alone. An overage meter that reaches its cap never classifies the account as `Exhausted` by itself.
- **RF1.4 (bug fix)** The Claude `extra_usage` metric becomes `billing = Overage`; `remaining` is clamped at 0 while `used` keeps the provider's value verbatim (as the Grok helper already does). The same applies to the Grok on-demand and Kiro "(excedente)" metrics. Regression tests: a Claude report with a 5-hour window at 60 % used and an extra budget at 90 % used has the 5-hour window as binding, no pace for the extra meter, no threshold alert from it, and no reset notice for it.
- **RF1.5** Extensions (PRD 0017): output schema 1 accepts an optional `billing` field (`included` or `overage`, unknown values rejected as `invalid_output`), additive; `docs/extensions.md` documents it.
- **Acceptance criterion:** table tests of `is_plan_limit` and of every listed consumer with included and overage metrics side by side; the `export::v1` reference output and `--json` snapshot tests are unchanged for fixtures without overage.

### RF2 — Adapter normalization with fixtures (high priority)

- **RF2.1** Claude: `extra_usage.is_enabled = false` or absent emits no metric and sets the overage state to `disabled`; `is_enabled = true` with `used_credits` absent emits no metric and the state `unknown` (never 0); with `used_credits = 0` the state is `armed`. The amount is money in **US dollars** (decision R): the metric becomes `Spend` with `unit = "USD"` and `billing = Overage`, with the scale fixed by RF2.6.
- **RF2.2** Cursor: the on-demand metric gains `billing = Overage`, keeps `Spend` in USD; `used` absent stays `None`; a present `limit` of 0 means on-demand is off (`disabled`).
- **RF2.3** Grok: the on-demand metric gains `billing = Overage`; the cap-0 exhausted case keeps its note and maps to `cap_reached` only when the provider reports on-demand as enabled, otherwise `disabled`.
- **RF2.4** Kiro: the "(excedente)" metric gains `billing = Overage`; when `overageCharges` and its `currency` are present, an additional overage `Spend` metric in that currency is emitted (the money amount today lives only in a note); the existing notes stay.
- **RF2.5** ChatGPT / Codex: no change of meaning; a test proves that the Codex credits balance is never classified as overage.
- **RF2.6 (resolved in part on 2026-10-23, decision R)** Claude extra usage is billed in **US dollars**: the owner's account with extra usage is charged in USD. What remains to verify is the **scale** the API returns: whether `used_credits` and `monthly_limit` are dollars or cents. The delivery compares one live response from the owner's account with the amount the Claude web console shows for the same moment, records the result (and the anonymized fixture, PRD 0028 RF5) in `docs/providers.md`, and converts cents to dollars in the adapter when that is the scale. Until that check is recorded, the adapter keeps the value as provider units and the front ends show no `$` sign; the check is an owner smoke listed in `0000_control.md` §2.1. A non-USD account (if one is ever observed) keeps provider units and a note, never a guessed currency.
- **RF2.7** `docs/providers.md` gains a generated "Overage" column (per provider and mode: `reported in money`, `reported in provider units`, `not reported`) from a new `overage` field of `access::describe`, checked by the existing `providers_doc_matrix` drift test.
- **Acceptance criterion:** `wiremock` fixtures per provider for overage disabled, enabled with nothing spent, active, past the cap and with the fields missing or `null`; each asserts `billing`, the values (missing stays `None`) and the overage state; no fixture leaks URLs or tokens into notes.

### RF3 — Overage state per account (high priority)

- **RF3.1** Pure `overage::assess(report: &Report) -> Option<Overage>` with `state` (`Disabled`, `Armed`, `Active`, `CapReached`, `Unknown`), `spent: Option<f64>`, `unit`, `currency: Option<String>` (ISO 4217 when the unit is money), `cap: Option<f64>`, `cycle_ends_at: Option<DateTime<Utc>>` and the source metric label. Several overage metrics of one account (Kiro per resource) are summed only when they share the currency; otherwise the state lists them separately (`items`).
- **RF3.2** `Active` means `spent > 0`; `CapReached` means `spent ≥ cap` with a positive cap; `Unknown` means overage is enabled but the amount is missing. `None` when the provider exposes no overage data at all.
- **RF3.3** Localized texts in the core (`overage.*`): "On extra · $6.25", "On extra · 12.5 provider units", "Extra usage on · nothing spent", "Extra budget used up", "Extra usage amount unknown"; and the combined card summary with the binding state, "Limit hit · on extra $6.25", from one key with placeholders (DS-MSG-12). Money is formatted by the caller with the currency (DS-MSG-7); catalogs in `en-US`, `es` and `it` with the Portuguese fallback.
- **Acceptance criterion:** table tests for each state, mixed currencies, a missing amount, a zero cap, and the texts in the four languages.

### RF4 — "Extra usage started" alert (high priority)

- **RF4.1** New `AlertKind::OverageStarted`, evaluated by `alerts::evaluate` when the overage state of an account moves from `Disabled`, `Armed` or `Unknown` to `Active` (or straight to `CapReached`). It carries the amount and currency at the time of the reading.
- **RF4.2** Once per account per billing cycle: the cycle identity is the overage metric's `resets_at` within `resets::DRIFT_TOLERANCE`; when the provider gives no cycle end, the calendar month in UTC. The mark lives in `AlertState` (digests and dates only) and is pruned with it (`LEDGER_KEEP`).
- **RF4.3** Optional `AlertKind::OverageCapReached`, once per cycle, when a positive cap is reached; it follows the existing `exhaustion` preference.
- **RF4.4** First-reading rule of PRD 0008 RF1.7: an overage already active on the first reading in the process does not fire unless the previous recorded reading (PRD 0006) is at most 10 minutes older and still not active.
- **RF4.5** `alerts::notice` texts: "Claude: extra usage started · $0.40 so far" with "Work · billing cycle ends 1 Nov"; with provider units, "12.5 provider units so far". Never the e-mail, the token or a note. Grouped with other alerts of the same account and refresh as today.
- **RF4.6** Preference `AlertPrefs::overage` (on by default whenever usage alerts are on), stored as `alerts.overage` in `desktop.json`; the TUI `--notify` uses the core default.
- **Acceptance criterion:** table tests with a fixed clock: armed → active fires once; oscillating amounts do not repeat; a new cycle fires again; a restart with the persisted state does not resend; the first-reading rule; cap reached; preference off.

### RF5 — Events `overage_started` and `overage_ended` (medium priority)

- **RF5.1** `events::diff` emits `overage_started` on the same edge as RF4.1 and `overage_ended` when an active overage returns to `Armed`, `Disabled` or a new cycle starts with nothing spent. Additive to events schema 1 (`docs/events.md`: consumers ignore unknown names); `--events` and `hooks.toml` accept both names.
- **RF5.2** Payload `current`/`previous` carry `overage_state`, `spent`, `unit`, `currency`, `cap` and `cycle_ends_at` as known values only; no notes.
- **Acceptance criterion:** reading-sequence tests (baseline active, start, end, new cycle, missing amount), the no-leak test extended to the new kinds, and a `--watch --jsonl` binary test with a fake provider.

### RF6 — `export::v1` and widget snapshot (medium priority)

- **RF6.1** `AccountExport` gains `overage: { state, spent, unit, currency, cap, cycle_ends_at } | null`, additive under the v1 policy (`docs/export-v1.md`); the JSON Schema and the reference output are regenerated; metric objects gain `billing` only when `overage`.
- **RF6.2** The widget snapshot (PRD 0014) carries the same object so a widget can show "on extra".
- **Acceptance criterion:** schema and reference tests; a fixture account with overage appears with the object and every other account is unchanged.

### RF7 — Desktop presentation (medium priority)

- **RF7.1** Card: when the overage state is `Active` or `CapReached`, the card's status chip reads the core summary ("Limit hit · on extra $6.25"); the overage metric is listed under the plan meters as a textual row (figure first, DS-MSG-4: "$6.25 extra of $50") and never drawn as a quota meter that implies "left" (DS-CMP-MTR-5); when a cap exists, a continuous bar of the spent share is allowed with its label (DS-CMP-MTR-3).
- **RF7.2** Details: an "Extra usage" section with the state, amount, cap, cycle end and the provider-units caveat for Claude (RF2.6).
- **RF7.3** Tray: the account row shows the "on extra" chip; the tray summary of PRD 0025 includes it.
- **RF7.4** Settings → Alerts: "Notify when extra usage starts" under Usage alerts (PRD 0020 rows; DS-CMP-SW).
- **RF7.5** Visual specifics (chip tone, placement) follow PRD 0022 and `docs/design-system.md` (DS-CMP-BADGE, DS-CMP-CALL, DS-MSG-4, DS-MSG-5).
- **Acceptance criterion:** smoke in the four languages and the reference themes with an active, capped, unknown and disabled overage; Claude shows US dollars once the RF2.6 scale check is recorded (provider units before it); an unknown amount shows "—", never "$0".

### RF8 — TUI and CLI presentation (medium priority)

- **RF8.1** TUI card: an "extra" line with the core summary when active; details: the RF7.2 section.
- **RF8.2** `--once` prints `Uso extra: <summary>` under the account; `--json` gains only the RF1.1 `billing` field on overage metrics.
- **RF8.3** `--notify` delivers RF4 alerts in the header banner with the existing `BEL`/OSC 9 rules.
- **Acceptance criterion:** `TestBackend` rendering tests and a `--once` text snapshot with an active overage.

### RF9 — Documentation and changelog notes (low priority)

- **RF9.1** `docs/providers.md` (RF2.7 and the RF2.6 scale record), `docs/events.md` (RF5), `docs/export-v1.md` (RF6 and RF10.4) and `README.md` describe overage, model-scoped limits, the units caveat and the fact that Cuotai never changes billing settings.
- **RF9.2** The RF1.4 fix is listed under **Fixed** in the changelog of the version that delivers it ("Claude extra usage no longer counts as the binding limit or fires quota alerts").
- **Acceptance criterion:** the documentation drift tests pass; the changelog check passes.

### RF10 — Model-scoped metrics (`Metric::scope`) (high priority; added 2026-10-23, review item MET-01)

- **RF10.1** Add `Metric::scope: Option<Scope>` with `Scope { kind, id, label }`, `kind` = `model` (others reserved: `resource`, `workspace`), serialized only when present (`#[serde(default, skip_serializing_if = "Option::is_none")]`), so every unscoped metric serializes as today in `--json` and `export::v1`. Additive and non-breaking, like `billing` (decision L).
- **RF10.2** **One shared predicate.** Next to RF1.3's `is_plan_limit`, the core exposes `metrics::limits_account(metric)` = `is_plan_limit(metric) && metric.scope.is_none()`. `metrics::binding`/`binding_where`, `metrics::classify_report`, `blocked_until`, the PRD 0010 RF4 "Recharging" group, the reset timeline (PRD 0026 RF2), the tray summary (PRD 0025 RF1) and the best-account ranking (PRD 0024 RF7) use it. A scoped metric still gets its own pace, threshold alerts and reset notices (it is a plan limit for that model); it never decides the account's state.
- **RF10.3** **Scoped exhaustion.** An exhausted scoped metric marks only its scope: the card and details show "{model} at its limit until 19:30" (`metrics.scope_blocked`), the account keeps its own state from its unscoped limits, and no `blocked_until` is set for the account. When **every** plan limit of an account is scoped and all are exhausted, the account is exhausted.
- **RF10.4** `export::v1` metric objects gain `scope` when present (additive); the JSON Schema and the reference output are regenerated.
- **RF10.5** Adapters that report per-model quotas fill it: Antigravity first; every adapter whose labels name a model is reviewed and listed in the delivery. Extension output schema 1 accepts an optional `scope` (additive, unknown kinds rejected).
- **Acceptance criterion:** table tests of `limits_account` beside `is_plan_limit` for every consumer listed in RF10.2; an Antigravity fixture with one model exhausted and the others free leaves the account not blocked, out of "Recharging" and with no `blocked_until`, while the exhausted model shows its own return; all models exhausted blocks the account; `--json` and `export::v1` fixtures without scope are unchanged.

## 6. Non-functional requirements

- **Honesty:** absent amounts are `None`, shown as "—" (DS-MSG-5); a provider unit is never shown as money; overage text never claims to be the invoice.
- **Privacy:** no new data leaves the device; notification texts carry only provider, account name, amount and cycle date (lock-screen safe, as PRD 0008 §6).
- **Cost:** no new provider query; every value comes from the responses already fetched.
- **Compatibility:** `--json` and `export::v1` changes are additive (two omitted-by-default fields, `billing` and `scope`, and a new nullable object), so they raise only the minor version through the `feat` commits that deliver them, with no `BREAKING CHANGE` footer; third-party consumers that read `kind = "quota"` without `billing` are told in `docs/export-v1.md` to filter overage.
- **i18n and accessibility:** every text in pt-BR, en-US, es and it; state conveyed by text and icon, never color alone (DS-PRI-6).

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `Metric` (`--json`, `export::v1`) | `billing: "overage"` on overage metrics only; `scope` on model-scoped metrics only | Additive (decision L: minor bump, no `BREAKING CHANGE` footer) |
| `export::v1` `AccountExport` | `overage` object or `null` | Additive (v1 policy) |
| Events schema 1 | `overage_started`, `overage_ended` | Additive (unknown names ignored) |
| Extension output schema 1 | optional `billing` and `scope` | Additive |
| `desktop.json` | `alerts.overage` (bool, default on) | Additive, lenient read |
| `notified.json` | overage marks in `AlertState` | Additive, pruned after 8 days |
| TOML | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | `Billing`, `Scope`, `is_plan_limit`, `limits_account`, adapter normalization, `overage::assess`, alert and event kinds, texts, export and widget fields |
| Desktop | card chip, details section, tray chip, settings row, notification delivery |
| TUI/CLI | card line, details, `--once` line, `--notify` banner |
| Widgets | read the RF6.2 object; no logic |

## 9. Dependencies and sequencing

RF1 and RF2 come first and can ship alone as the bug fix plus normalization; RF10 shares RF1's predicate work and ships with it or right after. RF3 precedes RF4–RF8. RF4 reuses PRD 0008's delivery; RF5 reuses PRD 0013's engine; RF6 reuses PRD 0012's contract. PRD 0026 RF1 ("extra spent") and PRD 0025 RF1 (tray summary) read RF3. Visual polish waits for PRD 0022 but RF7 can ship with the current components.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Claude amount shown at the wrong scale (cents as dollars) | RF2.6 scale check against the web console before the `$` sign appears; recorded in `docs/providers.md` |
| A consumer relied on per-model quotas blocking the account | additive `scope`, documented in `docs/export-v1.md`, changelog entry |
| Providers rename or drop overage fields | `wiremock` fixtures for missing fields; PRD 0028 schema-drift diagnostics |
| A consumer relied on Claude extra being a quota | Additive `billing` field, documented filter, changelog **Fixed** entry |
| Noise from repeated "extra started" notices | once per cycle, persisted, first-reading rule |

## 11. Order of execution and verification

1. **RF1, RF2 and RF10:** semantics, the Claude/Grok/Kiro fix, model scope and the fixtures.
2. **RF3:** state and texts.
3. **RF4 and RF5:** alert and events.
4. **RF6:** export and widget snapshot.
5. **RF7 and RF8:** presentation; **RF9** documentation with each step.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface. The requirement is only delivered when a Claude account whose extra usage starts produces exactly one notice in the cycle with the amount in the correct unit, the extra meter never becomes the binding limit, and no extra query is made.

## 12. Open questions

| Question | Recommendation |
|---|---|
| `billing` field versus a new `MetricKind::Overage` | `billing` field: additive, keeps the numeric kind, no new `kind` value in `--json`. |
| ~~Claude amount unit~~ | **Decided on 2026-10-23 (decision R): US dollars.** Still to verify and document: whether the API returns dollars or cents (RF2.6, owner smoke). |
| ~~`billing` breaking or not~~ | **Decided on 2026-10-23 (decision L):** additive, minor bump, no `BREAKING CHANGE` footer. |
| Should "extra usage started" be on by default once usage alerts are on | Yes (RF4.6); it is the most expensive surprise. |
| Should `overage_ended` exist or only `overage_started` | Keep both: scripts that pause work need the end edge. |
