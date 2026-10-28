# PRD 0030 — Cost: plan value, sources and price age

| Field | Value |
|---|---|
| ID | 0030 |
| Status | In progress |
| Owner | `cuotai-core` (`cost::*`: sources, index, prices, estimate, settings), desktop and TUI |
| Consumers | `apps/desktop` (cost card and details, Settings → Local cost), `crates/cuotai-tui` (`--cost`, `--cost-prices`, cost panel), `export::v1` `local_cost` consumers |
| Depends on | PRD 0015 (RF1–RF4: sources and consent, incremental index, price table, display); PRD 0029 (RF1: account `id`) for RF1 |
| Benefits from | PRD 0027 (local CLI detection, presence only), PRD 0023 (provider overage amounts, never summed with estimates) |
| Extends | PRD 0015 RF1 (new sources), RF2 (index schema), RF3 (1-hour cache writes, price age, templates), RF4 (plan value, unpriced models, display currency) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. The **estimate** is PRD 0015's cost at published API prices of this computer's local CLI logs; it is never a charge. The **plan price** is what the person says they pay for a subscription. Code references are to `origin/main` at `0ba9c9b`.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.81.0 | Delivered. Core and CLI: `plans` in `cost/settings.json`, `cost::plan_values` (cycle day, same-currency ratio, "at your rate", one combined line per source, no cross-currency sum), `Scanner::plan_values`, `--cost-plan`, `--cost-plan-clear`. Plans are keyed by the stable account id (PRD 0029 RF2); `SettingsStore::migrate_plans` moves a legacy `provider:name` key to the id once the configured accounts are known (desktop plans view, TUI panel, `--cost` with a configuration). Desktop: Settings → Local cost → Plans and second currency (one row per account: price, ISO currency, cycle day; shared `row()` with search keywords), the plan line on the cost card and details and an "Add plan price" link while none exists (`cost_plans`, `cost_set_plan`). TUI: the plan line in the account details; typing a plan stays `--cost-plan` (a TUI form is not built). |
| RF2 | 0.81.0 | Gemini CLI delivered (owner decision of 2026-10-25: only sources with a confirmed public format reference, numbers only). Evidence: the Gemini CLI session management guide says sessions live in `~/.gemini/tmp/<project_hash>/chats/`, and `chatRecordingService.ts` (google-gemini/gemini-cli) writes JSON Lines with a `sessionId` header and message records carrying `id`, `timestamp`, `type`, `model` and `tokens` (`input`, `output`, `cached`, `thoughts`, `tool`, `total`); both were read on 2026-10-25 and are quoted in the module doc of `cost/gemini.rs`. Same consent disclosure, retention and erase as the other sources; `content`, `thoughts` and `toolCalls` are never deserialized; a fixture with content placeholders proves only numbers come out. Gemini prices are not in the embedded table (unverified), so its cost reads unknown until the person adds a `pricing.json` entry. OpenCode and Qwen Code stay undelivered: no stable public format reference was confirmed (OpenCode has moved between JSON files and SQLite). Owner decision of 2026-10-25: a new source is added only with a confirmed public or stable format reference and extracts numbers only; RF2.3 (a "Gemini CLI found, enable local cost?" offer in the pickers) is optional and not built. Re-checked on 2026-10-25 against official documentation only (0.81.0): OpenCode's troubleshooting page (opencode.ai/docs/troubleshooting) names `~/.local/share/opencode/opencode.db` as a SQLite database "containing projects, sessions, and messages" whose file name may vary (`opencode db path`, `OPENCODE_DB`), but documents no schema, no token fields and no stability promise; Qwen Code's settings reference (qwenlm.github.io/qwen-code-docs) documents `general.chatRecording` ("Save chat history to disk") and `model.enableOpenAILogging` (JSON files under `logs/openai`) with no path layout, schema or token fields. Decision: both stay out under the owner rule (confirmed public, stable format only); RF2 is closed for this release with Gemini CLI as the only new source, and each tool is reconsidered when its documentation publishes a format. |
| RF3 | 0.81.0 | `PriceTable::age`, `CostSummary` `prices_effective`/`prices_age_days`/`prices_age` (text from 60 days), shown in `--cost`, `--cost-prices`, the TUI details and the desktop details; additive in `export::v1`. |
| RF4 | 0.81.0 | `unpriced_models`/`unpriced` in the summary, shown in `--cost`, the TUI details and the desktop details; `--cost-prices --template`. |
| RF5 | 0.81.0 | RF5.1–RF5.3 delivered: the Claude parser splits `cache_creation.ephemeral_5m/1h_input_tokens` (`Tokens::cache_write_1h`), migration 2 adds `usage.cache_write_1h` and forgets Claude files so the next scan corrects history once, price tables accept an optional `cache_write_1h` (built-in Claude rows at 2x input) and a model without it has an unknown cost for 1-hour tokens. Desktop (tooltip and totals) and TUI (`--cost-prices` column) show the new field. |
| RF6 | 0.81.0 | Delivered. Core and CLI: `display_currency` in the settings, `DisplayCurrency::text` ("at your rate", age, update after 30 days), `--cost-currency CODE RATE` / `off`. Desktop: the rate row in Settings → Local cost → Plans and second currency, a converted amount under each tile and the "at your rate" line on the card and details (display only; the export stays USD-only). TUI: the same line under the period in the account details. |
| RF7 | 0.81.0 | RF7.1 evidence (fixture `tests/fixtures/cost/claude_streamed_numbers.jsonl`, finding in the `claude.rs` module doc): in 19,918 local assistant lines, 507 of 9,525 `message.id`+`requestId` keys repeat with differing `output_tokens` (for example 3, 3, 1880): first-wins kept partial counts. RF7.2 delivered: `seen` keeps the accepted line's counts and a later line applies only the growth. |
| RF8 | 0.81.0 | README, `docs/advanced.md`, `docs/tui.md`, `docs/export-v1.md` and `docs/architecture.md` updated for what is delivered. |

