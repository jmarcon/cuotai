# Provider Catalog, Credentials and Limitations

The Cuotai catalog contains **36 providers**, combining billing admin APIs, Cloud Monitoring, coding-assistant subscriptions and integrations compatible with `9router`.

## Main integrations and credentials

| Identifier | Data queried | Configuration / restrictions |
|---|---|---|
| `open_ai_api` | Cost per currency; completions tokens and requests, UTC month | `OPENAI_ADMIN_KEY`, **Admin API key**. Optional `params.project_ids` and `params.models`, comma-separated |
| `anthropic_api` | USD cost and message tokens including cache, UTC month | `ANTHROPIC_ADMIN_KEY`, admin key. Optional `params.workspace_ids` and `params.models`. Priority Tier is not included in the cost report |
| `chat_gpt` | Percentage of the **Codex** windows and credit balance when returned | Native OAuth login or `CHATGPT_ACCESS_TOKEN`; optional `params.account_id`. Internal, undocumented interface. **Does not query the ChatGPT web message quota** |
| `claude` | Usage windows and renewal; extra usage when returned | Native OAuth login or `CLAUDE_OAUTH_TOKEN`. Undocumented interface, contract evidenced by community clients. Undocumented extra-usage units are preserved as provider units |
| `gemini_api` | Limits and recent consumption through Cloud Monitoring | Built-in OAuth login or `GOOGLE_CLOUD_ACCESS_TOKEN`, `params.project_id`; `monitoring.timeSeries.list`. Service `generativelanguage.googleapis.com` |
| `vertex_ai` | Limits and recent consumption through Cloud Monitoring | Same requirements; service `aiplatform.googleapis.com` |
| `aws_bedrock` | The month's UnblendedCost, filtered by the Amazon Bedrock service | `params.profile` required, authenticated AWS CLI, `ce:GetCostAndUsage`. Does not measure every inference quota |
| `azure_open_ai` | Monthly cost of the explicitly chosen resource | Built-in OAuth login or `AZURE_ACCESS_TOKEN`, `params.subscription_id`, `params.resource_id` (full ARM ID of `Microsoft.CognitiveServices/accounts`); Cost Management Reader. A shared Foundry resource includes other models |
| `cursor` | Team on-demand/total spend, or the personal subscription summary | `CURSOR_ADMIN_API_KEY` for the **Teams/Enterprise** Admin API, or an explicit import of the local SQLite session for a personal account |
| `copilot` | Personal interaction quota, or gross usage/net billing cost | Native OAuth login or `GITHUB_TOKEN`. `params.mode=user` (default) or `org` + `params.org`. `params.billing=premium_requests` (default) or `ai_credits`. A personal account does not include licenses billed by the organization |
| `windsurf` | Credit usage, allocation and additional credits in the cycle | `WINDSURF_SERVICE_KEY`. **Enterprise**, Billing Read and Teams Read-Only permissions. Per-seat allocation is not a shared balance; the remainder is not inferred |

A generation key does not replace an admin key or a subscription token. No cookie, Keychain entry or credential file of another app is extracted automatically; only Cuotai's own entries are read from the vault.

## Usage access matrix

How each account's usage is read, as `cuotai_core::access::describe` classifies it (PRD 0005) and as the wizard, the desktop dialog and the cards show it. A provider has one row for its default configuration plus one per mode (a `params` value or the native login) that changes the queried interface; modes with the same classification share a row.

- **Level:** `Official` is an interface the vendor documents for that credential (it does not promise permission on your account); `Undocumented` is internal or inferred and may change without notice; `Derived` infers usage from another source (Cloud Monitoring metrics, rate-limit headers); `Unavailable` has no query and the account shows the reason instead of numbers.
- **Credential:** the class of secret the query needs. **Plan:** the vendor plan the query requires, when any. **Scope:** what the number covers (`Whole cloud` is a bill for every product of the cloud account).
- **Evidence:** the vendor documentation, or the public 9router/community-client file whose contract the adapter follows. Every row was last checked on the descriptor's `verified_on` date.

