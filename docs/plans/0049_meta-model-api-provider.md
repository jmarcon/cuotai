# PRD 0049 — Meta Model API (Muse) provider

| Field | Value |
|---|---|
| ID | 0049 |
| Status | Awaiting owner verification (RF1–RF5 delivered in vNEXT; RF6, the real-account check, is the owner's) |
| Owner | `cuotai-core` (model, adapter, access descriptor, account fields, catalog entry) |
| Consumers | `apps/desktop` and `crates/cuotai-tui` (generic rendering only) |
| Depends on | PRD 0005 RF1 (`access::describe`), PRD 0047 (typed keys kept in the system vault) |
| Benefits from | PRD 0048 (a future session capture of the dev.meta.ai usage page) |
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

## 1. Context and evidence (checked on 2026-10-27)

| Fact | Source |
|---|---|
| dev.meta.ai is the Meta Model API developer platform; llama.developer.meta.com docs redirect there. | `https://llama.developer.meta.com/docs/api/models` answers 302 to `https://dev.meta.ai/docs/api/models` |
| Base URL `https://api.meta.ai/v1`, header `Authorization: Bearer <key>`. | <https://dev.meta.ai/docs/overview>, <https://dev.meta.ai/docs/authentication> |
| `GET /v1/models` lists models (`ListModelsResponse`); unauthenticated it answers 401. | <https://dev.meta.ai/docs/api-reference/models/list-models> |
| Successful token-based responses carry `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-tokens`, `x-ratelimit-limit-requests`, `x-ratelimit-remaining-requests`; 429 carries `Retry-After`. Limits apply per team, not per key. | <https://dev.meta.ai/docs/pricing-rate-limits> |
| No usage, spend, credit or balance endpoint is documented. Usage and billing are shown only in the console (`dev.meta.ai/usage`). | same page; the API reference index lists only Responses, Chat completions, Messages, Files, Images, Voice and Models |

Team and project ids appear only in console URLs; the API does not need them, so Cuotai does not ask for them.

## 2. Requirements

- **RF1** The adapter makes one token-free read, `GET https://api.meta.ai/v1/models`, with the account key. A 2xx answer whose body is a JSON object with a `data` array is a valid key; anything else is a schema error. Model ids are not copied anywhere.
- **RF2** When both the limit and the remaining header of a bucket (requests, tokens) are present and numeric, the bucket becomes a `RateLimit` metric. A missing or partial bucket is omitted and a note says the window is unknown: never zero. Meta documents the headers on token-based responses, so the listing may not carry them.
- **RF3** 401/403 are authentication errors, 429 is rate limited (with `Retry-After`), other codes are errors; no body or URL is exposed.
- **RF4** The card says that Meta documents no usage or balance API and points to the dev.meta.ai console.
- **RF5** The key-creation page is `https://dev.meta.ai/api-keys` (without team or project ids). When the API-key-link mechanism lands, this provider uses that URL.
- **RF6** Owner check: add the account with a real key, confirm the card reaches "Up to date" and whether rate-limit metrics appear.

## 3. Out of scope

- Spend, credits and balance: they need the console web session. The usage page's backend call is a candidate for a future PRD 0048 session capture; it is not implemented or probed here.
- Inference calls to obtain rate-limit headers: they would consume tokens.

## 4. Decision log

- 2026-10-27: provider slug `meta`, label "Meta Model API"; level `Derived`, like Groq.
- 2026-10-27: the model listing is not offered in the Models view (`no_evidence`): its schema is not published in field detail.
