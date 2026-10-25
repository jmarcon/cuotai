# PRD 0036 — 9router and aggregator/routing providers

| Field | Value |
|---|---|
| ID | 0036 |
| Status | Proposed |
| Owner | `cuotai-core` (model, aggregator adapters, access, refresh, alerts, export), desktop and TUI |
| Consumers | `apps/desktop` (tray tab, main-window section, details, account form, Settings › Dashboard and Alerts), `crates/cuotai-tui` (section, details, wizard, `--once`, `--json`), `export::v1`, local server (PRD 0012/0031), `--watch` and hooks (PRD 0013), widgets (PRD 0014) |
| Depends on | PRD 0029 (stable account identity) for RF5 and every per-sub-account preference; PRD 0005 (access descriptors) for RF1.6; PRD 0010 (freshness) for RF4; PRD 0012 (`export::v1`) for RF7 |
| Benefits from | PRD 0022 (UI refresh), PRD 0027 (local detection, consent-gated presence probe) for RF2.5, PRD 0028 (contract health, local canary, CAT-01) for RF3.8, PRD 0033 (source-dated readings, `as_of`) for RF4.3, PRD 0011 (incidents) for RF6.6, decision C of 2026-10-23 (TOML groups, "best account now") for RF6.5 |
| Related | PRD 0038 (OpenRouter provider: an aggregator that reports only its own credits, see §1.4), PRD 0002/0028 CAT-01 (removal of the static model lists copied from 9router's registry) |
| Extends | PRD 0005 RF1 (new credential class, scope and `via` evidence), PRD 0009 RF1 (binding per sub-account), PRD 0008 RF1 (pool alert), PRD 0012 RF1 (additive `sub_accounts`), PRD 0013 RF1.2 (events carry `parent`) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. An **aggregator** (or routing gateway) is a service that holds credentials for several upstream AI providers and routes requests to them. An **aggregator account** is the Cuotai account that points at one aggregator instance; each upstream credential the aggregator holds (9router calls it a *connection*, new-api a *channel*) becomes a **sub-account** of it. **Instance** means one running aggregator (local or remote).

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT (partial) | `Provider::NineRouter` and `Provider::is_aggregator`, `Report::sub_reports` / `SubReport` (additive, skipped when empty), `Snapshot::units()`, `metrics::classify_report` returning the pool class for an aggregator, the `access::classify` arm (`Undocumented`, new credential class `AggregatorLogin`, new scope `Instance`, `access.risk.nine_router`) with the regenerated matrix. **Pending:** binding, pace, alerts, resets, freshness, history and events iterate `units()` (today they still see the aggregator account only); the `AggregatorShape` adapter trait (there is one adapter, so `nine_router.rs` is the only implementation). |
| RF2 | vNEXT (partial) | `account_edit` descriptors (`base_url`, `allow_private_http`, `include`, `exclude`; optional password variable in `credential_env`, blank = no password), base URL rules per host class (loopback HTTP, HTTPS elsewhere, private-network opt-in, no userinfo/query/fragment) validated in `Config::validate` (`ConfigError::BaseUrl`), `Monitor::test_nine_router` with every RF2.2 outcome (health, version, login, connection list; connected, not 9router, password wrong, lockout with the wait, login refused, password required, unreachable), the exposure and private-HTTP notes, TOML example. **Pending:** wiring Test connection into the TUI wizard and the desktop form (mandatory before the first save), `tls_ca_file` / `tls_pin_sha256` with the trust-on-first-use dialog, the consent-gated presence probe (needs PRD 0027), the pasted `auth_token` session for SSO instances (needs PRD 0016 slot), the vault-stored password (env variable only today), hidden connections editor, the version-below-minimum warning (needs `tested_min`, task T2). |
| RF3 | vNEXT (partial) | `providers/nine_router.rs`: the closed `Route` allow-list (health, version, login, connection list, usage by id `[A-Za-z0-9_-]{1,128}`; never `/api/providers`, reset routes, settings, keys or `/v1`; never `force=1`), in-memory login shared per instance (renewed 10 minutes before the JWT `exp`, a rejected password or `Retry-After` window is not retried, redirects not followed and classified as authentication), paged discovery (cap 10 pages / 1000), field allow-list at deserialization, usage normalization (quota metrics with windows from labels, `unlimited` and unmeasured entries without a meter, `total = 0` is no cap, invalid `resetAt` is `None`, `limitReached` without detail is an exhausted signal, soft `{message}` failures classified without showing the text, 401 = reconnect, 404 = gone, 5xx/timeout = error), the upstream mapping table, opaque sub-account ids, masked labels, `via` evidence and the "via 9router" caveats. `wiremock` fixtures cover every branch and a request recorder proves the allow-list. **Pending:** RF3.8 `tested_min` and PRD 0028's canary (owner task T2). |
| RF4 | vNEXT (partial) | Interval floor of 120 s for an aggregator account, concurrency 2 inside the global semaphore, 15 s per request, 60 s per instance (the account deadline is raised to 60 s), connections past the deadline become timeout sub-reports, a manual refresh never sends `force=1`. **Pending:** per-sub-account freshness, `last_good` and stale treatment (PRD 0010), refresh of one connection only, the 30-second post-reset query for sub-accounts. |
| RF5 | — | Proposed (needs PRD 0029). |
| RF6 | vNEXT (partial) | RF6.3 `aggregators::pools` / `Pool::summary` ("Claude · 2 of 3 available", "all exhausted · returns …", the earliest return time), computed over active connections only. **Pending:** per-sub-account binding, pace and alerts, `AlertKind::PoolExhausted` and the alert grouping preference, "best account now" ranking and incidents for sub-accounts. |
| RF7 | vNEXT (partial) | `Report::sub_reports` reaches `--json` and the `export::v1` report (redacted like the account's own texts; the schema documents `SubReport`). **Pending:** the `sub_accounts` / `aggregator` blocks of `AccountExport`, local-server items with `parent`, events with `parent` and `pool_exhausted` / `pool_recovered`, widgets. |
| RF8 | vNEXT (partial) | The 9router brand mark in the desktop (`lib.js`). **Pending:** the dedicated tray tab, grouped "All" row, main-window section, details, account form and Test connection dialog. |
| RF9 | vNEXT (partial) | `--list-providers` marks `nine_router` as an aggregator; `--once` prints the pools and every connection under the aggregator account; `--json` carries `sub_reports`. **Pending:** the TUI dashboard section (collapse key, filter), the wizard Test connection and fingerprint prompt, `--strict-exit` handling. |
| RF10 | — | Proposed (second adapter, gated on evidence). |
| RF11 | vNEXT (partial) | `docs/providers.md` section "Aggregators" and 9router paragraph, the regenerated matrix, `cuotai.example.toml`, README counts and the `THIRD_PARTY_NOTICES` line. **Pending:** `docs/export-v1.md`, `docs/events.md`, `docs/local-server.md` and `docs/tui.md` descriptions once their code lands. |

## 1. Context

### 1.1 What 9router is

9router ([github.com/decolua/9router](https://github.com/decolua/9router), homepage [9router.com](https://9router.com), MIT, about 31 k stars) is a self-hosted Next.js application that connects coding tools (Claude Code, Codex, Cursor, Cline, Copilot, Antigravity…) to more than 40 upstream providers through one OpenAI/Anthropic-compatible endpoint, with multi-account load balancing and automatic fallback. It is installed with `npm install -g 9router` or Docker and listens by default on `http://localhost:20128` (dashboard at `/dashboard`, inference at `/v1`; [README](https://github.com/decolua/9router/blob/main/README.md)). It can be exposed remotely (VPS, Docker, Cloudflare tunnel, Tailscale). Versions: the npm package is at **0.5.109** (`registry.npmjs.org/9router/latest`, matching `package.json` of `9router-app` at `main` revision `f39d8f07481ae8518e93175f0ecaaafa61573c6c`, checked 2026-10-23); the GitHub releases lag behind (latest tagged release `v0.5.35`, 2026-07-16). The project moves fast (commits daily), so every contract below is pinned to a revision.

9router has its own quota dashboard (`Dashboard → Usage → Provider limits`, [quota-tracking docs](https://github.com/decolua/9router/blob/main/gitbook/content/en/features/quota-tracking.md)). The dashboard reads it through **undocumented internal JSON routes** of the same Next.js app (none is published as an API):

| Route (revision `f39d8f07`) | What it returns | Notes |
|---|---|---|
| `GET /api/health` | `{ "ok": true }` | Public, CORS `*`. Presence probe. |
| `GET /api/version` | `{ currentVersion, latestVersion, hasUpdate }` | Public. Makes the instance query the npm registry (cached 1 h). |
| `POST /api/auth/login` `{ password }` | Sets the `auth_token` cookie: an HS256 JWT valid **24 h** ([`dashboardSession.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/lib/auth/dashboardSession.js)) | Login limiter locks an IP after repeated failures (429 + `Retry-After`); refused (403) when SSO (OIDC/SAML) is the auth mode or when the request arrives through a tunnel with dashboard access disabled; a fresh install's default password is refused remotely ([`login/route.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/app/api/auth/login/route.js)). |
| `GET /api/providers/client?page&pageSize≤500&accountStatus&provider&sort` | `{ connections[], providerOptions[], pagination, totals }` of the **usage-eligible** connections, each reduced to an allow-list: `id`, `provider`, `authType`, `name` (masked when it looks like a key), `email`, `displayName`, `priority`, `isActive`, `testStatus`, `lastError`, `expiresAt`, `lastUsedAt`, … and a safe `providerSpecificData` subset ([`providers/client/route.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/app/api/providers/client/route.js)) | The route also runs `backfillCodexEmails()` (a write in the instance's own database). |
| `GET /api/providers` | Every connection with `apiKey`/`accessToken`/`refreshToken`/`idToken` removed, but the **rest of `providerSpecificData` kept** (for example `copilotToken`) ([`providerConnectionResponse.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/lib/providerConnectionResponse.js)) | **Cuotai must never call it** (RF3.6). |
| `GET /api/usage/{connectionId}[?force=1]` | The normalized usage of one connection: `{ plan, quotas: { "<label>": { used, total, remaining, remainingPercentage, resetAt, … } }, limitReached, … }` or a soft failure `{ message }`; 401 when the credential refresh fails, 404 when the connection is gone ([`usage/[connectionId]/route.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/app/api/usage/%5BconnectionId%5D/route.js)) | Dispatches to `open-sse/services/usage/*.js` ([`usage.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/open-sse/services/usage.js)); for OAuth connections it may **refresh and persist upstream tokens** first; some handlers cache per token (Claude) unless `force=1`. |
| `POST /api/usage/{id}/claude-reset`, `POST /api/usage/{id}/codex-reset-credits` | Spend a reset grant / reset credit | **Mutating and costly: never called** (RF3.6). |

Auth model ([`dashboardGuard.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/dashboardGuard.js)): every `/api/*` route that is not on the public list is **deny-by-default** and passes only with (a) a valid `auth_token` JWT cookie, (b) the `x-9r-cli-token` header, which is derived from the host's machine id with a fixed salt, or (c) the instance setting `requireLogin = false`, which opens the management API **to anyone who can reach the port**. The inference API keys (`sk-…`, `/api/keys`) authorize only `/v1`, never `/api/usage`. There is no read-only management token.

Upstream providers whose usage 9router reads (`USAGE_HANDLERS`, revision `f39d8f07`): `claude`, `codex`, `github` (Copilot), `gemini-cli`, `antigravity`, `kiro`, `qoder`/`qoder-cn`, `iflow`, `ollama`, `glm`/`glm-cn`, `minimax`/`minimax-cn`, `minimax-code`/`minimax-code-global`, `vercel-ai-gateway`, `codebuddy-cn`/`codebuddy-intl`, `grok-cli`, `kimi`, `opencode-go`, `opencode-zen`, `deepseek`, `groq`, `zed`, `xiaomi-mimo`, `commandcode`, `cursor`.

### 1.2 How Cuotai uses 9router today: copied contracts, not an instance

Two different things must not be confused:

| | **A. Contracts adapted from 9router's source (today)** | **B. Talking to a running 9router instance (this PRD)** |
|---|---|---|
| What | `providers/router_*.rs` (`router_accounts`, `router_existing`, `router_grok`, `router_keys`, `router_minimax`, `router_sessions`), `antigravity.rs` and `oauth/reference*.rs` reimplement in Rust the upstream quota queries and login flows that 9router's `open-sse/services/usage/*.js` and `src/lib/oauth/providers/*.js` perform, pinned at revision `f01fb909e37189008080632ddaf404f096345cde`; `builtin_models.rs` copies its registry model lists (to be removed by PRD 0028 CAT-01, decided 2026-10-23 for PRD 0002). | Cuotai calls the instance's own management routes (§1.1) and shows the quotas of every connection the user already added **in 9router**. |
| Credentials | The user's own upstream credential (env var, vault, native login) per Cuotai account. | One credential for the instance (dashboard password or none); **upstream tokens never leave the instance**. |
| Talks to | The upstream provider directly. | Only the instance. |
| Attribution | `THIRD_PARTY_NOTICES` (MIT, "Cuotai does not incorporate the 9router inference proxy …"), `docs/providers.md` §"Compatibility with 9router" ("You do not need to run 9router alongside Cuotai"). | No code is copied: the instance is a remote service; the response shapes are re-described in Rust fixtures. The notice gains a line naming the routes and revision as a reference (RF11.3). |

A keeps working unchanged. B is new: the user who runs 9router should not re-create every connection in Cuotai (and could not, for credentials 9router imported from tools). This PRD names the new provider **`nine_router`** (display "9router"; an enum variant cannot start with a digit) and keeps the `router_*` module names for A.

### 1.3 Owner request

When the user adds 9router, Cuotai shows the quotas of the multiple providers connected inside it, in a **dedicated tab/section** for 9router. The same pattern must generalize to other aggregation/routing services.

### 1.4 Other aggregators (quick survey)

| Service | Hosting | Exposes usage **per upstream**? | Endpoint and auth | Verdict |
|---|---|---|---|---|
| **9router** | self-hosted (local or remote) | **Yes**: subscription quotas of every connection | §1.1; dashboard password / none | **RF1–RF9** (first adapter) |
| **LiteLLM proxy** | self-hosted | **Partly**: USD budget, spend and reset per provider when `provider_budget_config` is set; per-key/team budgets | `GET /provider/budgets` with the master/virtual key ([docs](https://docs.litellm.ai/docs/proxy/provider_budget_routing)); `/key/info` | **RF10 candidate** (documented, read-only GET) |
| **new-api** (QuantumNous) | self-hosted, AGPL-3.0 | **Yes, as balances**: each channel stores `balance` (USD) with `balance_updated_time` and `used_quota` ([`model/channel.go`](https://github.com/QuantumNous/new-api/blob/main/model/channel.go)); user quota via `/api/user/self` | Admin access token; channel list is admin-only; `GET /api/channel/update_balance/:id` makes the instance query the upstream (**never call**) | RF10 candidate after LiteLLM; HTTP use only, no code copied (AGPL) |
| **one-api** (songquanpeng) | self-hosted, MIT | Same channel-balance model as new-api | Admin token | Covered by the new-api adapter if the shapes match; low activity (last push 2026-01) |
| **Portkey** | SaaS / self-hosted gateway | Budgets and usage limits on keys, workspaces and integrations; Analytics Admin API (costs, tokens) ([budget limits](https://portkey.ai/docs/product/administration/enforce-budget-and-rate-limit)) | Admin API key; budgets are Enterprise/select Pro | Later; per-integration budget is an "aggregator" shape, analytics are cost history (PRD 0030) |
| **Requesty** | SaaS | Per-key/project spend limits; key usage in the management API (enterprise) ([key management](https://docs.requesty.ai/features/key-management-api)) | Management key; base URL differs between pages | Later, single-account shape (like OpenRouter) |
| **OpenRouter** | SaaS | **No** upstream quotas: OpenRouter is the biller; it reports its own credits and key limit | `/api/v1/credits` (management key), `/api/v1/key` | **PRD 0038** (plain provider; `aggregator = false`) |
| **Vercel AI Gateway** | SaaS | No; team credit balance and lifetime spend | `GET https://ai-gateway.vercel.sh/v1/credits` (Bearer) ([docs](https://vercel.com/docs/ai-gateway/usage)) | Plain provider candidate; 9router already reads it as an upstream |
| **Helicone**, Cloudflare AI Gateway | SaaS | No quotas; observability (request logs, cost) and rate-limit **policies** set by headers ([Helicone rate limits](https://docs.helicone.ai/features/advanced-usage/custom-rate-limits)) | — | Out of scope (cost history, not limits) |
| Unify-like routers | SaaS | Own credit balance only | — | Plain provider if ever requested |

Three shapes follow and define the class (RF1.1): **relay of upstream quotas** (9router), **per-upstream budgets or balances** (LiteLLM, new-api/one-api, Portkey integrations), and **own credits only** (OpenRouter, Vercel, Requesty, Unify), which is not an aggregator in Cuotai's model but an ordinary provider.

## 2. Problem

- A 9router user has 5–20 upstream connections already authorized there; reproducing each in Cuotai is tedious, sometimes impossible (credentials imported by 9router from local tools), and duplicates secrets on the machine.
- Cuotai's model is flat (one account → one report). An instance with many connections either becomes one unreadable card or many accounts that the user did not create and cannot edit.
- Every engine (binding, pace, alerts, history, resets, export) assumes that one account is one upstream; an aggregator breaks that without a defined rule.
- A user often has the same Claude subscription both directly in Cuotai and inside 9router: shown twice, it fires two alerts and double-counts the "All" summary.
- The 9router management API is undocumented, can renew upstream tokens as a side effect, has mutating neighbours (reset grants) and an unsanitized sibling route, and may be reached over a network: a careless client would leak secrets or spend credits.

## 3. Goals

- An **aggregator provider class** in the core: one aggregator account produces sub-accounts whose readings go through the same binding, pace, alert, freshness and history engines as direct accounts.
- **9router** as the first aggregator: add it with a base URL and (optionally) its dashboard password, local or remote, discover its connections, show each connection's quotas.
- A **dedicated 9router tab/section** in the tray and the main window, and a section in the TUI, following `docs/design-system.md`.
- Additive contracts only: `--json` and `export::v1` stay valid for consumers that ignore the new fields.
- Strict safety: GET-only allow-list (plus login), no upstream token ever read or stored, TLS rules for remote instances, bounded query cost.
- A documented evaluation path for a second adapter (LiteLLM, then new-api/one-api).

## 4. Non-goals

- Changing anything in the aggregator (enable/disable a connection, reorder, delete, reset quotas, spend reset grants or credits, re-authorize a connection). Cuotai stays read-only; "fix it" actions open the instance's dashboard in the browser.
- Deriving or reading the 9router CLI token (`x-9r-cli-token`, a machine-id derivation) or reading 9router's database, `jwt-secret` or data directory: this is the credential extraction `THIRD_PARTY_NOTICES` already rules out.
- Combos, model fallback chains, request logs, token counts or cost estimates of the aggregator (9router's "savings tracker"); cost history is PRD 0030.
- Routing or proxying inference through Cuotai.
- OpenRouter (PRD 0038) and other own-credits services: they are ordinary providers.
- Importing 9router connections as direct Cuotai accounts (copying their tokens). A sub-account can be *matched* to a direct account (RF5), never converted.

## 5. Functional requirements

### RF1 — Aggregator class and nested readings in the core (high priority; prerequisite)

- **RF1.1** `Provider::is_aggregator()` (true for `NineRouter`, later LiteLLM/new-api) and an internal `aggregators` module with one adapter trait: `discover(ctx) -> Vec<Upstream>` (the instance's connections, safe fields only) and `fetch(ctx, &Upstream) -> SubReading`. Shapes (`AggregatorShape::QuotaRelay | UpstreamBudgets`) are recorded per adapter; "own credits only" services are not aggregators.
- **RF1.2** `Report` gains `sub_reports: Vec<SubReport>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`), so every other report serializes byte-for-byte as today. `SubReport` = `id` (RF5.1), `upstream` (the aggregator's provider slug verbatim, e.g. `"gemini-cli"`), `provider: Option<Provider>` (the mapped Cuotai provider, RF3.4; `None` for an upstream Cuotai does not know), `label`, `status: Status`, `metrics: Vec<Metric>`, `notes_l10n`, `checked_at`, `as_of: Option<DateTime<Utc>>` (RF4.3), `enabled_upstream: Option<bool>` (the aggregator's own on/off flag), `duplicate_of: Option<String>` (RF5).
- **RF1.3** The aggregator's own `Report` describes the **instance**: `status` (reachability and auth of the instance), `metrics` (instance-level metrics only; 9router has none, LiteLLM may carry a global budget), notes (version, warnings). A failed instance yields one failed report and no sub-reports; a failed connection yields one failed sub-report and never fails the instance.
- **RF1.4** One flattening view in the core, `Snapshot::units()` (and `RefreshCycle` equivalents), yields every direct account **and** every sub-account as a `Unit { key, parent: Option<key>, provider, report-like view }`. `metrics::binding`, `pace::assess`, `alerts::evaluate`, `resets::*`, `freshness::assess`, history recording (PRD 0006) and `events::diff` iterate units, so no engine learns about nesting. The aggregator unit itself takes part only with its instance-level metrics.
- **RF1.5** `metrics::classify_report` for the aggregator account returns the **pool class**: `Available` when at least one enabled, non-duplicate sub-account is `Available`, `Exhausted` when all measured ones are exhausted, `Unmeasured` otherwise (RF6.3).
- **RF1.6** Access (PRD 0005): the exhaustive `access::classify` arm for `NineRouter` declares `level = Undocumented`, a new credential class `AggregatorLogin` ("dashboard password of your instance, or none"), a new scope `Instance`, `metered = false` (RF4.5 explains why), evidence = the pinned `dashboardGuard.js`/`usage` route files, and `access.summary.nine_router`/`access.reason.nine_router`/`access.risk.nine_router` ("Reading usage can make 9router renew the upstream sign-ins it holds"). Each `SubReport` carries `via` evidence: the 9router handler file for that upstream (e.g. `open-sse/services/usage/claude.js`). The `docs/providers.md` matrix is regenerated with the drift test.
- **Acceptance criterion:** table tests of `units()` with direct accounts, an aggregator with mixed sub-statuses and a failed instance; every listed engine produces per-sub-account results identical to the same reading presented as a direct account; `--json` snapshot tests are unchanged for fixtures without aggregators; the `access` exhaustive-match and drift tests pass.

### RF2 — Adding a 9router instance (high priority)

- **RF2.1 Fields** (`account_edit` descriptors, shared by the TUI wizard and the desktop form): **Base URL** (required, default `http://127.0.0.1:20128`; scheme + host + optional port + optional path prefix for a reverse proxy; no query, fragment or userinfo); **Sign-in** select: "No password (login disabled on the instance)" or "Dashboard password" (stored in the OS vault through the existing credential-reference mechanism, or `password_env` naming a variable; never in TOML); optional **Upstream filter** (`include`/`exclude` lists of upstream slugs) and **Hidden connections** (by sub-account id); TLS fields of RF2.3. Account name optional per the 2026-10-23 decision (PRD 0029): default label "9router", required only with two instances.
- **RF2.2 Test connection** (button in the form and wizard, mandatory before the first save, as decided for PRD 0016 sessions): `GET /api/health` → `GET /api/version` (shown as "9router 0.5.109") → login if a password is set → `GET /api/providers/client?pageSize=1`. Outcomes, each with one core sentence and no raw body: reachable and authorized ("Found 7 connections with usage"); not 9router (health not `{ok:true}`); password wrong (401); locked out (429, shows the wait from `Retry-After`; Cuotai never retries a login automatically within that wait); password login disabled because SSO is on (403, RF2.6); dashboard blocked through the tunnel (403); "no password" chosen but the instance requires one (401/redirect to `/login`); version below the tested minimum (warning, not a block).
- **RF2.3 Local versus remote and TLS.** Loopback hosts (`localhost`, `127.0.0.0/8`, `::1`) may use `http`. Any other host requires `https`, except when the user turns on "Allow unencrypted connection on my private network", allowed only for RFC 1918, link-local, CGNAT/Tailscale `100.64.0.0/10` and `.local`/`.ts.net` names, with a persistent warning chip on the account. TLS uses the platform verifier; a self-signed instance is supported **only** by (a) `tls_ca_file` (a PEM path the user chose) or (b) `tls_pin_sha256` (leaf certificate SHA-256), captured trust-on-first-use in the Test step, which shows the fingerprint and asks for confirmation. There is no "accept any certificate" option. A pin mismatch is an authentication-class error ("The instance's certificate changed"), never silently re-pinned.
- **RF2.4 Exposure warning.** When "no password" works against a non-loopback base URL, the account shows a warning callout once and a chip afterwards: "This 9router instance answers management requests without a password. Anyone who can reach it can read and change its connections." Cuotai still works.
- **RF2.5 Discovery (consent-gated, presence only).** With PRD 0027's local-detection consent, the catalog probes `GET http://127.0.0.1:20128/api/health` (200 ms timeout, once per catalog opening) and, on `{ok:true}`, shows "Found on this device" on the 9router tile with the base URL prefilled. No other port scan, no reading of `~/.9router` or any 9router file, no remote discovery.
- **RF2.6 SSO-only instances.** Password login is refused when the instance uses OIDC/SAML. v1 offers a documented fallback: paste the `auth_token` cookie as a user session (PRD 0016 slot, cookie name `auth_token` only, host = base URL host only, expiry read from the JWT `exp` without verifying it, "Session expires in N h" chip, PRD 0016 RF3.2 expiry note). A browser sign-in flow is an open question (§12).
- **RF2.7 TOML** (example in `cuotai.example.toml`):

```toml
[[accounts]]
provider = "nine_router"
base_url = "http://127.0.0.1:20128"
# password_env = "NINEROUTER_PASSWORD"   # or a vault reference written by the form
[accounts.params]
include = ["claude", "codex", "github"]  # optional
```

- **Acceptance criterion:** `account_edit` validation tests (scheme rules per host class, rejected userinfo/query, private-network toggle limits, pin format); `wiremock` tests of every RF2.2 outcome; a rustls test server with a self-signed certificate is refused without a pin, accepted with the right pin or CA file, refused after a pin change; the discovery probe never runs without consent.

### RF3 — 9router contract and normalization (high priority)

- **RF3.1 Session.** With a password, Cuotai logs in once per process (shared per instance like `OAuthSession`), keeps the `auth_token` value **in memory only**, re-logs in when the JWT is within 10 minutes of `exp` or on a 401/redirect, at most once per refresh; a failed login stops the instance's refresh with `AuthenticationError` and never loops (the limiter locks the IP). Cookies are sent only to the base URL origin; redirects are not followed except same-origin, and a redirect to `/login` is classified as authentication.
- **RF3.2 Discovery.** `GET /api/providers/client?accountStatus=all&pageSize=100&page=n` until `totalPages` (hard cap 10 pages / 1000 connections, then a note). The parser keeps only `id`, `provider`, `authType`, `name`, `displayName`, `email`, `isActive`, `testStatus`, `expiresAt`, `priority` and drops everything else at deserialization (including `lastError`, `connectionProxyUrl` — it can contain proxy credentials — and any unknown field). `isActive = false` sub-accounts are listed as "Off in 9router" without a usage query.
- **RF3.3 Usage.** `GET /api/usage/{id}` per active connection, **never with `force=1`**. Parsing of the 9router shape: `quotas` map → one `Metric` per entry with `used`/`limit = total`/`remaining`, `resets_at = resetAt` (ISO, invalid → `None`), `kind = Quota`; `unlimited`-style entries → no meter; `plan` → plan badge; `limitReached = true` with no quota detail → `Exhausted` signal; `{ message }` → a sub-report `Unsupported` ("9router has no usage for this connection") or, when 9router's auth-expired patterns match, `AuthenticationError` ("Reconnect this account in 9router"); 401 → `AuthenticationError`; 404 → the connection disappeared (dropped on the next discovery); 5xx/timeout → `Error`. The `message` text is never shown verbatim.
- **RF3.4 Upstream mapping table** (core, tested): `claude`→`Claude`, `codex`→`ChatGpt`, `github`→`Copilot`, `gemini-cli`→`GeminiAi`, `antigravity`→`Antigravity`, `kiro`→`Kiro`, `qoder`/`qoder-cn`→`Qoder`, `ollama`→`OllamaCloud`, `minimax`/`minimax-cn`→`MiniMax`, `kimi`→`Kimi`, `codebuddy-cn`/`codebuddy-intl`→`CodeBuddy`, `grok-cli`→`Grok`, `opencode-go`→`OpenCodeGo`, `groq`→`Groq`, `zed`→`Zed`, `xiaomi-mimo`→`XiaomiMimo`, `cursor`→`Cursor`; every other slug (`glm`, `deepseek`, `iflow`, `vercel-ai-gateway`, `commandcode`, `opencode-zen`, `minimax-code*`, future ones) → `provider = None`, a generic tile and 9router's provider name. The mapped provider gives the tile, the incident source (RF6.6) and the window table of RF3.5.
- **RF3.5 Windows and labels.** 9router's quota labels are free text ("session (5h)", "weekly (7d)", "weekly Opus (7d)", Codex `session`/`weekly`). A per-upstream table maps known labels to `MetricWindow` (`FiveHours`, `Weekly`, `Monthly`, `Daily`) and to Cuotai's localized label keys; an unknown label keeps the text, `window = None`, and so no pace (honest default). Model-scoped weekly limits get `Metric::scope` when PRD 0023's MET-01 lands.
- **RF3.6 Request allow-list.** The 9router client is a dedicated wrapper that can only issue `GET /api/health`, `GET /api/version`, `POST /api/auth/login`, `GET /api/providers/client`, `GET /api/usage/{id}` (id = `[A-Za-z0-9_-]{1,128}`), relative to the base URL. Any other method or path is a compile-time impossibility (typed enum of routes). `/api/providers`, `/api/usage/*/claude-reset`, `/api/usage/*/codex-reset-credits`, `/api/settings`, `/api/keys` and `/v1/*` are named in a negative test.
- **RF3.7 Honesty caveat.** 9router itself turns some missing fields into 0 (e.g. `toFiniteNumber(…, 0)` in `codex.js`, `Math.max(0, 100 − used)` in `claude.js`), which Cuotai's own adapters refuse to do (`docs/providers.md`, "Deliberate differences from 9router"). Sub-account metrics therefore carry the source "via 9router" and the details say "Values as normalized by 9router; a missing value may show as 0". Where 9router passes a value through unchanged, nothing else changes.
- **RF3.8 Versioning.** The adapter records `contract_revision = f39d8f07…` and `tested_min` = the first version whose routes match, **filled after the owner's local verification** (decision D8, Appendix B); an older or newer instance still runs, and a schema failure is reported through PRD 0028's diagnostics (field paths, never values). PRD 0028's local canary gains an optional 9router target on the owner's machine.
- **Acceptance criterion:** `wiremock` fixtures (§11) for every branch; a request-recorder test asserts that a full refresh issues only allow-listed requests; a fixture with injected secret-looking fields (`accessToken`, `copilotToken`, `connectionProxyUrl` with userinfo, `lastError` with a token) proves none reaches `Report`, notes, logs or export.

### RF4 — Refresh cadence, cost and freshness (high priority)

- **RF4.1** One aggregator refresh = 1 discovery (paged) + 1 usage request per active, non-hidden connection. Default interval 300 s, floor 120 s (an aggregator account cannot be set lower); concurrency per instance 2 (inside the global semaphore); per-connection timeout 15 s; per-instance deadline 60 s, after which the remaining connections keep their previous reading as stale (RF4.2).
- **RF4.2** Freshness, `last_good` and the stale treatment (PRD 0010) are **per sub-account**: a sub-account whose usage request failed keeps its last `Ready` reading as stale while its siblings update; the instance failing makes every sub-account stale with the instance's cause.
- **RF4.3** `checked_at` is when Cuotai got the answer. 9router may serve a cached value (the Claude handler caches per token) and exposes no age, so `as_of = None` and the details say "9router may show a cached reading". Adapters that expose a date (new-api `balance_updated_time`) fill `as_of` (PRD 0033).
- **RF4.4** "Refresh now" on an aggregator refreshes the whole instance (discovery + usage) without `force=1`; on a sub-account it refreshes that connection only (one usage request). The 30-s post-reset query (decision I, PRD 0033) applies to sub-accounts and counts toward the floor.
- **RF4.5** Cost: Cuotai's requests do not bill anything, but each usage request makes the instance query the upstream with its credentials and possibly renew a token. The cadence floor and the "never `force=1`" rule bound that load; `metered = false` because nothing is billed per query, and the access risk text (RF1.6) states the side effect.
- **Acceptance criterion:** clock-driven tests of the floor, the deadline, partial stale sub-accounts, per-sub refresh, and that a manual refresh never adds `force=1`.

### RF5 — Identity and duplicates with direct accounts (high priority; needs PRD 0029)

- **RF5.1** Sub-account id = `<aggregator stable id>/<hash of the 9router connection id>` (PRD 0029 stable id of the parent; SHA-256 truncated, so the raw connection UUID is not a public key). Key for export and events: `nine_router:<parent label>/<sub id>`. Every per-sub preference (hidden, order, alert opt-out, monogram, duplicate override) is keyed by it in `desktop.json`. A connection deleted and re-added in 9router is a new sub-account (history does not carry over; documented).
- **RF5.2** Duplicate detection: a sub-account is the **same** as a direct account when the mapped provider matches and a stable identity matches exactly: normalized email (case-folded, trimmed) from 9router's `email`/`githubEmail` versus the direct account's identity (PRD 0029 identity fields), or a provider account id where both expose one. Never by name or by fuzzy match. Emails are compared in memory and never exported unless `include_identity` is on.
- **RF5.3** Policy `dedup = auto` (default): the **direct account wins**. The duplicate sub-account stays visible in the 9router tab with the chip "Also monitored directly" and a link to the direct account, is excluded from the "All" view, the global tightest/best ranking (decision C), the menu-bar items and alerts, and its history is not recorded twice. Per sub-account override: "Show in both" or "Hide here". `duplicate_of` carries the direct account key.
- **Acceptance criterion:** tests with a direct Claude account and a 9router Claude connection of the same email (deduplicated), different email (both shown), missing email (both shown, no guess), and both overrides; no duplicate alert in a `alerts::evaluate` run.

### RF6 — Limits, pace, alerts and incidents across sub-accounts (high priority)

- **RF6.1** Binding (PRD 0009), pace (PRD 0007) and threshold/exhausted/recovered alerts (PRD 0008) apply **per sub-account**, exactly as for a direct account of the mapped provider; there is no binding across sub-accounts (they are different subscriptions).
- **RF6.2** Overage (PRD 0023): sub-account metrics are `billing = Included` unless the upstream mapping table says otherwise; 9router's shape carries no overage marker, so no overage state is derived (`None`).
- **RF6.3 Pool summary.** Because 9router falls back between connections, the instance-level figure that matters is the **pool**: per mapped upstream, "N of M available" and, when all are exhausted, the earliest return time among them. `aggregate::pool(report) -> Vec<Pool { upstream, available, total, returns_at }>`, core-localized texts ("Claude · 2 of 3 available", "Claude · all exhausted · returns 19:30").
- **RF6.4 Alert grouping.** Alert preference per aggregator account: "Each connection" or **"Only when a whole pool runs out" (default)**: the latter replaces per-connection `Exhausted`/`Threshold` notices with `AlertKind::PoolExhausted` (once per pool exhaustion, recovered when any connection returns) and keeps per-connection alerts silent. Notification title "9router · Claude: all accounts exhausted"; body with the return time. Duplicates (RF5.3) never alert.
- **RF6.5 Best account now (decision C).** Sub-accounts take part in the TOML group ranking like direct accounts (group = the mapped provider unless a group is set for the aggregator), so "best Claude account now" may point at a 9router connection ("via 9router").
- **RF6.6 Incidents (PRD 0011).** A sub-account whose upstream maps to a Cuotai provider shows that provider's incident status; unmapped upstreams have none. A self-hosted instance has no status page: its unreachability is the account's error state, never an incident.
- **Acceptance criterion:** table tests for pool summaries (all available, partial, all exhausted with and without reset times, all unmeasured), both alert modes with a fixed clock, the first-reading rule of PRD 0008 RF1.7 per sub-account, and the incident mapping.

### RF7 — `export::v1`, local server, events and widgets (medium priority)

- **RF7.1** `AccountExport` gains `sub_accounts: [SubAccountExport]` (empty array omitted) and `aggregator: { kind: "9router", version, pools: [...] } | null`, additive under the v1 policy; `SubAccountExport` reuses the per-account fields (`key`, `name` = label, `provider` = mapped slug or `null`, `upstream`, `class`, `report` redacted, `age_seconds`, `freshness`, `stale`, `last_good`, `binding`, `pace`, `duplicate_of`). Sub-accounts are **not** appended to the top-level `accounts` array, so `index` and counts stay meaningful for existing consumers. JSON Schema and reference output regenerated; `docs/export-v1.md` documents how to flatten.
- **RF7.2** Local server (`/v1/limits`, PRD 0012/0031) lists sub-accounts as items with `parent`; `best` (decision C) may name a sub-account key.
- **RF7.3** Events (PRD 0013): sub-account events carry `account` = sub key and `parent` = aggregator key; new event `pool_exhausted`/`pool_recovered`; hooks filter on `parent`. Additive to events schema 1.
- **RF7.4** Widgets (PRD 0014): sub-accounts are selectable items; the widget snapshot carries `parent`.
- **Acceptance criterion:** schema and reference tests; a fixture with one aggregator and three sub-accounts, all other accounts unchanged byte-for-byte; the no-leak test extended to sub-accounts (no email unless `include_identity`, no connection UUID).

### RF8 — Desktop presentation: dedicated aggregator tab and section (medium priority)

Visuals follow `docs/design-system.md`; the components below are proposed as **DS-CMP-AGG-1…5** for §4.28 by the design-system owner (this PRD does not edit it).

- **RF8.1 Tray filter tab (dedicated tab).** Each aggregator account gets its own icon tab after the provider tabs (DS-CMP-TAB-1): the aggregator tile, the sub-account count as the muted number (DS-CMP-TAB-2), the worst pool state as the 12 px state ring, accessible name "9router, 7 connections, Claude pool exhausted". Its sub-accounts do **not** add icon tabs of their upstream providers. Overflow follows DS-CMP-TAB-4.
- **RF8.2 Aggregator tab body.** Grouped list by mapped upstream (section micro labels, DS-CMP-LIST-1): "CLAUDE · 2 of 3 available" header, then one standard account row per sub-account (DS-CMP-ROW: upstream tile, connection label as title, trailing figure block of its binding, continuous meter DS-CMP-MTR-2 with pace corridor when the core supplies pace, status line, time chips). The MONO badge (DS-CMP-MONO) applies within an upstream group. Unmapped upstreams use a neutral generic tile and 9router's provider name. A header line above the groups shows the instance: label, "9router 0.5.109", the global age, and a ghost "Open dashboard" button (opens `base_url/dashboard/usage` through `open_external`).
- **RF8.3 "All" view.** By default the aggregator appears as **one group row**: aggregator tile, label, trailing figure block = pool availability ("5/7", unit word "available"), status line = the tightest non-duplicate sub-account ("Tightest: Claude · work 22% left") or the pool alert; clicking the row **expands it in place** (accordion, chevron down, DS-CMP-NAV-2 "expands here", DS-CMP-ROW-5) to show the pool lines and the sub-account rows, with a last ghost line "Open 9router tab" (decision D2). Settings › Dashboard: "9router connections in All: Grouped (default) / Listed with my accounts / Hidden". Listed mode shows sub-account rows with a neutral "via 9router" identity badge (DS-CMP-BADGE-1/4, core-supplied).
- **RF8.4 Main window.** The filter gains the aggregator entry; the card grid shows a section per aggregator below direct accounts: section header (micro label "9ROUTER · HOME", muted hint "7 connections · 9router 0.5.109", "Open dashboard"), then sub-account cards (DS-CMP-CARD) grouped by upstream with equal heights (DS-CMP-CARD-2). Sub-account cards have no enable switch (the switch belongs to 9router; "Off in 9router" cards are at 55 % opacity with that chip) and a per-card "Hide" action instead of edit/remove.
- **RF8.5 Details.** Sub-account details: upstream, mapped provider, plan, metrics (DS-CMP-SUB), source "via 9router" with the RF3.7 caveat, access line with the `via` evidence and DS-CMP-VER-1, duplicate state, and recovery actions limited to "Open dashboard". Instance details: base URL (host only), version, auth mode, TLS mode (CA file / pinned fingerprint shown truncated), exposure warning, pool table, last discovery time.
- **RF8.6 Honest-data rules.** Unknown values use the hatch and "—" (DS-CMP-HON-1); stale sub-accounts get the stale treatment (DS-CMP-HON-3); figures stay `--fg` (DS-CMP-ROW-1a); "Exhausted" replaces the figure; nothing reads "0%" because a field was absent on Cuotai's side.
- **RF8.7 Account form.** RF2 fields, Test connection with its outcomes as inline feedback (DS-CMP-MSG-1), the TOFU fingerprint confirmation as a dialog (DS-CMP-DLG), the exposure warning as a warning callout (DS-CMP-CALL). The catalog lists 9router under a new sign-in group "Self-hosted gateway" (DS-CMP-LIST-6), with "Found on this device" from RF2.5 (DS-CMP-CAT-2).
- **Acceptance criterion:** smoke in the four languages and the reference themes with an instance of 7 connections (mixed states, one duplicate, one unmapped upstream, one off, one stale), at 380 px tray width; keyboard path through the tab and group row; accessible names include pool states; no raw message, URL path or email (unless the user shows identity) on screen.

### RF9 — TUI and CLI (medium priority)

- **RF9.1** TUI dashboard: aggregator accounts render as a section after direct accounts: a header line ("9router · home — 5/7 available — 9router 0.5.109"), then sub-account cards grouped by upstream with the usual meters and pace glyphs; a key (`g`) collapses/expands the section; the filter cycles through aggregators. Duplicates show "also direct" and are skipped by the summary line.
- **RF9.2** Wizard: RF2 fields from the core descriptors, Test connection, fingerprint confirmation prompt; `--list-providers` lists `nine_router` with "aggregator".
- **RF9.3** `--once`: the aggregator block indented under its account line; `--json`: `sub_reports` (RF1.2); exit codes: an instance failure counts as an account failure (2); a failed sub-account alone does not (the instance is Ready), but `--strict-exit` (PRD 0031) treats it as one.
- **RF9.4** NO_COLOR and theme rules of the TUI NFR apply (2026-10-23 open item 0004).
- **Acceptance criterion:** `TestBackend` rendering tests of the section (collapsed/expanded, narrow width), a `--once` snapshot and a `--json` snapshot with one aggregator.

### RF10 — Second aggregator adapter, gated on evidence (low priority)

- **RF10.1** LiteLLM proxy (`litellm`): base URL + virtual or master key (vault/env); `GET /provider/budgets` → one sub-account per provider with a USD `Spend` metric, `limit = budget_limit`, `resets_at = budget_reset_at`, window from `time_period`; optional `GET /key/info` for the key's own budget as an instance metric. Delivered only after a live instance confirms the shape on the target LiteLLM version; documented as `Official` (it is in LiteLLM's docs).
- **RF10.2** new-api/one-api (`new_api`): admin token; channel list → one sub-account per channel with a USD `Balance` metric and `as_of = balance_updated_time`; **never** `update_balance`. HTTP use only (new-api is AGPL-3.0: no code or fixtures copied from it; fixtures written from observed shapes).
- **RF10.3** Each new adapter reuses RF1–RF9 unchanged; only its contract, mapping table, access arm and fixtures are new. Portkey/Requesty are revisited when a user asks (§12).
- **Acceptance criterion:** the same fixture families as §11 per adapter; an adapter is not merged without one owner-verified live reading.

### RF11 — Documentation and notices (low priority)

- **RF11.1** `docs/providers.md`: a section "Aggregators" (what is read, auth modes, local vs remote, TLS rules, the 9router cache and zero-normalization caveats, the token-renewal side effect, the never-called routes) and the regenerated matrix; `docs/providers.md` §"Compatibility with 9router" gains one paragraph distinguishing the copied contracts from the `nine_router` provider.
- **RF11.2** `docs/export-v1.md`, `docs/events.md`, `docs/local-server.md`, `docs/tui.md` and `README.md` describe sub-accounts, pools and duplicates; `cuotai.example.toml` gets the RF2.7 example.
- **RF11.3** `THIRD_PARTY_NOTICES`: the 9router entry gains "The `nine_router` provider calls a user's own 9router instance through its management routes as observed at revision f39d8f07…; no 9router code is incorporated for it."
- **Acceptance criterion:** documentation drift tests pass; the changelog check passes.

## 6. Non-functional requirements

- **Privacy:** no upstream token, cookie, API key, proxy URL or 9router error text is read into Cuotai memory beyond the deserializer's allow-list, logged, stored or exported; the dashboard password lives in the vault (or env), the JWT only in memory; connection emails are identity data (shown per the user's identity setting, exported only with `include_identity`); connection UUIDs are hashed in keys.
- **Security of remote instances:** HTTPS outside loopback (narrow private-network opt-in), platform roots or an explicit CA file/pin, no blanket certificate bypass, no cross-origin redirects, cookies bound to the base URL origin, request allow-list (RF3.6), login attempts bounded (no retry inside `Retry-After`), exposure warning (RF2.4). Errors never include the base URL path, the response body or headers (AGENTS.md).
- **Read-only:** Cuotai never issues a mutating request to an aggregator other than the login POST; the side effects the instance performs on its own (token renewal, email backfill) are documented, not triggered on purpose.
- **Timeouts and load:** 15 s per request, 60 s per instance, concurrency 2 per instance, cadence floor 120 s; the instance's slowness never blocks direct accounts (separate deadline in `refresh.rs`).
- **Compatibility:** `--json` and `export::v1` changes are additive (`sub_reports`, `sub_accounts`, `aggregator`, new event names), so they raise only the minor version with the `feat` commits that deliver them, without a `BREAKING CHANGE` footer; new TOML keys only for the new provider.
- **i18n and accessibility:** every new text in pt-BR (fallback), en-US, es and it (and the PRD 0035 catalogs once they exist); pool and duplicate states carried by text and icon, never color alone (DS-PRI-6); tabs and group rows keyboard reachable (DS-A11Y-4).
- **Honesty:** absent values stay `None`; the "via 9router" caveat is always available in details; no inferred window means no pace.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `Provider` | `NineRouter` (`nine_router`), later `Litellm`, `NewApi` | Additive enum value (`--list-providers`, matrix) |
| `Report` (`--json`) | `sub_reports` (omitted when empty) | Additive, byte-compatible otherwise |
| `export::v1` `AccountExport` | `sub_accounts`, `aggregator` | Additive (v1 policy) |
| Local server | sub-account items with `parent` | Additive |
| Events schema 1 | `parent` field; `pool_exhausted`, `pool_recovered` | Additive |
| `access` | credential `aggregator_login`, scope `instance`, sub-report `via` evidence | Additive slugs |
| TOML | `base_url`, `password_env`, `params.include/exclude`, `tls_ca_file`, `tls_pin_sha256`, `allow_private_http`, `dedup` | New keys for the new provider only |
| `desktop.json` | per-sub-account prefs keyed by sub id; "connections in All" mode; aggregator alert mode | Additive, lenient read |
| Vault | dashboard password; optional `auth_token` session (RF2.6) | New entries |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | aggregator trait, 9router client and allow-list, normalization and mapping tables, `sub_reports`, `units()`, pools, dedup, alert kind, access arm, account descriptors and validation, texts, export/events/server fields |
| Desktop | tray tab and group row, main-window section and cards, details, form and TOFU dialog, Settings rows, `open_external` to the dashboard |
| TUI/CLI | section, wizard, `--once`/`--json` output |
| Widgets | read RF7.4; no logic |

## 9. Dependencies and sequencing

RF1 (model and `units()`) first, then RF3 and RF2 with fixtures (they can ship behind the provider being hidden from the catalog), RF4, then RF5 once PRD 0029 delivers stable ids (until then dedup is off and every sub-account is shown), RF6, RF7, RF8/RF9 (RF8 visuals wait for PRD 0022 but can ship with current components), RF11 with each step, RF10 last. Wave 5f of `0000_control.md` (providers: 0036–0038). Coordinate with PRD 0028 CAT-01 (removing the copied model lists does not affect this PRD) and with PRD 0038 (OpenRouter shares nothing but the "own credits" classification of §1.4).

## 10. Risks

| Risk | Mitigation |
|---|---|
| 9router's management routes are undocumented and change often (daily commits) | Pinned revision, typed parser that fails as `query.schema` with PRD 0028 diagnostics, tested-version note, owner canary target, fixtures per branch |
| An unsanitized route or a new secret field leaks a token | Allow-list of routes (never `/api/providers`) and of fields at deserialization; no-leak fixtures with injected secrets |
| Reading usage triggers token renewal and upstream load on the instance | Cadence floor, no `force=1`, concurrency 2, documented access risk |
| Remote instance over plain HTTP or with `requireLogin = false` | HTTPS rule, narrow opt-in, exposure warning; Cuotai never weakens the instance's settings |
| Login lockout from repeated failures | One attempt per refresh, honour `Retry-After`, stop with an auth error |
| Values zero-filled by 9router read as real zeros | "via 9router" caveat (RF3.7); Cuotai never adds zeros of its own |
| Double counting with direct accounts | Exact-identity dedup, direct account wins, overrides |
| Licensing | 9router is MIT (attribution already in `THIRD_PARTY_NOTICES`, extended by RF11.3); new-api is AGPL-3.0, so only its HTTP interface is used and nothing is copied; LiteLLM's documented API is used as a client |
| UI overload with 20+ connections | Grouped by upstream, collapsed in "All", hide per connection, upstream filter in config |

## 11. Order of execution and verification

1. **RF1:** model, `units()`, access arm, drift test.
2. **RF3 and RF2:** client, allow-list, parsers and setup, with `wiremock` fixtures: health ok/not-9router; version; login success (Set-Cookie with a JWT whose `exp` is set), 401, 429 with `Retry-After`, 403 SSO, 403 tunnel; no-password success on loopback and on a non-loopback URL (exposure warning); redirect to `/login`; discovery with two pages, an inactive connection, a masked name, injected secret fields; usage for `claude` (5 h + weekly + model weekly), `codex` (session/weekly + `limitReached`), `github`, `gemini-cli`, `kiro`, an unmapped `glm`; `{ message }` soft failure, auth-expired message, 401, 404, 500, malformed JSON, missing fields, `total = 0`, invalid `resetAt`; per-request timeout and instance deadline; a request recorder for the allow-list; rustls self-signed server with and without pin/CA.
3. **RF4 and RF5:** cadence, stale per sub-account, dedup.
4. **RF6 and RF7:** pools, alerts, export, events.
5. **RF8 and RF9:** presentation; **RF11** with each step; **RF10** after a live LiteLLM reading.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke of the changed interface. Mocked HTTP is not proof of access: the requirement is delivered only when the owner's real local 9router instance, checked with the Appendix B procedure (remote HTTPS stays task T3), shows every connection's quotas in the dedicated tab, a duplicate direct Claude account is recognized, and a request log of the instance shows only allow-listed routes and no `force=1`.

## 12. Decisions and open items

### 12.1 Decisions (owner, 2026-10-23)

| # | Question | Decision |
|---|---|---|
| D1 | Provider slug | `nine_router` (display "9router"). Accepted. |
| D2 | Sub-accounts in "All" | **One grouped row by default** that expands in place on click (RF8.3); the dedicated tab is the full view; the Settings › Dashboard choice (grouped / listed / hidden) stays. |
| D3 | Default alert mode for aggregators | **Only when a whole pool is exhausted** (RF6.4); per-connection alerts are an opt-in. |
| D4 | SSO-only instances (OIDC/SAML) | v1: pasted `auth_token` session (RF2.6). Browser sign-in through the instance later (it needs a loopback callback 9router does not offer to third parties). |
| D5 | Read-only monitor token upstream | Open an issue on decolua/9router asking for a read-only, revocable monitor token. **Non-code task, owner/maintainer action** (task T1 below); draft text in Appendix A. Adopt it as a third auth mode once it exists. |
| D6 | `force=1` | A manual refresh **never** sends `force=1` (RF3.3, RF4.4). |
| D7 | Second aggregator adapter | **LiteLLM proxy first** (RF10.1); new-api/one-api after it. |
| D8 | Live verification | The owner runs 9router **locally** today: the local verification is owner-run with the read-only procedure of Appendix B (task T2). The version the owner runs is recorded there, and the minimum supported version is set to the first version whose routes match the contract (`tested_min` in RF3.8: **to be filled after verification**). Remote HTTPS verification stays open (no remote instance yet). |

### 12.2 Non-code tasks

| Task | Who | Output |
|---|---|---|
| T1 Upstream issue for a read-only monitor token (D5) | Owner/maintainer | Issue on github.com/decolua/9router from Appendix A; link recorded in this PRD |
| T2 Local live verification (D8) | Owner | Appendix B record (version, field names/types, zero-filled vs absent); sets `tested_min` |
| T3 Remote HTTPS verification | Owner, when a remote instance exists | Same procedure over HTTPS (platform roots, then a self-signed instance with a pin) |

### 12.3 Remaining open items

| Question | Recommendation |
|---|---|
| Dedup via account ids when no email is exposed | Only where both sides expose the same provider account id; otherwise show both. |
| Should 9router combos (fallback chains) become groups for "best account now" | Later; v1 groups by mapped upstream. |
| Minimum 9router version | Filled by T2 (D8); older instances run with a warning, not a block. |
| Remote instance behaviour (TLS, tunnel 403, login through a reverse proxy) | Verified by T3; until then only fixtures cover it. |

## Appendix A — Draft upstream issue (D5, T1)

> **Title:** Feature request: read-only, revocable token for monitoring usage from other tools
>
> Hi, and thank you for 9router. I maintain Cuotai, an open-source, read-only monitor for AI-provider quotas (it already credits 9router's usage handlers under the MIT license). Several users run 9router and would like Cuotai to show the quotas of the connections they already added here, instead of adding every account twice.
>
> Today the only way for another program to read `/api/providers/client` and `/api/usage/{id}` is the dashboard password (which grants full control) or `requireLogin = false`. Would you consider a **monitor token** that:
>
> - is created and revoked in the dashboard (one per consumer, with a name and a last-used time);
> - is sent as `Authorization: Bearer <token>` and is accepted **only** for GET requests to the usage routes (connection list with the safe fields, per-connection usage, version);
> - can never change settings, connections, keys, or spend reset grants/credits;
> - optionally carries the age of a cached usage value (for example `fetchedAt`), so monitors can tell a cached reading from a fresh one.
>
> We would be happy to test a pre-release or to help with a pull request if you agree on the shape. No credentials or user data are needed for this discussion. Thanks!

## Appendix B — Local live verification procedure (D8, T2)

Read-only, run by the owner against the local instance. **Never record values, tokens, cookies, emails, connection names or ids**; record only field names, JSON types and presence.

1. **Version and presence:** `GET http://127.0.0.1:20128/api/health` (expect `{ok:true}`); `GET /api/version` → record `currentVersion` and the instance's install method (npm or Docker).
2. **Login:** read the dashboard password from the OS vault (never type it into a shell history); `POST /api/auth/login` with it; record the status, that a `auth_token` cookie was set and its `Max-Age`; do not print the cookie.
3. **Connections:** `GET /api/providers/client?accountStatus=all&pageSize=100` with the cookie; record the top-level keys, the field names and types of one connection object, `pagination` keys, and the list of distinct `provider` slugs and `authType` values (slugs are not secrets).
4. **Usage per connection:** for each id, `GET /api/usage/{id}` **without** `force=1`, one at a time; per provider slug record: HTTP status, top-level keys, quota label strings, the keys and JSON types inside each quota, the format of `resetAt`, and for each field whether it was **absent**, **null**, or **0** (and, from the upstream app where the owner can see it, whether a 0 is real or zero-filled by 9router).
5. **Negative checks:** confirm the run issued only the requests above (9router's request log or a local proxy); no `force=1`, no POST other than login.
6. **Record** in a short table in this PRD (version, date, per-slug shape notes) and set `tested_min` in RF3.8 to the first version whose routes match; update the fixtures of §11 to these shapes.
