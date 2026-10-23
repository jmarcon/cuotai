# Built-in OAuth Login, Sessions and the Credential Vault

Cuotai supports native authentication flows (browser OAuth, Device Code, AWS SSO and assisted import) in both the TUI and the desktop app. Tokens are stored in the operating system's native vault (`cuotai.oauth`), and `cuotai.toml` stays free of secrets.

## Overview by provider

| Provider | Login offered | Prerequisite |
|---|---|---|
| Gemini API / Vertex AI | System browser, authorization code + PKCE, callback on `127.0.0.1` | Your own **Desktop**-type OAuth client ID, API Monitoring enabled and read permission on the project |
| Azure OpenAI | Device code, Microsoft browser | Your own Entra **public client** application, tenant and delegated/RBAC cost permissions |
| AWS Bedrock | **F2** in the TUI or **AWS SSO** on the desktop runs the official AWS CLI SSO login; the URL and code appear in the dialog | AWS CLI v2.22+ and a previously configured SSO profile |
| GitHub Copilot, Kimi Coding, Grok CLI | Device code in the browser | Native login compatible with 9router; no need to register your own application |
| Kiro | AWS SSO OIDC with dynamic client registration and device code | Builder ID (default) or Identity Center; region and start URL are configurable, no AWS CLI profile required |
| ChatGPT/Codex, Claude, Gemini CLI | Browser with local callback and PKCE | Experimental native login; Codex requires local port 1455 to be free. It is not official provider support |
| Cline | Browser with local callback | Cline session received in the callback or exchanged for a token; no need to copy an access token |
| Qoder, CodeBuddy | Browser with state/device polling | International/CN region selectable before login |
| Zed | Browser with an ephemeral RSA key | Native protocol, not standard OAuth; `user_id` and `system_id` are filled in automatically |
| Xiaomi MiMo | Browser with an X25519/AES-GCM callback **or a code to paste** | Paste the current attempt's code into the hidden TUI field or the desktop field. It obtains an inference key; the weekly quota may need a separate cookie |
| Cursor | Explicit import of the local SQLite session | Sign in to Cursor, choose Tab → import and confirm. Nothing reads the token merely because the wizard was opened |
| Qwen Code | Device OAuth with PKCE + renewal | Legacy flow discontinued by the provider; the TUI shows the warning. The endpoint still issued a challenge in the smoke test, but that does not prove access or quota is granted |
| Antigravity | Google OAuth in the browser + renewal | IDE/Code Assist session, distinct from a Gemini API key; tokens in the vault, no project provisioning |
| OpenAI/Anthropic API, Windsurf, Ollama and OpenCode Go | Key/variable methods kept | There is no button that simulates a login that does not exist |

## Running a native login (9router) from the TUI or the desktop app

1. **In the TUI:** run `cargo run -p cuotai-tui`, press **a** to add or **e** to edit an account. Choose the provider and the account name and, at the **Credential** step, press **Tab**.
   **On the desktop:** click **Add account** or **Edit**, choose **Browser login** and click **Sign in**.
2. For a native login the interface does not ask for a `client_id`. Modes that require your own application registration (Gemini API, Vertex AI, Azure OpenAI) describe, in the mode's own descriptor (`cuotai_core::login`), which registration fields apply (`client_id`, `tenant` or the `client_secret` variable). The TUI and the desktop app only render that list, without knowing in advance which providers ask for what. Kiro offers `auth_method` (`builder-id` or `idc`), region and start URL; Qoder, CodeBuddy and Xiaomi offer a region.
3. On start, Cuotai opens the browser and, when needed, shows the URL and the device code to copy. Authorize on the provider's page.
4. When the authorization arrives, the quota parameters are filled in automatically. Save to write the tokens to the OS vault and the configuration to the TOML file.
5. To renew a revoked or expired authorization, edit the account and use **F2** (TUI) or **Redo login / Reauthenticate** (desktop). Reauthentication reuses the account's existing `credential_id` (a core decision, not a front-end one): the token is overwritten in the vault and no credential is left orphaned.

The native login automatically selects the right product (personal Copilot quota, Kimi Coding, Gemini CLI, Grok CLI and Qoder bearer), without replacing those sessions with commercial generation keys. Zed IDs, the Kiro profile, the Kimi device ID and the Codex account ID are derived from the response when available; non-secret quota metadata is filled in and refresh credentials stay only in the vault.

The compatible flows use the native client identifiers and protocols found in the 9router reference revision. The provider may refuse, change or restrict them. In particular, Anthropic's published policy restricts Claude.ai login in third-party applications; implementing the protocol **is not authorization or a guarantee of acceptance**. Passwords and consent are handled in the provider's browser, never by Cuotai.

**Important distinction:** `gemini_api` and `vertex_ai` (Cloud Monitoring), as well as Azure, keep the bring-your-own-application flows described below. `gemini_ai` with native login follows Gemini CLI/Code Assist.

### Kiro: method per authentication type

