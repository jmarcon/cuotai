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
