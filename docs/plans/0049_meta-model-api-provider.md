# PRD 0049 — Meta Model API (Muse) provider

| Field | Value |
|---|---|
| ID | 0049 |
| Status | Awaiting owner verification (RF1–RF5 and RF7–RF10 delivered in vNEXT; RF6 and RF11, the real-account checks, are the owner's) |
| Owner | `cuotai-core` (model, adapter, access descriptor, account fields, catalog entry) |
| Consumers | `apps/desktop` and `crates/cuotai-tui` (generic rendering only) |
| Depends on | PRD 0005 RF1 (`access::describe`), PRD 0047 (typed keys kept in the system vault), PRD 0048 (session capture window) |
| Benefits from | — |
| Date | 2026-10-27 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement. "Muse" is the family of Meta models (Muse Spark, Muse Image, Muse Voice and others) served by the **Meta Model API**; the provider is named after the API.

## Implementation status (2026-10-27)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | `Provider::Meta` (slug `meta`), dispatch and the `providers::meta` adapter. |
| RF2 | vNEXT | Access descriptor (`Derived`, inference key, organization scope, `verified_on` 2026-10-27), core catalog texts and the regenerated `docs/providers.md` matrices. |
| RF3 | vNEXT | Account fields: the key as a typed vault secret in the desktop (PRD 0047), `META_API_KEY` as the TUI default; catalog aliases `muse`, `muse spark`, `llama`, `meta ai`. |
| RF4 | vNEXT | Example config, desktop brand monogram (no logo asset: none with a clear license was found). |
| RF5 | vNEXT | `wiremock` contract tests: headers present, absent, partial, 401/403, 429, schema drift, missing key. |
| RF6 | — | Owner check with a real key. |
| RF7 | vNEXT | `meta` session descriptor `session_env` (`llama_dev_sess`, host `dev.meta.ai`) with a PRD 0048 capture of `https://dev.meta.ai/usage` and its exact-host sign-in allow-list. |
| RF8 | vNEXT | Optional `team_id` / `project_id` params (`project_id` validated but not sent: the console's usage request carries only the team); team discovered through `/api/portal/teams` when the session has one team. |
| RF9 | vNEXT | Console usage read through the owner-confirmed `GET /api/portal/teams/{team}/usage/batch` (month to date, UTC) and its `metrics.<NAME>.series[TOTAL]` shape; access level `Undocumented` / user session for the session mode. |
| RF10 | vNEXT | `wiremock`, parser, allow-list and capture tests; debug-only diagnostics (navigation hosts/paths, unrecognised usage field names). |
| RF11 | — | Request and shape confirmed by the owner on 2026-10-27 (§5.1); pending: a non-zero sample to confirm the cents offset (§5.2) and the card in dev mode (§5.4 step 4). |

## 1. Context and evidence (checked on 2026-10-27)

| Fact | Source |
|---|---|
| dev.meta.ai is the Meta Model API developer platform; llama.developer.meta.com docs redirect there. | `https://llama.developer.meta.com/docs/api/models` answers 302 to `https://dev.meta.ai/docs/api/models` |
| Base URL `https://api.meta.ai/v1`, header `Authorization: Bearer <key>`. | <https://dev.meta.ai/docs/overview>, <https://dev.meta.ai/docs/authentication> |
| `GET /v1/models` lists models (`ListModelsResponse`); unauthenticated it answers 401. | <https://dev.meta.ai/docs/api-reference/models/list-models> |
| Successful token-based responses carry `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-tokens`, `x-ratelimit-limit-requests`, `x-ratelimit-remaining-requests`; 429 carries `Retry-After`. Limits apply per team, not per key. | <https://dev.meta.ai/docs/pricing-rate-limits> |
| No usage, spend, credit or balance endpoint is documented. Usage and billing are shown only in the console (`dev.meta.ai/usage`). | same page; the API reference index lists only Responses, Chat completions, Messages, Files, Images, Voice and Models |

Team and project ids appear only in console URLs; the Model API does not need them. The console usage (§5) does, so they are optional params for the session mode.

## 2. Requirements

- **RF1** The adapter makes one token-free read, `GET https://api.meta.ai/v1/models`, with the account key. A 2xx answer whose body is a JSON object with a `data` array is a valid key; anything else is a schema error. Model ids are not copied anywhere.
- **RF2** When both the limit and the remaining header of a bucket (requests, tokens) are present and numeric, the bucket becomes a `RateLimit` metric. A missing or partial bucket is omitted and a note says the window is unknown: never zero. Meta documents the headers on token-based responses, so the listing may not carry them.
- **RF3** 401/403 are authentication errors, 429 is rate limited (with `Retry-After`), other codes are errors; no body or URL is exposed.
- **RF4** The card says that Meta documents no usage or balance API and points to the dev.meta.ai console.
- **RF5** The key-creation page is `https://dev.meta.ai/api-keys` (without team or project ids). When the API-key-link mechanism lands, this provider uses that URL.
- **RF6** Owner check: add the account with a real key, confirm the card reaches "Up to date" and whether rate-limit metrics appear.
- **RF7** A `session_env` session descriptor requires the console cookie `llama_dev_sess`, sent only over HTTPS to `dev.meta.ai`. Its capture opens `https://dev.meta.ai/usage`, reads the `dev.meta.ai` cookies and may visit only `dev.meta.ai`, `auth.meta.com`, `auth.meta.ai`, `accountscenter.meta.com`, `www.meta.com`, `www.facebook.com`, `m.facebook.com` and `www.instagram.com`, matched exactly. Anonymous cookies (`datr`) and sign-in helpers never end the capture.
- **RF8** `params.team_id` and `params.project_id` are optional ids copied from `dev.meta.ai/usage?team_id=…&project_id=…` (digits, letters, `_`, `-`, at most 64 characters; anything else is a configuration error before any request). Without `team_id`, `GET /api/portal/teams` picks the session's only team; with several, a note asks for `team_id`.
- **RF9** With a session, the adapter calls `GET https://dev.meta.ai/api/portal/teams/{team_id}/usage/batch?metrics=…&start_date=…&end_date=…&timezone=…` (the current calendar month to date) with the cookie, `Content-Type: application/json` and the `Referer` of the usage page. 401/403 or a redirect is "session expired" (authentication error); other codes degrade to a note and the key check still shows; a 2xx body without a `metrics` object is a schema error. The sum of each metric's `TOTAL` series becomes a monthly metric (cost → `Spend` in the currency code, tokens, requests and the other counters → `Usage`); an empty series is left out, never zero, while explicit `0` points are a real zero. The session mode's access level is `Undocumented`, so the card says it may change without notice.
- **RF10** Tests use placeholder data only. Debug builds print the capture window's navigation host and path (no query) and the field names (no values) of an unrecognised usage body.
- **RF11** Owner check with a real session (§5.4).

## 3. Out of scope

- Inference calls to obtain rate-limit headers: they would consume tokens.

## 4. Decision log

- 2026-10-27: provider slug `meta`, label "Meta Model API"; level `Derived`, like Groq.
- 2026-10-27: the model listing is not offered in the Models view (`no_evidence`): its schema is not published in field detail.
- 2026-10-27 (owner): usage may be read through a dev.meta.ai web session captured in a Cuotai window (PRD 0048 style). Agent: slot `session_env` (as Alibaba), cookie `llama_dev_sess` (the one `/api/auth/logout` clears), team discovery from `/api/portal/teams` with `team_id`/`project_id` params as overrides, inferred body parser with a debug field-name diagnostic until the owner confirms the shape.
- 2026-10-27 (owner): the signed-in console's usage request and answer were observed and confirmed (§5.1). Agent: the adapter now sends exactly that request (`usage/batch`, the 15 metric names, month to date) and drops `project_id` from it; the timezone is `UTC` because no adapter reads the system IANA zone and `iana-time-zone` is not a direct dependency; `ecto_1_sess` becomes an optional session cookie; the cost is `amount / currency.offset` (offset `100` read as cents, inferred).

## 5. Console usage evidence (2026-10-27)

The agent used no sign-in and no credentials; the usage request and answer in §5.1 were observed by the owner in a signed-in browser (values zeroed, ids omitted). "Confirmed" means observed in an HTTP answer or in the public bundle; "inferred" means not observed.

### 5.1 Confirmed

| Fact | Evidence |
|---|---|
| The console is a Next.js (Turbopack) app on Vercel; Sentry app key `model-api-web`. | `dev.meta.ai/` HTML (`server: Vercel`); 50 chunks under `/_next/static/immutable/chunks/*.js` (e.g. `07n8c26xo_us2.js`, `turbopack-2_04ewhudzrza.js`). |
| Signed out, `/usage?team_id=…&project_id=…` and `/billing` answer `307` to `/`. | `curl -D -` |
| The console has no GraphQL, `doc_id`, `fb_dtsg` or `lsd`: its data comes from same-origin REST routes under `/api/`. | Bundle grep: the only `graphQL` strings are Sentry options. Routes found: `/api/auth/{login,logout,me,session/reset,callback}`, `/api/portal/teams`, `/api/portal/teams/{id}`, `…/{id}/api-keys`, `…/{id}/billing-banner`, `…/{id}/model-capabilities`, `…/{id}/subscription-quota`, `/api/portal/gating`, `/api/billing/embed?team_id=…&dest=…`, `/api/model-api/models?project_id=…`, `/api/uploads/token?project_id=…`. |
| GETs send only `Content-Type: application/json` (plus `x-dev-atod-token`/`x-dev-od-alias` for Meta's internal dev hosts) and same-origin cookies; no CSRF header. | Bundle module `415234` (headers hook) and `449260` (fetch helper, reads `error_code`/`action_url` from JSON errors). |
| `GET /api/portal/teams` answers `{teams:[…]}`; items carry `team_id` and `role`. | Bundle: `(await t.json()).teams??[]`, `F.find(e=>String(e.team_id)===H)`, `G?.role`. Signed out: `401 {"error":"Not authenticated"}`. |
| `team_id` and `project_id` are query parameters of the console pages (`useSearchParams().get("team_id")`, `setTeamProject`). | Bundle. |
| **`/api/portal/teams/{id}/usage` exists**: signed out it answers `401 {"error":"Not authenticated"}`, like `…/projects`, `…/rate-limits`, `…/members`; `…/spend`, `…/costs`, `…/credits`, `…/balance`, `…/invoices`, `…/billing`, `…/usage/summary`, `/api/portal/usage` and `/api/portal/projects/{id}/usage` answer `404`. | `curl` probes with team id `1`. |
| **Usage request (owner, signed in, 2026-10-27)**: `GET /api/portal/teams/{team_id}/usage/batch?metrics=REQUESTS,USAGE_BILLABLE_COST,INPUT_TOKENS,CACHED_PROMPT_TOKENS,UNCACHED_PROMPT_TOKENS,BATCH_INPUT_TOKENS,BATCH_CACHED_PROMPT_TOKENS,BATCH_UNCACHED_PROMPT_TOKENS,OUTPUT_TOKENS,BATCH_OUTPUT_TOKENS,IMAGES_GENERATED_STANDARD,SEARCH_GROUNDING_REQUESTS,AUDIO_TRANSCRIPTION_SECONDS,VIDEO_FRAMES_PROCESSED,IMAGES_PROCESSED&start_date=YYYY-MM-DD&end_date=YYYY-MM-DD&timezone=<IANA>`, headers `content-type: application/json`, `referer: https://dev.meta.ai/usage?team_id={team_id}`, cookies `llama_dev_sess`, `ecto_1_sess`, `datr`; no `project_id`. | Owner, DevTools. |
| **Usage answer (owner)**: `{as_of, models: [], metrics: {<NAME>: {metric, series: [{type: "TOTAL", identifier, data_points: [{date, value}]}]}}}`; `USAGE_BILLABLE_COST` points also carry `amount`, and the metric carries `currency: {code: "USD", symbol, format, offset: 100}`; a metric's `series` may be empty. | Owner, DevTools (values zeroed). |
| Sign-in: `/api/auth/login[?next=…]` answers `302` to `https://auth.meta.com/oidc/?app_id=2403310080153219&redirect_uri=https://auth.meta.ai/model_api&response_type=code&scope=openid+linking&…&code_challenge_method=S256`, whose `state` returns to `https://dev.meta.ai/api/auth/callback`; it sets `llama_dev_csrf` (path `/api/auth/callback/<attempt>`). | `curl -D -` |
| Cookies: `POST /api/auth/logout` answers `303` and clears `llama_dev_sess` (the session); `/api/auth/callback` clears `llama_dev_csrf`, `llama_dev_pkce`, `llama_od_routing`, `llama_auto_login`. All `HttpOnly; Secure; SameSite=Lax; Path=/`. | `curl -D -` |
| The bundle links `accountscenter.meta.com`, `work.meta.com/accounts_center` (managed accounts), `auth.meta.com/oauth/device/`, `billing.dev.meta.ai` (the `/billing` iframe origin). | Bundle. |

### 5.2 Inferred (not observed)

- `currency.offset: 100` means the `amount` values are minor units (cents), so the cost is `amount / offset`: the owner's sample was all zeros. `value` is read when `amount` is absent. `models` (empty in the sample) may carry a per-model breakdown; it is ignored.
- The timezone is sent as `UTC` (the console sends the browser's IANA zone), so day boundaries may differ from the console page by the UTC offset.
- Currency defaults to USD only because Meta's pricing page is in USD.
- Sign-in hosts beyond `auth.meta.com`/`auth.meta.ai`: `auth.meta.com` refuses non-browser clients (`400`), so `www.meta.com`, `www.facebook.com`, `m.facebook.com` and `www.instagram.com` (Meta account sign-in with Facebook or Instagram) are inferred. A missing host shows as a refused navigation in the debug log.
- That `llama_dev_sess` alone authenticates `/api/portal/*` (`llama_od_routing` is cleared at callback and looks like internal routing).

### 5.3 Diagnostics (debug builds only)

- The capture window prints `session capture: navigation allowed|refused <host><path>` for each page or frame load (no query string), and the cookie names it saw when it ends. Tauri's navigation hook does not see the page's `fetch`/XHR calls, so the usage request itself is not observable there.
- The adapter prints `meta usage: unrecognised metric keys [...]` (names under `metrics` it does not map) and `meta usage: response without a metrics object, keys [...]` (field names, two levels, no values) before a schema error.

### 5.4 Owner verification (RF11)

1. In a normal browser, sign in at `https://dev.meta.ai/usage` and open DevTools → Network, filter `Fetch/XHR`, reload.
2. Find the request whose path starts with `/api/portal/teams/` and ends with `/usage` (or any other request the page makes for the usage numbers).
3. Report only: the request path with the ids replaced by `<team>`/`<project>`, its query parameter names (not values), the response's field names (from the Preview tab; no values needed), and the name of the session cookie if not `llama_dev_sess` (Application → Cookies → `dev.meta.ai`, names only). Never copy cookies, headers or the response body.
4. In Cuotai (dev mode), add or edit the `meta` account, press "Sign in" for the dev.meta.ai session, sign in, and confirm the card shows this month's spend, requests and token totals; in the terminal, copy any `meta usage:` line. With a non-zero spend, compare the card's value with the console's to confirm the cents offset.