`auth_method` has a single descriptor in the core and each option declares where it applies. The native login offers **exactly** `builder-id` (default) and `idc`, the two methods accepted by the AWS SSO OIDC flow. The `KIRO_ACCESS_TOKEN` credential offers `google` (the explicit default for new accounts), `github`, `import`, `api_key` and `external_idp`; it does not offer `builder-id` or `idc`.

When you switch between credential and login, the core filters the list. If the previous value does not fit the new type, it selects the first applicable value and returns a localized warning; the TUI and the desktop app only display that contract. A direct or stale submission with an invalid combination is rejected with `edit_error.option_not_valid_for_auth_mode`.

The restriction applies only when editing and saving. Old configurations without `auth_method` (the implicit historical default `builder-id`) or with `credential_env` + `builder-id` keep loading and querying with no automatic migration.

## Provider-specific flows

### Personal Cursor: import through the wizard

1. Sign in normally to the **Cursor** application.
2. In Cuotai, add or edit Cursor and select the local import (Tab in the TUI, or the authentication mode on the desktop).
3. Leave the path empty for the default installation, or provide a specific `state.vscdb` (spaces in the path are accepted, for example Cursor Insiders).
4. Confirm reading and importing the Cursor session. The read is read-only, in a single SQLite transaction, of the `cursorAuth/accessToken`/`cursorAuth/token` keys and the machine ID; it does not modify the database and does not read every preference or secret.
5. Save the account. The token goes into the Keychain; the TOML stores only the session reference, an optional path and public metadata. The import does not authenticate again and does not renew the Cursor token; an expired token requires signing in to the editor and importing again.

A personal session uses `/api/usage-summary` on `cursor.com`, with a cookie derived from the account token. **It is never sent to the Admin API `/teams/spend`**. The `CURSOR_ADMIN_API_KEY` variable mode remains for Teams/Enterprise.

### Xiaomi MiMo: manual code

When the page shows **Authentication Code / Paste this into MiMo Code**, go back to Cuotai's pending login screen, paste the code into the matching field and submit. The paste accepts unpadded Base64URL and line breaks. An invalid code, or one from another session, shows an error without ending the attempt; the original expiry is not extended.

The code is encrypted to that attempt's ephemeral key. The automatic callback is still accepted in parallel. The typed code is never persisted; only the decoded token is written to the vault after the final confirmation.

**Authorization by code is not a quota session:** the code delivers an inference key (`sk-`), not console cookies. For the balance and the monthly Token Plan, paste the `platform.xiaomimimo.com` session (`api-platform_serviceToken` and `userId`) in the account wizard, which keeps it in the system vault, or set `params.platform_session_env` to the **name** of the variable holding that Cookie. The MiMo Desktop weekly quota is a separate session (`session_env`, `serviceToken`, with `region`), pasted or named the same way; if both are present, the Desktop mode takes precedence. A session in a variable can move into the system vault with explicit confirmation, after which the TOML keeps only `[accounts.session]` (see "Web sessions in the vault" in [providers.md](providers.md)).

### Qwen Code OAuth

Select **Qwen → OAuth login**. The flow uses the device grant with PKCE of the official Qwen Code client (`chat.qwen.ai/api/v1/oauth2/device/code` and `/token`, scope `openid profile email model.completion`). Access and refresh tokens go into the vault, separate from DashScope's `QWEN_API_KEY`.