## 1. Context

PRD 0015 delivered a local estimate from Claude Code and Codex CLI logs: consented sources (`cost/settings.rs`, `<data>/cuotai/cost/settings.json`), an incremental SQLite index (`cost/index.rs`), a built-in price table with an `effective` date and optional user `pricing.json` (`cost/prices.rs:78-127`, `cost/prices.json`, effective `2026-10-21`), and per-model estimates that keep unknown prices as `null` with `unknown_tokens` and `unknown_share` (`cost/estimate.rs:115-122`). USD is the only currency (`cost/prices.rs:23-24`, `pricing.json` rejects anything else, `:130`).

The 2026-10-21 review found gaps and two accuracy defects:

- **COST-01.** People on a subscription want to know whether the plan is worth it: "this month you used ≈ US$ 212 at API prices on a US$ 100 plan". Nothing compares the estimate with what they pay.
- **COST-02.** Only two sources exist behind `trait Source` (`cost/source.rs:12-30`); Gemini CLI, OpenCode and other tools write local logs too.
- **COST-03.** The price table ages silently; nothing tells the person the prices are months old.
- **COST-04.** Unpriced models only appear as an "unknown share"; the person cannot see which models, nor easily write the `pricing.json` entry.
- **FIX-13.** The Claude parser reads `cache_creation_input_tokens` only (`cost/claude.rs:44-55`, `:106-111`), but Claude Code also writes `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` (the fixture at `cost/claude.rs:149` has both). 1-hour cache writes cost more than 5-minute writes; today they are priced as 5-minute writes.
- **FIX-14.** Deduplication by `message.id` + `requestId` is `INSERT OR IGNORE INTO seen` (`cost/index.rs:263-266`, `:283-289`): the **first** line for a key wins. If Claude Code writes several lines per message while streaming, with `output_tokens` growing, the index keeps the partial count. This has to be confirmed with a real transcript first.
- **Open item from PRD 0015 (owner accepted):** many users pay in other currencies (Brazil); a manual, display-only exchange rate (`fx_rate`) is acceptable.

## 2. Problem

- The estimate answers "how much would this cost on the API" but not "is my plan paying off".
- Users of other CLIs see nothing.
- Old prices, unpriced models and wrongly priced 1-hour cache writes make the estimate less honest than it looks.
- A possible undercount of output tokens.
- Amounts only in USD for people who think in another currency.

## 3. Goals

- Compare the estimate with a plan price the person types, per account and month, clearly labelled as an estimate.
- Add sources only behind a public format reference, with PRD 0015's consent, numbers-only extraction and retention.
- Show the age of prices, the list of unpriced models and a ready-to-fill `pricing.json` template.
- Price 1-hour cache writes correctly and keep the right output count once the transcript behaviour is confirmed.
- Show amounts also in a second currency at a rate the person typed, never fetched, never summed.

## 4. Non-goals

- Downloading prices, plan prices or exchange rates (PRD 0015 §4 still applies: no network for cost).
- Detecting the plan from the provider; plan prices are typed by the person.
- Summing different currencies, or summing an estimate with a provider-reported amount (PRD 0023 overage, Bedrock billing).
- Reading conversation content, file paths or prompts (PRD 0015 §6 still applies).
- Splitting one CLI's logs between two accounts of the same provider (the logs do not say which account was used).

## 5. Functional requirements

### RF1 — Plan value (high priority; COST-01)

