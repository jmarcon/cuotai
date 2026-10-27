// PRD 0047 G1: environment variables and `params.*` names are terminal-only. Core texts that name them keep that
// wording for the TUI and CLI; the desktop swaps each one, by its stable id, for its own plain wording (a key of
// `ui/locales/*.json`). `action: "session"` marks a text whose fix is a web session the account dialog can take.
// scripts/test-desktop-wording-ui.mjs checks that every core text naming a variable or a parameter is listed here.

const w = (id, fallback, action = null) => ({ id, fallback, action });

const KEY_MISSING = w("card.keyMissing", "No key saved for this account. Open Edit to add it.");
const SESSION_NEEDED = w("coreText.sessionNeeded", "Connect this account's web session to see usage.", "session");
const SESSION_EXPIRED = w("coreText.sessionExpired", "The account's web session expired. Connect a new one to see usage.", "session");
const SETTING_INVALID = w("coreText.settingInvalid", "A setting of this account is not valid. Open Edit to fix it.");
const SETTING_MISSING = w("coreText.settingMissing", "This account needs one more setting to show usage. Open Edit to add it.");
const LOGIN_SECRET = w("coreText.loginSecretMissing", "The sign-in setup of this account is incomplete. Check it in Edit.");
const KEY_REQUIRED = w("coreText.keyRequired", "Type the key for this account.");