**Provider limitation:** the [official documentation](https://github.com/QwenLM/qwen-code/blob/81582ca19d0772afdd06ace9cad4845c207001f8/docs/users/configuration/auth.md) states that the free OAuth was discontinued on 2026-04-15. The interface offers the attempt with a warning; no Qwen Code quota endpoint was found, and a successful login is not turned into a false consumption measurement.

### Antigravity

Authorize the Google session in the browser through the OAuth mode. The registration does not use the Gemini API key. Queries are read-only, with no generation and no `onboardUser`. Percentages and renewal appear on the cards and in the details; the 1,000 scale used by 9router is not presented as a request count. Missing quotas stay unknown and partial failures appear in the notes.

### Google Cloud Monitoring (Gemini API / Vertex AI): one-time setup

1. In [Google Cloud](https://console.cloud.google.com/apis/credentials), enable the **Cloud Monitoring API** on the queried project and configure the OAuth consent screen. For an external app in testing, add your account to the test users.
2. Create an **OAuth client ID → Desktop app** for Cuotai. Do not use another product's client ID.
3. In Cuotai, choose Gemini API or Vertex AI, select the OAuth mode and enter the `client_id`.
4. If your Google registration requires a `client_secret`, export it in a variable and enter **the name of that variable** in the next field. Secrets never enter the TOML.
5. Start the login. The browser requests `https://www.googleapis.com/auth/monitoring.read`; the return uses a free local port.
6. Fill in `project_id`, review and save. The Google identity needs `monitoring.timeSeries.list` (for example, Monitoring Viewer on the project).

### Azure OpenAI: one-time setup

1. In [Microsoft Entra](https://portal.azure.com), register your application. Under **Authentication → Advanced settings**, enable **Allow public client flows**.
2. Configure the delegated permission **Azure Service Management → user_impersonation** and grant the consent the tenant requires. The authorizing account needs **Cost Management Reader** on the queried resource or subscription.
3. In Cuotai, choose Azure OpenAI in OAuth mode and enter the **Application (client) ID** and the tenant (GUID/domain; empty uses `organizations`). No client secret is needed for this public client.
4. Confirm the login and type the code shown on the Microsoft page that Cuotai opened (`https://management.azure.com/user_impersonation` and `offline_access`).
5. Enter `subscription_id` and `resource_id`, review and save.

### AWS Bedrock: SSO profile

Prepare once with `aws configure sso`. When configuring AWS Bedrock, enter the profile and trigger the SSO login (`F2` in the TUI, or the **AWS SSO** mode on the desktop). Cuotai shows the URL and code provided by `aws sso login --profile ... --use-device-code --no-browser`. The AWS CLI itself maintains and uses its SSO cache; Cuotai does not copy those tokens to the Keychain. SSO login does not replace `ce:GetCostAndUsage` and does not remove the charge for Cost Explorer queries.

## Persistence, cancellation and renewal

- **System vault:** by default, the built-in logins store the access token, the refresh token and the secret context in the native vault, service **`cuotai.oauth`** (Keychain on macOS, Windows Credential Manager and Linux Secret Service). Without authorized access there is an explicit error; there is never an automatic fallback to SQLite.
- **Format in the TOML:** the TOML stores `accounts.oauth.credential_id`, client ID/tenant/client-secret reference when applicable, and non-secret `options` (`region`, `auth_method`, `start_url`). `oauth` and `credential_env` cannot coexist in the same account.
- **Staged commit:** tokens from a new login stay in memory until the account is finally saved. The core writes to the vault first and to the TOML afterwards; if the file write fails, it restores the previous credential or removes the new one. If saving the TOML succeeds and the vault key changed (for example, when changing the Kiro method or region, the `client_id` or the tenant while keeping the same `credential_id`), the old key is removed **only after** the save, unless another account in the saved file still references it. A failure in that cleanup is reported as a typed non-fatal result (configuration and new credential saved; previous credential kept in the vault), shown truthfully in the TUI and the desktop app, never as a fictitious rollback.
- **Automatic renewal:** before querying, tokens with less than 60 seconds of validity are renewed when a refresh token exists and the protocol supports it (Claude, Codex, Google, Kimi, Kiro, Grok, CodeBuddy, Cline and GitHub where applicable). A rotation that has already started finishes persisting even if the query is cancelled, so the new refresh token is never lost.

## Web sessions (PRD 0016)

Xiaomi MiMo, Alibaba Coding Plan and Alibaba Token Plan query with a web session you paste yourself (an environment variable named by `session_env`/`platform_session_env` stays as an alternative); Cuotai never imports cookies from a browser. Their adapters read either one through the same session descriptor, and a session lacking the descriptor's required cookies is refused before any request, naming only those cookies. The full contract is in "Web sessions in the vault" in [providers.md](providers.md); in short:

- **Flow:** sign in to the provider's site in your browser, copy the `Cookie` header, the request as cURL or the inspector's cookie table, and paste it into the TUI wizard's **Colar sessão** step or the desktop dialog's **Web session** box. Both first show the `UserSession` level and the warning "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service." F4 / **Test session** runs one read-only query; saving stores the session.
- **What is stored:** only the cookies the provider's session slot declares, joined into one `Cookie` value, plus the expiry date when the pasted table showed one. They go to the same Cuotai vault entry type as OAuth tokens (Keychain, Credential Manager or Secret Service; the development SQLite store below keeps its plaintext warning), under a key bound to the account's random `credential_ref`, the provider and the slot. The TOML keeps only `[accounts.session]` (the reference and the slot names).
- **What is discarded:** every other cookie, header, URL and line of the paste, before anything is written; the paste field is cleared when the wizard or dialog closes, and errors name cookies, never values.
- **Forgetting:** **Forget this session** (TUI summary: F5 twice; desktop: the button, then its confirmation) deletes the vault entry and the reference. Since 0.77.1, removing the account or switching it to another provider deletes its session entry too, put back when the TOML save fails; a vault that refuses the delete leaves the account in place.
- **Risks:** a session is a full login: anyone holding it can act as you on that site until it expires or is revoked. Only signing out on the site (or changing the password) revokes it for the provider; forgetting removes Cuotai's copy only. The console interface is internal and may be outside the provider's terms of use for third-party tools; the decision to use it is yours.

## SQLite credentials: development only

To avoid Keychain prompts on every rebuild during local development:

```sh
cargo run -p cuotai-tui -- --dev-credentials-sqlite ./cuotai-dev.sqlite3
```

The database stores **unencrypted tokens**, including refresh tokens and the secret context. Use it only in a trusted local environment. On Unix, new files get mode `0600`; existing files accessible to group/others and symbolic links are refused. `release` builds refuse the option before creating or opening the database (the SQLite implementation is not even compiled without `debug_assertions`).