- **RF1.1** `cost/settings.json` gains `plans: { <account id>: { price, currency, cycle_day? } }` (PRD 0029 `id`; until PRD 0029 RF1 lands, the legacy key), `price` > 0 with two decimals, `currency` an ISO 4217 code, `cycle_day` 1–28 (default 1, the calendar month). Typed by the person; never downloaded, never in the TOML.
- **RF1.2** `cost::plan_value(summary, plans, accounts, today) -> Vec<PlanValue>` per account whose provider is shown by a source (`SourceId::providers`, `cost/view.rs:144-146`): the estimate of the current cycle, the plan price, and `ratio = estimate / price` **only when both are in the same currency** (or when RF6 gives a rate for the plan currency, then labelled "at your rate"). Otherwise both are shown side by side and no ratio.
- **RF1.3** When two or more accounts are shown by the same source, the estimate cannot be split: one combined line "Claude accounts: ≈ US$ 212 at API prices · plans US$ 200" (plans summed only when in one currency), never one line per account.
- **RF1.4** Texts (`cost.plan.*`) always say "estimate at API prices"; no plan price → nothing is shown (no prompt beyond an "Add plan price" link in the cost details).
- **Acceptance criterion:** table tests: no plan price shows nothing; same currency gives a ratio; different currencies without a rate give no ratio and no sum; two accounts of one source give one combined line; a cycle day of 15 splits the month at the 15th.

### RF2 — More log sources (medium priority; COST-02)

