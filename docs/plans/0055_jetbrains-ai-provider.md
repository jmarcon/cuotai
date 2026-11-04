# PRD 0055 — JetBrains AI (AI Assistant, Junie) provider

| Field | Value |
|---|---|
| ID | 0055 |
| Status | Awaiting owner verification (RF1–RF7 delivered in vNEXT; RF8, the real-account check, is the owner's) |
| Owner | `cuotai-core` (model, adapter, access descriptor, account fields, catalog entry) |
| Consumers | `apps/desktop` and `crates/cuotai-tui` (generic rendering only) |
| Depends on | PRD 0005 RF1 (`access::describe`) |
| Benefits from | PRD 0045 (breakdowns by team), PRD 0047 (typed secrets in the desktop) |
| Date | 2026-11-04 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement. "JetBrains AI" is the subscription family (AI Free, AI Pro, AI Ultimate, AI Enterprise) whose **AI credits** pay for AI Assistant, Junie and the third-party agents run from JetBrains IDEs and JetBrains Air. The organization console was called "Air Governance" in older pages and is now **JetBrains Central Console** (`console.jetbrains.cloud`). Issue: [#51](https://github.com/jmarcon/cuotai/issues/51).

## Implementation status (2026-11-04)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | `Provider::JetBrainsAi` (slug `jetbrains_ai`), dispatch and the `providers::jetbrains_ai` adapter. |
| RF2 | vNEXT | Service-account client-credentials exchange with an in-process token cache. |
| RF3 | vNEXT | Month-to-date credits by tool through `POST /credits-usage/group-by/tool`; missing or partial values never become zero. |
| RF4 | vNEXT | Error classification (token and metrics endpoints) with fixed notes; no body, URL or secret in any note. |
| RF5 | vNEXT | Access descriptor (`Official`, admin key, organization scope, Business or Enterprise), catalog texts in every core locale, regenerated `docs/providers.md` matrices. |
| RF6 | vNEXT | Account fields (`credential_env` = client ID, `params.client_secret_env` = client secret variable), credential help, catalog aliases, example config. |
| RF7 | vNEXT | `wiremock` contract tests: token exchange, request body, cache reuse, retry after a stale token, missing and partial values, 400/401/403/429/5xx, schema drift, missing credentials. |
| RF8 | — | Owner check with a real service account. |

## 1. Research (checked on 2026-11-04)

The agent signed in nowhere and used no credential: every fact below comes from public documentation or public source code. "Mocked" behaviour in §4 is not proof of access to a real organization.

### 1.1 Organization usage: the documented Analytics API

| Fact | Source |
|---|---|
| The Analytics API gives programmatic access to an organization's AI usage: adoption, activity and **AI credits consumption** across teams and tools. Only on the **Business and Enterprise** plans. | <https://www.jetbrains.com/help/air/analytics-api.html> |
| Base URL `https://api.jetbrains.cloud/metrics/`, then `/api/v2` and the endpoint path. v1 is kept for compatibility and will be deprecated. | same page; <https://www.jetbrains.com/help/air/analytics-api-v1.html> |
| Every request sends `Authorization: Bearer <access token>`, `Content-Type: application/json`, `Accept: application/json`; every endpoint is a `POST` with a JSON filter body. Dates are `YYYY-MM-DD` **in UTC**. | <https://www.jetbrains.com/help/air/analytics-api-v2.html> |
| `POST /credits-usage/group-by/tool` with `{fromDate, toDate}` (required; `tools`, `principalType`, `ides` optional) answers `{"items":[{"tool":"aia","tariff":288.47,"topup":381.28}, …]}`: `tariff` is consumption from the built-in quota, `topup` consumption from purchased top-up credits. Documented tools: `junie`, `aia`, `claude_agent`, `codex_agent`, `gemini_agent`, `opencode_agent`, `pi`, `copilot`, `other`. | same page |
| `POST /adoption/quota-consumption/group-by/date` answers `{items:[{date,limit,topup}], totalLimit, totalTopup}`; `POST /groups/spend` answers `creditsSpent` per group ("the total AI credits spent"); `/credits-usage/group-by/date-principal-tool.cursor` answers per-user rows with names and e-mails. | same page |
| The token comes from a **service account** (client ID and client secret, created by a user with *Manage organization*; the secret is shown once). The service account needs a role with **View AI analytics**; an organization-wide role returns the whole organization, a group-scoped role only its groups. | <https://www.jetbrains.com/help/air/service-accounts.html>, <https://www.jetbrains.com/help/air/analytics-api.html> |
| Token exchange: `POST https://oauth.account.jetbrains.com/api/rest/oauth2/token`, HTTP Basic `client_id:client_secret` (UTF-8, Base64), form body `grant_type=client_credentials&scope=jcp-public`; answer `{access_token, token_type:"Bearer", expires_in:3600, scope}`. No refresh token: clients **should cache** the token until it nears expiry. Errors `invalid_client` (401), `invalid_request`/`unsupported_grant_type`/`invalid_scope`/… (400). | <https://www.jetbrains.com/help/air/exchange-client-id-and-secret-for-an-access-token.html> |
| Rate limits, data freshness and per-request cost are not documented. Reading analytics spends no AI credits (it is not an inference call). | the pages above (no mention) |
| One AI credit equals USD 1.00, charged in the local currency. | <https://www.jetbrains.com/help/ai-assistant/licensing-and-subscriptions.html> |

### 1.2 Individual quota (AI Free, Pro, Ultimate)

| Fact | Source |
|---|---|
| Individual plans: AI Free 3 credits, AI Pro 10, AI Ultimate 35 per 30 days (the page also lists 20 and 70 variants); the quota resets every 30 days from the first use of the license, not on the calendar month. Top-up credits are spent only after the monthly quota and last 12 months. | <https://www.jetbrains.com/help/ai-assistant/licensing-and-subscriptions.html> |
| The remaining monthly credits are shown only in the IDE's JetBrains AI widget; purchased and organization-assigned credits on the JetBrains Account page. **No API for a personal quota is documented.** | same page |
| The IDE keeps its last quota state in an internal file, `<IDE config dir>/options/AIAssistantQuotaManager2.xml` (HTML-encoded JSON attributes `quotaInfo` with `tariffQuota`/`topUpQuota` `current`/`maximum`/`available` in units of 1/100 000 credit, and `nextRefill.next`), and logs fresher states to `idea.log`. The file is persisted rarely and may lag the IDE by weeks; its format is internal. | CodexBar (MIT): [`docs/jetbrains.md`](https://github.com/steipete/CodexBar/blob/main/docs/jetbrains.md), [`JetBrainsStatusProbe.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/JetBrains/JetBrainsStatusProbe.swift); [jaimo](https://github.com/0xAPPA/jaimo) (IDE plugin, no license file) reads the same log entries |
| The IDE talks to the JetBrains AI service with the JetBrains Account session of the signed-in user; that session belongs to the IDE and its endpoints are not published. | no public contract found |

### 1.3 Verdict

- **Organization (Business, Enterprise): viable and delivered.** The Analytics API is documented, read-only and spends no credits; its credential is a service-account client ID and secret, which the user creates for integrations and keeps in environment variables, never a person's password. Cuotai reads the month-to-date credits by tool.
- **Individual (AI Free, Pro, Ultimate): not delivered.** No documented interface exists. The only read-only source is the IDE's internal quota file and log, which can lag by weeks and whose format may change with any IDE build; the network alternative would reuse the IDE's JetBrains Account session against undocumented endpoints. Both are left for a later decision (§5).

## 2. Requirements

- **RF1** `Provider::JetBrainsAi`, slug `jetbrains_ai`, label "JetBrains AI", category "Coding tool"; catalog aliases `jetbrains`, `junie`, `ai assistant`, `air`, `intellij`.
- **RF2** The adapter exchanges the client ID (`credential_env`, default `JETBRAINS_CLIENT_ID`) and secret (`params.client_secret_env`, default `JETBRAINS_CLIENT_SECRET`) for a token at the fixed token URL, with HTTP Basic and `grant_type=client_credentials&scope=jcp-public`. The token is kept in process memory only, keyed by an HMAC of the client ID and secret under a random per-process key, and reused until 60 s before `expires_in` (or 3600 s when absent). It is never written to disk, TOML, notes or snapshots. When a cached token is refused (401), the adapter drops it and exchanges once more.
- **RF3** One read per refresh: `POST https://api.jetbrains.cloud/metrics/api/v2/credits-usage/group-by/tool` with `{"fromDate": <first day of the current UTC month>, "toDate": <today UTC>}`.
  - **RF3.1** Each item becomes a monthly `Usage` metric "AI credits — {tool}" (period "current month", unit credits) whose value is `tariff + topup`; a tool with only one of them shows that one and a note says the value is partial. Known tool ids map to display names (AI Assistant, Junie, Claude Agent, Codex, Gemini CLI, OpenCode, Pi, GitHub Copilot, Other); an unknown id of up to 40 `[a-z0-9_-]` characters is shown as is; anything else is skipped with a note.
  - **RF3.2** Two organization totals: "Organization AI credits (built-in quota)" (sum of `tariff`) and "Organization AI credits (top-up)" (sum of `topup`), each only when every item carries that field; otherwise the total is omitted and a note says why. The API reports no organization limit, so no metric has a limit, remaining value or reset.
  - **RF3.3** An empty `items` array emits no metric and a note ("no AI credits reported this month"). A body that is not an object or has no `items` array is a schema error. Negative or non-finite numbers count as missing.
- **RF4** Errors: a token answer 400/401 (`invalid_client` and the rest) is an authentication error ("JetBrains rejected the service account client ID or secret"); a metrics 401 after the retry is an authentication error; 403 is an authentication error that names the *View AI analytics* permission and the Business or Enterprise plan; 429 is rate limited with `Retry-After`; other codes use `query.http`. Missing variables are configuration errors naming the variable. No body, URL or secret reaches a note.
- **RF5** Access descriptor: `Official`, credential `AdminKey`, scope `Organization`, plan requirement "Business or Enterprise", documentation `https://www.jetbrains.com/help/air/analytics-api-v2.html`, `verified_on` 2026-11-04. The model catalog makes no request (`Unsupported` with a reason).
- **RF6** Account fields: the client-ID variable as the credential, `client_secret_env` as an optional text field (variable name only); in the desktop both are typed secrets kept in the system vault (PRD 0047), labelled "Service account client ID" and "Client secret", with the JetBrains Central Console as their key page. Credential help explains where the service account is created and which permission it needs. Example in `cuotai.example.toml`.
- **RF7** `wiremock` tests with placeholder data only (no real organization, token or person).
- **RF8** Owner check: create a service account with *View AI analytics* in a Business or Enterprise organization, export the two variables, add the account and confirm the card reaches "Up to date" with per-tool rows that match the Console's AI analytics page for the month.

## 3. Out of scope

- Per-user, per-group and per-model breakdowns (`date-principal-tool.cursor`, `groups/spend`, `token-usage/*`): they carry names and e-mails; PRD 0045 decides how breakdowns are shown.
- Individual quotas (§1.2, §5).
- Any call that runs AI Assistant, Junie or another agent.

## 4. Verification

- Automated: `cargo test -p cuotai-core jetbrains_ai` runs the adapter against `wiremock`. Mocked HTTP proves the request shape and the parsing of the documented examples, not access to a real organization.
- Owner: RF8.

## 5. Open questions and decision log

- 2026-11-04 (agent): the organization Analytics API is the only documented, read-only and non-billing interface; it is delivered. The month window is the calendar month in UTC, because the API's dates are UTC and individual 30-day resets do not apply to an organization total.
- 2026-11-04 (agent): `credits-usage/group-by/tool` is used instead of `quota-consumption/group-by/date` because one request gives both the totals and the per-tool split (Junie versus AI Assistant was the issue's ask).
- Open (owner): whether to add an individual mode reading the IDE's local `AIAssistantQuotaManager2.xml` (Level `Undocumented`, credential `LocalCli`-like, no network). It needs a filesystem reader per OS and accepts that the value may be weeks old; CodexBar (MIT) documents the format.