/** Core id → the desktop's own wording (and, when it has one, the fix the card offers). */
export const DESKTOP_WORDING = {
  // Missing keys (the core names the variable to export).
  "query.missing_credential": KEY_MISSING,
  "query.router_keys.kimi_missing_credential": KEY_MISSING,
  "query.alibaba_billing.missing_secret": KEY_MISSING,
  "query.alibaba_billing.missing_security_token": KEY_MISSING,
  "query.inference.missing_endpoint_credential.hint_default": KEY_MISSING,
  "query.inference.missing_endpoint_credential.hint_override": KEY_MISSING,
  // A web session is missing or does not read the quota.
  "query.router_keys.xiaomi_native_key_has_no_quota": w("coreText.xiaomiNativeKey",
    "This key does not show usage. To see the quota, connect your Xiaomi account session.", "session"),
  "query.router_keys.xiaomi_aistudio_session_required": SESSION_NEEDED,
  "query.alibaba_plans.api_key_requires_session": SESSION_NEEDED,
  "query.alibaba_plans.token_plan_auth_method_required": SESSION_NEEDED,
  "query.router_existing.missing_gemini_session": SESSION_NEEDED,
  "query.router_grok.missing_oauth_token": SESSION_NEEDED,
  "query.session.variable_missing": SESSION_NEEDED,
  "query.session.missing_cookies_env": SESSION_NEEDED,
  // The web session expired.
  "query.router_keys.xiaomi_session_expired": SESSION_EXPIRED,
  "query.alibaba_plans.console_login_required": SESSION_EXPIRED,
  "query.alibaba_plans.qwen_cloud_missing_sec_token": SESSION_EXPIRED,
  "query.alibaba_plans.qwen_cloud_session_invalid": SESSION_EXPIRED,
  "query.alibaba_plans.token_plan_session_expired": SESSION_EXPIRED,
  "query.alibaba_plans.session_http_auth": SESSION_EXPIRED,
  "query.alibaba_plans.cli_auth": SESSION_EXPIRED,
  "query.alibaba_plans.cli_not_found": SESSION_NEEDED,
  // A `params.*` value is invalid.
  "query.alibaba_billing.billing_cycle_month_out_of_range": SETTING_INVALID,
  "query.alibaba_billing.invalid_billing_cycle_format": SETTING_INVALID,
  "query.alibaba_billing.invalid_billing_cycle_month": SETTING_INVALID,
  "query.alibaba_billing.invalid_region": SETTING_INVALID,
  "query.alibaba_plans.invalid_region": SETTING_INVALID,
  "query.alibaba_plans.invalid_cli_profile": SETTING_INVALID,
  "query.alibaba_plans.cli_failure": SETTING_INVALID,
  "query.cloud.invalid_azure_resource_id": SETTING_INVALID,
  "query.cloud.invalid_resource_identifier": SETTING_INVALID,
  "query.coding.invalid_organization_id": SETTING_INVALID,
  "query.inference.invalid_path_identifier": SETTING_INVALID,
  "query.router_accounts.invalid_qoder_credential_type": SETTING_INVALID,
  "query.router_accounts.invalid_qoder_region": SETTING_INVALID,
  "query.router_keys.xiaomi_invalid_region": SETTING_INVALID,
  "query.router_sessions.codebuddy_region": SETTING_INVALID,
  "query.router_sessions.kiro_auth_method": SETTING_INVALID,
  "query.router_sessions.kiro_profile_arn": SETTING_INVALID,
  "query.router_sessions.kiro_profile_region": SETTING_INVALID,
  "query.subscriptions.copilot_billing": SETTING_INVALID,
  "query.subscriptions.copilot_mode": SETTING_INVALID,
  "query.eleven_labs.region": SETTING_INVALID,
  "query.open_router.params": SETTING_INVALID,
  "edit_error.param.resource_id_chars": SETTING_INVALID,
  "edit_error.config_region": SETTING_INVALID,
  "edit_error.config_open_router": SETTING_INVALID,
  "edit_error.config_base_url": SETTING_INVALID,
  "edit_error.env_invalid": KEY_REQUIRED,
  "edit_error.env_required": KEY_REQUIRED,
  "edit_error.param.env_name": KEY_REQUIRED,
  // A `params.*` value is missing.
  "query.missing_param": SETTING_MISSING,
  "query.inference.grok_team_id_required": SETTING_MISSING,
  "query.router_existing.missing_gemini_project": SETTING_MISSING,
  "query.inference.perplexity_computer_opt_in_required": SETTING_MISSING,
  "query.router_keys.kimi_coding_opt_in_required": SETTING_MISSING,
  // Informational notes.
  "note.alibaba_plans.api_key_query": w("coreText.queriedWithKey", "Quota read with the account's API key."),
  "note.alibaba_plans.session_query": w("coreText.queriedWithSession", "Quota read with the account's web session."),
  "note.alibaba_plans.token_plan_session_query": w("coreText.queriedWithSession", "Quota read with the account's web session."),
  "note.antigravity.project_override": w("coreText.projectOverride", "The project set for this account replaced the discovered one."),
  "note.alibaba_billing.bill_not_requested": w("coreText.optionalSetting", "More detail is available with one more setting in Edit."),
  "note.cloud.azure_resource_attribution": w("coreText.optionalSetting", "More detail is available with one more setting in Edit."),
  "note.inference.perplexity_computer_opt_in": w("coreText.perplexityComputer", "Showing Perplexity Computer usage only; Sonar has no usage endpoint."),
  "note.eleven_labs.key_rejected": w("coreText.keyRejected", "The provider rejected the key. Check or replace it in Edit."),
  "query.cloud.aws_auth": w("coreText.awsAuth", "AWS sign-in failed for the account's profile. Check the profile's credentials."),
  // Web sessions.
  "user_session.warning_env": w("coreText.sessionWarning", "This is a full sign-in session, used only to query this service. Keep it in the system vault."),
  "access.credential_hint.user_session": w("coreText.userSessionHint", "Needs a web session pasted in the account dialog; it expires and is never imported."),
  "user_session.migrate.consent": w("coreText.migrateConsent", "{label}: copy this session to the system vault. It is sent only to {hosts}."),
  "user_session.migrate.done": w("coreText.migrateDone", "Session moved to the system vault."),
  "user_session.migrate.env_missing": w("coreText.migrateUnavailable", "The session set up in the terminal is not available to the app."),
  "user_session.migrate.nothing": w("coreText.migrateNothing", "This account has no session to move."),
  "secret_input.state.environment": w("dialog.secret.terminal", "Key set up in the terminal"),
  // Sign-in.
  "oauth_error.config.client_secret_name_only": LOGIN_SECRET,
  "oauth_error.google.client_secret_env_empty": LOGIN_SECRET,
  "oauth_error.google.client_secret_env_required": LOGIN_SECRET,
  "oauth_error.google.client_secret_env_unset": LOGIN_SECRET,
  "oauth_error.session.client_secret_env_missing": LOGIN_SECRET,
  "edit_error.config_oauth": LOGIN_SECRET,
  "edit_error.oauth_required": w("coreText.loginRequired", "Sign in before saving."),
  "login.xiaomi_mimo.oauth.completion_notice": w("coreText.xiaomiLoginDone", "MiMo key authorized. To see the quota, connect the account's web session."),
  "login.xiaomi_mimo.oauth.help": w("coreText.xiaomiLoginHelp", "The code authorizes an inference key only; the quota needs the account's web session."),
  "login.claude.oauth.help": w("coreText.claudeLoginHelp", "Experimental sign-in; if the provider refuses it, use an API key. No password is typed here."),
  "login.alibaba_token_plan.bailian_console.help": w("coreText.bailianLoginHelp", "Signs in with the bl tool in its own profile; the sign-in stays in bl, outside Cuotai."),
  "login.progress.bailian_profile": w("coreText.bailianProfile", "The Token Plan quota will be read with this bl profile."),
  "models.source.aws_bedrock": w("coreText.bedrockModels", "Models listed by the AWS CLI for the account's profile; this does not prove model access."),
};

/** The desktop's wording for a core `Localized`, or the value itself when the core text is already GUI-safe. */
export const desktopWording = (value) => {
  const entry = value && typeof value === "object" ? DESKTOP_WORDING[value.id] : null;
  return entry ? { id: entry.id, params: value.params ?? {}, fallback: entry.fallback } : value;
};

/** The fix a core text asks for in the desktop (`"session"`), or null. */
export const desktopAction = (value) => (value && DESKTOP_WORDING[value.id]?.action) || null;
