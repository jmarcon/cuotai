# PRD 0038 — OpenRouter provider

| Field | Value |
|---|---|
| ID | 0038 |
| Status | Awaiting owner verification (RF1–RF14 delivered in vNEXT; the real-account unit check of RF5.4 and RF13.1 is the owner's, and RF10 is resolved as not available upstream) |
| Owner | `cuotai-core` (`model`, `providers/open_router.rs`, `access`, `account_edit`, `incidents`, `models`, locales), desktop and TUI (generic rendering only) |
| Consumers | `apps/desktop` (cards, details, account dialog, tray), `crates/cuotai-tui` (cards, details, `--once`, wizard), `export::v1` and `--watch` consumers, PRDs 0002, 0006, 0007, 0008, 0011, 0027 |
| Depends on | PRD 0005 (RF1: the mandatory access descriptor) |
| Benefits from | PRD 0023 (RF1: `Metric::billing` and the shared `is_plan_limit` predicate), PRD 0027 (catalog entry and search), PRD 0028 (drift paths and `verified_on` for the new interfaces), PRD 0036 (aggregator and routing section) |
| Coordinates with | PRD 0036 (9router and aggregator/routing providers): this PRD makes OpenRouter a first-class provider and owns its adapter and metrics; PRD 0036 owns the generic "router" grouping and any dedicated section or tab |
| Extends | PRD 0002 (one more verified model-catalog adapter), PRD 0011 (one more incident source) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. "Inference key" is an OpenRouter API key that can call the completion endpoints. "Management key" (formerly called a provisioning key) is a key that can only call account-administration endpoints and **cannot** make completion calls. "Credits" are OpenRouter's pre-paid balance, denominated in USD in the documentation of the key endpoint.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Provider variant, dispatch, adapter, `product` and `breakdown` validation, example TOML, issue template and desktop brand. |
| RF2 | vNEXT | Key cap, budget layers and reset, with fixtures F1 to F5. |
| RF3 | vNEXT | Spend by period and BYOK. |
| RF4 | vNEXT | Free-model counter (`Quota` for free-tier keys, `Usage` otherwise; `classify_report` no longer lets an exhausted `Usage` counter exhaust an account). |
| RF5 | vNEXT | Credit balance without a gauge, in "credits" (no `$`); RF5.4 (real-account unit check) is owner-blocked. |
| RF6 | vNEXT | Optional `/activity` breakdown by provider or model, cached per UTC day; failure keeps the balance. |
| RF7 | vNEXT | Error classification and wrong-key-class diagnosis. `429` is `RateLimited`; a `Retry-After` (seconds or HTTP date, capped at 24 h) rides on the note as the `retry_after_seconds` parameter (`QueryError::with_retry_after`, `Report::retry_after`) and `RefreshCycle` delays only that account when it exceeds the computed backoff. Generic: any adapter can opt in. |
| RF8 | vNEXT | Access arms (key and account), texts in every core catalog and the regenerated matrices. |
| RF9 | vNEXT | Eligibility verified by the fixtures; `overage::assess` is unchanged. |
| RF10 | vNEXT | Resolved as **not available upstream** (read-only check, 2026-10-25): `status.openrouter.ai` is a Datadog Status Pages SPA; `/api/v2/summary.json`, `/api/v2/components.json`, `/feed.rss`, `/index.json`, `/proxy/status.openrouter.ai` and `/robots.txt` return `403 AccessDenied`, `/history.rss` returns `404`, `/api/v1/summary` returns the HTML shell, and the page script reads an undocumented internal `config.json` asset (its only data routes) with no public API. `incidents::source` stays `None` by PRD 0011's never-guess rule; the registry document records the evidence. |
| RF11 | vNEXT | Live model listing for the key view (`offset` pages, `output_modalities=all`); `Unsupported` for the account view. |
| RF12 | vNEXT | One catalog entry (category Router, aliases `openrouter`, `open router`, `router`, `or`) with a native sign-in and a key mode; the product (key or account) is chosen in the account form instead of a second entry. |
| RF13 | vNEXT | Fixtures F1 to F23 in `providers/open_router.rs`; RF13.1 (real-account check) is owner-blocked. |
| RF14 | vNEXT | Documentation, example config, this status and the control row. |

## 1. Context

### 1.1 What the repository supports today

Checked on the `design/system-and-ui-refresh` worktree (based on `origin/main` 0.75.x) on 2026-10-23:

| Place | Result |
|---|---|
| `Provider::ALL` (`crates/cuotai-core/src/model.rs`, 36 providers; `VARIANTS` has 37 with `Extension`) | **No OpenRouter variant.** |
| `crates/cuotai-core/src/providers/*.rs`, including every `router_*.rs` (9router-derived contracts: `router_accounts`, `router_existing`, `router_grok`, `router_keys`, `router_minimax`, `router_sessions`) | **No mention** of OpenRouter. The `router_*` modules are 9router usage contracts for other vendors (OpenCode Go, Groq, Xiaomi MiMo, Qoder, Ollama Cloud, Kiro, CodeBuddy, Zed, Grok CLI, MiniMax, Gemini CLI), not routers. |
| `docs/providers.md`, `cuotai.example.toml`, `README.md`, locales, `access.rs`, `incidents.rs`, `models.rs`, desktop `lib.js` | `git grep -i 'openrouter\|open_router\|open-router'` over the whole tracked tree returns **no match** (0 files). |
| Docs under `docs/plans/` | OpenRouter appears only as a future idea in the owner decisions of 2026-10-23 (PRD 0036 "OpenRouter-like routers", PRD 0038 "dedicated provider PRDs"). |

So OpenRouter is **not supported, not even partially**. A user can today only point a generic extension (PRD 0017) at it. The closest existing adapters, used as patterns in this PRD, are Cline (`providers/coding.rs`: Bearer key, USD `Balance`, private `fn cline(account, ctx, base)` so `wiremock` can inject the base URL), Kimi (`Official`, inference key, balance) and Groq (`providers/router_keys.rs` plus `catalog.rs`: a verified `GET /models` listing).

### 1.2 What OpenRouter documents (research of 2026-10-23)

OpenRouter is an LLM router: one API and one pre-paid balance in front of many upstream providers. Its public API exposes the following read-only account data. All URLs are the official documentation.

| Endpoint | Credential | What it returns | Source |
|---|---|---|---|
| `GET https://openrouter.ai/api/v1/key` | **Inference key** (any key; "information about the API key used to authenticate the current request"); `Authorization: Bearer …` | `data.label` (masked), `limit` (USD, nullable), `limit_remaining` (USD, nullable), `limit_reset` (`daily`, `weekly`, `monthly` or `null`), `usage`, `usage_daily`, `usage_weekly`, `usage_monthly` (USD, UTC periods; weeks Monday to Sunday), `byok_usage` and its daily/weekly/monthly variants (external, not OpenRouter credits), `include_byok_in_limit`, `is_free_tier`, `is_management_key`, `is_provisioning_key` (deprecated), `expires_at` (nullable), `creator_user_id`, `organization_id`, `workspace_id`, `allowed_data_regions`, `free_model_daily_requests {used, limit, remaining}` (resets at UTC midnight), `budgets[]` and `effective_budget` (each `scope`, `limit_usd`, `spend_usd`, `remaining_usd`, `reset_interval`, `resets_at`, `include_byok`), and a deprecated `rate_limit` ("safe to ignore"). 200, 401, 500. | [Get current key](https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key), [API rate limits](https://openrouter.ai/docs/api-reference/limits) |
| `GET https://openrouter.ai/api/v1/credits` | **Management key only** (an inference key gets `403 "Only management keys can perform this operation"`) | `data.total_credits` (purchased) and `data.total_usage` (used), doubles. | [Get remaining credits](https://openrouter.ai/docs/api/api-reference/credits/get-credits.md), [Management API keys](https://openrouter.ai/docs/guides/overview/auth/management-api-keys.md) |
| `GET https://openrouter.ai/api/v1/activity` | **Management key only** | Last 30 **completed** UTC days, rows of `date`, `model`, `provider_name`, `endpoint_id`, `requests`, `prompt_tokens`, `completion_tokens`, `reasoning_tokens`, `cached_tokens`, `usage` (USD credits), `byok_usage_inference`; optional `date`, `api_key_hash`, `group_by=workspace`, `workspace_id`, `user_id`. | [Get user activity](https://openrouter.ai/docs/api/api-reference/analytics/get-user-activity.md) |
| `GET https://openrouter.ai/api/v1/models` | Bearer per the OpenAPI spec | Paginated (`offset`, `limit` 1 to 1000, default 500; `links.next`, `total_count`); `id`, `canonical_slug`, `name`, `description`, `pricing`, `context_length`, `supported_parameters`; `:free` variants are separate entries; `output_modalities` defaults to `text`. | [List models](https://openrouter.ai/docs/api/api-reference/models/get-models.md), [Free variants](https://openrouter.ai/docs/guides/routing/model-variants/free) |
| `GET https://openrouter.ai/api/v1/generation?id=gen-…` | Bearer | Cost, tokens and latency of **one** generation, which needs a generation id that only an inference call produces. | [Get generation](https://openrouter.ai/docs/api/api-reference/generations/get-generation.md) |
| OAuth PKCE (`https://openrouter.ai/auth`, `POST /api/v1/auth/keys`) | The user authorizes | Returns an ordinary API key bound to the user's account ("Inference is billed to that user's credits"); localhost callbacks are allowed. | [OAuth PKCE](https://openrouter.ai/docs/guides/overview/auth/oauth.md) |

Facts that shape the design:

- **Two credential classes, two views.** The account-level balance (`/credits`) and the per-upstream spend (`/activity`) need a **management key**, which cannot make completion calls; the key's own limit, spend windows and the free-model counter come from `/key` with an **inference key**. An inference key alone can therefore never show the account balance. `limit_remaining` is `null` when the key has no cap.
- **Free models.** Variants whose id ends in `:free` have 20 requests per minute and 50 requests per day for accounts with fewer than 10 credits purchased, 1000 per day with at least 10 (the tier uses all-time credits purchased, not `is_free_tier`). The daily counter uses the UTC day. The per-minute limit is not reported anywhere. For an exempt account or BYOK request, `free_model_daily_requests.remaining` "doesn't reflect an enforced ceiling". Source: [API rate limits](https://openrouter.ai/docs/api-reference/limits), [FAQ](https://openrouter.ai/docs/faq).
- **Resets.** Daily at 00:00 UTC, weekly on Monday 00:00 UTC, monthly on the 1st at 00:00 UTC; lifetime budgets never reset; a budget's `resets_at` is an ISO 8601 UTC instant or `null`.
- **Errors.** 401 missing or invalid key (an expired management key also returns 401, "API key expired"); 403 not a management key, or a budget exhausted; 402 on inference when the balance is negative ("including for free models") or on an in-flight budget; 429 `rate_limit_exceeded` from free-model or DDoS limits, with `Retry-After`; successful responses carry no `X-RateLimit-*` headers.
- **BYOK.** Requests with the user's own provider keys cost 5 % of the list price beyond a plan-dependent free allowance, deducted from credits; `byok_usage*` reports the external spend separately ([BYOK](https://openrouter.ai/docs/guides/overview/auth/byok.md), [FAQ](https://openrouter.ai/docs/faq)).
- **What the documentation does not say:** whether calls to `/key`, `/credits`, `/activity` or `/models` are billed (nothing documents a charge); whether `/key` has its own rate limit (only free-model limits and Cloudflare DDoS protection are described); the unit of `/credits` (the page says "credits"; the key endpoint says USD); whether `/models` accepts a request without a key; whether a management key may call `/key` or `/models`; what OpenRouter's status page exposes in machine-readable form (`status.openrouter.ai` returned 403 to the automated fetch used for this research, so its platform and feed are **unverified**).

### 1.3 Market expectation

OpenRouter is the most common router among developers who use several models, and the question people ask is "how many credits do I have left, and which key is burning them?". The data to answer it exists and is read-only, but spread across two credential classes; today Cuotai answers nothing.

## 2. Problem

- A user of OpenRouter cannot see credits, key limits or the free-model counter next to the rest of their AI accounts.
- A naive adapter would mislead: an inference key's `usage` is the **key's** spend, not the account's, and `limit: null` means "no cap", not "0 left". Treating an exhausted free-model counter as the account being blocked would be wrong when paid credits remain.
- The balance and the per-upstream breakdown need a different key class from the one people already have in their environment, and that distinction has to be explained, not discovered through a 403.
- Documentation gaps (billing of metadata calls, rate limit of `/key`, unit of `/credits`, status feed) must not be papered over with assumptions.

## 3. Goals

- Add OpenRouter as a first-class, read-only provider with two honest views: the **key** (inference key) and the **account** (management key).
- Show the key's cap with its reset, its spend by UTC day, week and month, the free-model daily request counter and, with a management key, the credit balance and an optional breakdown by upstream provider.
- Classify every error (wrong key class, expired key, rate limit, schema drift) and never turn an absent value into zero.
- Declare the access descriptor, incident source, model-catalog support and catalog entry required by PRDs 0005, 0011, 0002 and 0027, with `wiremock` fixtures for every shape above.

## 4. Non-goals

- Making any completion call, creating or deleting keys, changing limits or topping up credits (Cuotai stays read-only; management keys are used only for the two documented read endpoints).
- Per-generation cost (`/generation`), which needs ids that only inference traffic produces; cost from local logs stays PRD 0015.
- A native "Sign in with OpenRouter" (OAuth PKCE) in this PRD. It would store an ordinary inference key in the vault; it is an open question (section 12), not a requirement.
- Predicting the end-of-month bill, "days of credits left", or a **low-balance alert**: balances stay out of the threshold alerts (PRD 0008 §4, restated by PRD 0023 §4). RF9 lists the follow-up.
- Reading the per-minute free-model limit, BYOK fees or the account's upstream-provider keys (not exposed by the read endpoints).
- The generic aggregator section, tab or shared metric shape for routers: that belongs to PRD 0036. This PRD only supplies the data and the `Router` catalog category.

## 5. Functional requirements

### RF1 — Provider registration, account fields and example configuration (high priority; prerequisite)

- **RF1.1** Add `Provider::OpenRouter` to `model.rs` (`label` "OpenRouter", `slug` `open_router`, `ALL`, `VARIANTS`), the adapter `providers/open_router.rs` with the dispatch arm in `lib.rs`, and every exhaustive `match` the compiler then demands (`access::classify`, `incidents::source`, `models::support`, `Provider::is_metered` stays `false`). Counts in `docs/providers.md`, PRD text and tests move from 36 to 37 providers (38 variants).
- **RF1.2** The query interface is chosen by `params.product`: absent or `"key"` (default) queries `/key` with an inference key; `"account"` queries `/credits` (and, optionally, `/activity`, RF6) with a management key. Following the pattern of `product = "coding"` (Kimi) and `product = "computer"` (Perplexity), `account_edit` declares the field with a choice and conditional help, and `Config::validate` rejects any other value (`edit_error.*`, localized).
- **RF1.3** Credentials stay out of snapshots and TOML: `credential_env` names an environment variable (suggested `OPENROUTER_API_KEY` for `key`, `OPENROUTER_MANAGEMENT_KEY` for `account`) or the account uses a vault reference exactly like the other key providers; the field help states the class needed in each product (`credential_help.open_router`). A missing variable is `NeedsConfiguration`.
  > **Amended by [PRD 0047](0047_typed-secrets-in-the-desktop-environment-variables-in-the-terminal.md) (2026-10-27):** vault field in the desktop, variable name in the TUI.
- **RF1.4** Base URL is the constant `https://openrouter.ai` (private `fn` taking `base` so tests inject a `wiremock` URL, as `cline` does). There is no user-configurable URL. Only HTTPS to that host is allowed and a redirect to another host is a rejected response (`QueryError::insecure()`), so the key is never sent elsewhere.
- **RF1.5** `cuotai.example.toml` gains two commented examples (default `key` account and `product = "account"`), and the `Provider` list in `.github/ISSUE_TEMPLATE/provider-problem.yml`, the desktop brand map and the `official` group in `apps/desktop/ui/js/lib.js` (`B("OpenRouter", …)`, initials, official-list membership) gain the entry.
- **Acceptance criterion:** `Provider::from_slug("open_router")` round-trips; adding a variant without the `access`, `incidents` and `models` arms does not compile; `Config::validate` accepts both products and rejects an unknown one; the example TOML parses and validates; a table test proves `is_metered(OpenRouter)` is `false`.

### RF2 — Key view: key cap, effective budget and reset (high priority)

Product `key`: a single `GET /api/v1/key` per refresh.

- **RF2.1** If `data.limit` and `data.limit_remaining` are both finite non-negative numbers, emit `Metric` "Limite da chave" (Portuguese fallback; en-US "Key limit") with `kind = Quota`, `unit = "USD"`, `limit = limit`, `remaining = limit_remaining` (verbatim, not clamped), `used = limit − limit_remaining`. `limit: null` (or `limit_remaining: null`) emits **no** metric and is never "0 left" or "unlimited"; a `limit` without `limit_remaining` (or the reverse) is also omitted rather than guessed.
- **RF2.2** `window` and `resets_at` follow `limit_reset`: `daily` → `Daily`, next 00:00 UTC; `weekly` → `Weekly`, next Monday 00:00 UTC; `monthly` → `Monthly`, the 1st of the next month 00:00 UTC; `null` → `window = None`, `resets_at = None` (a lifetime cap); any other string → `window = Other`, `resets_at = None` (never guessed). When `budgets` contains an entry with `scope = "key_limit"` and a `resets_at`, that exact instant wins over the computed one.
- **RF2.3** Every other entry of `budgets` (guardrail, workspace, member, API-key scopes) becomes a `Quota` metric in USD ("Orçamento (<scope>)"), with `limit_usd`, `spend_usd`, `remaining_usd` verbatim and the window from `reset_interval` (`null` → `None`), `resets_at` verbatim; the `key_limit` entry is not duplicated. `effective_budget` identifies which metric blocks requests first; it is not a separate metric, and a note names its scope when it is not the key limit. A `remaining_usd` ≤ 0 is documented as "requests are rejected with 403 until reset", so it classifies as exhausted with `blocked_until` = `resets_at` when known.
- **RF2.4** Unknown scope strings and extra fields are accepted (the response grows over time); `budgets`, `effective_budget` and `free_model_daily_requests` are optional so an older or partial payload still works from the top-level fields.
- **RF2.5** A note (`note.open_router.key_scope`) states once that these numbers describe **this key**, not the account balance, and points to the `account` product for the balance. It never includes the key label, which the API returns partially masked.
- **Acceptance criterion:** fixtures F1 to F5 and F11 (RF13) assert kind, unit, window, `resets_at`, `used`/`remaining` and that a `null` limit yields no Quota; the binding metric of an account with a 2.1 % key cap left is that cap; no note contains the label or the key.

### RF3 — Key view: spend by period and BYOK (medium priority)

- **RF3.1** From `usage_daily`, `usage_weekly`, `usage_monthly` and `usage` emit `Spend` metrics in USD ("Gasto hoje", "Gasto na semana", "Gasto no mês", "Gasto total da chave") with `window` `Daily`, `Weekly`, `Monthly` and `None`, `used` = the value, `limit`/`remaining` = `None`, and `resets_at` of the next UTC boundary for the three windowed ones. An absent field omits its metric.
- **RF3.2** From `byok_usage_monthly` and `byok_usage` emit `Spend` metrics "BYOK no mês (cobrado pelo provedor)" and "BYOK total", with a note (`note.open_router.byok`) that this is spend on the user's own upstream keys, not OpenRouter credits, and that a 5 % fee beyond the plan's free allowance is deducted from credits and is not reported by the API. `include_byok_in_limit = true` adds a note that BYOK counts toward the key cap.
- **RF3.3** `Spend` metrics never take part in binding, pace or threshold alerts (existing eligibility rules); they are values for the card, details, history and exports.
- **Acceptance criterion:** fixtures assert each window and `resets_at` at a fixed clock (Sunday 23:59 UTC to Monday 00:00 UTC for the week; 31 December to 1 January for the month); missing fields produce no metric.

### RF4 — Key view: free-model daily counter (high priority)

- **RF4.1** When `free_model_daily_requests` has integer `used`, `limit` and `remaining` (all non-negative) and `limit > 0`, emit "Modelos gratuitos (:free): requisições hoje" with `unit = "requisições"`, `window = Daily`, `resets_at` = next 00:00 UTC, `used`, `limit`, `remaining` verbatim. A zero or missing `limit` omits the metric (the documentation says it is not an enforced ceiling for exempt accounts).
- **RF4.2** **Kind depends on whether the free models are the account's only way in.** When `is_free_tier = true`, the kind is `Quota`: the counter really blocks the key, so it takes part in binding, pace, threshold alerts and reset notices. When `is_free_tier = false` (or absent) the kind is `Usage`: the counter is shown but an exhausted `:free` counter must not mark an account with paid credits as blocked. The kind is assigned by the adapter from the field, never from the label.
- **RF4.3** A note (`note.open_router.free_tier`) states the documented tiers (50 or 1000 requests per day depending on credits purchased, 20 per minute not reported) and that the tier follows all-time purchased credits, not `is_free_tier`.
- **Acceptance criterion:** fixtures F6 to F8: free-tier key at 12 of 50 is a `Quota` with ratio 0.76 and pace/alert eligible; the same key at 50 of 50 is exhausted with `blocked_until` the next UTC midnight; a paid key at 1000 of 1000 stays `Usage`, is not binding and leaves the account `Ready`; `limit = 0` omits the metric.

### RF5 — Account view: credit balance (high priority)

Product `account`: `GET /api/v1/credits` with a management key.

- **RF5.1** Emit "Saldo de créditos" as `kind = Balance`, `unit = "USD"`, `remaining = total_credits − total_usage` (negative preserved, as Cline does, because OpenRouter documents negative balances rejecting even free models), `used = None`, `limit = None`, no window, no `resets_at`. **No gauge is drawn**: `total_credits` is cumulative over the account's lifetime, so a used/purchased ratio would shrink with every top-up and mislead.
- **RF5.2** Emit "Créditos usados (total)" as `Spend`, `used = total_usage`, same unit, no window. Both fields must be present and finite; either one missing is `Error` with `query.schema`, because a balance computed from half the data would be wrong.
- **RF5.3** A note (`note.open_router.credits_unit`) says the value is `total_credits − total_usage` as OpenRouter reports it and that the credits endpoint documents only "credits"; the unit label `USD` and the `$` sign are shown only after the owner confirms the 1 credit = 1 USD assumption (RF5.4). Until then the front ends show "créditos" without a currency sign.
- **RF5.4 (owner-blocked)** Confirm with a real management key that `total_credits` and `total_usage` are USD-valued credits (the key endpoint documents USD, the credits endpoint does not) and that a topped-up amount matches the dashboard balance; also confirm that repeated calls do not change the balance. Same pattern as PRD 0023 RF2.6.
- **RF5.5** A balance ≤ 0 classifies the account as exhausted through the existing `Balance` rules (`metrics::classify_report`), consistent with the documented 402 behavior; a balance above 0 never binds. No threshold alert exists for balances.
- **Acceptance criterion:** fixtures F9 to F12: 100.50 purchased and 25.75 used gives 74.75; low credits (0.80); negative (−1.50 preserved); missing `total_usage` is a schema error; no `used`/`limit` is set on the balance metric; an inference key receiving `403` yields the RF7.2 note.

### RF6 — Account view: optional spend breakdown by upstream provider (medium priority; opt-in)

OpenRouter does expose a per-upstream breakdown: `/activity` rows carry `provider_name` and `model`. This is the only per-upstream data OpenRouter documents.

- **RF6.1** `params.breakdown` (absent or `"off"` by default, `"provider"`, `"model"`) is valid only with `product = "account"`; it adds one extra `GET /api/v1/activity` (no parameters) to the refresh.
- **RF6.2** Rows are summed per `provider_name` (or `model`) over the returned days; free (`usage = 0`) rows are skipped; the five largest become `Spend` metrics in USD ("Gasto 30 dias — <provider>") with `window = Other` and `period = "30 dias concluídos (UTC)"`, the rest is one "Outros" metric; a row with `null` or empty `provider_name` counts under "Outros". `byok_usage_inference` is summed apart ("BYOK (externo)") and never mixed into the credits figure. Today's partial day is not in the response, which the note `note.open_router.activity_window` states ("completed UTC days only; updates after midnight UTC").
- **RF6.3** Bounded and cheap: at most one `/activity` request per account per UTC day per process plus an in-memory cache of at least one hour, so manual and scheduled refreshes do not re-query it; a response over 10 000 rows or 5 MB is a schema error (`QueryError::schema()`) rather than silently truncated; no pagination is assumed because none is documented.
- **RF6.4** Per-upstream metrics are `Spend` and so never binding or pace-eligible. When `Metric::scope` exists (PRD 0023 amendment MET-01), these metrics set it to the upstream so "best account now" ranking (owner decision C) ignores them. Their grouping in a router section is PRD 0036's decision.
- **RF6.5** A failure of `/activity` (403, 5xx, schema) does **not** fail the account: the balance from RF5 still shows and a note (`note.open_router.activity_failed`) says the breakdown is unavailable.
- **Acceptance criterion:** fixtures F13 to F16: three providers across two days are summed correctly; empty `data` shows no breakdown and no zero; rows with null `provider_name`; 6 providers collapse into five plus "Outros"; the cache serves a second refresh with zero extra requests (`wiremock` `expect(1)`); `/activity` 403 leaves the balance intact.

### RF7 — Error classification and honesty (high priority)

- **RF7.1** Mapping, reusing `providers::json` and adding OpenRouter-specific notes without ever including a response body, URL or key: `401` → `AuthenticationError` ("key missing, invalid or expired", `query.open_router.auth`); `403` → `AuthenticationError`; `429` → `RateLimited` (the next scheduled refresh retries, there is no inner retry loop, and a `Retry-After` header larger than the account's refresh interval is honored by delaying that account only); `402` and other 4xx → `Error` with `query.http`; `5xx`, transport and timeout → `Error` with the existing transport/timeout notes; a body that does not parse, a missing `data` object, or a `data` object with none of the known groups (`limit*`, `usage*`, `free_model_daily_requests`, `budgets`) → `Error` with `query.schema`; non-finite or negative figures → `query.schema`.
- **RF7.2** **Wrong key class is diagnosed, not reported as a generic 403.** `product = "account"` receiving `403` with the documented "management key" meaning gets `query.open_router.management_key_required` ("this operation needs a management key; create one at openrouter.ai/settings/management-keys, or use the default key view"). `product = "key"` with a response that has `is_management_key = true` still works for the fields present and adds a note that this key cannot make completion calls. An expired management key (`401 "API key expired"`) says to create a new one; the message text of the API is never copied.
- **RF7.3** Metrics from a failed query are never replaced by zeros; the existing last-reading and stale-reading rules (PRD 0010) apply.
- **RF7.4** Privacy: `label`, `creator_user_id`, `organization_id` and `workspace_id` are parsed only if needed and are never stored in `Report`, notes, `--json`, `export::v1`, history or events.
- **Acceptance criterion:** fixtures F17 to F22 assert status and note ids for 401, 403-wrong-class, 402, 429 (with `Retry-After`), 500, truncated JSON and a payload with unknown top-level shape; a no-leak test greps every note, `Debug` output and error for the key, `Bearer`, the label and the base URL.

### RF8 — Access descriptor, texts and generated matrix (high priority; PRD 0005)

- **RF8.1** In `access::classify` add arms (exhaustive match, no wildcard): `OpenRouter` default (`product = "key"`): `Official`, `InferenceKey`, `Scope::Account`, evidence `doc("https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key")`, summary `access.summary.open_router` ("Key limit, spend by period and free-model request counter through the documented key endpoint"); `OpenRouter` with `product = "account"`: `Official`, `AdminKey` (the management key), `Scope::Account`, evidence `doc("https://openrouter.ai/docs/api/api-reference/credits/get-credits")`, summary `access.summary.open_router_account` ("Credit balance and optional spend by upstream provider through the documented management endpoints"). `verified_on` is the date of the real-account check of RF13.
- **RF8.2** `credential_hint` and `risk` texts say: the default view needs an **inference** key and shows only that key's numbers; the account view needs a **management** key, which cannot make completion calls; a management key can also create and delete keys, so it is more powerful than the inference key and should live in the vault, not in a shared shell profile (`access.risk.open_router_management_key`).
- **RF8.3** `metered = false`: the documentation states no per-call charge and `is_metered` requires proof of billing (`model.rs`). The uncertain points (billing of metadata calls, rate limit of `/key`) are RF13 verification items; if the real-account check shows a charge or a limit, this changes and `METERED_MIN_REFRESH_SECONDS` applies.
- **RF8.4** Add `access.summary.open_router`, `access.summary.open_router_account`, `access.risk.open_router_management_key` and `credential_help.open_router` to every core catalog present at delivery (today `en-US.json`, `es.json`, `it.json`; Portuguese fallback in code), and the new `field.open_router.*` and `note.open_router.*`/`query.open_router.*` ids. The modes `product = "account"` and (RF6) `breakdown` go into the `MODES` list of the `access` tests.
- **RF8.5** Regenerate the matrix: `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix`; the plain run must pass afterwards. `docs/providers.md` also gets the main-integrations row, the "37 providers" count and the PRD 0023 "Overage" cell `not reported`.
- **Acceptance criterion:** `access` tests cover both modes (`Official` carries a documentation URL, no text carries a credential), every new id resolves in every catalog, the drift test passes after regeneration and fails when the level is flipped.

### RF9 — Billing, overage, pace, alerts and history semantics (medium priority)

- **RF9.1** (PRD 0023) OpenRouter has no overage concept: usage is deducted from pre-paid credits. Every metric is `billing = Included` (the default); the credit balance is a `Balance`, not overage (PRD 0023 §4); key caps and budgets are user-set limits, not extra billing. `overage::assess` returns `None` and the matrix cell reads `not reported`. Negative balances are shown verbatim and never described as "extra usage".
- **RF9.2** (PRDs 0007, 0008, 0009) Only `Quota` metrics with a positive finite `limit` and a window take part: the key cap (when `limit_reset` is `daily`, `weekly` or `monthly`), scoped budgets with a `reset_interval`, and the free-tier counter (RF4.2). They get pace lines, threshold alerts, `Exhausted`/`Recovered` and reset notices; lifetime caps (no `resets_at`) are eligible for binding and threshold alerts but not for pace. The balance, spend windows, BYOK and breakdown never are.
- **RF9.3** (PRD 0006) The local measurement history stores all kinds as today, so balance and spend series exist for the statistics of PRD 0026 with no new code; nothing in this PRD adds a balance trend or "days of credits left". That, and a low-balance alert, are listed as a follow-up for PRD 0024/0008 and need an owner decision on the threshold in dollars (section 12).
- **RF9.4** No reset notice for a balance: it has no reset.
- **Acceptance criterion:** table tests with the fixtures prove binding, pace and alert eligibility for each metric above and ineligibility for every `Spend`, `Balance` and `Usage` metric; `overage::assess` is `None` for all fixtures.

### RF10 — Incident status (PRD 0011) (low priority; gated on evidence)

- **RF10.1** `incidents::source(Provider::OpenRouter)` returns the OpenRouter status page **only after** it is verified at implementation per PRD 0011 RF1.2: the platform and its public machine-readable feed (Statuspage v2 `/api/v2/summary.json`, incident.io widget, RSS or other), the stable component selectors for the API (chat completions, data API, authentication) and the terms of use, each read over the real network and recorded in the source registry with its `verified_on` date. The research for this PRD could not read `status.openrouter.ai` (403 to an automated fetch) and found third-party trackers only, so **nothing is assumed**.
- **RF10.2** Until verified, the arm returns `None` ("no source", not "no incidents"), exactly like the 19 providers without a verified source. The scope, when verified, is the API components only: a page-wide event must not mark the account degraded, and an OpenRouter incident says nothing about the upstream providers it routes to.
- **RF10.3 (resolved 2026-10-25, not available upstream)** The read-only investigation found no machine-readable source (evidence in §0 RF10 and in the exclusions table of `docs/providers.md`); the arm stays `None` and the unit test asserts it.
- **Acceptance criterion:** the exhaustive `incidents::source` test has an `OpenRouter` entry; with a source, anonymized captures cover operational, degraded and a page-wide event outside the scope; without one, the unit test asserts `None` and the registry document lists it as not verified.

### RF11 — Model catalog (PRD 0002; CAT-01 evidence rule) (medium priority)

- **RF11.1** `models::support` for `OpenRouter` with `product = "key"` is `live` with source `GET /api/v1/models`, backed by the official documentation above as the CAT-01 evidence (documented, read-only, no inference). `product = "account"` is `Unsupported` with a reason ("a management key is not documented to list models; use an inference key"), zero network.
- **RF11.2** The adapter follows `models::paginate` with `offset`/`limit` (limit 500), stops on `links.next = null` or after `total_count` ids, rejects a repeated page or a count mismatch with `query.schema`, deduplicates ids and keeps the real ids (`openai/gpt-4`, `…:free`). It requests `output_modalities=all` so embedding, image and audio models are not hidden by the documented default of `text` (open question 4). It sends the inference key and makes no call during a quota refresh (PRD 0002 §6).
- **RF11.3** The listing does not prove access: guardrails, allowed-model and data-region policies can still reject a listed model, and the UI says so with the existing catalog caveat. `pricing` and `context_length` are not stored in v1 of the catalog (the PRD 0002 `ModelInfo` has id, name and description only).
- **Acceptance criterion:** `wiremock` fixtures: two pages with `:free` variants, one page, empty list, repeated `links.next`, `total_count` mismatch, 401 and 429 preserving the previous cache; `r`, `--once` and `--json` trigger no model call; `Unsupported` for `account` makes no request.

### RF12 — Catalog entry and the router section (PRD 0027, coordinated with PRD 0036) (medium priority)

- **RF12.1** `account_edit::catalog()` gets two entries: OpenRouter (`key`, recommended: the user already has an inference key) and "OpenRouter — account balance" (`account`), category `Router` (a category already named in PRD 0027 RF1.1), aliases `openrouter`, `open router`, `router` and `or`, one-line hints derived from RF8.2 ("Key limit and free requests with your API key" / "Credit balance with a management key"). The search of RF2.1 of PRD 0027 finds both by "openrouter".
- **RF12.2** No presence detection for OpenRouter: the PRD 0027 RF6 table covers files in known tool locations, and an environment variable is not a reviewed path (and a GUI-launched desktop app does not see the shell environment). Open question 5 proposes a consented OpenRouter-specific variable probe.
  > **Amended by [PRD 0047](0047_typed-secrets-in-the-desktop-environment-variables-in-the-terminal.md) (2026-10-27):** variable detection and variable wording are terminal-only; the desktop never detects or mentions variables.
- **RF12.3** PRD 0036 (aggregators) owns the router grouping: the two OpenRouter accounts, and the optional RF6 breakdown, appear in whichever dedicated section or tab 0036 defines. Until 0036 lands they render as ordinary cards, with `Router` only a catalog category. This PRD adds nothing that depends on 0036; 0036 adds nothing here.
- **Acceptance criterion:** catalog and search tests find the entries by name, alias and slug; recommended mode opens `key`; the doc table in `docs/providers.md` lists the aliases; no TUI or desktop branch on `Provider::OpenRouter`.

### RF13 — Contract fixtures, deterministic tests and the real-account check (high priority)

`wiremock` fixtures live beside the adapter in `#[cfg(test)]` blocks (names in the local style, the base URL injected). Each asserts kind, unit, window, `resets_at` (fixed clock), the values, that missing stays `None`, and that no note contains the key, a URL or a body.

| ID | Shape | Assertion |
|---|---|---|
| F1 | `/key` normal: limit 100, remaining 74.5, `monthly`, usage windows, `is_free_tier=false` | Quota 25.5 used / 74.5 left, monthly, `resets_at` 1st of next month 00:00 UTC; four Spend metrics |
| F2 | Low key credits: remaining 2.10 of 100 | Binding is the key cap; ratio 0.021; threshold alert eligible |
| F3 | Key cap exhausted: remaining 0 (and a negative `remaining_usd` in `budgets`) | Exhausted, `blocked_until = resets_at`, remaining verbatim |
| F4 | No limit set: `limit`, `limit_remaining`, `limit_reset` all `null` | No Quota; Spend metrics still present; account `Ready`, not exhausted; key-scope note |
| F5 | Lifetime cap: `limit` 50, `limit_reset` `null` | Quota with `window=None`, no `resets_at`; no pace; binding allowed |
| F6 | Free tier: `is_free_tier=true`, `free_model_daily_requests` 12/50 | Quota, Daily, next UTC midnight |
| F7 | Free tier exhausted 50/50 | Exhausted, `blocked_until` next UTC midnight |
| F8 | Paid key with 1000/1000 `:free` requests; and `limit = 0` | `Usage`, never binding, account stays `Ready`; second case omits the metric |
| F9 | `/credits` normal: 100.5 purchased, 25.75 used | Balance 74.75, no `used`/`limit`; Spend 25.75 |
| F10 | `/credits` low: 100.0 and 99.2 | Balance 0.8 |
| F11 | `/credits` negative: 10 and 11.5; and `budgets` with guardrail/workspace layers plus an unknown `scope` | Balance −1.5 preserved; one Quota per layer, unknown scope accepted |
| F12 | `/credits` missing `total_usage` or non-finite | `Error`, `query.schema`, no metric |
| F13 | `/activity` three providers, two days | Per-provider sums, `byok` apart |
| F14 | `/activity` empty `data` | No breakdown, no zero |
| F15 | `/activity` six providers plus `null` provider_name | Top five plus "Outros"; free rows skipped |
| F16 | `/activity` 403 while `/credits` is 200; and a second refresh within an hour | Balance intact, breakdown note; `expect(1)` request count |
| F17 | 401 on `/key` and an expired management key | `AuthenticationError`, expiry note |
| F18 | 403 on `/credits` with an inference key | `AuthenticationError`, `management_key_required` |
| F19 | 402 | `Error`, `query.http`, no body text |
| F20 | 429 with `Retry-After` | `RateLimited`, that account is delayed |
| F21 | 500 and timeout | `Error`, existing notes |
| F22 | Truncated JSON; `data` absent; unknown top-level shape; unknown `limit_reset` (`yearly`); extra unknown fields; redirect to another host | `query.schema` for the first three, `Other` window without `resets_at` for the fourth, ignored fields, rejected redirect |
| F23 | `/models` two pages with `:free` ids, repeated `links.next`, count mismatch | RF11 |

- **RF13.1 (owner-blocked)** One real-account verification with both key classes, recorded as `verified_on` in the access descriptor (the local canary of PRD 0028 may later produce the report): the shapes of `/key`, `/credits` and `/activity`; the USD meaning of `/credits`; that a month of polling at the default cadence neither changes the balance nor draws a `429`; whether a management key may call `/key` and `/models`; whether `/models` needs a key; and the real status-page platform (RF10). Mocked HTTP is not proof of real-account access.
- **Acceptance criterion:** all fixtures pass in `cargo test --workspace`; the shape check of PRD 0028 (schema-drift diagnostics) names the paths it relies on (`data.limit`, `data.limit_remaining`, `data.limit_reset`, `data.free_model_daily_requests`, `data.budgets`, `data.total_credits`, `data.total_usage`) so the owner's local canary can compare them.

### RF14 — Documentation, changelog and plan control (low priority)

- **RF14.1** `docs/providers.md` (main-integrations row, generated matrix, model-catalog matrix), `README.md` (supported providers), `cuotai.example.toml`, `docs/architecture.md` (count), the `credential_help` and a short "OpenRouter: which key do I need?" paragraph explaining inference key versus management key.
- **RF14.2** The `feat(provider)` commit carries a `Changelog:` trailer (one sentence from the user's point of view) and `Refs: PRD 0038 RF1` (one line per requirement delivered), writes `vNEXT` in this PRD's status and its `docs/plans/0000_control.md` row, and touches no version location (PRD 0039). `THIRD_PARTY_NOTICES` needs no entry: no upstream code is adapted, the contract comes from OpenRouter's public documentation.
- **Acceptance criterion:** the documentation drift tests and the commit gate pass.

## 6. Non-functional requirements

- **Honesty:** a key's numbers are never presented as the account's; `limit: null` is "no cap", not zero; a missing field is `None` and shown as "—"; the `$` sign for `/credits` waits for RF5.4; nothing says an exhausted `:free` counter blocks a paid account.
- **Privacy and security:** the keys exist only in the process or the vault; the API's `label`, `creator_user_id`, `organization_id` and `workspace_id` never reach reports, notes, history, exports or events; HTTPS only to `openrouter.ai`, no redirect to another host; a management key is documented as the more powerful credential.
- **Cost and rate:** no inference calls, so no credits are spent by Cuotai as far as the documentation shows; one request per refresh for either product, at most one extra `/activity` per UTC day (RF6.3); `/models` only by manual sync. The billing and rate limit of the metadata endpoints are unverified (RF13.1). The default cadence is unchanged.
- **Compatibility:** TOML gains only the provider slug and the `product` and `breakdown` params; `Report`, `--json` and `export::v1` gain nothing new beyond the new provider value (the `provider` enum is additive, the JSON Schema and reference output are regenerated if they list slugs); no `BREAKING CHANGE`.
- **i18n and accessibility:** every new text exists in every core catalog present at delivery with the Portuguese fallback; state is conveyed by text and icon, not color (DS-PRI-6).
- **Maintainability:** one private adapter module, no per-provider branch in the TUI or the desktop; unknown response fields are tolerated, required ones are listed in RF13 for drift detection.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `Provider` / TOML `provider` | `open_router` | Additive |
| `Account.params` | `product = "key" \| "account"`, `breakdown = "off" \| "provider" \| "model"` | Additive, validated |
| `Report` / `--json` / `export::v1` | New provider value, metrics of existing kinds | Additive |
| `access` matrix, `docs/providers.md` | Two rows for `open_router` | Generated |
| Model catalog | `OpenRouter` live support | Additive |
| Locales | `access.*`, `credential_help.*`, `field.open_router.*`, `note.open_router.*`, `query.open_router.*` | Additive |
| `desktop.json`, `notified.json`, events | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | variant, adapter, access arms, account fields, incident arm, model support, texts |
| Desktop / TUI | render the generic descriptors and reports; brand entry in `lib.js` |
| PRD 0036 | the router grouping and any dedicated section |

## 9. Dependencies and sequencing

RF1 and RF8 first (the build fails without the arms), then RF2 to RF4 (key view), RF5 and RF7 (account view and errors) and RF13 fixtures with each step. RF9 is verification of existing rules. RF6, RF11, RF12 and RF10 follow independently and can ship in separate commits. Visual polish depends on PRD 0022 and the router section on PRD 0036, neither blocks delivery. PRD 0023 RF1 (`billing` field) is not required: every OpenRouter metric is included.

## 10. Risks

| Risk | Mitigation |
|---|---|
| The credits endpoint unit is undocumented | RF5.3/RF5.4: no `$` until the owner confirms; `USD` meaning verified with a real management key |
| Users paste an inference key where a management key is needed (and the reverse) | RF7.2 specific note, hints in the access descriptor and the credential help |
| An exhausted `:free` counter blocks a paid account in the dashboard | RF4.2: `Quota` only for a free-tier key, `Usage` otherwise |
| `/key` or `/credits` are rate-limited or billed, undocumented | One call per refresh, `429` is `RateLimited` with `Retry-After` honored, RF13.1 real-account check; `is_metered` flips if proven |
| OpenRouter changes or deprecates fields (`rate_limit`, `is_provisioning_key` already deprecated; `budgets` is new) | Optional parsing, required-path list for PRD 0028, `wiremock` fixtures with missing and extra fields |
| A management key is more powerful than the user expects | Risk text, vault storage, never logged, never in exports |
| `/activity` rows are large or its window changes | Hard row/byte caps, once per UTC day, failure does not fail the account |
| Status page unverifiable | RF10 returns `None` until verified |
| Overlap with PRD 0036 | Separate ownership (RF12.3): data here, grouping there |

## 11. Order of execution and verification

1. **RF1, RF8:** variant, dispatch, descriptors, texts, regenerated matrix, example config.
2. **RF2, RF3, RF4, RF7:** key view and error classification with fixtures F1 to F8 and F17 to F22.
3. **RF5:** account view and fixtures F9 to F12.
4. **RF9:** eligibility tests across both views.
5. **RF6, RF11, RF12, RF10, RF14:** breakdown, models, catalog entry, incident source, documentation; each can be its own commit.
6. **RF13.1:** the owner's real-account check.

At each step: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the desktop tests, and a smoke test (`cargo run -p cuotai-tui -- --config <fixture TOML> --once` and `--json` with a local fake). The requirement is only delivered when a real inference key shows its cap, spend and free-request counter, a real management key shows the credit balance, a wrong key class is explained instead of reported as a bare 403, no field is ever zero when absent, and the keys appear nowhere in output.

## 12. Open questions

### Owner decisions (2026-10-25)

| # | Decision | Delivered as |
|---|---|---|
| 3 | **Low-balance threshold: yes**, with a documented default. | `params.low_balance` (account product only): a `note.open_router.low_balance` note when the balance is at or below the value; **default 5** credits, `0` turns it off. A note, not an alert: balances stay out of the threshold-alert engine (PRD 0023 section 4). |
| 5 | **Environment detection of `OPENROUTER_API_KEY`: yes**, presence-only and with consent. | `account_edit::detect_env_keys(consented, is_set)` checks nothing without consent and never reads the value (`process_env_is_set` tests presence). The wizard/picker that asks for consent and offers the variable belongs to PRD 0027 RF6/RF7, which is not delivered yet; this PRD ships the core API only. CLI/TUI only: the desktop process does not inherit the shell environment. |
| 6 | **Native "Sign in with OpenRouter" (PKCE): yes.** | Browser flow in `oauth/reference_browser.rs` (loopback callback with a random path, no state parameter; exchange at `POST /api/v1/auth/keys`). It stores the returned inference key in the vault; no expiry or refresh token, `user_id` is discarded. Native sign-in cannot be combined with `product = "account"` (`Config::validate`). |


| # | Question | Recommendation |
|---|---|---|
| 1 | Two OpenRouter accounts (key and account) or one account with an optional second credential for the balance | Two accounts selected by `product` (RF1.2): a single account has one credential reference today, and a second secret belongs to a broader account-model change (PRD 0029). Revisit if users find the split awkward. |
| 2 | Show the `$` sign for `/credits` before the real-account check | No (RF5.3); show "créditos" until RF5.4 is confirmed. |
| 3 | Low-balance alert (a fixed dollar floor) | Yes, but in a follow-up on the PRD 0008/0024 alert engine: PRD 0023 §4 keeps balances out of threshold alerts, and the floor needs an owner-chosen default (for example US$ 5). |
| 4 | `output_modalities=all` in the model listing | Yes, so embeddings, image and audio models are not hidden; confirm that `all` is accepted with the real endpoint. |
| 5 | Offer OpenRouter detection by `OPENROUTER_API_KEY` in the environment | Only as a consented, presence-only check (PRD 0027 RF7 pattern) in the CLI/TUI; not in the desktop app, whose process does not inherit the shell. Needs an owner decision. |
| 6 | Native "Sign in with OpenRouter" (PKCE, localhost callback) storing an inference key in the vault | A good later improvement over pasting a key; it returns only an inference key, so the balance still needs a management key. Owner decision; separate PRD or RF. |
| 7 | Number of spend metrics shown on the card (four windows plus BYOK) | Keep all in the data; let PRD 0022/0036 decide which are card-level and which details-only. |
| 8 | Where the breakdown renders and whether a `Metric::scope` for upstream is added | PRD 0036 decides the section; the scope field follows PRD 0023 MET-01 when it exists. |
| 9 | Does a management key work on `/key` and `/models` | Verify in RF13.1; until then `account` uses only `/credits` and `/activity`. |
| 10 | Status page source | Verify per PRD 0011 RF1.2 at implementation (RF10). |

## 13. Sources (OpenRouter official documentation, read 2026-10-23)

- Get current key: <https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key>
- Get remaining credits: <https://openrouter.ai/docs/api/api-reference/credits/get-credits>
- Get user activity: <https://openrouter.ai/docs/api/api-reference/analytics/get-user-activity.md>
- List models: <https://openrouter.ai/docs/api/api-reference/models/get-models.md>
- Get generation: <https://openrouter.ai/docs/api/api-reference/generations/get-generation.md>
- API rate limits (free-model limits, 402 and 429, reset times): <https://openrouter.ai/docs/api-reference/limits>
- Management API keys: <https://openrouter.ai/docs/guides/overview/auth/management-api-keys.md>
- Create key (`limit_reset`, `include_byok_in_limit`): <https://openrouter.ai/docs/api/api-reference/api-keys/create-keys.md>
- BYOK: <https://openrouter.ai/docs/guides/overview/auth/byok.md>
- OAuth PKCE: <https://openrouter.ai/docs/guides/overview/auth/oauth.md>
- Free model variants: <https://openrouter.ai/docs/guides/routing/model-variants/free>
- FAQ (credits, free-model tiers, BYOK fee): <https://openrouter.ai/docs/faq>
- Status page (not machine-readable-verified): <https://status.openrouter.ai/>
