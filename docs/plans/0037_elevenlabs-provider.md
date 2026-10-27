# PRD 0037 — ElevenLabs provider

| Field | Value |
|---|---|
| ID | 0037 |
| Status | Awaiting owner verification (RF1–RF10 delivered in vNEXT; RF11, the real-account check, is the owner's) |
| Owner | `cuotai-core` (model, adapter, access descriptor, account fields, incident registry), desktop and TUI (generic rendering only) |
| Consumers | `apps/desktop` (cards, details, tray, account dialog), `crates/cuotai-tui` (cards, details, `--once`, wizard), `export::v1` and `--watch` consumers |
| Depends on | PRD 0005 RF1 (`access::describe`, the exhaustive classification) |
| Benefits from | PRD 0023 RF1 (`Metric::billing`, for the extra-usage metrics), PRD 0027 (catalog entry), PRD 0011 (public status source), PRD 0028 (contract-health `verified_on` and shape canary), PRD 0006/0007/0008/0009 (history, pace, alerts and binding, which work with no change once the credits metric is a monthly `Quota` with `resets_at`) |
| Extends | PRD 0005 (one more descriptor), PRD 0011 (one more registry entry) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. **Credits** is ElevenLabs' current name for what its API fields still call *characters* (`character_count`, `character_limit`); see §1. "Extra usage" means consumption billed on top of the plan's included credits, as defined in PRD 0023.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Provider variant, dispatch, adapter and region allow-list (`Config::validate` rejects free-form regions). |
| RF2 | vNEXT | Access descriptor (`Official`, `verified_on` 2026-10-23), catalogs and regenerated matrix. The PRD 0027 catalog entry is delivered, with the aliases `eleven labs`, `11labs`, `tts`, `text to speech`, `voice` and `voice agents`. |
| RF3 | vNEXT | Delivered. |
| RF4 | vNEXT | Delivered. |
| RF5 | vNEXT | Extra-credits and cost metrics ship as `Usage`/`Spend`; The extra-credits and cost metrics carry `Billing::Overage` (PRD 0023), and `overage::assess` reads every ElevenLabs extra-usage state (disabled, armed, active, cap reached, unknown) from fixtures. The metric labels and units are identifiers (Portuguese, as the adapter emits them); display text goes through `metric_text` in the viewer's language (PRD 0034). |
| RF6 | vNEXT | Delivered. |
| RF7 | vNEXT | Delivered. |
| RF8 | vNEXT | Delivered: the registry arm `eleven_labs` (`IncidentIoWidget`, multi-page) selects the sub-page by `region` through `incidents::source_for`; the *Other APIs* id `01M52E4R81RBEYD94QJACM67BQ` was verified identical on all four sub-pages on 2026-10-25. Fixtures: the captured quiet summary and component catalog, plus synthetic incident, unrelated-component and maintenance responses in the widget shape (no sub-page had a live incident). |
| RF9 | vNEXT | Adapter contract tests; the multi-sub-page incident tests are delivered with RF8. |
| RF10 | vNEXT | Delivered. |
| RF11 | — | Owner check: the owner runs the real-account procedure. |

## 1. Context

ElevenLabs sells text-to-speech, speech-to-text and voice-agent APIs against a monthly pool of **credits**. People who script audio generation, run voice agents or batch-dub content run out of credits mid-cycle, and the only way to know today is the web dashboard. The owner decision of 2026-10-23 asks for a dedicated ElevenLabs provider (PRD 0037, next to OpenRouter in PRD 0038).

**What the official documentation offers** (all checked on 2026-10-23; the response schema is quoted from the API reference):

| Need | Interface | Notes |
|---|---|---|
| Credits used, plan limit, next reset, tier, extra-usage settings and amount | `GET https://api.elevenlabs.io/v1/user/subscription` ([reference](https://elevenlabs.io/docs/api-reference/user/subscription/get)) | One call, no body, no query parameters. Required response fields: `tier`, `character_count`, `character_limit`, `max_credit_limit_extension` (integer or the string `"unlimited"`; `0` means usage-based billing is disabled), `can_extend_character_limit`, `current_overage` (`{ amount: string, currency }`), `status` (`trialing`, `active`, `incomplete`, `past_due`, `free`, `free_disabled`), voice-slot counters and limits, `open_invoices`, `has_open_invoices`. Optional and nullable: `next_character_count_reset_unix`, `billing_period`, `character_refresh_period` (`monthly_period`, `3_month_period`, `6_month_period`, `annual_period`), `currency` (`usd`, `eur`, `inr`, `pln`, `gbp`), `next_invoice`, `pending_change`. Deprecated: `max_character_limit_extension` and `allowed_to_extend_character_limit` (use `max_credit_limit_extension`). |
| Same subscription object plus personal data | `GET /v1/user` ([reference](https://elevenlabs.io/docs/api-reference/user/get)) | Nests `subscription`, but also returns `user_id`, `workspace_id`, `first_name` and an API-key preview. Cuotai deliberately does **not** use it (§6 Privacy). |
| Usage over time by product, model, voice, user | `POST /v1/workspace/analytics/query/usage-by-product-over-time` ([reference](https://elevenlabs.io/docs/api-reference/analytics/workspace/usage)) and the deprecated `GET /v1/usage/character-stats` ([reference](https://elevenlabs.io/docs/api-reference/usage/get)) | Time-bucketed credit usage with `group_by` dimensions; permissions are not stated on either page. Out of scope (§4). |
| Authentication | `xi-api-key` HTTP header on every request ([authentication](https://elevenlabs.io/docs/api-reference/authentication)) | A key can be restricted by **scope** (which endpoints it may call), by **credit quota** and, for Enterprise (preview), by IP allow-list (`403` from other addresses). Personal keys need a Full Seat; service-account keys are managed by workspace admins and do not expire ([workspace API keys](https://elevenlabs.io/docs/overview/administration/workspaces/api-keys)). The individual scope names are not published in the docs. |
| Errors | JSON `{"detail": {"type", "code", "message", "status", "request_id", "param"}}`; `code` identifies the cause and `status` is legacy ([errors](https://elevenlabs.io/docs/eleven-api/resources/errors)) | `401 authentication_error` (`invalid_api_key`, `missing_api_key`, `invalid_authorization_header`, `unauthorized`), `402 insufficient_credits`, `403 authorization_error` (`insufficient_permissions`, `forbidden`, `workspace_access_denied`, `feature_not_available`, `subscription_required`), `429 rate_limit_error` (`rate_limit_exceeded`, `concurrent_limit_exceeded`, `system_busy`), `500 internal_error`, `503 service_unavailable`/`maintenance`. |
| Rate limits | Concurrency per plan: Free 2, Starter 3, Creator 5, Pro 10, Scale 15, Business 15 ([429 help article](https://elevenlabs.io/docs/help-center/technical/api-error-code-429.md)) | The documentation says nothing about limits or credit cost for non-generation endpoints such as `/v1/user/subscription`. No `Retry-After` header is documented. |
| Residency | Default `api.elevenlabs.io`, plus `api.us.elevenlabs.io`, `api.eu.residency.elevenlabs.io`, `api.in.residency.elevenlabs.io`, `api.sg.residency.elevenlabs.io` | Listed as alternative servers on the reference pages; whether a key is bound to one region is not documented (§12). |
| Public status | [status.elevenlabs.io](https://status.elevenlabs.io), an incident.io page with one sub-page per region (United States, EU, India, Singapore) and groups ElevenAPI (*Other APIs*, *Speech to Text*, *Text to Speech*), ElevenAgents and ElevenCreative | `GET https://status.elevenlabs.io/api/v1/summary` answered `200 application/json` on 2026-10-23 with `subpages[].summary.{page_title, page_url, ongoing_incidents, in_progress_maintenances, scheduled_maintenances}`. |

**Credits versus characters.** ElevenLabs renamed its billing unit from *characters* to *credits* and states that "the value remains unchanged" ([billing](https://elevenlabs.io/docs/product-guides/administration/billing)). The API did not rename the fields: `character_count` and `character_limit` are the credits used and the credits in the plan for the current period, and the deprecation notes already point to `max_credit_limit_extension`. How many credits a generation costs depends on plan, product and model, which is irrelevant to Cuotai: it reads the counters, not the price list.

**Extra usage on ElevenLabs.** Three different mechanisms exist ([billing](https://elevenlabs.io/docs/product-guides/administration/billing), [usage-based billing](https://elevenlabs.io/docs/help-center/account/general/what-is-usage-based-billing.md)):

| Mechanism | Who has it | Visible in `GET /v1/user/subscription` |
|---|---|---|
| **Usage-based billing** (legacy): once the monthly quota is used, further credits are billed at a fixed price per 1,000, optionally capped, charged at cycle end or earlier when the balance exceeds twice the subscription price | Enterprise and legacy Creator, Pro, Scale and Business plans; not offered to new self-serve subscriptions | Yes: `max_credit_limit_extension` (cap, `"unlimited"` or `0`), `can_extend_character_limit`, `current_overage` (amount and currency) |
| **Pay As You Go**: prepaid credits (stored as a dollar value, valid 12 months) used after the subscription credits run out; the replacement for usage-based billing on self-serve plans | New self-serve subscriptions | **Not documented**: no field of the schema carries a PAYG balance |
| Enterprise overage contracts | Enterprise | Presumably through the same fields (unverified) |

## 2. Problem

- A voice or dubbing workload can burn the month's credits in an afternoon. Generation then fails (`402 insufficient_credits`) in the middle of a job, and the only early signal is the web dashboard.
- Cuotai monitors comparable providers (Kimchi credits, Cline balance, OpenCode Go windows) but not ElevenLabs, whose interface is **documented** and cheap to read.
- Extra usage (legacy usage-based billing) bills real money after the plan runs out; PRD 0023 exists to flag exactly that moment, and ElevenLabs reports it in one documented object.
- The API still says *characters* while the product says *credits*; showing the raw field names would confuse people, and showing a bare number without a unit would be worse.
- A new provider must declare its access level, catalog entry, example config and documentation or the build and the drift tests fail (PRD 0005 RF1.3, RF5).

## 3. Goals

- One new provider, `eleven_labs`, that reads the **credits used, plan limit and next reset** from the documented subscription endpoint with an API key, with no generation call and no credit spent by Cuotai.
- The credits meter behaves as any monthly plan quota: history, pace, threshold alerts, binding indicator, reset notices and exhaustion all work through the existing generic code.
- Extra usage (cap, credits beyond the plan, amount billed so far) is reported honestly and ready for PRD 0023's `billing = overage` semantics.
- Missing, unreadable or unexpected fields never turn into zero; every failure is classified without exposing keys, URLs or response bodies.
- The access descriptor, catalog entry, public-status source, example config, fixtures and documentation are delivered in the same change.

## 4. Non-goals

- Generating audio, listing voices or models, or any call that spends credits. The adapter performs exactly one `GET` per refresh.
- Usage analytics over time (breakdown by product, model, voice or user): `POST /v1/workspace/analytics/query/usage-by-product-over-time` and the deprecated `GET /v1/usage/character-stats` have undocumented permissions and a different purpose; history and statistics come from Cuotai's own local readings (PRDs 0006 and 0026). A later PRD may add them once the scope names are published.
- Pay As You Go balance, invoices, coupons, pending plan changes and the plan's price: none is exposed in a documented, stable way and invoices are sensitive. Cuotai says the PAYG balance is not reported rather than showing a partial picture (RF6.3).
- Reading `GET /v1/user`, per-key credit quotas, agent/conversation (ElevenAgents) minutes, or concurrency in use.
- A model catalog for ElevenLabs (PRD 0002): text-to-speech models are not language models, and PRD 0028's rule that model listing is gated on evidence applies. The provider ships with the model listing as `Unsupported` (RF1.5).
- Estimated cost from local logs (PRD 0015) and plan value (PRD 0030): ElevenLabs has no local CLI log, and the plan price is not in the API.
- Changing any ElevenLabs setting, including usage-based billing or a key's quota (Cuotai stays read-only).

## 5. Functional requirements

### RF1 — Provider identity, dispatch and account fields (high priority; prerequisite)

- **RF1.1** Add `Provider::ElevenLabs` to `model.rs` after `Kimchi` (before `Extension`): serde slug `eleven_labs` (the snake_case rule used by `open_ai_api` and `code_buddy`), label "ElevenLabs", added to `Provider::ALL`, whose length and every hard-coded "36" (tests, `docs/providers.md` header, `README.md`, `.github/ISSUE_TEMPLATE/provider-problem.yml`) become 37. `is_metered()` stays `false`: the query is a single non-generation `GET` and is not billed per query.
- **RF1.2** Dispatch in `lib.rs` to a new private adapter `providers/eleven_labs.rs`, `pub(crate) async fn fetch(account, ctx) -> Result<Data, QueryError>`, following `providers/kimchi.rs`: `secret(account, ctx, "ELEVENLABS_API_KEY")` (an overridable default name; the value must be verified against the vendor SDK documentation during implementation), `ctx.http`, `ctx.timeout`, the shared `providers::send` (HTTPS only, never reports URL or body), `Accept: application/json`.
- **RF1.3** The key travels only in the `xi-api-key` header; never in the URL or query string, never as `Authorization`. The base URL is one of a **fixed allow-list** chosen by the optional account param `region`: `default` (`https://api.elevenlabs.io`, the default), `us` (`https://api.us.elevenlabs.io`), `eu` (`https://api.eu.residency.elevenlabs.io`), `in` (`https://api.in.residency.elevenlabs.io`), `sg` (`https://api.sg.residency.elevenlabs.io`). An unknown value is a configuration error at `Config::validate`/draft validation time; a free-form URL is never accepted, so the key cannot be sent to an arbitrary host. Tests may override the base through the adapter's `_at(base)` seam only.
- **RF1.4** `account_edit.rs` publishes the provider's field descriptors: credential via environment-variable name (`credential_env`, default `ELEVENLABS_API_KEY`), no native login, and a `region` choice field (labels from the core catalogs, default `default`). Credential help id `credential_help.eleven_labs`: "Use an ElevenLabs API key. Restrict it to the permission that reads the account's subscription (it does not need text-to-speech access) and, if you use several workspaces, pick the key of the workspace you want to watch. Cuotai stores only the variable name." Validation reuses the shared rules; `params.region` is the only param.
  > **Amended by [PRD 0047](0047_typed-secrets-in-the-desktop-environment-variables-in-the-terminal.md) (2026-10-27):** the credential is a vault field in the desktop and a variable name (default `ELEVENLABS_API_KEY`) in the TUI.
- **RF1.5** The model-listing support function reports `Unsupported` for `eleven_labs` with the standard localized reason, so no model request is made.
- **RF1.6** `cuotai.example.toml` gains the account below, next to the other API-key providers, and the `--list-providers` output includes the new slug:

  ```toml
  [[accounts]]
  name = "ElevenLabs"
  provider = "eleven_labs"
  credential_env = "ELEVENLABS_API_KEY"
  # Optional data-residency endpoint: default, us, eu, in or sg.
  # params = { region = "eu" }
  ```

- **Acceptance criterion:** the workspace builds only with the new arms in every exhaustive `match`; a TOML round trip preserves the account and unrelated comments; `Config::validate` accepts the five regions and rejects `region = "https://evil.test"`; `--list-providers` prints `eleven_labs`; a `wiremock` test asserts the request is `GET /v1/user/subscription` with exactly one `xi-api-key` header equal to the configured value, no `Authorization` header, no query string and no body.

### RF2 — Access descriptor, catalog entry and credential guidance (high priority; prerequisite)

- **RF2.1** `access::classify` gets its `Provider::ElevenLabs` arm: `level = Official` (the endpoint is in the public API reference and the key header is in the public authentication page), `credential = InferenceKey` (the same kind of key that generates audio, which is why RF1.4 recommends a restricted one), `plan_requirement = None`, `scope = Account`, `metered = false`, `evidence = Documentation("https://elevenlabs.io/docs/api-reference/user/subscription/get")` with `verified_on = 2026-10-23`. No mode switches the interface (the region only changes the host), so no entry is added to the access tests' `MODES` list.
- **RF2.2** Catalog entries in `locales/en-US.json`, `es.json` and `it.json` (the Portuguese fallback in code): `access.summary.eleven_labs` ("Reads credits used, plan limit and next reset from the documented ElevenLabs subscription endpoint, plus extra usage when enabled.") and the credential help of RF1.4. `access.reason.eleven_labs` is **not** added: the provider is not `Unavailable`.
- **RF2.3** Catalog entry for PRD 0027 (when delivered): category `ApiPlatform`, `native_login = false`, `local_import = None`, `recommended = true`, hint derived from the credential class ("API key"; no override needed), aliases `elevenlabs`, `eleven labs`, `11labs`, `tts`, `text to speech`, `voice`, `voice agents`. Before PRD 0027 ships, the provider appears in the existing pickers like any other, with its level label.
- **RF2.4** `docs/providers.md`: the usage access matrix is regenerated with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix` and the plain run passes; the "Main integrations" table gets a row for `eleven_labs` (RF10).
- **Acceptance criterion:** the `access` tests pass for 37 providers (`Official` has a URL, no `Unavailable` reason is required, every id exists in the three catalogs, no text carries a credential); the drift test fails before regeneration and passes after; with PRD 0027, "eleven", "11labs" and "tts" find the entry in both editors.

### RF3 — Subscription query and the credits meter (high priority)

- **RF3.1** One request per refresh: `GET {base}/v1/user/subscription`. The adapter parses the body into a `serde_json::Value` and reads **only the fields it uses** (RF3–RF6); invoices, coupons, `pending_change` and every other field are never deserialized into a typed struct, copied into a metric or note, or logged.
- **RF3.2** Credits metric: label "Créditos do plano" (the Portuguese `label` convention of the neighbouring adapters; front ends localize through the unit and card), `kind = Quota`, `unit = "créditos"`, `used = character_count`, `limit = character_limit`, `remaining = max(0, limit − used)` only when both are present, `resets_at` per RF4. `used` keeps the provider's value verbatim even above the limit (extra usage, RF5).
- **RF3.3 (missing means unknown)** `character_count` absent, `null`, negative, non-finite or not a JSON number is `QueryError::schema()` (no metric invented). `character_limit` absent or invalid with a valid `character_count` yields a metric with `used` and no `limit`/`remaining`, plus the note `note.eleven_labs.limit_missing`. A `character_limit` of `0` is kept as `Some(0)` (it is a real value for disabled plans; `metrics.rs` already classifies it as exhausted without a cap).
- **RF3.4** Integers beyond `2^53` are accepted as the nearest `f64`; JSON numbers written as `1.0e4` are accepted; numeric strings are rejected as schema errors for credits (the documentation types them as integers).
- **RF3.5** Notes (all with stable ids and Portuguese fallbacks): `note.eleven_labs.source` ("Lido por GET /v1/user/subscription com a chave de API; a consulta não gera áudio."), `note.eleven_labs.tier` with the plan name when `tier` matches `^[a-z0-9_]{1,40}$` (otherwise omitted), and `note.eleven_labs.credits_terminology` is **not** emitted per refresh (the unit "créditos" and the documentation carry it).
- **Acceptance criterion:** fixtures `normal`, `near_limit`, `exhausted` and `missing_fields` of RF9: the credits metric has the expected numbers; an exhausted fixture classifies the account as `Exhausted` through `metrics::classify_report`; a body with `character_count` missing returns a schema error with no metric; `character_limit` missing leaves `limit`/`remaining` as `None` and never `0`.

### RF4 — Window, reset and eligibility for history, pace and alerts (high priority)

- **RF4.1** `resets_at = DateTime::from_timestamp(next_character_count_reset_unix, 0)` (the field is in **seconds**, unlike the analytics endpoints' milliseconds). `null`, absent, zero, negative or out-of-range values give `resets_at = None`; the adapter never derives a reset from `billing_period`.
- **RF4.2** `window` follows `character_refresh_period`: `monthly_period` gives `MetricWindow::Monthly`; `3_month_period`, `6_month_period` and `annual_period` give `Other`; an absent value gives `Other` (not assumed monthly; see decision 4 in §12). `period` stays `None`.
- **RF4.3** Consequences, all through existing generic code and tested with the fixture: a monthly credits metric with `resets_at` is a plan limit, so it takes part in `metrics::binding`, `pace::assess`, `alerts::evaluate` (threshold, exhausted, recovered), `resets::upcoming` (reset notices), the local history (PRD 0006), freshness (PRD 0010) and the statistics of PRD 0026. A metric with window `Other` or no `resets_at` is still shown and alerts on thresholds, but pace is not computed.
- **RF4.4** Rollover caveat: ElevenLabs states that up to two months of unused credits can roll over ([billing](https://elevenlabs.io/docs/product-guides/administration/billing)); the documentation does not say whether `character_limit` includes the rolled-over credits. Cuotai shows the pair as the provider reports it and `docs/providers.md` states this. A change in `character_limit` between two readings in the same cycle is not an error.
- **Acceptance criterion:** table tests on the five `character_refresh_period` values plus absent; `resets_at` for a seconds timestamp, `null` and `0`; a `pace::assess` test on the credits fixture (eligible when monthly with reset, not eligible with `Other`); a `resets::upcoming` test proving a reset notice is planned from `resets_at`; the credits metric becomes `binding` when the account has no other quota.

### RF5 — Extra usage (overage) metrics and state (high priority; aligned with PRD 0023)

- **RF5.1** Extra usage is **enabled** when `max_credit_limit_extension` is a positive integer or the string `"unlimited"`; **disabled** when it is `0`. If the field is absent or has another type, the state is unknown: no overage metric is emitted and no statement is made (the deprecated `allowed_to_extend_character_limit` and `max_character_limit_extension` are used only as a fallback when the new field is absent, `true`/positive meaning enabled and `false`/`0` disabled). `can_extend_character_limit = false` with a positive cap is a contradiction: the account is treated as disabled and the note `note.eleven_labs.overage_inconsistent` is added.
- **RF5.2** When enabled and the credits pair is known, emit **"Créditos extras usados"**: `kind = Usage`, `unit = "créditos"`, `used = max(0, character_count − character_limit)` (derived from two provider-reported values, so `0` is a real zero here and not an absent amount), `limit = max_credit_limit_extension` when it is an integer (`None` for `"unlimited"`, with the note `note.eleven_labs.overage_unlimited`), `remaining` = `max(0, limit − used)` when both are known, `window` and `resets_at` as in RF4. When the credits pair is incomplete, no extra-credits metric is emitted.
- **RF5.3** When `current_overage.amount` is a decimal string that parses to a finite non-negative number and `current_overage.currency` is one of `usd`, `eur`, `inr`, `pln`, `gbp` (falling back to the top-level `currency` when the nested one is absent), emit **"Custo do uso extra"**: `kind = Spend`, `unit` = the upper-case ISO 4217 code, `used` = the amount, `limit = None`, same window and `resets_at`. It is emitted when extra usage is enabled **or** the amount is greater than zero; when extra usage is disabled and the amount is zero, nothing is emitted. An unparsable amount omits the metric and adds `note.eleven_labs.overage_amount_unreadable`; it is never `0`.
- **RF5.4** Both metrics carry `billing = Billing::Overage` as soon as PRD 0023 RF1 exists. Until then they are `Usage`/`Spend`, which `metrics::is_plan_limit`-equivalent logic already excludes from binding, pace, threshold alerts and reset notices, so this PRD is safe to ship first and gains the flag in a one-line change when PRD 0023 lands. The credits metric of RF3 stays `Quota` with `Billing::Included`.
- **RF5.5** State mapping for `overage::assess` (PRD 0023 RF3): `Disabled` when RF5.1 says disabled; `Armed` when enabled and `used = 0`; `Active` when extra credits or an amount greater than zero are present; `CapReached` when a positive integer cap is reached (`used ≥ limit`); `Unknown` when the enablement cannot be established. The money and credit metrics are two items of the same account (different units), listed separately by `assess` as PRD 0023 RF3.1 specifies for mixed units; the headline amount is the money one when present, otherwise the credits.
- **RF5.6** Pay As You Go: its balance is **not supported** (no documented endpoint exposes it; owner decision of 2026-10-23). The UI states it as "not reported": every successful report carries the standing note `note.eleven_labs.payg_not_reported` ("Saldo Pay As You Go: não informado por este endpoint."), shown in the details (not as a card warning), so that a plan at 100 % is never read as "cannot generate" and a missing PAYG balance is never read as zero. No metric is emitted for it.
- **Acceptance criterion:** fixtures `extended_active`, `extended_unlimited`, `extension_disabled` and `armed` of RF9: right metrics, units, values, notes and (once PRD 0023 exists) `billing` and overage state; a disabled fixture emits no overage metric; an unparsable amount emits none and no zero; no fixture leaks URLs or tokens into notes; with PRD 0023, a fixture with the plan at 100 % and extra usage active produces one "extra usage started" alert per cycle and the extra-credits metric never becomes the binding limit.

### RF6 — Plan state and voice slots (medium priority)

- **RF6.1** `status` outside `active`, `free` and `trialing` adds a state note without changing the report status: `note.eleven_labs.status.past_due`, `note.eleven_labs.status.incomplete`, `note.eleven_labs.status.free_disabled` (the credits shown may no longer be usable). An unknown `status` string adds nothing. `has_open_invoices = true` adds `note.eleven_labs.open_invoices` (a boolean fact only; no amount, date or invoice content).
- **RF6.2** Voice slots as `kind = Usage` (never `Quota`, so they never become the binding limit or fire alerts): "Vozes personalizadas" (`voice_slots_used` of `voice_limit`, unit `vozes`) and "Vozes profissionais" (`professional_voice_slots_used`, the group-wide counter, of `professional_voice_limit`). Each is emitted only when both integers are present; absent values drop the metric. `window` and `resets_at` are `None`.
- **RF6.3** `pending_change`, `next_invoice`, `open_invoices`, coupon flags and `current_overage` beyond RF5 are not read.
- **Acceptance criterion:** fixtures `past_due` and `voice_slots` of RF9; absent counters drop the metric; an `Unknown` status string adds no note; voice metrics are never `binding`.

### RF7 — Error classification (high priority)

- **RF7.1** The adapter does not use the generic `json` helper, which discards the body; it uses `send` and classifies the HTTP status itself. On a non-success status it reads at most 16 KiB of the body, tries to parse `detail.code` (falling back to the legacy `detail.status`), and uses the string **only** to choose among fixed note ids from an allow-list; the provider's `message`, `request_id` and everything else are discarded.
- **RF7.2** Mapping:

  | HTTP / code | `Status` | Note id (fixed Portuguese fallback) |
  |---|---|---|
  | `401` (`invalid_api_key`, `missing_api_key`, `invalid_authorization_header`, `unauthorized`, anything else) | `AuthenticationError` | `note.eleven_labs.key_rejected` — check or recreate the key and the variable name |
  | `403` `insufficient_permissions`, `forbidden`, `feature_not_available`, `subscription_required` | `AuthenticationError` | `note.eleven_labs.scope_missing` — the key is valid but may not read the subscription; check its restriction (and, for Enterprise, its IP allow-list or workspace) |
  | `403` `workspace_access_denied` | `AuthenticationError` | `note.eleven_labs.workspace_denied` |
  | `402` | `Error` | the generic `query.http` note (not expected on this endpoint) |
  | `429` (`rate_limit_exceeded`, `concurrent_limit_exceeded`, `system_busy`, anything else) | `RateLimited` | `note.eleven_labs.rate_limited` — the core's existing backoff doubles the interval up to 1 h |
  | `5xx`, including `503` `maintenance` | `Error` | `query.http` (the incident source of RF8 adds context) |
  | `422`, malformed or non-object JSON, missing `character_count` | `Error` | `query.schema` |
  | Transport, timeout, plain HTTP to a remote host | the existing `QueryError::transport`/`timeout`/`insecure` | unchanged |

- **RF7.3** Redirects are not followed (the shared client policy); a `3xx` answer is an `Error` with `query.http`, so the header never reaches another host.
- **Acceptance criterion:** fixtures `unauthorized_401`, `forbidden_403`, `rate_limited_429` (both codes), `server_error_500`, `maintenance_503`, `malformed_json`, `redirect_302` and a body that echoes the configured key in `detail.message`: each maps to the table, and the test proves neither the key, nor the URL, nor the provider message appears in any note, `Debug` output or serialized `Report`.

### RF8 — Public incident status source (medium priority; PRD 0011)

- **RF8.1** `incidents::source` gets an `eleven_labs` arm. ElevenLabs publishes an incident.io widget summary at `https://status.elevenlabs.io/api/v1/summary` (verified `200 application/json` on 2026-10-23). Its shape differs from the flat summaries of OpenAI, Groq and Perplexity: it is `{ "subpages": [ { "subpage": "united-states" | "eu" | "india" | "singapore", "summary": { "page_title", "page_url", "ongoing_incidents", "in_progress_maintenances", "scheduled_maintenances" } } ] }`. The `IncidentIoWidget` parser is extended to accept this multi-page shape, selecting the sub-page by the account's `region` (`default` and `us` map to `united-states`, `eu` to `eu`, `in` to `india`, `sg` to `singapore`) and requiring the exact component selector below.
- **RF8.2** Scope: the **ElevenAPI → Other APIs** component (the group that hosts account endpoints such as the subscription). Text to Speech, Speech to Text, ElevenAgents and ElevenCreative incidents do not make this account's quota query unhealthy and are excluded, as PRD 0011 does for unrelated components. Before implementing, verify with a real read that the `affected_components[].id` of *Other APIs* is stable and identical or mapped per sub-page; if no stable per-region id can be established, register `None` and add an exclusion row to the registry instead of guessing.
- **RF8.3** Everything else is PRD 0011 behavior: opt-in, identified User-Agent, no credentials, five-minute cache, one request per URL shared across accounts, failure marked stale, no HTML scraping. The registry row (`eleven_labs`, source kind, endpoint link, component list, check date) and the verification paragraph go into `docs/providers.md`.
- **Acceptance criterion:** anonymized captured fixtures of the multi-sub-page summary (quiet, an *Other APIs* incident on one region only, an unrelated *Text to Speech* incident, maintenance); the `incidents` exhaustiveness test counts 37 providers; a failed query during an *Other APIs* incident gains `note.incident.ongoing`, while a *Text to Speech* incident does not.

### RF9 — Contract tests with `wiremock` fixtures (high priority)

Deterministic fixtures under the adapter's test module, local `wiremock` server only; none is proof of real-account access (RF11).

| Fixture | Body (relevant fields) | Expected |
|---|---|---|
| `normal` | `tier "starter"`, `status "active"`, `character_count 4000`, `character_limit 10000`, `next_character_count_reset_unix` 12 days ahead, `character_refresh_period "monthly_period"`, `max_credit_limit_extension 0`, `can_extend_character_limit false`, `current_overage {"amount":"0.00","currency":"usd"}` | One monthly `Quota` (4000 of 10000, remaining 6000, `resets_at`); no overage metric; tier note; the standing `payg_not_reported` note |
| `near_limit` | `9400` of `10000` | Binding at 94 % available 6 %; threshold alert at 90 % through `alerts::evaluate`; pace computed |
| `exhausted` | `10000` of `10000`, extra usage disabled | Account classified `Exhausted` |
| `armed` | `4000` of `10000`, `max_credit_limit_extension 5000`, amount `"0.00"` | Credits metric plus "Créditos extras usados" `used 0`, `limit 5000`; state `Armed` |
| `extended_active` | `10600` of `10000`, `max_credit_limit_extension 5000`, `current_overage {"amount":"0.18","currency":"usd"}` | Credits `used 10600`, `remaining 0`; extra credits `used 600 of 5000`; cost `0.18 USD`; state `Active` |
| `extended_cap_reached` | `15000` of `10000`, cap `5000` | Extra credits `5000 of 5000`; state `CapReached` |
| `extended_unlimited` | cap `"unlimited"`, count above limit | Extra credits without `limit`, note `overage_unlimited`; state `Active` |
| `extension_disabled_with_amount` | cap `0`, amount `"2.50"` | Cost metric still emitted (amount greater than zero); credits-extra metric omitted |
| `missing_fields` | `next_character_count_reset_unix: null`, `character_refresh_period` absent, `character_limit` absent, `current_overage` absent, `max_credit_limit_extension` absent | Credits metric with `used` only, `limit`/`remaining`/`resets_at` `None`, window `Other`; no overage metric; no zero anywhere |
| `no_count` | `character_count` absent | Schema error, no metric |
| `bad_types` | `character_count "1000"` (string), negative limit | Schema error / limit omitted per RF3.3 |
| `past_due` | `status "past_due"`, `has_open_invoices true` | Status note and open-invoices note; credits unchanged |
| `voice_slots` | slot counters present, one absent | Voice metrics only for complete pairs |
| `unauthorized_401` | `{"detail":{"code":"invalid_api_key","message":"…key sk_…"}}` | `AuthenticationError`, `key_rejected`, no echo |
| `forbidden_403` | `{"detail":{"code":"insufficient_permissions"}}` | `AuthenticationError`, `scope_missing` |
| `rate_limited_429` | `concurrent_limit_exceeded` and `system_busy` | `RateLimited` |
| `server_error_500`, `maintenance_503`, `malformed_json`, `redirect_302` | — | RF7 mapping |
| `request_shape` | any 200 | Method, path, single `xi-api-key`, no `Authorization`, no query, no body |

- **Acceptance criterion:** `cargo test -p cuotai-core eleven_labs` passes on Linux, macOS and Windows; no fixture contains a real key, e-mail, user id or invoice; the region allow-list has its own unit test (RF1.3).

### RF10 — Documentation, example config and changelog (medium priority)

- **RF10.1** `docs/providers.md`: a "Main integrations" row (`eleven_labs`; data queried "credits used of the plan limit, next reset, extra usage cap and amount, voice slots"; configuration `ELEVENLABS_API_KEY` or the account's variable, optional `params.region`; restrictions: "restrict the key to the subscription-read permission; Pay As You Go balance is not reported; unused credits may roll over and the limit semantics are the provider's"), the regenerated access matrix (RF2.4), the incident registry row (RF8.3), the "credits (not characters)" terminology note, the reference list entries, and the statement that Cuotai makes one non-generation `GET` per refresh and that the vendor does not document a credit cost or rate limit for it.
- **RF10.2** `README.md`: the supported-providers list and the authentication-caveats section name ElevenLabs. `cuotai.example.toml` as in RF1.6. `.github/ISSUE_TEMPLATE/provider-problem.yml` lists the new slug. `THIRD_PARTY_NOTICES` needs no entry: the contract comes from the vendor's public documentation, not from adapted third-party code.
- **RF10.3** The delivering `feat(provider)` commit carries the trailer `Changelog: ElevenLabs provider: credits used, plan limit, next reset and extra usage, read with an API key.` and `Refs: PRD 0037 RF1` (one line per requirement), writes `vNEXT` in this PRD's "Implementation status" and in its `docs/plans/0000_control.md` row, and touches no version location (PRD 0039; the release computes the minor bump).
- **Acceptance criterion:** the documentation drift tests and the commit gate (`cargo xtask changelog-gate`) pass.

### RF11 — Real-account verification (owner account available)

The owner has an ElevenLabs account (decision of 2026-10-23), so this RF is not blocked on access: the owner runs the procedure below once the adapter exists and records the answers. Only the delivery of the checklist depends on the owner.

- **RF11.1 Questions to answer and record in `docs/providers.md`:** (a) the minimal key restriction that allows `GET /v1/user/subscription` (the scope names are not published; a third-party guide mentions a "User" permission set to *Access*; the owner confirms on the real account, decision 1), (b) that repeated reads do not change `character_count` and that no credit is consumed, (c) whether `character_limit` includes rolled-over credits, (d) whether a key restricted by a per-key **credit quota** changes the numbers reported, (e) which of `character_refresh_period`, `billing_period` and `next_character_count_reset_unix` are present for the owner's plan, (f) whether a key is bound to a data-residency region and what the wrong region answers, and (g) what an account with usage-based billing active reports, if the owner's plan has it (otherwise the fixtures of RF9 stay modeled on the documented schema and the caveat stays in the docs).
- **RF11.2 Owner procedure (read-only, shapes not values):**
  1. Create two keys in the ElevenLabs dashboard: one unrestricted and one restricted to the narrowest permission that appears to cover the user/subscription read. Export each in the environment (`export ELEVENLABS_API_KEY=...`) or keep it in the OS vault; never paste it into a file, a shell history line, an issue or a commit.
  2. Note the credits shown in the ElevenLabs dashboard (Settings → Subscription or Usage) before starting.
  3. With the restricted key run `cuotai --config <test.toml> --once` (and again with `--json`) against the new `eleven_labs` account: expect the credits meter, the reset date and, if enabled, extra usage; a `403` must show `note.eleven_labs.scope_missing` (then widen the restriction one permission at a time and record which one fixes it, item (a)).
  4. Repeat the refresh ten times at the default cadence, then compare the dashboard credits with step 2: they must not differ except for the owner's own generation (item (b)). If the plan has a per-key credit quota, repeat with a quota-limited key (item (d)).
  5. Record the **shape** of the response with a throwaway local command (for example `curl -sS -H "xi-api-key: $ELEVENLABS_API_KEY" https://api.elevenlabs.io/v1/user/subscription | jq 'with_entries(.value |= type)'`), which prints only field names and JSON types, plus for each of `character_refresh_period`, `billing_period`, `next_character_count_reset_unix`, `max_credit_limit_extension` and `current_overage` whether it is present or `null`. Never record or share the full response, `open_invoices`, `next_invoice`, user or workspace identifiers, or the key.
  6. Try the wrong region (`params.region = "eu"` on a default-region key, and the reverse) and record the status and the Cuotai note (item (f)).
  7. Write the answers (a) to (g) in the PRD's implementation-status row and `docs/providers.md`; set the descriptor's `verified_on` to that day only if the shape matched the fixtures, otherwise open a fix.
- **RF11.3** Until RF11 is done the access descriptor's `verified_on` stays at the documentation-check date (2026-10-23) and the PRD status cannot become *Implemented*; PRD 0028's canary, once local, records the response *shape* (field names and types, never values) for this endpoint.
- **Acceptance criterion:** an owner checklist entry with the answers (a) to (g), a `--once` smoke on the real account showing the credits meter, the reset and (if enabled) extra usage, and a note that no credit was consumed by the reads.

## 6. Non-functional requirements

- **Honesty:** absent values stay `None`; "credits" is the unit; the extra-usage amount is the provider's `current_overage` and never an invoice; the PAYG balance is stated as not reported rather than inferred; derived values (extra credits = used − limit) are labelled in `docs/providers.md`.
- **Privacy and security:** only `/v1/user/subscription` is called, never `/v1/user` (which carries a user id, workspace id, first name and a key preview). Invoices, discounts, coupons and pending changes are never parsed. The key is read through `Credentials`, sent only in `xi-api-key` over HTTPS to a fixed host allow-list, kept out of snapshots, TOML, notes, logs and `Debug` output; the TOML stores only the variable name.
- **Cost:** one `GET` per refresh, no generation, not `metered`; the vendor documents no credit cost for it (to be confirmed by RF11.1(b)). The default cadence applies; the existing 429 backoff protects the account.
- **Compatibility:** TOML gains a provider slug and an optional `params.region`; `--json` and `export::v1` gain only the new provider's accounts and, with PRD 0023, the additive `billing` flag. Existing configurations are untouched.
- **Robustness:** unknown fields and values are ignored (the response has dozens of fields and the vendor is actively changing them, as the deprecated character-named fields show); unknown enum strings never fail the whole report.
- **i18n and accessibility:** every new text exists in the Portuguese fallback, `en-US`, `es` and `it`; state is conveyed by text, not color alone (design-system rule DS-PRI-6).
- **Maintainability:** one adapter file, one descriptor arm, one registry row; no per-provider code in the TUI or the desktop (AGENTS.md).

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `Provider` enum / TOML | New slug `eleven_labs`; optional `params.region` (`default`, `us`, `eu`, `in`, `sg`) | Additive |
| `--json`, `export::v1` | New provider accounts and metrics with `kind` `quota`, `usage`, `spend` and units `créditos`, ISO currency, `vozes` | Additive; with PRD 0023, `billing: "overage"` on the two extra-usage metrics |
| Access descriptor / catalogs | One arm, `access.summary.eleven_labs`, `credential_help.eleven_labs`, notes `note.eleven_labs.*` in `en-US`, `es`, `it` | Additive |
| Incident registry | `eleven_labs` entry; `IncidentIoWidget` accepts the multi-sub-page shape | Additive; existing sources unchanged |
| Docs and issue template | Matrix regenerated, new rows, slug list | Additive |
| `desktop.json`, events, widgets | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | enum, adapter, region allow-list, access arm and texts, account fields and validation, incident source and parser extension, fixtures |
| Desktop | renders the generic report; the account dialog shows the descriptor's fields and help (no ElevenLabs-specific code) |
| TUI/CLI | same, generic; `--once` and `--json` print the new metrics |
| Documentation | `docs/providers.md`, `README.md`, `cuotai.example.toml`, issue template, `CHANGELOG.md`, this PRD |

## 9. Dependencies and sequencing

RF1, RF2, RF3, RF4, RF7 and RF9 form the deliverable core and depend only on PRD 0005. RF5 ships safely before PRD 0023 (RF5.4) and gains the `billing` flag when PRD 0023 RF1 lands. RF6 and RF8 are independent and can follow in the same or a later release; if the multi-sub-page parser proves larger than expected, RF8 can ship as an exclusion row and be completed later without touching the rest. RF10 accompanies each step. RF11 runs after the first release candidate. In the owner's wave plan this PRD belongs to wave 5f (providers) with PRDs 0036 and 0038, after PRD 0023 so the overage flag is available.

## 10. Risks

| Risk | Mitigation |
|---|---|
| The character-named fields change meaning or are renamed (the vendor already deprecated two of them) | Read both new and deprecated overage fields (RF5.1); fixtures for absent fields; PRD 0028 shape canary; schema errors surface as "interface changed" with a documentation pointer |
| Scope names for a restricted key are undocumented, so users may create a key that cannot read the subscription | `scope_missing` note with guidance; RF11.1(a) records the exact permission; the credential help says to test the key |
| `character_limit` may or may not include rolled-over credits, so a "100 %" reading can mislead | Documented in `providers.md`; values shown exactly as reported; RF11.1(c) |
| Legacy usage-based billing is being retired and Pay As You Go is not exposed, so "exhausted" does not always mean "blocked" | `payg_not_reported` note (RF5.6); no claim about generation ability |
| The vendor documents no rate limit or credit cost for this endpoint | One call per refresh at the default cadence; backoff on 429; RF11.1(b) |
| A key is region-bound and a wrong `region` returns `401`/`403` | The `region` field with a help text; `key_rejected`/`scope_missing` notes suggest checking the region; RF11.1(f) |
| The status page is shaped per region and the component id may differ | Sub-page selection by `region`; verified-id rule or an exclusion row (RF8.2) |
| Reading the account endpoint leaks personal or billing data into reports | Only named fields are read; invoices and `/v1/user` excluded; the no-leak fixture |

## 11. Order of execution and verification

1. **RF1, RF2, RF7:** enum, dispatch, region allow-list, descriptor, error mapping and the request-shape test, with the matrix regenerated.
2. **RF3, RF4:** the credits meter and reset eligibility with fixtures `normal`, `near_limit`, `exhausted`, `missing_fields`.
3. **RF5, RF6:** extra usage and plan-state/voice-slot metrics; wire `billing` once PRD 0023 RF1 exists.
4. **RF8:** the incident source (or its exclusion row).
5. **RF9, RF10:** the full fixture matrix, documentation, issue template and the `Changelog:`/`Refs:` trailers with each step.
6. **RF11:** the owner's real-account check.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix` then the plain run, and a `--once`/`--json` smoke of the changed behavior against a local `wiremock` or fake server. The requirement is only delivered when a real account shows the correct credits, limit and reset in the TUI and the desktop, a pass over the exhausted fixture raises the existing exhaustion alert, a failure of each class (bad key, restricted key, rate limit, server error) shows its own explanation without exposing the key, and Cuotai spends no credits.

## 12. Decisions and open questions

### Decisions (owner, 2026-10-23)

| # | Question | Decision |
|---|---|---|
| 1 | Which scope or permission does a restricted key need for `GET /v1/user/subscription`? | Not published. Confirm on the owner's real account (RF11.1(a), RF11.2) and put the exact name in the credential help; until then the help says "the permission that reads the account's subscription". |
| 2 | Is real-account verification possible? | Yes: the owner has an ElevenLabs account. RF11 is owner-run, not blocked on access, and includes a read-only procedure that records response shapes and field presence only (RF11.2). |
| 3 | Pay As You Go balance? | Not supported (no documented endpoint). The UI states it as "not reported" (RF5.6). A documented field found later becomes a separate RF or PRD. |
| 4 | Treat an absent `character_refresh_period` as monthly so pace works? | No in v1: window `Other`, no pace (RF4.2). Revisit after RF11.1(e). |
| 5 | Default environment variable? | `ELEVENLABS_API_KEY`, overridable per account (RF1.2). |
| 6 | Region handling? | Explicit `region` param (`default`, `us`, `eu`, `in`, `sg`); no auto-discovery, which would send the key to several hosts (RF1.3). |
| 7 | Voice-slot metrics? | Keep them as low-noise `Usage` rows (RF6.2). |
| 8 | Headline of `overage::assess` for one credits item and one money item? | Money is the headline (RF5.5). |
| 9 | No stable status component id per region? | Register an exclusion row instead of guessing (RF8.2). |

### Still open

| Question | Recommendation |
|---|---|
| Should the usage-analytics endpoint (per-model or per-voice credit use) be added later? | Yes, as a separate PRD once permissions are published; it would feed PRD 0026's statistics, never the quota meter. |
| Does the owner's plan have legacy usage-based billing active? | If not, the extra-usage fixtures follow the documented schema and the caveat stays in the docs (RF11.1(g)). |

## 13. References

All consulted on 2026-10-23.

- [Get user subscription](https://elevenlabs.io/docs/api-reference/user/subscription/get) (`GET /v1/user/subscription`, response schema).
- [Get user](https://elevenlabs.io/docs/api-reference/user/get) (`GET /v1/user`, deliberately not used).
- [API authentication](https://elevenlabs.io/docs/api-reference/authentication) (`xi-api-key`, scope, credit quota and IP restrictions) and [workspace API keys](https://elevenlabs.io/docs/overview/administration/workspaces/api-keys) (user and service-account keys).
- [Errors](https://elevenlabs.io/docs/eleven-api/resources/errors) and [API error code 429](https://elevenlabs.io/docs/help-center/technical/api-error-code-429.md) (error body, status codes, concurrency by plan).
- [Billing](https://elevenlabs.io/docs/product-guides/administration/billing) (credits replace characters, monthly reset, two-month rollover, Pay As You Go), [What is usage-based billing](https://elevenlabs.io/docs/help-center/account/general/what-is-usage-based-billing.md) and [What happens when I run out of credits](https://elevenlabs.io/docs/help-center/account/general/what-happens-when-i-run-out-of-credits.md).
- [Usage by product over time](https://elevenlabs.io/docs/api-reference/analytics/workspace/usage) and [character stats (deprecated)](https://elevenlabs.io/docs/api-reference/usage/get) (not used).
- [ElevenLabs status](https://status.elevenlabs.io) and its widget summary `https://status.elevenlabs.io/api/v1/summary` (incident.io).
- Internal: PRD 0005 (access level), 0011 (incident status), 0023 (overage), 0027 (catalog), 0028 (contract health), `docs/architecture.md` "Adding a provider", `crates/cuotai-core/src/providers/kimchi.rs` (adapter pattern).