The table is generated: a core test compares it with `access::describe` and fails when they differ. After changing a classification, regenerate it with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix` and commit the result.

<!-- access-matrix:start (generated from access::describe; do not edit by hand) -->
| Identifier | Mode | Level | Credential | Plan | Scope | Evidence |
|---|---|---|---|---|---|---|
| `open_ai_api` | default | Official | Admin key | — | Organization | [documentation](https://developers.openai.com/api/reference/resources/admin/subresources/organization) |
| `anthropic_api` | default | Official | Admin key | — | Organization | [documentation](https://platform.claude.com/docs/en/manage-claude/usage-cost-api) |
| `chat_gpt` | default | Undocumented | Subscription session | — | Account | reference [`rate_limit_resets.rs`](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs) |
| `claude` | default | Undocumented | Subscription session | — | Account | reference [`claude.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/claude.js) |
| `gemini_api` | default | Derived | Cloud identity | — | Product | [documentation](https://cloud.google.com/monitoring/api/metrics_gcp_p_z) |
| `vertex_ai` | default | Derived | Cloud identity | — | Product | [documentation](https://cloud.google.com/monitoring/api/metrics_gcp_p_z) |
| `aws_bedrock` | default | Official | Cloud identity | — | Product | [documentation](https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html) |
| `azure_open_ai` | default | Official | Cloud identity | — | Product | [documentation](https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01) |
| `cursor` | default | Official | Admin key | Teams or Enterprise | Organization | [documentation](https://cursor.com/docs/account/teams/admin-api) |
| `cursor` | `query_mode = "personal"` or native login | Undocumented | Subscription session | — | Account | reference [`CursorStatusProbe.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift) |
| `copilot` | default | Official | Admin key | — | Account | [documentation](https://docs.github.com/en/rest/billing/usage) |
| `copilot` | `query_mode = "quota"` or native login | Undocumented | Subscription session | — | Account | reference [`github.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/github.js) |
| `copilot` | `mode = "org"` | Official | Admin key | — | Organization | [documentation](https://docs.github.com/en/rest/billing/usage) |
| `windsurf` | default | Official | Admin key | Enterprise | Organization | [documentation](https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance) |
| `mini_max` | default | Undocumented | Inference key | — | Account | reference [`minimax.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/minimax.js) |
| `kimi` | default | Official | Inference key | — | Account | [documentation](https://platform.kimi.ai/docs/api/balance.md) |
| `kimi` | `product = "coding"` | Undocumented | Inference key | — | Account | reference [`kimi.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kimi.js) |
| `kimi` | native login | Undocumented | Subscription session | — | Account | reference [`kimi.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kimi.js) |
| `qwen` | default | Unavailable | Inference key | — | Account | [documentation](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope) |
| `qwen` | native login | Unavailable | Subscription session | — | Account | [documentation](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/) |
| `groq` | default | Derived | Inference key | — | Organization | [documentation](https://console.groq.com/docs/rate-limits) |
| `perplexity` | default | Unavailable | Inference key | — | Account | [documentation](https://docs.perplexity.ai/docs/admin/computer-analytics-api.md) |
| `perplexity` | `product = "computer"` | Official | Admin key | Enterprise | Organization | [documentation](https://docs.perplexity.ai/docs/admin/computer-analytics-api.md) |
| `mistral` | default | Official | Admin key | Enterprise | Organization | [documentation](https://docs.mistral.ai/admin/admin-api/usage-metrics.md) |
| `cloudflare_ai` | default | Unavailable | Inference key | — | Account | [documentation](https://developers.cloudflare.com/workers-ai/platform/pricing/) |
| `ollama_cloud` | default | Undocumented | Inference key | — | Account | reference [`misc.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/misc.js) |
| `xiaomi_mimo` | default | Undocumented | User session | — | Account | reference [`xiaomi-mimo.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/xiaomi-mimo.js) |
| `open_code_go` | default | Undocumented | Inference key | — | Account | reference [`opencode-go.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/opencode-go.js) |
| `qoder` | default | Undocumented | Subscription session | — | Account | reference [`misc.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/misc.js) |
| `gemini_ai` | default | Unavailable | Inference key | — | Account | [documentation](https://ai.google.dev/gemini-api/docs/rate-limits) |
| `gemini_ai` | `product = "cli"` or native login | Undocumented | Subscription session | — | Account | reference [`google.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/google.js) |
| `grok` | default | Official | Admin key | — | Organization | [documentation](https://docs.x.ai/developers/rest-api-reference/management/billing.md) |
| `grok` | `product = "cli"` or native login | Undocumented | Subscription session | — | Account | reference [`grok-cli.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/grok-cli.js) |
| `nvidia_nim` | default | Unavailable | Inference key | — | Account | [documentation](https://docs.api.nvidia.com/nim/reference/llm-apis) |
| `poolside` | default | Unavailable | Inference key | — | Account | [documentation](https://docs.poolside.ai/api/overview.md) |
| `kiro` | default | Undocumented | Subscription session | — | Account | reference [`kiro.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kiro.js) |
| `cline` | default | Official | Inference key | — | Account | [documentation](https://docs.cline.bot/enterprise-solutions/api-reference.md) |
| `cline` | `organization_id` set | Official | Inference key | — | Organization | [documentation](https://docs.cline.bot/enterprise-solutions/api-reference.md) |
| `cline` | native login | Official | Subscription session | — | Account | [documentation](https://docs.cline.bot/enterprise-solutions/api-reference.md) |
| `code_buddy` | default | Undocumented | Subscription session | — | Account | reference [`codebuddy-cn.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/codebuddy-cn.js) |
| `zed` | default | Undocumented | Subscription session | — | Account | reference [`zed.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/zed.js) |
| `alibaba_model_studio` | default | Unavailable | Inference key | — | Account | [documentation](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope) |
| `alibaba_coding_plan` | default | Undocumented | Inference key | — | Account | reference [`AlibabaCodingPlanUsageFetcher.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift) |
| `alibaba_coding_plan` | `session_env` set | Undocumented | User session | — | Account | reference [`AlibabaCodingPlanUsageFetcher.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift) |
| `alibaba_token_plan` | default | Undocumented | Local CLI | — | Account | reference [`token-plan.ts`](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts) |
| `alibaba_token_plan` | `session_env` set | Undocumented | User session | — | Account | reference [`token-plan.ts`](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts) |
| `alibaba_cloud` | default | Official | Cloud identity | — | Whole cloud | [documentation](https://www.alibabacloud.com/help/en/user-center/developer-reference/api-bssopenapi-2017-12-14-queryaccountbalance) |
| `antigravity` | default | Undocumented | Subscription session | — | Account | reference [`antigravity-weekly.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/antigravity-weekly.js) |
| `kimchi` | default | Undocumented | Inference key | — | Account | reference [`status.ts`](https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts) |

Billed per query (`metered`): `aws_bedrock`.
<!-- access-matrix:end -->

## Option applicability by authentication

The identifiers below are machine values. `credential` means the provider's manual/variable credential; `oauth` means native login. An option with no restriction proven by the adapter stays available for every type the provider offers. The core is the source of this table: the TUI and the desktop app receive the filtered list and the option's full index, with no provider-specific rules.

| Provider | Field | Option | Authentication types |
|---|---|---|---|
| Copilot | `query_mode` | `billing` | `credential`, `oauth` |
| Copilot | `query_mode` | `quota` | `credential`, `oauth` |
| Copilot | `mode` | `user` | `credential`, `oauth` |
| Copilot | `mode` | `org` | `credential`, `oauth` |
| Copilot | `billing` | `premium_requests` | `credential`, `oauth` |
| Copilot | `billing` | `ai_credits` | `credential`, `oauth` |
| Kimi | `product` | `default` | `credential`, `oauth` |
| Kimi | `product` | `coding` | `credential`, `oauth` |
| Grok | `product` | `management` | `credential`, `oauth` |
| Grok | `product` | `cli` | `credential`, `oauth` |
| Gemini AI | `product` | `default` | `credential`, `oauth` |
| Gemini AI | `product` | `cli` | `credential`, `oauth` |
| Qoder | `region` | `intl` | `credential`, `oauth` |
| Qoder | `region` | `cn` | `credential`, `oauth` |
| Qoder | `credential_type` | `auto` | `credential`, `oauth` |
| Qoder | `credential_type` | `pat` | `credential`, `oauth` |
| Qoder | `credential_type` | `bearer` | `credential`, `oauth` |
| Kiro | `auth_method` | `builder-id` | `oauth` |
| Kiro | `auth_method` | `idc` | `oauth` |
| Kiro | `auth_method` | `google` | `credential` |
| Kiro | `auth_method` | `github` | `credential` |
| Kiro | `auth_method` | `import` | `credential` |
| Kiro | `auth_method` | `api_key` | `credential` |
| Kiro | `auth_method` | `external_idp` | `credential` |
| CodeBuddy | `region` | `cn` | `credential`, `oauth` |
| CodeBuddy | `region` | `intl` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `sgp` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `cn` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `ams` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `ru` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `in` | `credential`, `oauth` |
| Alibaba Model Studio | `region` | `intl` | `credential` |
| Alibaba Model Studio | `region` | `cn` | `credential` |
| Alibaba Coding Plan | `region` | `intl` | `credential` |
| Alibaba Coding Plan | `region` | `cn` | `credential` |
| Alibaba Cloud | `region` | `intl` | `credential` |
| Alibaba Cloud | `region` | `cn` | `credential` |
| Alibaba Token Plan | `login.console_site` | `international` | `bailian_console` |

In Kiro, `auth_method` is a single shared descriptor. The native login receives exactly `builder-id` and `idc`; the environment credential receives `google` (the explicit default for new accounts), `github`, `import`, `api_key` and `external_idp`. When you switch type, an incompatible value is replaced by the first applicable value and the interface shows the localized warning returned by the core.

Old files are not rewritten on load: a missing `auth_method` and old combinations such as `credential` + `builder-id` keep being read and queried with the adapter's historical semantics. When you edit and save, however, the combination has to follow the table.

## Catalog of API keys, PATs and sessions

The names below are the defaults configured in the wizard. `credential_env` can point to another variable, without copying the key into the TOML. Being in the catalog **does not mean** the provider offers a quota through an inference API key.

| Provider / identifier | Default variable / credential | Implemented query / limitation |
|---|---|---|
| Minimax.io / `mini_max` | `MINIMAX_IO_API_KEY` | Token/Coding Plan: windows by count or percentage, including M-series. Fallback between the endpoints 9router uses; an `sk-api-` key keeps the balance query |
| Kimi / `kimi` | `KIMI_API_KEY` | International Moonshot balance by default. `params.product=coding` (or native login) queries the Kimi Coding weekly quota and rate limit |
| Qwen / `qwen` | `QWEN_API_KEY` | DashScope by key still has no compatible quota. Qwen Code OAuth is available with a deprecation warning; no quota endpoint has been confirmed for that session |
| Alibaba Model Studio / `alibaba_model_studio` | `DASHSCOPE_API_KEY` | Separate registration, region `intl` or `cn`. An inference API key does not grant a public balance/quota query |
| Alibaba Coding Plan / `alibaba_coding_plan` | `BAILIAN_CODING_PLAN_API_KEY` or a configured variable | Regional RPC `queryCodingPlanInstanceInfoV2`: usage/limit for 5h, week and month. If the gateway requires login, `params.session_env` references an explicit cookie; it does not extract a browser session or try another region |
| Alibaba Token Plan (Qwen Cloud) / `alibaba_token_plan` | `params.cli_profile = "cuotai-qwen"` (international `bl` login) **or** `params.session_env = "QWEN_CLOUD_COOKIE"` (Cookie) | 5h/week quota through `bl usage token-plan --output json` on the given profile; `session_env` uses direct HTTP to the Qwen Cloud console and takes precedence when present. The `QWEN_API_KEY` key only authenticates inference |
| Alibaba Cloud / `alibaba_cloud` | `ALIBABA_CLOUD_ACCESS_KEY_ID` | BSS balance through the AccessKey Secret referenced in `params.secret_env`; optional STS in `security_token_env`. `billing_cycle=YYYY-MM` queries the bill for **all products**, not only AI |
| Antigravity / `antigravity` | Google OAuth or `ANTIGRAVITY_ACCESS_TOKEN` | Project/plan through `loadCodeAssist`; per-model quotas through `fetchAvailableModels` on paid plans; session/week through `retrieveUserQuotaSummary`. A model without a quota fraction keeps unknown values and does not force the session to 0%. Optional `project_id` |
| Groq / `groq` | `GROQ_API_KEY` | GET `/openai/v1/models`, reading `x-ratelimit-*` as 9router does. No generation. If the response carries no headers, it reports connected with no quota reported, never an invented zero |
| Perplexity / `perplexity` | `PERPLEXITY_API_KEY` | Sonar: unavailable. **Computer Enterprise Analytics** is a separate opt-in (`params.product=computer`), with an Analytics key; sums the credits of complete UTC days |
| Mistral / `mistral` | `MISTRAL_API_KEY` | `/v1/admin/spend-limit` with an **Admin API Key**, `x-api-key` header. Missing values stay unknown; the monthly-limit state appears in the notes |
| Cloudflare AI / `cloudflare_ai` | `CLOUDFLARE_AI_API_KEY` | No Neurons/Workers AI quota query is confirmed in this integration. Generic Workers invocations and AI Gateway are not substitutes |
| Ollama Cloud / `ollama_cloud` | `OLLAMA_CLOUD_API_KEY` | GET `/api/usage`. Today it returns a usage report (`totals`: requests, tokens, USD spend over the `range`, 7d by default), shown as usage without a percentage because the API no longer exposes the session/week/month limits (only at ollama.com/settings/usage). If the endpoint returns `limits.session/weekly/monthly` again, they become windows in %, with the free renewal estimated from the sign-up anniversary. POST `/api/me` complements plan/sign-up; no cookie required |
| Xiaomi MiMo / `xiaomi_mimo` | Explicit session through a variable | `platform_session_env`: console balance and monthly Token Plan; `session_env`/`region`: Desktop weekly quota. The native code generates an inference key and does not replace those sessions |
| OpenCode Go / `open_code_go` | `OPENCODE_GO_API_KEY` | GET `https://opencode.ai/zen/go/v1/usage`: rolling, weekly and monthly percentage, with renewal |
| Qoder / `qoder` | `QODER_API_KEY` | `pt-` PAT exchanged for a job token and GET `/api/v2/quota/usage`; `params.region=intl\|cn`, `params.credential_type=auto\|pat\|bearer` |
| Gemini AI / `gemini_ai` | `GEMINI_API_KEY` | An AI Studio API key still has no quota in this interface. `params.product=cli` (or native login) uses the Gemini CLI session and `retrieveUserQuota`; it does not confuse products |
| Grok (xAI) / `grok` | `X_GROK_API_KEY` | Management API by default. `params.product=cli` (or native login) uses the Grok CLI session for on-demand credits, prepaid credits and the SuperGrok pool through REST/gRPC |
| NVIDIA NIM / `nvidia_nim` | `NVIDIA_NIM_API_KEY` | No public per-key balance query confirmed; it does not use inference to extract rate-limit headers |
| Poolside / `poolside` | `POOLSIDE_API_KEY` | No public account balance/quota query confirmed; infrastructure metrics are not a balance |
| Kimchi / `kimchi` | `KIMCHI_API_KEY` | GET `/v1/credits` and `/v1/budget`: USD balance, budget per scope/provider and renewal. A budget failure keeps the valid credits with a note |

For Mistral, Perplexity Computer and Grok, the variable in the Credential step must hold the **right class** of key. If you keep the admin key in another variable, set `params.admin_key_env`, `params.analytics_key_env` or `params.management_key_env`, respectively. The alternative reference takes precedence; only the key actually used is read.

## Other assistants

- **Cline (`cline`)**: queries the personal balance with `CLINE_API_KEY` (a Cline API key or account auth token) or native login; `/users/me` discovers the ID before the balance query. `params.organization_id` selects the organization balance. It does not aggregate incomplete history or the quotas of BYOK providers.
- **Kiro (`kiro`)**: native login recommended (**AWS Builder ID** or **IAM Identity Center**) or `KIRO_ACCESS_TOKEN`, internal `getUsageLimits` query on CodeWhisperer/Q. With the login, `params.auth_method=builder-id|idc`; with a variable, `google` (default), `github`, `import`, `api_key` or `external_idp`. `params.profile_arn` lets you indicate your profile. Kiro quota is distinct from Bedrock billing.
  - With the account's own `profile_arn` (discovered by the login or entered in the optional field), it queries credits with a POST `{profileArn}` on the known regional host (`us-east-1` or `eu-central-1`) before 9router's three legacy attempts. It shows plan, free trial, overage enabled and renewal; bonuses that cannot be separated are flagged.
  - **Multiple accounts:** add one Kiro entry per identity, with distinct names. Do the native login separately in each entry; each one gets its own `credential_id` and queries its own quota.
- **CodeBuddy (`code_buddy`)**: native login or `CODEBUDDY_ACCESS_TOKEN`, internal POST `/v2/billing/meter/get-user-resource`. `params.region=cn|intl`; renewal packages and bonuses appear separately.
- **Zed (`zed`)**: native login or `ZED_ACCESS_TOKEN` + required `params.user_id`; GET `cloud.zed.dev/client/users/me`, `Authorization: <user_id> <token>`. Optional `params.system_id`. It does not extract tokens from the editor automatically.
- **Personal Cursor**: building the cookie preserves the letter case of the JWT identifier. The import refuses tokens with less than 60 seconds of validity.
- **Poolside**: remains blocked (the official client reveals `GET /admin/account/{account_id}/usage` for rate-limit consumption authenticated by a platform admin token, not by the inference key).

## Compatibility with 9router

Code reference: [9router, revision `f01fb909e37189008080632ddaf404f096345cde`](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde). The Rust modules `providers/router_*.rs` reproduce queries and `oauth/reference*.rs` implement the login/renewal flows from `src/lib/oauth/providers/` and native helpers. The MIT license and attribution are in [`THIRD_PARTY_NOTICES`](../THIRD_PARTY_NOTICES).

These queries include **internal/unofficial interfaces**. They can change, expire or require a specific session. You do not need to run 9router alongside Cuotai. Cuotai does not import its credential databases, browser cookies or other apps' session files, does not generate AI responses to measure limits and does not call operations that spend credits to renew a quota.

In **environment variable** mode, select the product under **Parameters** according to the table:

| Path | Configuration |
|---|---|
| Kimi Coding | `product = "coding"`; `KIMI_API_KEY`. Alternative OAuth: `token_env` (variable name); the API key takes precedence when both exist |
| Personal Copilot | `query_mode = "quota"`; the account's `GITHUB_TOKEN`, not a copilotToken. Billing fields are hidden in this mode |
| Gemini CLI | `product = "cli"`; optional `session_env` (default `GEMINI_CLI_ACCESS_TOKEN`), optional `project_id` (resolved by `loadCodeAssist`). A narrowly scoped Cloud Monitoring token does not replace this session |
| Grok CLI | `product = "cli"`; optional `session_env` (default `GROK_CLI_ACCESS_TOKEN`), optional `email`/`user_id`. It does not use the Management Key in this mode |
| Xiaomi MiMo | `session_env = "XIAOMI_MIMO_COOKIE"`, an account cookie exported by the user; `region = "sgp"` (or cn/ams/ru/in) |
| Qoder | PAT/job token in `QODER_API_KEY`; `region = "intl"` (or cn), `credential_type = "auto"` (or pat/bearer). Exchanges PAT→job token without running inference |
| Kiro/CodeBuddy/Zed | Native login in the wizard/dialog is preferred; tokens and metadata are filled in automatically. The manual variable method remains available; for Kiro it defaults to `google` and does not offer `builder-id`/`idc` |

Deliberate differences from 9router: missing fields do not become zero or unlimited; Gemini CLI shows `remainingFraction` in **%**, not as 1000 real requests; Ollama flags a renewal computed from the sign-up date as an estimate; error responses do not expose the raw body; timezone-less dates of CodeBuddy packages are interpreted as UTC.

## Query semantics and costs

- `used`, `limit` and `remaining` are optional. A missing value does **not** mean zero. An API balance is not computed from spend, and currencies are not mixed.
- `status` describes the **query**, not the provider's overall health. `ready` means the query finished, not that quota is available. HTTP 401/403, 429, network, timeout and incompatible response are told apart.
- Monthly costs are cumulative; `resets_at` in those reports indicates the start of the next tracking period, not a guaranteed credit grant.
- **OpenAI and Anthropic:** tokens are limited to completions/messages; costs have a broader scope. Filtering `models` changes tokens, **not** the cost scope.
- **Google Cloud Monitoring:** limits are sampled daily; consumption is the sum of the last 10 minutes with a delay. Limit and usage series are separate, not comparable percentages. It can also bill for reads (two initial queries per refresh, plus pages).
- **AWS Bedrock:** costs are delayed and possibly estimated. **Cost Explorer charges per request** (check the current price; reference US$ 0.01). Refreshing every 300 s, running 24 h, can generate about US$ 2.88/day per account. Cuotai requires a minimum interval of 300 s for AWS Bedrock and warns about the cost when editing the account (TUI and desktop). Set `accounts.refresh_seconds = 3600` or more for AWS accounts.
- **Windsurf:** analytics in hourly buckets; allocation and consumption can have distinct scopes. The app does not invent a remaining balance from that comparison.

## Model catalog per account

The list is only queried on demand (`load_models`/`sync_models`), never by the quota refresh. **Every provider tries to list**, always read-only and with no inference cost, using the same credential the account already stores for the quota:

- **live query:** the provider's listing endpoint. When the credential is not accepted (for example, an admin key where the endpoint asks for an inference key), the account shows the classified failure (authentication, limit, transport, schema) and keeps the previous catalog; nothing is invented;
- **built-in list:** only where the provider publishes no endpoint that lists an account's models (CodeBuddy, Windsurf, Xiaomi MIMO, Qoder, Alibaba Token Plan). It is the list from 9router's registry (`providers/builtin_models.rs`, MIT, see `THIRD_PARTY_NOTICES`), and the source text says it is **not a query of the account**;
- **no list:** only Alibaba Cloud (billing AccessKey, no models endpoint), with the reason and no request.

Pagination follows tokens to the end (limit of 50 pages, a repeated token is rejected); an invalid/incomplete page, HTTP 401/403/429 or cancellation fail without overwriting the saved catalog. IDs are deduplicated and sorted without rewriting. Response bodies, URLs and tokens never appear in errors.

Endpoints marked **undocumented** come from open-source clients (the vendors' own CLIs and 9router) and may change without notice, like the quota queries of those same providers. The **Verification** column says what was proven against real accounts; the rest has only local HTTP fixture tests.

Details that do not fit in the table:

- **AWS Bedrock:** `aws bedrock list-foundation-models --profile <params.profile>`, with `--region <params.region>` when set; without `params.region`, the profile's default region in the AWS CLI applies. Requires `bedrock:ListFoundationModels`; it is not Cost Explorer and never runs on refresh.
- **Gemini AI:** with the AI Studio key (default `product`), paginated `models.list`; the ID is the returned `name` (`models/...`). With `product = "cli"` or native login, it tries `GET /v1beta/models` with the session token, which may be refused.
- **Claude (subscription):** `GET https://api.anthropic.com/v1/models?limit=1000` with the OAuth token and the `anthropic-beta: oauth-2025-04-20` and `anthropic-version` headers, paginated by `after_id` while `has_more`; `has_more` without `last_id` rejects the listing.
- **ChatGPT / Codex (subscription):** `GET https://chatgpt.com/backend-api/codex/models?client_version=<Codex CLI version>` with the session token (and `chatgpt-account-id` if `params.account_id`). Entries whose `visibility` is not `list` are left out, as in Codex's selector.
- **Antigravity:** same session and `fetchAvailableModels` request as the quota; a free plan or a response without `models` is `Unsupported`.
- **Alibaba Model Studio:** `region = "intl"` uses Singapore's paginated listing; the other regions try the region's OpenAI-compatible endpoint.
- **Kimi:** with a platform key, `api.moonshot.ai/v1/models`; with `product = "coding"` or native login, `api.kimi.com/coding/v1/models` (key in `x-api-key`, or OAuth token as Bearer with the quota's device headers).
- **GitHub Copilot:** exchanges the GitHub token for a Copilot token (`/copilot_internal/v2/token`) and reads `api.githubcopilot.com/models`, keeping only chat models the plan does not disable.
- **Kiro:** `ListAvailableModels` on the host of the `profile_arn`'s region (default `us-east-1`), with the same per-authentication-method headers as the quota.
- **Cursor:** RPC `aiserver.v1.AiService/GetUsableModels` in Connect JSON, with the session token; accounts with only the admin key (Basic) get the provider's refusal.
- **Zed:** `users/me` (organization) → `client/llm_tokens` → `cloud.zed.dev/models`; disabled models are left out. Plans without hosted models return an empty list, which is the real result.
- **Azure OpenAI:** `…/accounts/{resource}/models` of the configured resource, with the ARM token; page links outside the ARM host are rejected.
- **Grok:** a CLI session (`product = "cli"`) lists through `cli-chat-proxy.grok.com/models` with the client headers; with an API key, `api.x.ai/v1/models`.

Verification: `live` = confirmed against a real account; `fixture` = local tests only.

| Identifier | List source | Endpoint | Verification |
|---|---|---|---|
| `open_ai_api` | live query, documented | GET `api.openai.com/v1/models`; an Admin key may be refused | fixture |
| `anthropic_api` | live query, documented | GET `api.anthropic.com/v1/models` (`x-api-key`); an Admin key may be refused | fixture |
| `chat_gpt` | live query, undocumented | GET `chatgpt.com/backend-api/codex/models` | live |
| `claude` | live query, undocumented for the token | GET `api.anthropic.com/v1/models` with OAuth | live |
| `gemini_api` | live query, documented | GET `generativelanguage.googleapis.com/v1beta/models`; a Cloud Monitoring token may be refused | fixture |
| `vertex_ai` | live query, documented | GET `aiplatform.googleapis.com/v1beta1/publishers/google/models`; a Cloud Monitoring token may be refused | fixture |
| `aws_bedrock` | live query, documented | `ListFoundationModels` through the AWS CLI | fixture |
| `azure_open_ai` | live query, documented | ARM `…/accounts/{resource}/models` | fixture |
| `cursor` | live query, undocumented | `GetUsableModels` (Connect JSON) | live |
| `copilot` | live query, undocumented | `api.githubcopilot.com/models` | live |
| `windsurf` | built-in list | no endpoint | — |
| `mini_max` | live query, documented | GET `api.minimax.io/v1/models` | live |
| `kimi` | live query | `api.moonshot.ai/v1/models` (key) or `api.kimi.com/coding/v1/models` | live (coding) |
| `qwen` | live query, undocumented | `dashscope-intl…/compatible-mode/v1/models` (key) or `portal.qwen.ai/v1/models` (OAuth) | fixture |
| `groq` | live query, documented | GET `/openai/v1/models` | live |
| `perplexity` | live query, documented | GET `api.perplexity.ai/v1/models` | fixture |
| `mistral` | live query, documented | GET `api.mistral.ai/v1/models`; an Admin key may be refused | fixture |
| `cloudflare_ai` | live query, documented | GET `/client/v4/accounts/{account_id}/ai/models/search`; requires `params.account_id` | fixture |
| `ollama_cloud` | live query, documented | GET `ollama.com/v1/models` | live |
| `xiaomi_mimo` | built-in list | no endpoint | — |
| `open_code_go` | live query, undocumented | GET `opencode.ai/zen/go/v1/models` | live |
| `qoder` | built-in list | the real catalog needs Cosy signing (RSA+AES+MD5), not ported yet | — |
| `gemini_ai` | live query | `models.list` (AI Studio key) or `/v1beta/models` with the session | fixture |
| `grok` | live query | `cli-chat-proxy.grok.com/models` (CLI session) or `api.x.ai/v1/models` (key) | fixture |
| `nvidia_nim` | live query, documented | GET `integrate.api.nvidia.com/v1/models` | fixture |
| `poolside` | live query, documented | GET `inference.poolside.ai/v1/models` | fixture |
| `kiro` | live query, undocumented | `ListAvailableModels` | live |
| `cline` | live query, documented | GET `api.cline.bot/api/v1/models` | fixture |
| `code_buddy` | built-in list | no endpoint (`/v3/config` does not carry the account's models) | — |
| `zed` | live query, undocumented | `cloud.zed.dev/models` with an LLM token | live (empty list on the account's plan) |
| `alibaba_model_studio` | live query | paginated `/api/v1/models` (intl) or `/compatible-mode/v1/models` | fixture |
| `alibaba_coding_plan` | live query, documented | GET `coding(-intl).dashscope.aliyuncs.com/v1/models` | fixture |
| `alibaba_token_plan` | built-in list | no endpoint | — |
| `alibaba_cloud` | no list | billing AccessKey, no models endpoint | — |
| `antigravity` | live query, undocumented | the session's `fetchAvailableModels` (paid plans) | live |
| `kimchi` | live query, undocumented | GET `llm.kimchi.dev/v1/models/metadata?include_in_cli=true` | fixture |

## Public incident source registry

Since 0.46.0, library consumers can call `cuotai_core::incidents::source(provider)` without an account, credentials, an async runtime or network access (PRD 0011 RF1). It returns a `StatusSource` with the wire format, public endpoint, component selectors and last verification date. `includes_component(id)` performs exact matching, never a page-wide fallback. A source is **not a health reading**; `None` means **unverified**, never “operational” or “no incidents”.

**Owner decision: incident queries are opt-in, off until enabled; `--no-status` remains an override.** Even credential-free public requests disclose an IP address and interest in a provider to these domains.

### Status client (since 0.47.0)

`Monitor::incidents()` returns the `IncidentMonitor` that holds the preference (off by default) and the cache; `set_enabled(true)` turns queries on (PRD 0011 RF2). Rebuilding the monitor with `with_config` keeps both; `with_incidents` carries them into a monitor built from scratch. While the feature is off, no status request is made and `refresh_incidents`/`incident_statuses` return nothing. `Monitor::status_domains()` lists, without network and also while off, the hosts the configured providers' sources would reach (one per host), and `StatusSource::host()` gives one source's host. Since 0.48.0 the desktop turns it on only from **Settings → Privacy** (RF4, `status_queries` in `desktop.json`) and opens a status or incident page only when `incidents::is_status_link(url)` accepts it (plain `https` on a verified source host, normalized, without port, credentials, query or fragment); badge texts come from `IncidentState::label()` (`incident.state.*`). Since 0.49.0 the TUI, `--once` and `--json` turn it on only with `--status` or `CUOTAI_STATUS=1`, and `--no-status` overrides both (RF5); `export::v1` publishes each account's provider status as `incident`.

- **Requests:** only the sources of providers with a configured account, once per URL (shared OpenAI, Claude and Google Cloud endpoints are fetched once for every account and product) and at most once per five minutes, also after a failure. They use the monitor's HTTP client and query semaphore, send no credential, cookie, query parameter or account data, follow no redirect, identify themselves as `cuotai/<version> (+https://github.com/jmarcon/cuotai)`, stop after 5 seconds and refuse bodies above 1 MiB. `Monitor::refresh` runs them alongside the quota queries; nothing about a status failure reaches a report.
- **States:** `operational`, `degraded`, `partial_outage`, `major_outage`, `maintenance` and `unknown`, computed only from the provider's scoped components and events. Statuspage v2 needs a scoped component in the summary, otherwise the state is unknown; the incident.io widget and Google Cloud are operational when no active event touches the scope; the AWS and xAI feeds stay **unknown** without an active scoped event, because their history cannot prove health. An event that lists the scoped components as operational is unknown, not operational, and a page-wide indicator is never used.
- **Active incident:** the most severe scoped event, with its title (HTML, entities, Markdown and control characters removed, at most 120 characters), start and a public `https` link on the source host (query and fragment dropped), otherwise the status page itself. Statuspage incidents use `/incidents/<id>` on the source host; AWS takes the newest update of each service/region event and its first update as the start; resolved xAI entries are ignored.
- **Failures:** an invalid, oversized, redirected, failed or timed-out response keeps the last valid reading with `stale = true` (or `unknown` when there is none); it never changes the quota query or the account `Status`.
- **Correlation (RF3):** when a query ends in `error`, `rate_limited` or `authentication_error` and the provider's state is `degraded` or worse, the report gains the note `note.incident.ongoing` ("The provider reports an ongoing incident; the failure may not be from your account."). An authentication failure also gains `note.incident.auth_wait`, a suggestion to wait before signing in again that blocks nothing. The `Status` stays as queried and no note names the incident as the cause. A failed query waits for the cycle's single status request (5 seconds at most) instead of making its own.
- **Desktop (RF4):** a card badge for `degraded`, `partial_outage`, `major_outage` or `maintenance` (marked as the last reading when stale) or for a failed latest read of an operational or unknown page; `operational` and a plain `unknown` show no badge and are spelled out in the details. The tray icon gets a red dot while a configured provider is in `partial_outage` or `major_outage`.
- **TUI and CLI (RF5):** the same badge rule on the card's top border and in a `--once` `Status do provedor:` line; the details list the full status (operational and unknown included) with the incident, start, reading time and page. `--json` keeps the `Snapshot` shape.

The following endpoints and public-use documentation were fetched on **2026-10-20**, without authentication. All included endpoints returned HTTP 200 with the stated JSON/RSS shape. Component IDs, not display names or overall page indicators, select relevant services. Shared OpenAI, Claude and Google Cloud endpoints remain identical across their provider entries so a future client can coalesce by URL. These are product-level observations, not account-, plan- or region-specific guarantees.

| Identifier | Format | Public source | Verified scope (component IDs in the core) | Verified on |
|---|---|---|---|---|
| `open_ai_api` | `JsonCustom(IncidentIoWidget)` | [endpoint](https://status.openai.com/api/v1/summary) | Chat Completions; Responses; Fine-tuning; Embeddings; Images; Batch; Audio; Moderations; Realtime; Files; Login (API) | 2026-10-20 |
| `anthropic_api` | `StatuspageV2` | [endpoint](https://status.claude.com/api/v2/summary.json) | Claude API (api.anthropic.com); Claude Console (platform.claude.com) | 2026-10-20 |
| `chat_gpt` | `JsonCustom(IncidentIoWidget)` | [endpoint](https://status.openai.com/api/v1/summary) | Codex in ChatGPT Desktop; Login (ChatGPT); Codex Web; Codex API; CLI; VS Code extension | 2026-10-20 |
| `claude` | `StatuspageV2` | [endpoint](https://status.claude.com/api/v2/summary.json) | claude.ai; Claude Code; Claude Cowork | 2026-10-20 |
| `gemini_api` | `JsonCustom(GoogleCloud)` | [endpoint](https://status.cloud.google.com/incidents.json) | Cloud Monitoring (measurement only) | 2026-10-20 |
| `vertex_ai` | `JsonCustom(GoogleCloud)` | [endpoint](https://status.cloud.google.com/incidents.json) | Vertex Gemini API; Vertex AI Online Prediction; Cloud Monitoring | 2026-10-20 |
| `aws_bedrock` | `AtomRss(AwsHealth)` | [endpoint](https://status.aws.amazon.com/rss/all.rss) | Amazon Bedrock (all regions in the feed) | 2026-10-20 |
| `azure_open_ai` | None | — | Not verified (see exclusions below) | — |
| `cursor` | `StatuspageV2` | [endpoint](https://status.cursor.com/api/v2/summary.json) | cursor.com; IDE; CLI; Cloud Agents | 2026-10-20 |
| `copilot` | `StatuspageV2` | [endpoint](https://www.githubstatus.com/api/v2/summary.json) | Copilot; Copilot AI Model Providers; API Requests | 2026-10-20 |
| `windsurf` | `StatuspageV2` | [endpoint](https://status.windsurf.com/api/v2/summary.json) | Cascade; Windsurf Tab | 2026-10-20 |
| `mini_max` | `StatuspageV2` | [endpoint](https://status.minimax.io/api/v2/summary.json) | Large Language Models (LLM) | 2026-10-20 |
| `kimi` | `StatuspageV2` | [endpoint](https://status.moonshot.cn/api/v2/summary.json) | Open API; Kimi Code; K3 (Kimi Code); K2.8 Preview (Kimi Code); K2.7 Code Highspeed (Kimi Code) | 2026-10-20 |
| `qwen` | None | — | Not verified (see exclusions below) | — |
| `groq` | `JsonCustom(IncidentIoWidget)` | [endpoint](https://groqstatus.com/api/v1/summary) | API | 2026-10-20 |
| `perplexity` | `JsonCustom(IncidentIoWidget)` | [endpoint](https://status.perplexity.com/api/v1/summary) | API; Computer | 2026-10-20 |
| `mistral` | None | — | Not verified (see exclusions below) | — |
| `cloudflare_ai` | `StatuspageV2` | [endpoint](https://www.cloudflarestatus.com/api/v2/summary.json) | Workers AI | 2026-10-20 |
| `ollama_cloud` | None | — | Not verified (see exclusions below) | — |
| `xiaomi_mimo` | None | — | Not verified (see exclusions below) | — |
| `open_code_go` | None | — | Not verified (see exclusions below) | — |
| `qoder` | None | — | Not verified (see exclusions below) | — |
| `gemini_ai` | None | — | Not verified (see exclusions below) | — |
| `grok` | `AtomRss(Xai)` | [endpoint](https://status.x.ai/feed.xml) | Global API; US API; grok.com; Grok Build | 2026-10-20 |
| `nvidia_nim` | `StatuspageV2` | [endpoint](https://status.ngc.nvidia.com/api/v2/summary.json) | NVIDIA Build; Cloud Functions | 2026-10-20 |
| `poolside` | None | — | Not verified (see exclusions below) | — |
| `kiro` | None | — | Not verified (see exclusions below) | — |
| `cline` | None | — | Not verified (see exclusions below) | — |
| `code_buddy` | None | — | Not verified (see exclusions below) | — |
| `zed` | None | — | Not verified (see exclusions below) | — |
| `alibaba_model_studio` | None | — | Not verified (see exclusions below) | — |
| `alibaba_coding_plan` | None | — | Not verified (see exclusions below) | — |
| `alibaba_token_plan` | None | — | Not verified (see exclusions below) | — |
| `alibaba_cloud` | None | — | Not verified (see exclusions below) | — |
| `antigravity` | None | — | Not verified (see exclusions below) | — |
| `kimchi` | None | — | Not verified (see exclusions below) | — |

### Verification, stability and use policy

- **Statuspage v2:** each publisher explicitly documents unauthenticated, programmatic use and the summary's components, incidents and maintenance: [Claude](https://status.claude.com/api), [GitHub](https://www.githubstatus.com/api), [Cursor](https://status.cursor.com/api), [Windsurf](https://status.windsurf.com/api), [MiniMax](https://status.minimax.io/api), [Moonshot](https://status.moonshot.cn/api) and [NVIDIA NGC](https://status.ngc.nvidia.com/api). The canonical Claude and Windsurf hosts replace redirects from `status.anthropic.com` and `status.codeium.com`. Their standard `robots.txt` disallows crawling `/api/` and `/embed/`; this registry uses only the publisher's explicitly documented API, not HTML crawling or indexing. No broader scraping permission is inferred.
- **Cloudflare:** [API use policy](https://www.cloudflarestatus.com/api) explicitly requests automated API polling instead of HTML scraping, no authentication, and an identifiable User-Agent with a contact URL. Its v2-compatible summary was about 236 KiB at verification; RF2's response-size ceiling must accommodate it. A major page-wide outage with Workers AI operational was observed and retained as a scope-boundary fixture. City outages, Workers KV and unrelated products must not become Workers AI incidents.
- **OpenAI, Groq and Perplexity:** the enabled [incident.io Widget API](https://docs.incident.io/status-pages/api#widget-api) is a public read-only JSON API intended for embedding status, with caching strongly recommended and high traffic subject to discussion with the operator. `ongoing_incidents`, `in_progress_maintenances` and `scheduled_maintenances` contain `affected_components[].id`. Their compatibility `/api/v2/summary.json` endpoints returned null incident/maintenance lists, so those are **not** used for incident monitoring. The component catalogs were checked separately ([OpenAI](https://status.openai.com/api/v2/components.json), [Groq](https://groqstatus.com/api/v2/components.json), [Perplexity](https://status.perplexity.com/api/v2/components.json)); OpenAI's summary omitted later Codex components, so the complete components endpoint is the verification evidence. Duplicate “Login” labels are separated by IDs. No robots policy was published at these hosts' `/robots.txt` (404).
- **Google Cloud:** [programmatic consumption guidance](https://docs.cloud.google.com/service-health/docs/service-health-fallback#consume_google_cloud_service_health_data_programmatically), [JSON schema](https://status.cloud.google.com/incidents.schema.json) and [product catalog](https://status.cloud.google.com/products.json) explicitly support the public JSON history file. Use stable `affected_products[].id`, never unstable titles. `robots.txt` permits access. Only broad public incidents are included. `gemini_api` covers **Cloud Monitoring, its measurement dependency only**; the standalone Gemini API is not Vertex Gemini API. `vertex_ai` includes the verified inference products plus the same Monitoring dependency, not every Google product.
- **AWS:** the [AWS Health Dashboard guide](https://docs.aws.amazon.com/health/latest/ug/aws-health-dashboard-status.html) confirms unauthenticated public health events and available RSS feeds, but warns the format can change and recommends authenticated EventBridge for complete data. The public RSS 2.0 feed declares `ttl=5` (minutes); respect at least that interval. The [Bedrock N. Virginia feed](https://status.aws.amazon.com/rss/bedrock-us-east-1.rss) verifies the `bedrock` service slug; the global feed avoids claiming one region represents all regions. Extract the exact service from the GUID fragment before its region suffix; do not match `multipleservices` to Bedrock without structured evidence. This can miss broad multi-service incidents and is not a Cost Explorer health guarantee. Empty/history-only feeds cannot alone prove current operational status. The undocumented UTF-16 `health.aws.amazon.com/public/currentevents` API is **not** used. No robots policy was returned by the RSS host.
- **xAI:** the [public status page](https://status.x.ai/) publishes [RSS 2.0](https://status.x.ai/feed.xml); the feed self-link identifies the same URL. Its component is the bracketed title prefix, and categories carry severity and resolution (the captured entries are resolved). Exact component selection excludes unrelated X/mobile/voice incidents. `robots.txt` disallows `/api/`, `/mp/` and `/design`, not this subscription feed. The v2 JSON candidate returned 403 and is not used. No stable JSON API or stronger feed-version promise was verified; history alone must not be interpreted as current operational health.

These checks establish use of expressly public subscription/API interfaces, not an unrestricted content license, an SLA or a guarantee that terms will never change. A future client must identify itself, cache for at least five minutes, honor rate limits, and treat malformed, incomplete, stale or ambiguous responses as unknown. It must neither scrape HTML nor evade a login or bot challenge. Never forward account data, cookies or quota credentials to a status source.

### Explicit exclusions (checked 2026-10-20)

`None` is a deliberate conservative decision, not a claim the vendor has no status page:

| Providers | Verification result / why no reliable scoped source is registered |
|---|---|
| `azure_open_ai` | The [public Azure RSS](https://azure.status.microsoft/en-us/status/feed/) responded, but no machine-readable Azure OpenAI component selector was verified in it; whole-cloud incidents are not substituted. |
| `mistral` | `status.mistral.ai` and its v2 summary returned 403 with a bot challenge, including a browser User-Agent; no bypass or unverifiable URL is registered. |
| `poolside`, `kimchi` | Public incident.io pages and compatibility summaries responded, but their Widget APIs returned 404. Compatibility summaries have null incident lists. Atom/RSS affected components/state are embedded in HTML prose, not a verified structured component contract, so these are not registered. |
| `zed` | [Instatus public API](https://status.zed.dev/public-api) exists, but the summary has no component association and the documented v3 components array differed from the observed wrapped response. A Zed page-wide state could alarm for unrelated Delta/Collaboration/Extension Registry components; no scoped incident contract is registered. |
| `gemini_ai`, `antigravity` | No verified feed for the AI Studio/CLI/Antigravity product scope. The Cloud catalog's Gemini Code Assist is not assumed to cover every Gemini AI mode or Antigravity. |
| `open_code_go`, `cline` | Public status pages responded with HTML; tried public summary/feed candidates returned 404. No documented machine-readable scoped contract verified. |
| `qwen`, `alibaba_model_studio`, `alibaba_coding_plan`, `alibaba_token_plan`, `alibaba_cloud` | Alibaba status sites expose HTML applications; no public, documented incident feed with relevant product selectors was verified. |
| `ollama_cloud`, `xiaomi_mimo`, `qoder`, `code_buddy` | No reliable public status endpoint verified; candidate status hosts failed DNS/network resolution. |
| `kiro` | Candidate public status host returned 403; Bedrock's AWS service must not stand in for Kiro. |

Moonshot's source is its published Open API/Kimi Code product status, not a claim of parity between domestic and international deployments. MiniMax excludes speech/video; GitHub excludes Actions/Pages; Windsurf excludes Netlify; NGC excludes Fleet Command. Missing components or a source with no relevant scoped incident never justify asserting account health.

### Offline verification fixtures

`crates/cuotai-core/src/incidents/fixtures/` holds minimized, anonymized real captures from the URLs above on the verification date: the Statuspage summaries, complete OpenAI and relevant Groq/Perplexity component catalogs, an empty OpenAI widget response, Google incident/product JSON and AWS/xAI RSS. `widget-maintenance.json` preserves the public [HashiCorp Widget API](https://status.hashicorp.com/api/v1/summary)'s real maintenance shape with populated components (the mapped pages had none at capture); its names, IDs, URLs and prose were anonymized. Public component IDs and names in provider catalogs remain intact because they are matching evidence, not personal data. Incident prose is replaced and long histories are reduced; states, timestamps, structural nulls and selector fields are retained. The AWS example is an unrelated multi-service event and the xAI examples are resolved events, not simulated active outages.

RF1 tests use only these local captures, covering all 36 provider decisions, positive component membership, exact-match boundaries, shared-source isolation, unrelated cloud services and the page-wide-outage/healthy-target case. RF2/RF3 tests serve the same captures and synthetic active incidents of each format from local `wiremock` servers; no live status assertion is a test expectation.

## References

- [OpenAI: organization costs and usage](https://developers.openai.com/api/reference/resources/admin/subresources/organization)
- [Anthropic: Usage and Cost API](https://platform.claude.com/docs/en/manage-claude/usage-cost-api)
- [Codex: rate-limit query client](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs)
- [Claude: community evidence of the contract](https://github.com/loocor/codmate/blob/main/services/ClaudeUsageAPIClient.swift)
- [Google: Service Runtime quota metrics](https://cloud.google.com/monitoring/api/metrics_gcp_p_z)
- [AWS: get-cost-and-usage](https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html)
- [Azure: Cost Management Query](https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01) and [cost attribution by resource](https://learn.microsoft.com/en-us/azure/foundry/concepts/manage-costs)
- [Cursor: Admin API](https://cursor.com/docs/account/teams/admin-api), [personal contract in CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift) and [imported session identity](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift)
- [GitHub: billing usage](https://docs.github.com/en/rest/billing/usage) and [authentication for usage reports](https://docs.github.com/en/billing/tutorials/automate-usage-reporting)
- [Windsurf: team credits](https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance) and [analytics and units](https://docs.windsurf.com/windsurf/accounts/api-reference/user-page-analytics)
- [Google OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Microsoft OAuth device code](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code)
- [AWS CLI SSO login](https://docs.aws.amazon.com/cli/latest/reference/sso/login.html)
- [Anthropic: authentication restrictions](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use)
- [MiniMax Token Plan](https://platform.minimax.io/docs/token-plan/faq.md) and [quota interpretation in the official CLI](https://github.com/MiniMax-AI/cli/blob/main/src/utils/quota.ts)
- [Kimi: international account balance](https://platform.kimi.ai/docs/api/balance.md)
- [Mistral: Admin API authentication](https://docs.mistral.ai/admin/admin-api/authentication.md) and [usage and limits](https://docs.mistral.ai/admin/admin-api/usage-metrics.md)
- [Perplexity Computer Analytics](https://docs.perplexity.ai/docs/admin/computer-analytics-api.md)
- [xAI Management Billing](https://docs.x.ai/developers/rest-api-reference/management/billing.md)
- [Cline: account and balance API](https://docs.cline.bot/enterprise-solutions/api-reference.md)
- [Groq: spend limits](https://console.groq.com/docs/spend-limits)
- [Cloudflare Workers AI: pricing and tracking](https://developers.cloudflare.com/workers-ai/platform/pricing/)
- [Ollama: authentication](https://docs.ollama.com/api/authentication)
- [Gemini: limits](https://ai.google.dev/gemini-api/docs/rate-limits)
- [Qwen via DashScope](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope) and [Qwen Code device OAuth](https://github.com/QwenLM/qwen-code/blob/81582ca19d0772afdd06ace9cad4845c207001f8/packages/core/src/qwen/qwenOAuth2.ts)
- [Alibaba Token Plan official CLI](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts), [Token Plan in CodexBar](https://github.com/steipete/CodexBar/pull/2361) and [Coding Plan RPC in CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift)
- [Xiaomi MiMo: documentation](https://mimo.mi.com/llms-full.txt) and [balance/Token Plan in CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/MiMo/MiMoUsageFetcher.swift)
- [OpenCode Go](https://opencode.ai/go/) and [Qoder: usage](https://docs.qoder.com/cli/usage.md)
- [NVIDIA NIM](https://docs.api.nvidia.com/nim/reference/llm-apis) and [Poolside API](https://docs.poolside.ai/api/overview.md)
- [Kiro: billing](https://kiro.dev/docs/billing.md), [getUsageLimits contract in 9router](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kiro.js) and [credits in CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Kiro/KiroUsageLimitsAPI.swift)
- [CodeBuddy: usage](https://www.codebuddy.cn/docs/ide/Account/usage.md) and [Zed: plans](https://zed.dev/docs/account/plans-and-pricing.md)
- [Kimchi: official credits and budgets contract](https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts)
- [9router: query dispatch](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage.js) and [authentication flows](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde/src/lib/oauth/providers)