- **RF2.1** A new source is added behind `trait Source` only when a public reference documents its log format (the tool's own documentation or source code), recorded in the source's module doc and in `THIRD_PARTY_NOTICES` when code is adapted. Candidates: Gemini CLI, OpenCode, Qwen Code; each is a separate delivery.
- **RF2.2** Same consent disclosure (directories, pattern, "only numbers"), off by default, numbers-only extraction (time, model, token counts, an anonymous session digest), retention and erase per source, the `--cost-sources` listing.
- **RF2.3** PRD 0027's local credential detection may say "Gemini CLI found — enable local cost?", but never enables a source without consent.
- **Acceptance criterion:** per source, a fixture of real log lines with content replaced by placeholders; a test that a line with content and paths yields only numbers; consent and erase tests as PRD 0015.

### RF3 — Price age (medium priority; COST-03)

- **RF3.1** `PriceTable::age(today) -> Option<u32>` in days since `effective` of the table that priced each model (built-in or user). From **60 days**, the cost views show `cost.prices.age` ("Prices from {days} days ago") beside the total, in the TUI panel, the desktop card and details, `--cost` and `--cost-prices`.
- **RF3.2** `local_cost` in `export::v1` carries `prices_effective` (the oldest effective date used) and `prices_age_days` (additive).
- **Acceptance criterion:** tests at 59, 60 and 400 days; mixed built-in and user tables show the oldest.

### RF4 — Unpriced models and templates (medium priority; COST-04)

- **RF4.1** The cost details list the models with tokens but no price ("No price for: claude-foo-6, gpt-x-mini"), from the estimate's per-model `cost: null` entries; `--cost` prints the same list.
- **RF4.2** `cuotai --cost-prices --template MODEL [MODEL…]` prints a `pricing.json` fragment with the model names, `currency: "USD"`, today's `effective` and `null` rates to fill in, valid once filled; it never guesses a price.
- **Acceptance criterion:** a binary test of the template; a view test with one priced and one unpriced model.

### RF5 — 1-hour cache writes (high priority; FIX-13)

- **RF5.1** The Claude parser reads `usage.cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` when present; `cache_write` holds the 5-minute part and a new `cache_write_1h` the 1-hour part. Lines with only the total keep counting it as `cache_write`.
- **RF5.2** A forward migration appended to `cost/index.rs` `MIGRATIONS` adds `cache_write_1h` to `usage`; Claude files are marked for a full re-read so history is corrected once.
- **RF5.3** Price tables gain an optional `cache_write_1h` rate; the built-in table carries Anthropic's published 1-hour write prices. A model without that rate counts its 1-hour tokens as unknown (never priced at the 5-minute rate).
- **Acceptance criterion:** parser tests with both fields, only the total, and zero 1-hour tokens; a migration test from schema 1; an estimate test showing the 1-hour price applied.

### RF6 — Display currency (low priority)

- **RF6.1** `cost/settings.json` gains `display_currency: { code, rate, set_on }`: a rate typed by the person ("1 USD = 5.42 BRL"), never fetched.
- **RF6.2** Every USD amount of the estimate is shown with a second line "≈ R$ 1,149 at your rate (set 12 days ago)"; after 30 days the line adds "update your rate". Totals are computed in USD and converted once for display; nothing is stored or exported converted.
- **RF6.3** Settings → Local cost (desktop) and `cuotai --cost-currency CODE RATE` / `--cost-currency off` (TUI/CLI) set it.
- **Acceptance criterion:** formatting tests in the four languages; a test that `local_cost` in the export stays USD-only; the rate never appears in any network request (no request is made).

### RF7 — Output-token deduplication (medium priority; FIX-14)

- **RF7.1** **Evidence first:** capture a real Claude Code transcript with streaming responses, reduce it to the `message.id`, `requestId` and `usage` numbers of each line (no content), and add it as a fixture. Decide from it whether repeated lines carry growing `output_tokens`. Record the finding in the source module doc.
- **RF7.2** If confirmed: the `seen` table keeps the token counts of the line it accepted (forward migration); a later line with the same key and higher `output_tokens` (or any higher count) applies the difference to `usage`; lower or equal lines stay duplicates.
- **RF7.3** If not confirmed: keep first-wins and document why, closing FIX-14.
- **Acceptance criterion:** the fixture test passes with the chosen rule; a re-scan of the same file is idempotent.

### RF8 — Documentation (low priority)

- **RF8.1** `README.md` (cost section: plan value, display currency, honesty notes), `docs/export-v1.md` (`prices_effective`, `prices_age_days`), `docs/architecture.md` (index schema 2), the PRD 0015 notes.
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

## 6. Non-functional requirements

- **Honesty:** every comparison says "estimate at API prices" and never "you saved"; unknown stays unknown; no currency is summed with another; converted amounts say "at your rate".
- **Privacy:** no network; settings and index stay `0600` under the data directory; plan prices and rates never leave the machine except as the person exports them; new sources extract only numbers.
- **Performance:** RF5.2's re-read runs in the background worker (`cost/worker.rs`) with the existing chunking; the UI never blocks.
- **Compatibility:** index migrations are forward-only; old `pricing.json` files stay valid (`cache_write_1h` optional).
- **i18n and accessibility:** pt-BR, en-US, es and it (plus PRD 0035 languages); currency and number formatting through the locale; the price-age text is text, not only an icon.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `cost/settings.json` | `plans`, `display_currency` | Additive, lenient read |
| Cost index | `cache_write_1h` column; `seen` token counts (if RF7.2) | Forward migration, re-read of Claude files |
| `pricing.json` | optional `cache_write_1h` | Additive |
| `export::v1` `local_cost` | `prices_effective`, `prices_age_days`; per-model `cache_write_1h` tokens | Additive (schema 1) |
| CLI | `--cost-prices --template`, `--cost-currency` | New flags |
| TOML | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | parsers, index migrations, price age, plan value, conversion, templates, texts |
| Desktop | plan price and currency inputs, plan value line, price-age chip, unpriced list, display currency setting |
| TUI/CLI | cost panel lines, `--cost` output, new flags |

## 9. Dependencies and sequencing

RF5 and RF7 are accuracy fixes and go first (RF7 starts with the transcript capture). RF3 and RF4 depend only on PRD 0015. RF1 needs account ids (PRD 0029 RF1) to avoid keying plan prices by name. RF6 is independent. RF2 sources are separate deliveries, each after its format reference is confirmed. Wave 5e of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Plan value read as a recommendation to cancel or keep a plan | factual texts, "estimate at API prices", no advice |
| Re-read after the migration is slow on large logs | background worker, progress in the cost details |
| Third-party log formats change | per-source fixture, `incompatible schema` classification, source turns `partial` instead of guessing |
| A stale manual rate misleads | the rate's age is always shown; reminder after 30 days |

## 11. Order of execution and verification

1. **RF7.1** transcript capture, then **RF5** and **RF7.2/RF7.3**.
2. **RF3** and **RF4**.
3. **RF1** after PRD 0029 RF1; **RF6**.
4. **RF2** per source; **RF8** with each step.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of `--cost`. The requirement is only delivered when a fixture with 1-hour cache writes is priced at the 1-hour rate and a plan in BRL is never divided by a USD estimate without a typed rate.

## 12. Open questions

| Question | Recommendation |
|---|---|
| A preset list of common plan prices (Claude Pro, Max, ChatGPT Plus) | No: prices change and vary by country; the person types the price. |
| Which source after the two existing ones | Gemini CLI first (decision Q already puts it in the detection table), then OpenCode. |
| Should `fx_rate` also convert provider-reported amounts (PRD 0023 overage) | Not in this version; provider amounts stay in their own currency. |

## 13. Decisions (owner, 2026-10-23)

| Topic | Decision |
|---|---|
| Where plan prices live | In the machine-local `cost/settings.json` (RF1.1), never in the TOML. |
| Which new log source first | Gemini CLI, because its recorder format is documented in the tool's public repository (2026-10-25, owner decision: pick only sources with a confirmed public format reference). OpenCode and Qwen Code wait for a stable reference. |
| Typing a plan in the TUI | Kept as `--cost-plan` (and `--cost-currency`); the interactive TUI shows the lines but has no form (2026-10-25). |
