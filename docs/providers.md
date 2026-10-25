# Provider Catalog, Credentials and Limitations

The Cuotai catalog contains **40 providers**, combining billing admin APIs, Cloud Monitoring, coding-assistant subscriptions and integrations compatible with `9router`.

## Overview by category

For parameters, default variables and limitations of each provider, use the tables below; this overview groups the identifiers (the `provider` value in `cuotai.toml`) by kind of service.

| Category | Providers |
|---|---|
| **Assistants and subscriptions** | **Claude** (`claude`), **OpenAI Codex / ChatGPT** (`chat_gpt`), **GitHub Copilot** (`copilot`), **Cursor** (`cursor` — personal and Teams), **Windsurf** (`windsurf`), **Kiro / AWS** (`kiro`), **Antigravity** (`antigravity`), **Cline** (`cline`), **Zed** (`zed`), **CodeBuddy** (`code_buddy`), **Qoder** (`qoder`), **OpenCode Go** (`open_code_go`) |
| **CLIs and coding plans** | **Gemini CLI** (`gemini_ai`), **Grok CLI / xAI** (`grok`), **Kimi Coding & API** (`kimi`), **MiniMax Token/Coding Plan** (`mini_max`), **Alibaba Coding Plan** (`alibaba_coding_plan`), **Alibaba Token Plan / Qwen Cloud** (`alibaba_token_plan`), **Xiaomi MiMo** (`xiaomi_mimo`), **Ollama Cloud** (`ollama_cloud`), **Qwen Code** (`qwen`) |
| **Cloud billing and admin APIs** | **OpenAI Admin API** (`open_ai_api`), **Anthropic Admin API** (`anthropic_api`), **Google Gemini API Monitoring** (`gemini_api`), **Google Vertex AI** (`vertex_ai`), **AWS Bedrock Cost Explorer** (`aws_bedrock`), **Azure OpenAI Cost Management** (`azure_open_ai`), **Alibaba Cloud BSS** (`alibaba_cloud`), **Mistral Admin** (`mistral`), **Perplexity Computer Analytics** (`perplexity`), **Kimchi** (`kimchi`), **Groq** (`groq`), **ElevenLabs** (`eleven_labs`) |
| **Aggregators** | **9router** (`nine_router`), **LiteLLM proxy** (`litellm`) |
| **Additional catalog** | **Alibaba Model Studio** (`alibaba_model_studio`), **Cloudflare AI** (`cloudflare_ai`), **NVIDIA NIM** (`nvidia_nim`), **Poolside** (`poolside`) |

## Main integrations and credentials

| Identifier | Data queried | Configuration / restrictions |
|---|---|---|
| `open_ai_api` | Cost per currency; completions tokens and requests, UTC month | `OPENAI_ADMIN_KEY`, **Admin API key**. Optional `params.project_ids` and `params.models`, comma-separated. States its data time (`as_of`, PRD 0033 RF3): the end of the last complete daily cost bucket. Every other adapter leaves `as_of` unset until its provider documents a data time |
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

### Web sessions in the vault (PRD 0016)

Xiaomi MiMo (`session_env`, `platform_session_env`), Alibaba Coding Plan and Alibaba Token Plan (`session_env`) read a web session. The core declares, per session slot, the cookie names it accepts (and which of them are required), the only hosts that may receive it (HTTPS) and that it travels as the `Cookie` header (`cuotai_core::user_session::descriptors`). The recommended route is to paste the session into the system vault from the account wizard; an environment variable named by the slot's parameter stays as an alternative:

- **Providers (PRD 0016 RF5):** the three adapters read every session through the descriptor, from the vault or from the variable, with the same queries and precedence as before (a session beats the API key of the Coding Plan and the `bl` profile of the Token Plan; MiMo's account session beats its platform session). A session without the slot's required cookies is refused before any request with a message that names only the missing cookies (and the variable, when it came from one): `serviceToken` for the MiMo account session, `api-platform_serviceToken` and `userId` for the MiMo platform session; an unset variable is named with its parameter. A variable's value is sent as it is; only a paste is reduced to the accepted names. The report says whether the Alibaba quota was read with the vault session or the variable. Evidence for the names: 9router's `mimoAccount.js` keeps exactly `serviceToken`, `userId` and the `mimo<region>_ph`/`_slh` cookies, CodexBar's `MiMoCookieImporter.swift` requires the two platform cookies, and CodexBar's Alibaba importer treats `login_aliyunid_ticket` plus one of `login_aliyunid_pk`/`login_current_pk`/`login_aliyunid` as a signed-in console session but sends the whole cookie jar of the console domains. The Alibaba names therefore stay optional until a live console session confirms that the reduced set is enough.
- **New providers:** a provider only gets a session slot with evidence (the vendor's documentation, or a contract verified in 9router or a community client) that its query is read-only and that the session alone is enough, and through its access descriptor (PRD 0005): an interface classified `User session` must have a descriptor and the reverse, which the `access` tests enforce.
- **Stored:** the session goes to a Cuotai vault entry (Keychain, Credential Manager or Secret Service; the debug-only `--dev-credentials-sqlite` store keeps its plaintext warning and release builds refuse it). The TOML keeps only `[accounts.session]` with a random `credential_ref` and the stored `slots`; the entry is bound to that reference, the provider and the slot, so a reference copied to another account, provider or slot finds nothing and two accounts cannot share one.
- **Migration:** an account that still names a variable is offered the move on its summary (TUI: F3, then F3 again to confirm) or in the account dialog (desktop: **Move to the vault**, then **Confirm the move**). Cuotai copies the variable's current value into the vault, removes the parameter from the TOML and leaves the variable in your environment for you to remove; nothing moves when a variable is unset or empty, and a refused save undoes the vault write (a new entry is removed, a replaced session comes back). Accounts that keep the variable work unchanged.
- **Paste:** the TUI wizard's **Colar sessão** step (after the parameters) and the desktop dialog's **Web session** box take a paste of the `Cookie:` request header, a `name=value; …` list, the cookie table of the browser's inspector or a request copied as cURL, in a masked field. The core keeps only the cookie names the slot accepts (parts named `name.0`, `name.1`, … are joined in order) and discards everything else before writing; errors name the cookie (missing, repeated, incomplete, expired), never a value. F4 / **Test session** runs one read-only query with the paste before anything is saved; saving stores it in the vault, replacing the account's earlier session in the same entry. The field is cleared when the wizard or dialog closes.
- **Use:** a session is sent only over HTTPS to the hosts declared for its slot and never follows a redirect (a redirect counts as a rejected session). A rejected vault or pasted session reads "session expired or revoked; paste a new one". When the pasted cookie table showed an expiry, only that date is kept: cards show "expires in N days" (highlighted under 3 days), `--json` reports carry `session_expires_at`, and with usage alerts on a session ending within 3 days gets its own notification.
- **Warning:** a pasted session is a full login, not a read-only key. While an account's query uses one, its card (and the TUI details) names the `UserSession` level ("Sessão do usuário" / "User session" / "Sesión del usuario") with the core's warning, "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service." (`cuotai_core::user_session::notice`; an account still reading a variable is told so and pointed to the vault instead); the TUI paste step and the desktop **Web session** box show the same level and warning before you paste (`user_session::paste_notice`).
- **Forget:** an account that keeps a session in the vault offers **Forget this session** on its summary (TUI: F5, then F5 again to confirm) or in the account dialog (desktop: **Forget this session**, then **Confirm: forget the session**). Cuotai deletes the account's vault entries and then removes `[accounts.session]` from the TOML (`user_session::stage_forget`); a refused save puts the session back so the account keeps working, and a reference whose entry was already gone just loses the reference. Since 0.77.1, removing the account, or switching it to another provider, also deletes its session entries (`account_edit::remove_account_save_only` and `user_session::stage_dropped`): the entries are deleted before the TOML is saved and put back when the save fails, and a vault that refuses the delete leaves both the account and its session in place with an error. Entries left behind by removals in earlier releases are not found again automatically; remove them with your system's keychain tool if you want them gone. Logging out on the provider's site (or changing the password) is what revokes the session for the provider; forgetting only removes Cuotai's copy.
- **What is never shown:** the session value never enters the TOML, `Debug` output, error messages or reports; a reference without its vault entry asks for the session again.
- **Risks:** whoever holds the session can act as you on that site until it expires or is revoked, so a leaked vault entry or a paste left in the clipboard or shell history is a leaked login; paste only into Cuotai's masked field and clear the clipboard afterwards. The query uses an internal console interface that the provider's terms of use may not allow for third-party tools; you decide whether to use it, and the provider can change or block it at any time. A provider that revokes the session (sign-out elsewhere, password change, security check) makes the card read "session expired or revoked; paste a new one".

## Usage access matrix

How each account's usage is read, as `cuotai_core::access::describe` classifies it (PRD 0005). This page is where that classification lives: since the owner decision of 2026-10-25 the TUI wizard, the desktop dialog, the cards and the details no longer show the level (Official, Undocumented, Derived, Unavailable) or its summary; the machine contracts (`export::v1`, the desktop view) still carry it. A provider has one row for its default configuration plus one per mode (a `params` value or the native login) that changes the queried interface; modes with the same classification share a row.

- **Level:** `Official` is an interface the vendor documents for that credential (it does not promise permission on your account); `Undocumented` is internal or inferred and may change without notice; `Derived` infers usage from another source (Cloud Monitoring metrics, rate-limit headers); `Unavailable` has no query and the account shows the reason instead of numbers.
- **Credential:** the class of secret the query needs. **Plan:** the vendor plan the query requires, when any. **Scope:** what the number covers (`Whole cloud` is a bill for every product of the cloud account).
- **Evidence:** the vendor documentation, or the public 9router/community-client file whose contract the adapter follows. Every row was last checked on the descriptor's `verified_on` date.

**Providers you cannot add from the app.** A provider that is `Unavailable` in every mode has no usage query at all, so an account for it would only show the reason instead of numbers. The "Add account" picker (desktop and TUI) leaves these out, as the core's `access::has_usage_query` decides: today Qwen (DashScope key and Qwen Code OAuth), Cloudflare Workers AI, NVIDIA NIM, Poolside and Alibaba Model Studio. An existing account of one of them keeps working as before: it is still edited, and the card still shows the reason. Providers with at least one queried mode stay in the picker (for example Perplexity with `product = computer` and Gemini AI with `product = cli`).

The table is generated: a core test compares it with `access::describe` and fails when they differ. After changing a classification, regenerate it with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix` and commit the result.

<!-- access-matrix:start (generated from access::describe; do not edit by hand) -->
| Identifier | Mode | Level | Credential | Plan | Scope | Evidence | Verified on | Overage |
|---|---|---|---|---|---|---|---|---|
| `open_ai_api` | default | Official | Admin key | — | Organization | [documentation](https://developers.openai.com/api/reference/resources/admin/subresources/organization) | 2026-10-20 | not reported |
| `anthropic_api` | default | Official | Admin key | — | Organization | [documentation](https://platform.claude.com/docs/en/manage-claude/usage-cost-api) | 2026-10-20 | not reported |
| `chat_gpt` | default | Undocumented | Subscription session | — | Account | reference [`rate_limit_resets.rs`](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs) | 2026-10-20 | not reported |
| `claude` | default | Undocumented | Subscription session | — | Account | reference [`claude.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/claude.js) | 2026-10-20 | reported in provider units |
| `gemini_api` | default | Derived | Cloud identity | — | Product | [documentation](https://cloud.google.com/monitoring/api/metrics_gcp_p_z) | 2026-10-20 | not reported |
| `vertex_ai` | default | Derived | Cloud identity | — | Product | [documentation](https://cloud.google.com/monitoring/api/metrics_gcp_p_z) | 2026-10-20 | not reported |
| `aws_bedrock` | default | Official | Cloud identity | — | Product | [documentation](https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html) | 2026-10-20 | not reported |
| `azure_open_ai` | default | Official | Cloud identity | — | Product | [documentation](https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01) | 2026-10-20 | not reported |
| `cursor` | default | Official | Admin key | Teams or Enterprise | Organization | [documentation](https://cursor.com/docs/account/teams/admin-api) | 2026-10-20 | not reported |
| `cursor` | `query_mode = "personal"` or native login | Undocumented | Subscription session | — | Account | reference [`CursorStatusProbe.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift) | 2026-10-20 | reported in money |
| `copilot` | default | Official | Admin key | — | Account | [documentation](https://docs.github.com/en/rest/billing/usage) | 2026-10-20 | not reported |
| `copilot` | `query_mode = "quota"` or native login | Undocumented | Subscription session | — | Account | reference [`github.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/github.js) | 2026-10-20 | not reported |
| `copilot` | `mode = "org"` | Official | Admin key | — | Organization | [documentation](https://docs.github.com/en/rest/billing/usage) | 2026-10-20 | not reported |
| `windsurf` | default | Official | Admin key | Enterprise | Organization | [documentation](https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance) | 2026-10-20 | not reported |
| `mini_max` | default | Undocumented | Inference key | — | Account | reference [`minimax.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/minimax.js) | 2026-10-20 | not reported |
| `kimi` | default | Official | Inference key | — | Account | [documentation](https://platform.kimi.ai/docs/api/balance.md) | 2026-10-20 | not reported |
| `kimi` | `product = "coding"` | Undocumented | Inference key | — | Account | reference [`kimi.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kimi.js) | 2026-10-20 | not reported |
| `kimi` | native login | Undocumented | Subscription session | — | Account | reference [`kimi.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kimi.js) | 2026-10-20 | not reported |
| `qwen` | default | Unavailable | Inference key | — | Account | [documentation](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope) | 2026-10-20 | not reported |
| `qwen` | native login | Unavailable | Subscription session | — | Account | [documentation](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/) | 2026-10-20 | not reported |
| `groq` | default | Derived | Inference key | — | Organization | [documentation](https://console.groq.com/docs/rate-limits) | 2026-10-20 | not reported |
| `perplexity` | default | Unavailable | Inference key | — | Account | [documentation](https://docs.perplexity.ai/docs/admin/computer-analytics-api.md) | 2026-10-20 | not reported |
| `perplexity` | `product = "computer"` | Official | Admin key | Enterprise | Organization | [documentation](https://docs.perplexity.ai/docs/admin/computer-analytics-api.md) | 2026-10-20 | not reported |
| `mistral` | default | Official | Admin key | Enterprise | Organization | [documentation](https://docs.mistral.ai/admin/admin-api/usage-metrics.md) | 2026-10-20 | not reported |
| `cloudflare_ai` | default | Unavailable | Inference key | — | Account | [documentation](https://developers.cloudflare.com/workers-ai/platform/pricing/) | 2026-10-20 | not reported |
| `ollama_cloud` | default | Undocumented | Inference key | — | Account | reference [`misc.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/misc.js) | 2026-10-20 | not reported |
| `xiaomi_mimo` | default | Undocumented | User session | — | Account | reference [`xiaomi-mimo.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/xiaomi-mimo.js) | 2026-10-20 | not reported |
| `open_code_go` | default | Undocumented | Inference key | — | Account | reference [`opencode-go.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/opencode-go.js) | 2026-10-20 | not reported |
| `qoder` | default | Undocumented | Subscription session | — | Account | reference [`misc.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/misc.js) | 2026-10-20 | not reported |
| `gemini_ai` | default | Unavailable | Inference key | — | Account | [documentation](https://ai.google.dev/gemini-api/docs/rate-limits) | 2026-10-20 | not reported |
| `gemini_ai` | `product = "cli"` or native login | Undocumented | Subscription session | — | Account | reference [`google.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/google.js) | 2026-10-20 | not reported |
| `grok` | default | Official | Admin key | — | Organization | [documentation](https://docs.x.ai/developers/rest-api-reference/management/billing.md) | 2026-10-20 | not reported |
| `grok` | `product = "cli"` or native login | Undocumented | Subscription session | — | Account | reference [`grok-cli.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/grok-cli.js) | 2026-10-20 | reported in provider units |
| `nvidia_nim` | default | Unavailable | Inference key | — | Account | [documentation](https://docs.api.nvidia.com/nim/reference/llm-apis) | 2026-10-20 | not reported |
| `poolside` | default | Unavailable | Inference key | — | Account | [documentation](https://docs.poolside.ai/api/overview.md) | 2026-10-20 | not reported |
| `kiro` | default | Undocumented | Subscription session | — | Account | reference [`kiro.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kiro.js) | 2026-10-20 | reported in money |
| `cline` | default | Official | Inference key | — | Account | [documentation](https://docs.cline.bot/enterprise-solutions/api-reference.md) | 2026-10-20 | not reported |
| `cline` | `organization_id` set | Official | Inference key | — | Organization | [documentation](https://docs.cline.bot/enterprise-solutions/api-reference.md) | 2026-10-20 | not reported |
| `cline` | native login | Official | Subscription session | — | Account | [documentation](https://docs.cline.bot/enterprise-solutions/api-reference.md) | 2026-10-20 | not reported |
| `code_buddy` | default | Undocumented | Subscription session | — | Account | reference [`codebuddy-cn.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/codebuddy-cn.js) | 2026-10-20 | not reported |
| `zed` | default | Undocumented | Subscription session | — | Account | reference [`zed.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/zed.js) | 2026-10-20 | not reported |
| `alibaba_model_studio` | default | Unavailable | Inference key | — | Account | [documentation](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope) | 2026-10-20 | not reported |
| `alibaba_coding_plan` | default | Undocumented | Inference key | — | Account | reference [`AlibabaCodingPlanUsageFetcher.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift) | 2026-10-20 | not reported |
| `alibaba_coding_plan` | `session_env` set or session in the vault | Undocumented | User session | — | Account | reference [`AlibabaCodingPlanUsageFetcher.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift) | 2026-10-20 | not reported |
| `alibaba_token_plan` | default | Undocumented | Local CLI | — | Account | reference [`token-plan.ts`](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts) | 2026-10-20 | not reported |
| `alibaba_token_plan` | `session_env` set or session in the vault | Undocumented | User session | — | Account | reference [`token-plan.ts`](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts) | 2026-10-20 | not reported |
| `alibaba_cloud` | default | Official | Cloud identity | — | Whole cloud | [documentation](https://www.alibabacloud.com/help/en/user-center/developer-reference/api-bssopenapi-2017-12-14-queryaccountbalance) | 2026-10-20 | not reported |
| `antigravity` | default | Undocumented | Subscription session | — | Account | reference [`antigravity-weekly.js`](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/antigravity-weekly.js) | 2026-10-20 | not reported |
| `kimchi` | default | Undocumented | Inference key | — | Account | reference [`status.ts`](https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts) | 2026-10-20 | not reported |
| `eleven_labs` | default | Official | Inference key | — | Account | [documentation](https://elevenlabs.io/docs/api-reference/user/subscription/get) | 2026-10-23 | not reported |
| `nine_router` | default | Undocumented | Aggregator login | — | Instance | reference [`dashboardGuard.js`](https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/dashboardGuard.js) | 2026-10-25 | not reported |
| `open_router` | default | Official | Inference key | — | Account | [documentation](https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key) | 2026-10-23 | not reported |
| `open_router` | `product = "account"` | Official | Admin key | — | Account | [documentation](https://openrouter.ai/docs/api/api-reference/credits/get-credits) | 2026-10-23 | not reported |
| `litellm` | default | Official | Admin key | — | Instance | [documentation](https://docs.litellm.ai/docs/proxy/provider_budget_routing) | 2026-10-25 | not reported |

Billed per query (`metered`): `aws_bedrock`.
<!-- access-matrix:end -->

### Paid extra usage (overage)

The **Overage** column says whether the interface reports consumption that the provider bills on top of the plan: **reported in money** (Cursor on-demand, Kiro overage charges), **reported in provider units** (Claude extra usage, Grok on-demand credits) or **not reported**. Cuotai lists that amount apart from the plan limits (`billing: "overage"` in `--json` and `export::v1`): it is never the binding limit, has no pace and raises no threshold alert; it has its own state per account (`disabled`, `armed`, `active`, `cap_reached`, `unknown`) and one "extra usage started" notice per billing cycle. A missing amount is shown as unknown, never as zero, and a cap of zero means the provider's on-demand is off. Cuotai never changes a provider's billing settings.

**Claude units caveat.** The Claude interface does not document the unit of `used_credits` and `monthly_limit`. The account is billed in US dollars, but whether the API returns dollars or cents is still to be checked against the amount the Claude web console shows (PRD 0023 RF2.6, an owner smoke), so Cuotai keeps the value as provider units and shows no `$` sign until that check is recorded here.

## Provider catalog and search

The account pickers (desktop and TUI) list providers from `cuotai_core::account_edit::catalog` (PRD 0027): one entry per provider and connection mode, grouped by **sign-in method**. The one-line hint is derived from the access descriptor, not written per provider; the native-login mode is the recommended one when a provider has it. `account_edit::search` ranks the same entries (case- and accent-insensitive; display name, identifier, aliases, then category, sign-in method and hint), so typing `codex` finds ChatGPT / Codex in both editors. The category is a search term only.

<!-- catalog-table:start (generated from account_edit::catalog; do not edit by hand) -->
| Display name | Identifier | Mode | Sign-in method | Category | Aliases | Hint |
|---|---|---|---|---|---|---|
| OpenAI API | `open_ai_api` | — | API key | API platform | openai, gpt | Admin API key |
| Anthropic API | `anthropic_api` | — | API key | API platform | claude api | Admin API key |
| ChatGPT / Codex | `chat_gpt` | `oauth` | Browser sign-in | Subscription | codex, openai, chatgpt, gpt | Sign in with your browser |
| ChatGPT / Codex | `chat_gpt` | `credential` | API key | Subscription | codex, openai, chatgpt, gpt | Account access token |
| Claude | `claude` | `oauth` | Browser sign-in | Subscription | anthropic, claude code, claude.ai | Sign in with your browser |
| Claude | `claude` | `credential` | API key | Subscription | anthropic, claude code, claude.ai | Account access token |
| Gemini (Cloud Monitoring) | `gemini_api` | — | Cloud profile | Cloud | google, gcp, cloud monitoring | Cloud credentials |
| Vertex AI | `vertex_ai` | — | Cloud profile | Cloud | google, gcp, vertex | Cloud credentials |
| AWS Bedrock | `aws_bedrock` | — | Cloud profile | Cloud | aws, amazon, bedrock | Cloud credentials · billed per query |
| Azure OpenAI | `azure_open_ai` | — | Cloud profile | Cloud | microsoft, azure, openai | Cloud credentials |
| Cursor | `cursor` | `oauth` | Browser sign-in | Coding tool | cursor ai | Sign in with your browser |
| Cursor | `cursor` | `credential` | API key | Coding tool | cursor ai | Admin API key |
| GitHub Copilot | `copilot` | `oauth` | Browser sign-in | Coding tool | github, gh, github copilot | Sign in with your browser |
| GitHub Copilot | `copilot` | `credential` | API key | Coding tool | github, gh, github copilot | Admin API key |
| Windsurf | `windsurf` | — | API key | Coding tool | codeium | Admin API key |
| Minimax.io | `mini_max` | — | API key | API platform | minimax | API key |
| Kimi | `kimi` | `oauth` | Browser sign-in | API platform | moonshot | Sign in with your browser |
| Kimi | `kimi` | `credential` | API key | API platform | moonshot | API key |
| Qwen | `qwen` | `oauth` | Browser sign-in | API platform | qwen code, dashscope, alibaba | Sign in with your browser |
| Qwen | `qwen` | `credential` | API key | API platform | qwen code, dashscope, alibaba | API key |
| Groq | `groq` | — | API key | API platform | — | API key |
| Perplexity | `perplexity` | — | API key | API platform | sonar | API key |
| Mistral | `mistral` | — | API key | API platform | le chat | Admin API key |
| Cloudflare AI | `cloudflare_ai` | — | API key | API platform | workers ai, cloudflare | API key |
| Ollama Cloud | `ollama_cloud` | — | API key | API platform | ollama | API key |
| Xiaomi MIMO | `xiaomi_mimo` | `oauth` | Web session | API platform | mimo, xiaomi | Paste a web session |
| Xiaomi MIMO | `xiaomi_mimo` | `credential` | Web session | API platform | mimo, xiaomi | Paste a web session |
| Opencode Go | `open_code_go` | — | API key | Coding tool | opencode, open code | API key |
| Qoder | `qoder` | `oauth` | Browser sign-in | Coding tool | — | Sign in with your browser |
| Qoder | `qoder` | `credential` | API key | Coding tool | — | Account access token |
| Gemini AI (API key) | `gemini_ai` | `oauth` | Browser sign-in | API platform | gemini cli, google, ai studio | Sign in with your browser |
| Gemini AI (API key) | `gemini_ai` | `credential` | API key | API platform | gemini cli, google, ai studio | API key |
| Grok (xAI) | `grok` | `oauth` | Browser sign-in | API platform | xai, x.ai, grok cli | Sign in with your browser |
| Grok (xAI) | `grok` | `credential` | API key | API platform | xai, x.ai, grok cli | Admin API key |
| NVidia NIM | `nvidia_nim` | — | API key | API platform | nvidia, nim | API key |
| Poolside | `poolside` | — | API key | API platform | — | API key |
| Kiro (AWS) | `kiro` | `oauth` | Browser sign-in | Coding tool | codewhisperer | Sign in with your browser |
| Kiro (AWS) | `kiro` | `credential` | API key | Coding tool | codewhisperer | Account access token |
| Cline | `cline` | `oauth` | Browser sign-in | Coding tool | — | Sign in with your browser |
| Cline | `cline` | `credential` | API key | Coding tool | — | API key |
| CodeBuddy | `code_buddy` | `oauth` | Browser sign-in | Coding tool | tencent | Sign in with your browser |
| CodeBuddy | `code_buddy` | `credential` | API key | Coding tool | tencent | Account access token |
| Zed | `zed` | `oauth` | Browser sign-in | Coding tool | — | Sign in with your browser |
| Zed | `zed` | `credential` | API key | Coding tool | — | Account access token |
| Alibaba Model Studio / DashScope | `alibaba_model_studio` | — | API key | API platform | dashscope, model studio, alibaba | API key |
| Alibaba Coding Plan | `alibaba_coding_plan` | — | API key | Coding tool | bailian, alibaba, coding plan | API key |
| Alibaba Token Plan (Qwen Cloud) | `alibaba_token_plan` | — | Local credentials | Coding tool | qwen cloud, alibaba | Uses the bl CLI |
| Alibaba Cloud — billing | `alibaba_cloud` | — | Cloud profile | Cloud | aliyun, alibaba | Cloud credentials |
| Antigravity | `antigravity` | `oauth` | Browser sign-in | Coding tool | google | Sign in with your browser |
| Antigravity | `antigravity` | `credential` | API key | Coding tool | google | Account access token |
| Kimchi | `kimchi` | — | API key | API platform | — | API key |
| ElevenLabs | `eleven_labs` | — | API key | API platform | eleven labs, 11labs | API key |
| 9router | `nine_router` | — | API key | Router | 9router, nine router, router, aggregator | Dashboard password (optional) |
| OpenRouter | `open_router` | `oauth` | Browser sign-in | Router | openrouter, open router, router, or | Sign in with your browser |
| OpenRouter | `open_router` | `credential` | API key | Router | openrouter, open router, router, or | API key |
| LiteLLM proxy | `litellm` | — | API key | Router | litellm, lite llm, proxy, gateway, aggregator | Admin API key |
<!-- catalog-table:end -->

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
| Alibaba Coding Plan / `alibaba_coding_plan` | `BAILIAN_CODING_PLAN_API_KEY` or a configured variable | Regional RPC `queryCodingPlanInstanceInfoV2`: usage/limit for 5h, week and month. If the gateway requires login, paste the console session in the account wizard (system vault) or reference it with `params.session_env`; it does not extract a browser session or try another region |
| Alibaba Token Plan (Qwen Cloud) / `alibaba_token_plan` | `params.cli_profile = "cuotai-qwen"` (international `bl` login) **or** a home.qwencloud.com session pasted in the account wizard (system vault) or named by `params.session_env = "QWEN_CLOUD_COOKIE"` (Cookie) | 5h/week quota through `bl usage token-plan --output json` on the given profile; the session uses direct HTTP to the Qwen Cloud console and takes precedence when present. The `QWEN_API_KEY` key only authenticates inference |
| Alibaba Cloud / `alibaba_cloud` | `ALIBABA_CLOUD_ACCESS_KEY_ID` | BSS balance through the AccessKey Secret referenced in `params.secret_env`; optional STS in `security_token_env`. `billing_cycle=YYYY-MM` queries the bill for **all products**, not only AI |
| Antigravity / `antigravity` | Google OAuth or `ANTIGRAVITY_ACCESS_TOKEN` | Project/plan through `loadCodeAssist`; per-model quotas through `fetchAvailableModels` on paid plans; session/week through `retrieveUserQuotaSummary`. A model without a quota fraction keeps unknown values and does not force the session to 0%. Optional `project_id` |
| Groq / `groq` | `GROQ_API_KEY` | GET `/openai/v1/models`, reading `x-ratelimit-*` as 9router does. No generation. If the response carries no headers, it reports connected with no quota reported, never an invented zero |
| Perplexity / `perplexity` | `PERPLEXITY_API_KEY` | Sonar: unavailable. **Computer Enterprise Analytics** is a separate opt-in (`params.product=computer`), with an Analytics key; sums the credits of complete UTC days |
| Mistral / `mistral` | `MISTRAL_API_KEY` | `/v1/admin/spend-limit` with an **Admin API Key**, `x-api-key` header. Missing values stay unknown; the monthly-limit state appears in the notes |
| Cloudflare AI / `cloudflare_ai` | `CLOUDFLARE_AI_API_KEY` | No Neurons/Workers AI quota query is confirmed in this integration. Generic Workers invocations and AI Gateway are not substitutes |
| Ollama Cloud / `ollama_cloud` | `OLLAMA_CLOUD_API_KEY` | GET `/api/usage`. Today it returns a usage report (`totals`: requests, tokens, USD spend over the `range`, 7d by default), shown as usage without a percentage because the API no longer exposes the session/week/month limits (only at ollama.com/settings/usage). If the endpoint returns `limits.session/weekly/monthly` again, they become windows in %, with the free renewal estimated from the sign-up anniversary. POST `/api/me` complements plan/sign-up; no cookie required |
| Xiaomi MiMo / `xiaomi_mimo` | Web session pasted in the account wizard (system vault) or named by a variable | `platform_session_env`: console balance and monthly Token Plan; `session_env`/`region`: Desktop weekly quota. The native code generates an inference key and does not replace those sessions |
| OpenCode Go / `open_code_go` | `OPENCODE_GO_API_KEY` | GET `https://opencode.ai/zen/go/v1/usage`: rolling, weekly and monthly percentage, with renewal |
| Qoder / `qoder` | `QODER_API_KEY` | `pt-` PAT exchanged for a job token and GET `/api/v2/quota/usage`; `params.region=intl\|cn`, `params.credential_type=auto\|pat\|bearer` |
| Gemini AI / `gemini_ai` | `GEMINI_API_KEY` | An AI Studio API key still has no quota in this interface. `params.product=cli` (or native login) uses the Gemini CLI session and `retrieveUserQuota`; it does not confuse products |
| Grok (xAI) / `grok` | `X_GROK_API_KEY` | Management API by default. `params.product=cli` (or native login) uses the Grok CLI session for on-demand credits, prepaid credits and the SuperGrok pool through REST/gRPC |
| NVIDIA NIM / `nvidia_nim` | `NVIDIA_NIM_API_KEY` | No public per-key balance query confirmed; it does not use inference to extract rate-limit headers |
| Poolside / `poolside` | `POOLSIDE_API_KEY` | No public account balance/quota query confirmed; infrastructure metrics are not a balance |
| Kimchi / `kimchi` | `KIMCHI_API_KEY` | GET `/v1/credits` and `/v1/budget`: USD balance, budget per scope/provider and renewal. A budget failure keeps the valid credits with a note |
| ElevenLabs / `eleven_labs` | `ELEVENLABS_API_KEY`; optional `params.region` (`default`, `us`, `eu`, `in`, `sg`) | One `GET /v1/user/subscription` per refresh (no audio is generated): credits used of the plan limit, next reset, extra-usage cap and amount, and voice slots. Restrict the key to the permission that reads the subscription. Pay As You Go balance is not reported. Unused credits may roll over and the limit semantics are the provider's. The API still names credits "characters"; Cuotai shows "credits". The vendor documents no credit cost or rate limit for this read |

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

## Aggregators

An **aggregator** (or routing gateway) is a service that holds credentials for several upstream providers and routes requests to them. An aggregator account is one Cuotai account that points at one running instance; each upstream connection the instance holds becomes a **sub-account** of it (`sub_reports` in `--json`, one line per connection in `--once`). The instance itself is the account: its status says whether it is reachable and authorized, and its class is the **pool** class (available when at least one active connection is, exhausted when every measured one is). A failed instance is one failed report with no sub-accounts; a failed connection is one failed sub-account and never fails the instance. Services that only report their own credits (OpenRouter and similar) are ordinary providers, not aggregators.

### 9router (`nine_router`)

Shows the quotas of the connections you already added **inside your own 9router instance** (`npm install -g 9router`, default `http://127.0.0.1:20128`), without re-creating each connection in Cuotai. Upstream tokens never leave the instance.

```toml
[[accounts]]
provider = "nine_router"
params = { base_url = "http://127.0.0.1:20128", include = "claude,codex,github" }
# credential_env = "NINEROUTER_PASSWORD"   # only when the dashboard login is on
```

- **What is read.** `GET /api/providers/client` (the connections, reduced to an allow-list of fields) and `GET /api/usage/{id}` for each active connection, plus `GET /api/version` for the version note. With a dashboard password: one `POST /api/auth/login`. These routes are **internal and undocumented** (observed at 9router revision `f39d8f07481ae8518e93175f0ecaaafa61573c6c`), so the access level is *Undocumented* and the reading can break when 9router changes. The first supported version is recorded after the owner's local verification (PRD 0036 task T2); older and newer instances still run.
- **Authentication.** 9router has no read-only token: the management routes accept the dashboard password's session, or anyone when the instance has `requireLogin = false`. Leave `credential_env` unset for an instance without login; otherwise put the dashboard password in an environment variable and name it in `credential_env` (the value never enters the TOML). The login cookie lives only in memory, is sent only to the instance, is renewed before it expires, and a rejected password or a lockout (`Retry-After`) is not retried inside its window. A redirect to the login page counts as an authentication error and is never followed. Instances that only allow SSO (OIDC/SAML) are not supported yet.
- **Local and remote.** Plain HTTP is accepted for `localhost`, `127.0.0.0/8` and `::1`. Any other host needs HTTPS; the only exception is `params.allow_private_http = "true"`, accepted for RFC 1918, link-local, `100.64.0.0/10` (Tailscale/CGNAT) and `.local`/`.ts.net` names. `base_url` cannot contain a user, password, query or fragment, and may carry a path prefix for a reverse proxy. HTTPS uses the built-in roots; a self-signed instance is supported only by `params.tls_ca_file` (a PEM with the CA that signed it, trusted in addition to the built-in roots, host name still checked) or `params.tls_pin_sha256` (the SHA-256 of the instance's own certificate; only that certificate is accepted). The two cannot be combined, and neither applies to plain HTTP. There is never an "accept any certificate" option. A pin is captured **trust on first use** in Test connection: when the instance presents a certificate this computer does not trust, the test shows its fingerprint (reading it sends only the public `GET /api/health`) and pins it only after you confirm it, preferably after comparing it with the server's. If a pinned certificate later changes, the account fails with an authentication error ("The instance's certificate changed") and is never re-pinned silently. When no password is set and the instance is not on loopback, the account carries a warning that anyone who can reach it can read and change its connections.
- **Test connection.** Adding a 9router account, or changing how it is reached, needs a successful **Test connection** before the save (F4 in the TUI wizard, the button in the desktop dialog; the save runs it by itself): `GET /api/health`, `GET /api/version`, the login when a password is set, and one page of the connection list. Each outcome is one sentence (connected with the version and the connection count, not a 9router, password refused, locked out with the wait, password login refused, password required, unreachable, untrusted or changed certificate); no response body, URL or token is shown.
- **Filters.** `params.include` / `params.exclude` (comma-separated 9router provider slugs) and `params.hidden` (opaque sub-account ids) are applied before any usage request is made.
- **Cost and side effects.** One refresh is one paged discovery plus one usage request per active connection (two at a time, 15 s each, 60 s for the whole instance). The refresh interval of an aggregator account cannot be below 120 s. Nothing is billed, but each usage request makes the **instance** query the upstream with its credentials and possibly renew a token. A refresh never sends `force=1`.
- **Never called.** The module can only build `GET /api/health`, `/api/version`, `/api/providers/client`, `/api/usage/{id}` and the login `POST`. `GET /api/providers` (it keeps unsanitized provider data), the `claude-reset` and `codex-reset-credits` routes, settings, keys and `/v1/*` cannot be built. Fields outside the allow-list (`lastError`, proxy URLs, tokens, unknown fields) are dropped at deserialization.
- **Honesty.** 9router itself turns some missing fields into 0 (for example in its Codex and Claude handlers) and may serve a cached reading without saying how old it is. Sub-account readings carry both notes ("via 9router"). A quota label 9router writes as free text keeps its text; a window (5 h, weekly, monthly, daily) is assigned only when the label says so, and an unknown label has no window and therefore no pace. Upstreams without a Cuotai provider keep a generic tile and 9router's provider name. `Metric` values are as normalized by 9router; Cuotai adds no zeros of its own.
- **Connections as accounts.** Each active connection gets the engines of a direct account of its mapped provider: binding metric, pace, usage alerts, reset notices, history and events, with a freshness and a preserved last reading of its own (a failed connection stays stale with its previous reading while the others update). By default an aggregator alerts only when **every** connection of one upstream is exhausted ("9router · Claude: all accounts exhausted") and when one returns; the desktop offers "each connection" per aggregator account. Connections are listed under their aggregator in `export::v1` (`sub_accounts`, `aggregator`), `/v1/limits` (with `parent`), events (`parent`, `pool_exhausted`, `pool_recovered`) and widgets.
- **Duplicates.** A connection is the same account as a direct Cuotai account when its mapped provider matches and its e-mail (9router's `email`, or `githubEmail` for Copilot) equals the direct account's e-mail exactly, after trimming and case-folding; never by name or by guess, and a connection or account without an e-mail is never a duplicate. The direct account wins: the connection stays in the 9router section marked "also monitored directly" and leaves the "All" view, the best-account ranking, alerts and history. `params.dedup = "off"` turns the detection off; `params.show_both = "<id>,…"` keeps single connections in both places. The e-mail is compared in memory only and never exported.
- **Groups.** A `group` set on the aggregator account applies to every connection for "best account now"; without one a connection ranks with the direct accounts of its mapped provider.
- **Refresh.** "Refresh now" on the instance reads every connection; on one connection (desktop) it reads the connection list and that connection's usage only. A connection's plan reset brings the instance's next refresh to 30 s after it, within the 120-s floor and at most once per reset.
- **Not delivered yet.** Pasted SSO sessions, the vault-stored password, the consent-gated presence probe and the minimum tested 9router version are still open.

### LiteLLM proxy (`litellm`)

Shows the USD budget and spend of each upstream provider that your own [LiteLLM proxy](https://docs.litellm.ai/docs/proxy/provider_budget_routing) has in `provider_budget_config`, one sub-account per provider.

```toml
[[accounts]]
provider = "litellm"
credential_env = "LITELLM_API_KEY"   # master or virtual key of the proxy
params = { base_url = "http://127.0.0.1:4000" }
```

- **What is read.** Only `GET /provider/budgets` with `Authorization: Bearer <key>`, a documented route (access level *Official*; read from the public documentation on 2026-10-25, not yet checked against a live proxy). Each provider becomes a `Spend` metric: `spend` used of `budget_limit`, its window from `time_period` (`1d` daily, `7d` weekly, `1mo`/`30d` monthly, others without a window) and `budget_reset_at` as the reset when it is a timestamp. A budget below one cent (the docs use `1e-12` to block a provider) is shown as no cap, and an absent spend stays unknown.
- **Mapping.** `openai`, `anthropic`, `azure`, `vertex_ai`, `gemini`, `bedrock`, `groq`, `mistral`, `perplexity` and `xai` map to their Cuotai providers; any other name keeps a generic tile and LiteLLM's name.
- **Rules shared with 9router.** The same base-URL rules (HTTP only on loopback, or a private network with `allow_private_http`), Test connection before the first save, `params.hidden`, the pools, alerts and export of sub-accounts. LiteLLM reports no e-mail, so no provider is ever taken for a direct account. Inference, key and team routes are never called.

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
| Xiaomi MiMo | the account session (`serviceToken`, …) pasted in the account wizard, or `session_env = "XIAOMI_MIMO_COOKIE"` naming a variable with the cookie you exported; `region = "sgp"` (or cn/ams/ru/in) |
| Qoder | PAT/job token in `QODER_API_KEY`; `region = "intl"` (or cn), `credential_type = "auto"` (or pat/bearer). Exchanges PAT→job token without running inference |
| Kiro/CodeBuddy/Zed | Native login in the wizard/dialog is preferred; tokens and metadata are filled in automatically. The manual variable method remains available; for Kiro it defaults to `google` and does not offer `builder-id`/`idc` |

Deliberate differences from 9router: missing fields do not become zero or unlimited; Gemini CLI shows `remainingFraction` in **%**, not as 1000 real requests; Ollama flags a renewal computed from the sign-up date as an estimate; error responses do not expose the raw body; timezone-less dates of CodeBuddy packages are interpreted as UTC.

## Query semantics and costs

- `used`, `limit` and `remaining` are optional. A missing value does **not** mean zero. An API balance is not computed from spend, and currencies are not mixed.
- `status` describes the **query**, not the provider's overall health. `ready` means the query finished, not that quota is available. HTTP 401/403, 429, network, timeout and incompatible response are told apart.
- Monthly costs are cumulative; `resets_at` in those reports indicates the start of the next tracking period, not a guaranteed credit grant.
- **OpenAI and Anthropic:** tokens are limited to completions/messages; costs have a broader scope. Filtering `models` changes tokens, **not** the cost scope.
- **Google Cloud Monitoring:** limits are sampled daily; consumption is the sum of the last 10 minutes with a delay. Limit and usage series are separate, not comparable percentages. It can also bill for reads (two initial queries per refresh, plus pages).
- **AWS Bedrock:** costs are delayed and possibly estimated. **Cost Explorer charges per request** (check the current price; reference US$ 0.01). Without `refresh_seconds` the account is queried **once an hour** (about US$ 0.24/day), and the floor is **900 s**: an override between 300 and 899 s is raised to 900 s with a note, and a value below 300 s is rejected. Cuotai warns about the cost when you edit the account (TUI and desktop) and shares one recorded request between the processes that use the same profile (`<OS local data dir>/cuotai/metered/`), so the desktop app, the TUI and `--watch` never double the bill. A metered account is never queried after a reset (see below).
- **After a reset:** 30 seconds after a quota resets, Cuotai queries the account once more (at most once per window and every 5 minutes), so a card does not keep showing "blocked until 14:30" for a whole refresh interval. Never for metered accounts. A last valid reading that is shown while a query fails (or one restored from the history, at most 24 hours old) collapses every metric whose reset has passed to "Renewed at 14:30; waiting for a reading": it is neither exhausted nor counted as available.
- **`as_of`, the time of the data:** a report may state when its data is from (`as_of`, in `--json` and in `export::v1` with `data_age_seconds`), but only when the provider documents it. Today only `open_ai_api` sets it (the end of the last complete daily cost bucket); every other adapter leaves it unset, and the cards show "data as of" only when the data lags the query by more than 15 minutes.
- **Windsurf:** analytics in hourly buckets; allocation and consumption can have distinct scopes. The app does not invent a remaining balance from that comparison.

## Model catalog per account

The list is only queried on demand (`load_models`/`sync_models`), never by the quota refresh. **A provider lists only where its listing interface is documented or was verified live**, always read-only and with no inference cost, using the same credential the account already stores for the quota:

- **live query:** the provider's listing endpoint. When the credential is not accepted (for example, an admin key where the endpoint asks for an inference key), the account shows the classified failure (authentication, limit, transport, schema) and keeps the previous catalog; nothing is invented;
- **no list:** Alibaba Cloud (billing AccessKey, no models endpoint) and every provider or mode without evidence of a listing interface (CodeBuddy, Windsurf, Xiaomi MIMO, Qoder, Alibaba Token Plan, Qwen, Kimchi, GitHub Copilot, and the Gemini AI session, Grok CLI and Model Studio non-international modes), with the reason and no request. There are no embedded model lists: Cuotai never invents models from a static table.

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
- **Kiro:** `ListAvailableModels` on the host of the `profile_arn`'s region (default `us-east-1`), with the same per-authentication-method headers as the quota.
- **Cursor:** RPC `aiserver.v1.AiService/GetUsableModels` in Connect JSON, with the session token; accounts with only the admin key (Basic) get the provider's refusal.
- **Zed:** `users/me` (organization) → `client/llm_tokens` → `cloud.zed.dev/models`; disabled models are left out. Plans without hosted models return an empty list, which is the real result.
- **Azure OpenAI:** `…/accounts/{resource}/models` of the configured resource, with the ARM token; page links outside the ARM host are rejected.
- **Grok:** a CLI session (`product = "cli"`) lists through `cli-chat-proxy.grok.com/models` with the client headers; with an API key, `api.x.ai/v1/models`.

Evidence of each listing (PRD 0028 RF10.1), generated from `models::evidence` and checked by a test; regenerate with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_models_matrix`. **documented** cites the vendor's documentation; **verified live** means an undocumented interface confirmed against a real account, citing the public contract the quota query follows. A mode without evidence is `Unsupported` for models.

<!-- models-matrix:start -->
| Identifier | Mode | Listing | Evidence | Verified on |
|---|---|---|---|---|
| `open_ai_api` | any | documented | [reference](https://platform.openai.com/docs/api-reference/models/list) | 2026-10-20 |
| `anthropic_api` | any | documented | [reference](https://docs.anthropic.com/en/api/models-list) | 2026-10-20 |
| `chat_gpt` | any | verified live | [reference](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs) | 2026-10-20 |
| `claude` | any | verified live | [reference](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/claude.js) | 2026-10-20 |
| `gemini_api` | any | documented | [reference](https://ai.google.dev/api/models#method:-models.list) | 2026-10-20 |
| `vertex_ai` | any | documented | [reference](https://cloud.google.com/vertex-ai/docs/reference/rest/v1beta1/publishers.models/list) | 2026-10-20 |
| `aws_bedrock` | any | documented | [reference](https://docs.aws.amazon.com/cli/latest/reference/bedrock/list-foundation-models.html) | 2026-10-20 |
| `azure_open_ai` | any | documented | [reference](https://learn.microsoft.com/en-us/rest/api/aiservices/accountmanagement/models/list) | 2026-10-20 |
| `cursor` | any | verified live | [reference](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift) | 2026-10-20 |
| `copilot` | any | none: `Unsupported` | — | — |
| `windsurf` | any | none: `Unsupported` | — | — |
| `mini_max` | any | documented | [reference](https://platform.minimax.io/docs) | 2026-10-20 |
| `kimi` | default | documented | [reference](https://platform.kimi.ai/docs) | 2026-10-20 |
| `kimi` | native login or product = coding | verified live | [reference](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kimi.js) | 2026-10-20 |
| `qwen` | any | none: `Unsupported` | — | — |
| `groq` | any | documented | [reference](https://console.groq.com/docs/api-reference#models-list) | 2026-10-20 |
| `perplexity` | any | documented | [reference](https://docs.perplexity.ai/) | 2026-10-20 |
| `mistral` | any | documented | [reference](https://docs.mistral.ai/api/) | 2026-10-20 |
| `cloudflare_ai` | any | documented | [reference](https://developers.cloudflare.com/api/resources/ai/subresources/models/methods/search/) | 2026-10-20 |
| `ollama_cloud` | any | documented | [reference](https://docs.ollama.com/api/openai-compatibility) | 2026-10-20 |
| `xiaomi_mimo` | any | none: `Unsupported` | — | — |
| `open_code_go` | any | verified live | [reference](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/opencode-go.js) | 2026-10-20 |
| `qoder` | any | none: `Unsupported` | — | — |
| `gemini_ai` | default | documented | [reference](https://ai.google.dev/api/models#method:-models.list) | 2026-10-20 |
| `gemini_ai` | native login or product = cli | none: `Unsupported` | — | — |
| `grok` | default | documented | [reference](https://docs.x.ai/docs/api-reference) | 2026-10-20 |
| `grok` | native login or product = cli | none: `Unsupported` | — | — |
| `nvidia_nim` | any | documented | [reference](https://docs.api.nvidia.com/nim/reference/llm-apis) | 2026-10-20 |
| `poolside` | any | documented | [reference](https://docs.poolside.ai/) | 2026-10-20 |
| `kiro` | any | verified live | [reference](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kiro.js) | 2026-10-20 |
| `cline` | any | documented | [reference](https://docs.cline.bot/) | 2026-10-20 |
| `code_buddy` | any | none: `Unsupported` | — | — |
| `zed` | any | verified live | [reference](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/zed.js) | 2026-10-20 |
| `alibaba_model_studio` | default | documented | [reference](https://www.alibabacloud.com/help/en/model-studio/) | 2026-10-20 |
| `alibaba_model_studio` | region = cn | none: `Unsupported` | — | — |
| `alibaba_coding_plan` | any | documented | [reference](https://www.alibabacloud.com/help/en/model-studio/) | 2026-10-20 |
| `alibaba_token_plan` | any | none: `Unsupported` | — | — |
| `alibaba_cloud` | any | none: `Unsupported` | — | — |
| `antigravity` | any | verified live | [reference](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/antigravity-weekly.js) | 2026-10-20 |
| `kimchi` | any | none: `Unsupported` | — | — |
| `eleven_labs` | any | none: `Unsupported` | — | — |
| `nine_router` | any | none: `Unsupported` | — | — |
| `open_router` | any | documented | [reference](https://openrouter.ai/docs/api/api-reference/models/get-models) | 2026-10-20 |
| `litellm` | any | none: `Unsupported` | — | — |
<!-- models-matrix:end -->

## Public incident source registry

Since 0.46.0, library consumers can call `cuotai_core::incidents::source(provider)` without an account, credentials, an async runtime or network access (PRD 0011 RF1). It returns a `StatusSource` with the wire format, public endpoint, component selectors and last verification date. `includes_component(id)` performs exact matching, never a page-wide fallback. A source is **not a health reading**; `None` means **unverified**, never “operational” or “no incidents”.

**Owner decision: incident queries are opt-in, off until enabled; `--no-status` remains an override.** Even credential-free public requests disclose an IP address and interest in a provider to these domains.

### Status client (since 0.47.0)

`Monitor::incidents()` returns the `IncidentMonitor` that holds the preference (off by default) and the cache; `set_enabled(true)` turns queries on (PRD 0011 RF2). Rebuilding the monitor with `with_config` keeps both; `with_incidents` carries them into a monitor built from scratch. While the feature is off, no status request is made and `refresh_incidents`/`incident_statuses` return nothing. `Monitor::status_domains()` lists, without network and also while off, the hosts the configured providers' sources would reach (one per host), and `StatusSource::host()` gives one source's host. Since 0.48.0 the desktop turns it on only from **Settings → Privacy** (RF4, `status_queries` in `desktop.json`) and opens a status or incident page only when `incidents::is_status_link(url)` accepts it (plain `https` on a verified source host, normalized, without port, credentials, query or fragment); badge texts come from `IncidentState::label()` (`incident.state.*`). Since 0.49.0 the TUI, `--once` and `--json` turn it on only with `--status` or `CUOTAI_STATUS=1`, and `--no-status` overrides both (RF5); `export::v1` publishes each account's provider status as `incident`.

- **Requests:** only the sources of providers with a configured account, once per URL (shared OpenAI and Claude endpoints are fetched once for every account and product) and at most once per five minutes, also after a failure. They use the monitor's HTTP client and query semaphore, send no credential, cookie, query parameter or account data, follow no redirect, identify themselves as `cuotai/<version> (+https://github.com/jmarcon/cuotai)`, stop after 5 seconds and refuse bodies above 1 MiB. `Monitor::refresh` runs them alongside the quota queries; nothing about a status failure reaches a report.
- **States:** `operational`, `degraded`, `partial_outage`, `major_outage`, `maintenance` and `unknown`, computed only from the provider's scoped components and events. Statuspage v2 needs a scoped component in the summary, otherwise the state is unknown; the incident.io widget and Google Cloud are operational when no active event touches the scope; the AWS and xAI feeds stay **unknown** without an active scoped event, because their history cannot prove health. An event that lists the scoped components as operational is unknown, not operational, and a page-wide indicator is never used.
- **Active incident:** the most severe scoped event, with its title (HTML, entities, Markdown and control characters removed, at most 120 characters), start and a public `https` link on the source host (query and fragment dropped), otherwise the status page itself. Statuspage incidents use `/incidents/<id>` on the source host; AWS takes the newest update of each service/region event and its first update as the start; resolved xAI entries are ignored.
- **Failures:** an invalid, oversized, redirected, failed or timed-out response keeps the last valid reading with `stale = true` (or `unknown` when there is none); it never changes the quota query or the account `Status`.
- **Correlation (RF3):** when a query ends in `error`, `rate_limited` or `authentication_error` and the provider's state is `degraded` or worse, the report gains the note `note.incident.ongoing` ("The provider reports an ongoing incident; the failure may not be from your account."). An authentication failure also gains `note.incident.auth_wait`, a suggestion to wait before signing in again that blocks nothing. The `Status` stays as queried and no note names the incident as the cause. A failed query waits for the cycle's single status request (5 seconds at most) instead of making its own.
- **Desktop (RF4):** a card badge for `degraded`, `partial_outage`, `major_outage` or `maintenance` (marked as the last reading when stale) or for a failed latest read of an operational or unknown page; `operational` and a plain `unknown` show no badge and are spelled out in the details. The tray icon gets a red dot while a configured provider is in `partial_outage` or `major_outage`.
- **TUI and CLI (RF5):** the same badge rule on the card's top border and in a `--once` `Status do provedor:` line; the details list the full status (operational and unknown included) with the incident, start, reading time and page. `--json` keeps the `Snapshot` shape.

The following endpoints and public-use documentation were fetched on **2026-10-20**, without authentication. All included endpoints returned HTTP 200 with the stated JSON/RSS shape. Component IDs, not display names or overall page indicators, select relevant services. Shared OpenAI and Claude endpoints remain identical across their provider entries so a future client can coalesce by URL. These are product-level observations, not account-, plan- or region-specific guarantees.

| Identifier | Format | Public source | Verified scope (component IDs in the core) | Verified on |
|---|---|---|---|---|
| `open_ai_api` | `JsonCustom(IncidentIoWidget)` | [endpoint](https://status.openai.com/api/v1/summary) | Chat Completions; Responses; Fine-tuning; Embeddings; Images; Batch; Audio; Moderations; Realtime; Files; Login (API) | 2026-10-20 |
| `anthropic_api` | `StatuspageV2` | [endpoint](https://status.claude.com/api/v2/summary.json) | Claude API (api.anthropic.com); Claude Console (platform.claude.com) | 2026-10-20 |
| `chat_gpt` | `JsonCustom(IncidentIoWidget)` | [endpoint](https://status.openai.com/api/v1/summary) | Codex in ChatGPT Desktop; Login (ChatGPT); Codex Web; Codex API; CLI; VS Code extension | 2026-10-20 |
| `claude` | `StatuspageV2` | [endpoint](https://status.claude.com/api/v2/summary.json) | claude.ai; Claude Code; Claude Cowork | 2026-10-20 |
| `gemini_api` | None | — | Official page exists, but its internal RPC is not a verified public API (see exclusions below) | — |
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
| `eleven_labs` | `JsonCustom(IncidentIoWidget)`, multi-page | [endpoint](https://status.elevenlabs.io/api/v1/summary) | Other APIs (ElevenAPI group; `region` selects the sub-page: `default`/`us` United States, `eu`, `in` India, `sg` Singapore) | 2026-10-25 |
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
| `eleven_labs` | None | — | Not verified (see exclusions below) | — |

### Verification, stability and use policy

- **Statuspage v2:** each publisher explicitly documents unauthenticated, programmatic use and the summary's components, incidents and maintenance: [Claude](https://status.claude.com/api), [GitHub](https://www.githubstatus.com/api), [Cursor](https://status.cursor.com/api), [Windsurf](https://status.windsurf.com/api), [MiniMax](https://status.minimax.io/api), [Moonshot](https://status.moonshot.cn/api) and [NVIDIA NGC](https://status.ngc.nvidia.com/api). The canonical Claude and Windsurf hosts replace redirects from `status.anthropic.com` and `status.codeium.com`. Their standard `robots.txt` disallows crawling `/api/` and `/embed/`; this registry uses only the publisher's explicitly documented API, not HTML crawling or indexing. No broader scraping permission is inferred.
- **Cloudflare:** [API use policy](https://www.cloudflarestatus.com/api) explicitly requests automated API polling instead of HTML scraping, no authentication, and an identifiable User-Agent with a contact URL. Its v2-compatible summary was about 236 KiB at verification; RF2's response-size ceiling must accommodate it. A major page-wide outage with Workers AI operational was observed and retained as a scope-boundary fixture. City outages, Workers KV and unrelated products must not become Workers AI incidents.
- **OpenAI, Groq and Perplexity:** the enabled [incident.io Widget API](https://docs.incident.io/status-pages/api#widget-api) is a public read-only JSON API intended for embedding status, with caching strongly recommended and high traffic subject to discussion with the operator. `ongoing_incidents`, `in_progress_maintenances` and `scheduled_maintenances` contain `affected_components[].id`. Their compatibility `/api/v2/summary.json` endpoints returned null incident/maintenance lists, so those are **not** used for incident monitoring. The component catalogs were checked separately ([OpenAI](https://status.openai.com/api/v2/components.json), [Groq](https://groqstatus.com/api/v2/components.json), [Perplexity](https://status.perplexity.com/api/v2/components.json)); OpenAI's summary omitted later Codex components, so the complete components endpoint is the verification evidence. Duplicate “Login” labels are separated by IDs. No robots policy was published at these hosts' `/robots.txt` (404).
- **ElevenLabs:** the incident.io status page has four sub-pages (United States, EU, India, Singapore). The widget summary at `https://status.elevenlabs.io/api/v1/summary` (200 `application/json`, 2026-10-25) returns `subpages[].summary` with the same lists as the flat widget, and the page's `/proxy/status.elevenlabs.io` structure plus the sub-page HTML show that the *ElevenAPI → Other APIs* component has one id (`01M52E4R81RBEYD94QJACM67BQ`) on all four sub-pages, so a single scope serves every region; Text to Speech, Speech to Text, ElevenAgents and ElevenCreative components are excluded. The core selects the sub-page from the account's `region` (one request for all regions); a response without that sub-page reads as unknown, never operational, and accounts in several regions report the worst state. The compatibility `/api/v2/summary.json` returns an empty component list and is not used. All four sub-pages were quiet at capture, so the incident and maintenance fixtures reuse the widget shape of the other incident.io sources with the real component id (synthetic, not a live incident). Fixtures: `eleven-labs-summary.json` (captured response), `eleven-labs-components.json` (component catalog from the page structure).
- **Google Cloud:** [programmatic consumption guidance](https://docs.cloud.google.com/service-health/docs/service-health-fallback#consume_google_cloud_service_health_data_programmatically), [JSON schema](https://status.cloud.google.com/incidents.schema.json) and [product catalog](https://status.cloud.google.com/products.json) explicitly support the public JSON history file. Use stable `affected_products[].id`, never unstable titles. `robots.txt` permits access. Only broad public incidents are included. The catalog has a verified Vertex Gemini API product, but no standalone Gemini API product selector. Cloud Monitoring is only `gemini_api`'s measurement dependency and cannot establish whether the Gemini API itself is operational. `vertex_ai` includes the verified inference products plus Monitoring, not every Google product.
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
| `open_router` | Checked 2026-10-25: `status.openrouter.ai` is a Datadog Status Pages single-page application (the shell is served at any path, including `/api/v1/summary`). `/api/v2/summary.json`, `/api/v2/components.json`, `/feed.rss`, `/index.json`, `/proxy/status.openrouter.ai` and `/robots.txt` answer `403 AccessDenied`, and `/history.rss` answers `404`. The page's script loads its data from a per-page `config.json` asset (internal to the platform, with no documented schema, terms or compatibility promise) and no public API route exists in the bundle, so there is no machine-readable source Cuotai may rely on. Not available upstream; no HTML scraping and no guessed component ids. Re-check if OpenRouter announces a feed. |
| `zed` | [Instatus public API](https://status.zed.dev/public-api) exists, but the summary has no component association and the documented v3 components array differed from the observed wrapped response. A Zed page-wide state could alarm for unrelated Delta/Collaboration/Extension Registry components; no scoped incident contract is registered. |
| `gemini_api` | Checked 2026-10-23: Google Cloud's public product catalog has no standalone Gemini API selector. Its Cloud Monitoring product covers only Cuotai's measurement dependency, so a quiet Monitoring status cannot assert that the Gemini API is operational. Google AI for Developers officially links the public [AI Studio status page](https://aistudio.google.com/status), whose live UI presents separate Gemini API, Multimodal Live API and Google AI Studio tracks. The UI gets that data from an unauthenticated `google.internal` `MakerSuiteService/ListIncidentsHistory` JSON+protobuf RPC with positional fields, an embedded site `x-goog-api-key` and origin-restricted headers. Google documents neither a stable public schema nor a programmatic-use policy for that internal interface, so Cuotai does not register it, copy the site key or scrape the HTML. |
| `gemini_ai`, `antigravity` | No verified feed for the AI Studio/CLI/Antigravity product scope. The Cloud catalog's Gemini Code Assist is not assumed to cover every Gemini AI mode or Antigravity. |
| `open_code_go`, `cline` | Public status pages responded with HTML; tried public summary/feed candidates returned 404. No documented machine-readable scoped contract verified. |
| `qwen`, `alibaba_model_studio`, `alibaba_coding_plan`, `alibaba_token_plan`, `alibaba_cloud` | Alibaba status sites expose HTML applications; no public, documented incident feed with relevant product selectors was verified. |
| `ollama_cloud`, `xiaomi_mimo`, `qoder`, `code_buddy` | No reliable public status endpoint verified; candidate status hosts failed DNS/network resolution. |
| `kiro` | Candidate public status host returned 403; Bedrock's AWS service must not stand in for Kiro. |

Moonshot's source is its published Open API/Kimi Code product status, not a claim of parity between domestic and international deployments. MiniMax excludes speech/video; GitHub excludes Actions/Pages; Windsurf excludes Netlify; NGC excludes Fleet Command. Missing components or a source with no relevant scoped incident never justify asserting account health.

### Offline verification fixtures

`crates/cuotai-core/src/incidents/fixtures/` holds minimized, anonymized real captures from the URLs above on the verification date: the Statuspage summaries, complete OpenAI and relevant Groq/Perplexity component catalogs, an empty OpenAI widget response, Google incident/product JSON and AWS/xAI RSS. `widget-maintenance.json` preserves the public [HashiCorp Widget API](https://status.hashicorp.com/api/v1/summary)'s real maintenance shape with populated components (the mapped pages had none at capture); its names, IDs, URLs and prose were anonymized. Public component IDs and names in provider catalogs remain intact because they are matching evidence, not personal data. Incident prose is replaced and long histories are reduced; states, timestamps, structural nulls and selector fields are retained. The AWS example is an unrelated multi-service event and the xAI examples are resolved events, not simulated active outages.

RF1 tests use only these local captures, covering all 40 provider decisions, positive component membership, exact-match boundaries, shared-source isolation, unrelated cloud services and the page-wide-outage/healthy-target case. RF2/RF3 tests serve the same captures and synthetic active incidents of each format from local `wiremock` servers; no live status assertion is a test expectation.

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
- [ElevenLabs: subscription API reference](https://elevenlabs.io/docs/api-reference/user/subscription/get)
- [Kimchi: official credits and budgets contract](https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts)
- [9router: query dispatch](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage.js) and [authentication flows](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde/src/lib/oauth/providers)
