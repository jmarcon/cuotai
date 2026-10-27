//! PRD 0016 RF1 — the `UserSession` credential: a web session the user
//! supplies for a provider that has no other route to its usage.
//!
//! * Each provider declares its session slots in [`descriptors`]: the
//!   cookie names it accepts, the hosts the session may be sent to and how
//!   it is sent. Front ends render the descriptor and never branch on the
//!   provider. A slot is keyed by the legacy parameter it supersedes
//!   (`session_env`, `platform_session_env`), so an adapter reads a vault
//!   session exactly as it reads one from an environment variable.
//! * The TOML holds only a reference, `[accounts.session]` with a random
//!   `credential_ref` and the stored `slots`; the value lives in a
//!   Cuotai-owned vault entry written through the same [`TokenStore`]
//!   (Keychain, Credential Manager, Secret Service, or the debug-only
//!   SQLite store) as OAuth tokens. The entry key binds the reference to the
//!   provider and slot, so a reference copied to another provider or slot
//!   finds nothing.
//! * [`stage_migration`] copies a `session_env`-style variable into the
//!   vault on explicit request and drops the reference to the variable;
//!   accounts that keep `session_env` work as before.
//! * RF2: [`reduce`] turns a paste (a `Cookie:` line, a `name=value` list,
//!   an inspector cookie table or a copied `cURL`) into the cookies the
//!   descriptor accepts, reassembling numbered fragments and discarding the
//!   rest; [`stage_pasted`] stores it, replacing the value in the same
//!   vault entry when the account already has one (RF3.4), and
//!   [`crate::Monitor::test_session`] queries with it before it is saved.
//! * RF3: a session only travels over HTTPS to its declared hosts and is
//!   never carried by a redirect (`may_receive`); a rejected vault or
//!   pasted session reads "session expired or revoked"; a known expiry
//!   (only the date) reaches [`crate::Report::session_expires_at`], the
//!   card label of [`expiry`] and the alerts.
//! * RF4: [`notice`] and [`paste_notice`] give the card and the paste step
//!   the `UserSession` level with a short risk warning; [`stage_forget`]
//!   deletes an account's vault entries and drops its reference, and
//!   [`stage_dropped`] deletes the entries an edit leaves unreferenced (an
//!   account removed, a provider switched), undone when the save fails.
//! * RF5: the Xiaomi MiMo and Alibaba Coding/Token Plan adapters read each
//!   slot through its descriptor (`providers::web_session`), from the vault
//!   or from the `session_env`-style variable kept as an alternative, and
//!   refuse a session without its required cookies before any request.
//! * SES-01: [`offers_new_paste`] tells the cards and details when to offer
//!   "Paste a new session". SES-02: [`stage_pasted`] writes only with the
//!   [`SaveClearance`] of the test before save ([`clear_for_save`],
//!   [`test_before_save`]): a passed test query, or an explicit "Save
//!   anyway" after a transport failure, never after a rejection.
//!
//! Session values never appear in the TOML, `Debug`, errors or reports.
//!
//! [`TokenStore`]: crate::oauth::TokenStore

use crate::oauth::{OAuthSession, OAuthToken};
use crate::text::{params, Localized, ParamValue};
use crate::{Account, Config, Credentials, Provider};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// How the session travels to its hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// As the request's `Cookie` header (`name=value; name=value`).
    CookieHeader,
    /// PRD 0036 RF2.1/RF10.1: a password or API key the adapter sends its
    /// own way (a login body, a bearer header); the paste is the value
    /// itself, not a cookie list.
    Secret,
}

/// PRD 0036 RF2.1: the slot of a vault-stored password or key. It
/// supersedes the account's `credential_env` (not a `params` entry).
pub const CREDENTIAL_SLOT: &str = "credential_env";

/// PRD 0036 RF2.6: the slot of a pasted 9router SSO `auth_token` session.
pub const NINE_ROUTER_SSO_SLOT: &str = "auth_session_env";

/// Longest secret value accepted from a paste.
const MAX_SECRET_LEN: usize = 4096;

/// One session a provider accepts, declared by the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionDescriptor {
    /// The account parameter this session supersedes; also the slot id in
    /// `[accounts.session].slots`.
    pub slot: &'static str,
    /// Cookie names the session must carry.
    pub required: &'static [&'static str],
    /// Further cookie names the provider reads; anything else is not part
    /// of the session.
    pub optional: &'static [&'static str],
    /// The only hosts that may receive the session, all over HTTPS.
    pub hosts: &'static [&'static str],
    /// PRD 0036 RF2.6: `hosts` is empty and the value travels only to the
    /// account's own validated `base_url` origin (a user-defined host; HTTP
    /// only where the base URL rules allow it: loopback, or the explicit
    /// private-network opt-in). The adapter builds every request from that
    /// origin, so no other host can receive it.
    pub account_host: bool,
    pub delivery: Delivery,
    /// The expiry is read from the JWT `exp` of the required cookie, not
    /// verified (PRD 0036 RF2.6).
    #[serde(skip)]
    jwt_expiry: bool,
    /// Where the user signs in to copy the session, for [`Self::instructions`].
    pub site: &'static str,
    /// PRD 0016 RF2.1: the HTTPS page where the user signs in and sees the
    /// usage, opened by the desktop "Open {site}" link; `None` for an app or
    /// a self-hosted address.
    pub open_url: Option<&'static str>,
    /// `user_session.slot.*` id and English fallback of [`Self::label`].
    #[serde(skip)]
    label: (&'static str, &'static str),
}

impl SessionDescriptor {
    /// What this session is, for pickers and consent texts.
    pub fn label(&self) -> Localized {
        Localized::fixed(self.label.0, self.label.1)
    }

    /// RF2.1: where to copy the session from, generic for every account
    /// (no capture of a real one).
    pub fn instructions(&self) -> Localized {
        let site = self.site;
        // The Xiaomi account cookie is issued to the MiMo Desktop app, not to
        // a web page (`xiaomimimo.com` itself does not resolve).
        if self.label.0 == "user_session.slot.xiaomi_mimo.session_env" {
            return Localized::fixed(
                "user_session.instructions.xiaomi_account",
                "This cookie comes from the MiMo Desktop app, not from a web page. With MiMo Desktop signed in, capture one of its requests to mimo-server-<region>.xiaomimimo.com (for example with a debugging proxy) and copy its Cookie header; it must contain serviceToken. Only this session's cookies are kept; the rest of the pasted text is discarded. With just a browser, use the platform.xiaomimimo.com session instead.",
            );
        }
        if self.delivery == Delivery::Secret {
            return Localized::with_param(
                "user_session.instructions.secret",
                "site",
                site,
                "Paste the {site} value. It stays only in the system vault and is sent only to this account's address; the TOML keeps just the reference.",
            );
        }
        Localized::with_param(
            "user_session.instructions",
            "site",
            site,
            "Sign in to {site} in your browser. In the developer tools (F12), Network tab, reload the page, pick a request to {site} and copy its Cookie header or use \"Copy as cURL\"; the cookie table of the Application tab works too. Only this session's cookies are kept; the rest of the pasted text is discarded.",
        )
    }
}

/// Alibaba console login cookies the Coding Plan and Token Plan adapters
/// read (CSRF, anonymous id and `sec_token`) plus the Aliyun login family.
/// CodexBar's `AlibabaCodingPlanCookieImporter.swift` treats
/// `login_aliyunid_ticket` with one of `login_aliyunid_pk`,
/// `login_current_pk` or `login_aliyunid` as a signed-in console, but sends
/// the whole cookie jar of the console domains; none is required here until
/// a live console session confirms that this reduced set is enough.
const ALIBABA_COOKIES: &[&str] = &[
    "login_aliyunid_ticket",
    "login_aliyunid",
    "login_aliyunid_pk",
    "login_current_pk",
    "login_aliyunid_csrf",
    "csrf",
    "cna",
    "sec_token",
];

const XIAOMI_ACCOUNT: SessionDescriptor = SessionDescriptor {
    slot: "session_env",
    // 9router `open-sse/shared/mimoAccount.js` keeps exactly these cookies
    // of the mimo-server session (`serviceToken`, `userId`, `mimo<region>_ph`
    // and `_slh`).
    required: &["serviceToken"],
    optional: &[
        "userId",
        "mimocn_ph",
        "mimocn_slh",
        "mimosgp_ph",
        "mimosgp_slh",
        "mimoams_ph",
        "mimoams_slh",
        "mimoru_ph",
        "mimoru_slh",
        "mimoin_ph",
        "mimoin_slh",
    ],
    hosts: &[
        "mimo-server-cn.xiaomimimo.com",
        "mimo-server-sgp.xiaomimimo.com",
        "mimo-server-ams.xiaomimimo.com",
        "mimo-server-ru.xiaomimimo.com",
        "mimo-server-in.xiaomimimo.com",
    ],
    account_host: false,
    delivery: Delivery::CookieHeader,
    jwt_expiry: false,
    site: "MiMo Desktop",
    open_url: None,
    label: (
        "user_session.slot.xiaomi_mimo.session_env",
        "Xiaomi account session (MiMo Desktop weekly quota)",
    ),
};

const XIAOMI_PLATFORM: SessionDescriptor = SessionDescriptor {
    slot: "platform_session_env",
    required: &["api-platform_serviceToken", "userId"],
    optional: &[],
    hosts: &["platform.xiaomimimo.com"],
    account_host: false,
    delivery: Delivery::CookieHeader,
    jwt_expiry: false,
    site: "platform.xiaomimimo.com",
    open_url: Some("https://platform.xiaomimimo.com/console/plan-manage"),
    label: (
        "user_session.slot.xiaomi_mimo.platform_session_env",
        "platform.xiaomimimo.com web session (balance and Token Plan)",
    ),
};

const ALIBABA_CODING_PLAN: SessionDescriptor = SessionDescriptor {
    slot: "session_env",
    required: &[],
    optional: ALIBABA_COOKIES,
    hosts: &[
        "bailian.console.aliyun.com",
        "bailian-cs.console.aliyun.com",
        "modelstudio.console.alibabacloud.com",
        "bailian-singapore-cs.alibabacloud.com",
    ],
    account_host: false,
    delivery: Delivery::CookieHeader,
    jwt_expiry: false,
    site: "bailian.console.aliyun.com / modelstudio.console.alibabacloud.com",
    open_url: Some("https://bailian.console.aliyun.com/cn-beijing/?tab=model#/efm/coding_plan"),
    label: (
        "user_session.slot.alibaba_coding_plan.session_env",
        "Alibaba console session (Coding Plan)",
    ),
};

const ALIBABA_TOKEN_PLAN: SessionDescriptor = SessionDescriptor {
    slot: "session_env",
    required: &[],
    optional: ALIBABA_COOKIES,
    hosts: &["home.qwencloud.com", "cs-data.qwencloud.com"],
    account_host: false,
    delivery: Delivery::CookieHeader,
    jwt_expiry: false,
    site: "home.qwencloud.com",
    open_url: Some("https://home.qwencloud.com/billing/subscription/token-plan-individual"),
    label: (
        "user_session.slot.alibaba_token_plan.session_env",
        "Qwen Cloud console session (Token Plan)",
    ),
};

const NINE_ROUTER_PASSWORD: SessionDescriptor = SessionDescriptor {
    slot: CREDENTIAL_SLOT,
    required: &[],
    optional: &[],
    hosts: &[],
    account_host: true,
    delivery: Delivery::Secret,
    jwt_expiry: false,
    site: "9router",
    open_url: None,
    label: (
        "user_session.slot.nine_router.credential_env",
        "9router dashboard password (kept in the system vault)",
    ),
};

const NINE_ROUTER_SSO: SessionDescriptor = SessionDescriptor {
    slot: NINE_ROUTER_SSO_SLOT,
    // 9router's dashboard keeps its login in this one cookie.
    required: &["auth_token"],
    optional: &[],
    hosts: &[],
    account_host: true,
    delivery: Delivery::CookieHeader,
    jwt_expiry: true,
    site: "9router",
    open_url: None,
    label: (
        "user_session.slot.nine_router.auth_session_env",
        "9router dashboard session (auth_token cookie, for SSO-only instances)",
    ),
};

const LITELLM_KEY: SessionDescriptor = SessionDescriptor {
    slot: CREDENTIAL_SLOT,
    required: &[],
    optional: &[],
    hosts: &[],
    account_host: true,
    delivery: Delivery::Secret,
    jwt_expiry: false,
    site: "LiteLLM",
    open_url: None,
    label: (
        "user_session.slot.litellm.credential_env",
        "LiteLLM master or virtual key (kept in the system vault)",
    ),
};

/// PRD 0016 RF2.1: whether `url` is the sign-in page of a declared
/// session, so a shell may open it in the browser.
pub fn is_open_url(url: &str) -> bool {
    Provider::ALL
        .iter()
        .flat_map(|p| descriptors(*p))
        .any(|d| d.open_url == Some(url))
}

/// The sessions `provider` accepts; empty when it has none.
pub fn descriptors(provider: Provider) -> &'static [SessionDescriptor] {
    match provider {
        Provider::NineRouter => &[NINE_ROUTER_PASSWORD, NINE_ROUTER_SSO],
        Provider::LiteLlm => &[LITELLM_KEY],
        Provider::XiaomiMimo => &[XIAOMI_ACCOUNT, XIAOMI_PLATFORM],
        Provider::AlibabaCodingPlan => &[ALIBABA_CODING_PLAN],
        Provider::AlibabaTokenPlan => &[ALIBABA_TOKEN_PLAN],
        _ => &[],
    }
}

/// The descriptor of `slot` for `provider`.
pub fn descriptor(provider: Provider, slot: &str) -> Option<&'static SessionDescriptor> {
    descriptors(provider).iter().find(|d| d.slot == slot)
}

/// PRD 0047: the vault descriptor of `slot` for `provider`: the web
/// session or password the provider declares in [`descriptors`], else the
/// API key, token or secret of [`crate::secret_input::secret_inputs`]
/// (delivered by the adapter itself, [`Delivery::Secret`]). `None` for a
/// slot the provider cannot keep in the vault.
pub fn vault_descriptor(provider: Provider, slot: &str) -> Option<SessionDescriptor> {
    if let Some(declared) = descriptor(provider, slot) {
        return Some(*declared);
    }
    let input = crate::secret_input::secret_input_slot(provider, slot)?;
    Some(SessionDescriptor {
        slot: input,
        required: &[],
        optional: &[],
        hosts: &[],
        account_host: false,
        delivery: Delivery::Secret,
        jwt_expiry: false,
        site: "",
        open_url: None,
        label: (
            "user_session.slot.secret",
            "Secret kept in the system vault",
        ),
    })
}

/// Prefix of the name a vault session slot is read under: the query puts
/// it in the slot's parameter in place of a variable name.
pub(crate) const VAULT_NAME: &str = "__CUOTAI_USER_SESSION:";

/// RF5.1: the required cookies of `slot` that `cookie` (a `Cookie` header
/// value, from the vault or a variable) lacks or carries empty, in the
/// descriptor's order. Names only; the value is never copied.
pub(crate) fn missing_required(provider: Provider, slot: &str, cookie: &str) -> Vec<&'static str> {
    let Some(descriptor) = descriptor(provider, slot) else {
        return Vec::new();
    };
    descriptor
        .required
        .iter()
        .copied()
        .filter(|name| {
            !cookie.split(';').any(|item| {
                item.split_once('=')
                    .is_some_and(|(key, value)| key.trim() == *name && !value.trim().is_empty())
            })
        })
        .collect()
}

/// RF3.3: whether a request to `url` may carry the session of `slot`: over
/// HTTPS to one of the hosts its descriptor declares, nothing else. The
/// crate's own contract tests also reach their loopback `wiremock`
/// servers, which stand in for the declared hosts.
pub(crate) fn may_receive(provider: Provider, slot: &str, url: &url::Url) -> bool {
    let declared = descriptor(provider, slot).is_some_and(|descriptor| {
        !descriptor.account_host
            && url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                descriptor
                    .hosts
                    .iter()
                    .any(|h| h.eq_ignore_ascii_case(host))
            })
    });
    declared
        || (cfg!(test)
            && descriptor(provider, slot).is_some_and(|d| !d.account_host)
            && url.scheme() == "http"
            && crate::transport::is_secure(url))
}

/// `[accounts.session]`: where an account's sessions live in the vault.
/// Never holds a session value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    /// Random UUID generated by Cuotai; unique per account.
    pub credential_ref: String,
    /// Slots of [`descriptors`] stored under the reference.
    pub slots: BTreeSet<String>,
}

impl SessionConfig {
    /// A fresh reference with no slot yet.
    fn fresh() -> Self {
        Self {
            credential_ref: uuid::Uuid::new_v4().to_string(),
            slots: BTreeSet::new(),
        }
    }

    /// A UUID reference and at least one slot, each declared for `provider`.
    pub(crate) fn is_valid(&self, provider: Provider) -> bool {
        uuid::Uuid::parse_str(&self.credential_ref).is_ok()
            && !self.slots.is_empty()
            && self
                .slots
                .iter()
                .all(|slot| vault_descriptor(provider, slot).is_some())
    }
}

/// Vault key of one slot: bound to the reference, the provider and the slot.
/// PRD 0029 RF6: the vault keys `account`'s web sessions are stored under.
pub(crate) fn account_session_keys(account: &crate::Account) -> Vec<String> {
    account.session.as_ref().map_or_else(Vec::new, |session| {
        session
            .slots
            .iter()
            .filter_map(|slot| storage_key(account.provider, session, slot))
            .collect()
    })
}

fn storage_key(provider: Provider, config: &SessionConfig, slot: &str) -> Option<String> {
    let namespace = uuid::Uuid::parse_str(&config.credential_ref).ok()?;
    let identity = serde_json::to_vec(&("user_session", provider, slot)).ok()?;
    Some(uuid::Uuid::new_v5(&namespace, &identity).to_string())
}

/// The vault payload: the session is the token and its known expiry (a
/// date only); nothing else is stored.
fn token(value: String, expires_at: Option<DateTime<Utc>>) -> OAuthToken {
    OAuthToken {
        access_token: value,
        refresh_token: None,
        expires_at,
        metadata: BTreeMap::new(),
    }
}

pub(crate) fn not_found() -> Localized {
    Localized::fixed(
        "user_session.not_found",
        "Session not found in the system vault; enter the session again in the account wizard.",
    )
}

fn invalid_reference() -> Localized {
    Localized::fixed(
        "user_session.invalid_reference",
        "Invalid session reference in [accounts.session]; enter the session again in the account wizard.",
    )
}

/// RF3.1: what a query reports when the provider rejected a vault session.
pub(crate) fn rejected() -> Localized {
    Localized::fixed(
        "user_session.rejected",
        "Session expired or revoked; paste a new one in the account wizard.",
    )
}

/// One session handed to a query: never `Debug`-printed with its value.
pub(crate) struct QuerySession {
    pub slot: &'static str,
    pub value: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl Drop for QuerySession {
    fn drop(&mut self) {
        wipe(&mut self.value);
    }
}

/// Session values of `account`, by slot, read from the vault for a query.
/// The values never leave the query's credentials.
pub(crate) async fn load(
    oauth: &OAuthSession,
    account: &Account,
) -> Result<Vec<QuerySession>, crate::providers::QueryError> {
    let Some(config) = &account.session else {
        return Ok(Vec::new());
    };
    let mut sessions = Vec::with_capacity(config.slots.len());
    for slot in &config.slots {
        let (Some(descriptor), Some(key)) = (
            vault_descriptor(account.provider, slot),
            storage_key(account.provider, config, slot),
        ) else {
            return Err(crate::providers::QueryError {
                diagnostic: None,
                status: crate::Status::NeedsConfiguration,
                note: invalid_reference(),
            });
        };
        let stored =
            oauth
                .load_by_key(&key)
                .await
                .map_err(|error| crate::providers::QueryError {
                    diagnostic: None,
                    status: crate::Status::Error,
                    note: error.l10n(),
                })?;
        match stored {
            Some(token) if !token.access_token.trim().is_empty() => {
                sessions.push(QuerySession {
                    slot: descriptor.slot,
                    expires_at: token.expires_at,
                    value: token.access_token,
                });
            }
            _ => {
                return Err(crate::providers::QueryError {
                    diagnostic: None,
                    status: crate::Status::NeedsConfiguration,
                    note: not_found(),
                })
            }
        }
    }
    Ok(sessions)
}

/// A `session_env`-style variable of a saved account that can move to the
/// vault. Holds the variable's name, never its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationCandidate {
    pub slot: &'static str,
    /// Name of the environment variable the account references today.
    pub env: String,
    pub label: Localized,
    pub hosts: &'static [&'static str],
    /// What moving this session to the vault does, shown before the user
    /// confirms it (RF1.2's explicit consent).
    pub consent: Localized,
}

/// Shown once a migration was saved.
pub fn migrated_notice() -> Localized {
    Localized::fixed(
        "user_session.migrate.done",
        "Session moved to the system vault; the TOML keeps only the reference. Remove the variable from your environment whenever you like.",
    )
}

/// The variable name `slot` of `account` reads today: `credential_env` for
/// [`CREDENTIAL_SLOT`], `params.<slot>` otherwise.
pub(crate) fn slot_env<'a>(account: &'a Account, slot: &str) -> Option<&'a str> {
    if slot == CREDENTIAL_SLOT {
        account.credential_env.as_deref()
    } else {
        account.params.get(slot).map(String::as_str)
    }
}

/// The hosts `descriptor` may send to, for consent texts: the declared
/// ones, or the host of the account's own base URL.
fn hosts_text(account: &Account, descriptor: &SessionDescriptor) -> String {
    if !descriptor.account_host {
        return descriptor.hosts.join(", ");
    }
    let base = match account.provider {
        Provider::LiteLlm => crate::providers::litellm::account_base(account).ok(),
        _ => crate::providers::nine_router::account_base(account).ok(),
    };
    base.and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The sessions of `account` still read from an environment variable that
/// [`stage_migration`] would move to the vault.
pub fn migration_candidates(account: &Account) -> Vec<MigrationCandidate> {
    descriptors(account.provider)
        .iter()
        .filter_map(|descriptor| {
            let env = slot_env(account, descriptor.slot)?.trim();
            if env.is_empty() {
                return None;
            }
            let label = descriptor.label();
            let hosts = hosts_text(account, descriptor);
            let consent = Localized::with_params(
                "user_session.migrate.consent",
                params([
                    ("label", ParamValue::from(label.clone())),
                    ("env", ParamValue::from(env.to_string())),
                    ("hosts", ParamValue::from(hosts.clone())),
                ]),
                "{label}: copy the value of {env} into the system vault and remove {env} from the TOML. The session is only sent to {hosts}; the variable stays in your environment until you remove it.",
            );
            Some(MigrationCandidate {
                slot: descriptor.slot,
                env: env.to_string(),
                label,
                hosts: descriptor.hosts,
                consent,
            })
        })
        .collect()
}

/// The vault half of a session change: the sessions were written, the TOML
/// not yet. Persist [`StagedSession::config`] next, or undo the vault writes
/// with [`rollback`] when that fails.
pub struct StagedSession {
    config: Config,
    /// Each written vault key with the session it replaced, if any.
    written: Vec<(String, Option<OAuthToken>)>,
}

impl StagedSession {
    /// PRD 0029 RF5.3: how many vault entries this stage wrote or deleted;
    /// for [`stage_dropped`] without a paste, the sessions it deleted.
    pub fn entries(&self) -> usize {
        self.written.len()
    }

    /// The configuration to persist: the account references the vault and
    /// no longer names a variable for the stored slots.
    pub fn config(&self) -> &Config {
        &self.config
    }
}

fn no_account() -> Localized {
    Localized::fixed(
        "user_session.migrate.no_account",
        "Account not found; reopen the wizard.",
    )
}

/// RF1.2: copy the sessions of the account at `index` from the environment
/// variables it names into the vault, once the user explicitly asked for
/// it. Nothing is written when any variable is unset or empty, and errors
/// name the variable, never its value.
pub async fn stage_migration(
    oauth: &OAuthSession,
    secrets: &dyn Credentials,
    config: &Config,
    index: usize,
) -> Result<StagedSession, Localized> {
    let account = config.accounts.get(index).ok_or_else(no_account)?;
    stage_candidates(oauth, secrets, config, index, migration_candidates(account)).await
}

/// PRD 0047 (GUI requirement G5): move every secret of the account at
/// `index` still read from an environment variable — its web sessions and
/// its API keys, tokens and secrets alike — into the vault, after the
/// user's explicit request. Same guarantees as [`stage_migration`]:
/// nothing is written when a variable is unset, errors name the variable
/// and never its value, and [`rollback`] undoes the writes.
pub async fn stage_vault_migration(
    oauth: &OAuthSession,
    secrets: &dyn Credentials,
    config: &Config,
    index: usize,
) -> Result<StagedSession, Localized> {
    let account = config.accounts.get(index).ok_or_else(no_account)?;
    let candidates = crate::secret_input::vault_migration_candidates(account);
    stage_candidates(oauth, secrets, config, index, candidates).await
}

async fn stage_candidates(
    oauth: &OAuthSession,
    secrets: &dyn Credentials,
    config: &Config,
    index: usize,
    candidates: Vec<MigrationCandidate>,
) -> Result<StagedSession, Localized> {
    let account = config.accounts.get(index).ok_or_else(no_account)?;
    if candidates.is_empty() {
        return Err(Localized::fixed(
            "user_session.migrate.nothing",
            "This account references no session in an environment variable.",
        ));
    }
    let mut values = Vec::with_capacity(candidates.len());
    for candidate in &candidates {
        let value = secrets
            .get(&candidate.env)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Localized::with_params(
                    "user_session.migrate.env_missing",
                    params([("name", ParamValue::from(candidate.env.clone()))]),
                    "The variable {name} is unset or empty in this process; export it before migrating.",
                )
            })?;
        // SES-02: only the descriptor's cookies reach the vault, never the
        // variable's whole cookie jar; a value without them stores nothing.
        let mut value = value;
        let reduced = reduce(account.provider, candidate.slot, &value, Utc::now());
        wipe(&mut value);
        let reduced = reduced?;
        values.push((candidate.slot, reduced.cookie.clone(), reduced.expires_at));
    }
    write_sessions(oauth, config, index, values).await
}

/// RF2/RF3.4: store the pasted sessions of the account at `index` of
/// `config` (the candidate configuration the front end is about to save,
/// with a new account already appended). An account that already has a
/// vault reference keeps it, so a new paste replaces the value in the same
/// vault entry; one without gets a fresh reference. The stored slots stop
/// naming a variable. Only the reduced cookies reach the vault.
///
/// SES-02: nothing is written without the [`SaveClearance`] of the test
/// before save ([`clear_for_save`]); `None` refuses the save with
/// `user_session.save.untested`.
pub async fn stage_pasted(
    oauth: &OAuthSession,
    config: &Config,
    index: usize,
    sessions: &[ReducedSession],
    clearance: Option<&SaveClearance>,
) -> Result<StagedSession, Localized> {
    if clearance.is_none() {
        return Err(untested());
    }
    let account = config.accounts.get(index).ok_or_else(no_account)?;
    let mut values = Vec::with_capacity(sessions.len());
    for session in sessions {
        let descriptor =
            vault_descriptor(account.provider, session.slot).ok_or_else(invalid_reference)?;
        values.push((descriptor.slot, session.cookie.clone(), session.expires_at));
    }
    write_sessions(oauth, config, index, values).await
}

/// Point the account at `index` to the vault for each slot of `values`,
/// validate the result and write the values; a failed write undoes the
/// earlier ones.
async fn write_sessions(
    oauth: &OAuthSession,
    config: &Config,
    index: usize,
    values: Vec<(&'static str, String, Option<DateTime<Utc>>)>,
) -> Result<StagedSession, Localized> {
    let account = config.accounts.get(index).ok_or_else(no_account)?;
    let mut stored = account.clone();
    let mut session = stored.session.take().unwrap_or_else(SessionConfig::fresh);
    for (slot, _, _) in &values {
        if *slot == CREDENTIAL_SLOT {
            stored.credential_env = None;
        } else {
            stored.params.remove(*slot);
        }
        session.slots.insert(slot.to_string());
    }
    stored.session = Some(session);
    let mut updated = config.clone();
    updated.accounts[index] = stored;
    updated
        .validate()
        .map_err(|error| crate::edit_error::config_error_note(&error))?;
    let session = updated.accounts[index].session.as_ref().expect("set above");
    let mut written = Vec::with_capacity(values.len());
    for (slot, value, expires_at) in values {
        let key = storage_key(account.provider, session, slot).ok_or_else(invalid_reference)?;
        let replaced = match oauth.load_by_key(&key).await {
            Ok(replaced) => replaced,
            Err(error) => {
                let _ = undo_writes(oauth, written).await;
                return Err(error.l10n());
            }
        };
        if let Err(error) = oauth
            .restore_by_key(key.clone(), token(value, expires_at))
            .await
        {
            let _ = undo_writes(oauth, written).await;
            return Err(error.l10n());
        }
        written.push((key, replaced));
    }
    Ok(StagedSession {
        config: updated,
        written,
    })
}

/// Undo the vault writes of a [`StagedSession`] after its configuration
/// could not be persisted: new entries are deleted and a replaced or
/// forgotten session comes back, so the account reads what it read before.
pub async fn rollback(oauth: &OAuthSession, staged: StagedSession) -> Result<(), Localized> {
    undo_writes(oauth, staged.written).await
}

async fn undo_writes(
    oauth: &OAuthSession,
    written: Vec<(String, Option<OAuthToken>)>,
) -> Result<(), Localized> {
    let mut result = Ok(());
    for (key, replaced) in written {
        let undone = match replaced {
            Some(previous) => oauth
                .restore_by_key(key, previous)
                .await
                .map_err(|error| error.l10n()),
            None => oauth
                .delete_by_key(key)
                .await
                .map_err(|error| error.l10n_for_purge()),
        };
        if let Err(note) = undone {
            result = Err(note);
        }
    }
    result
}

// ---------------------------------------------------------- SES-01, SES-02

/// SES-01: whether `report` failed because the account's vault session is
/// gone or no longer works (rejected or redirected by the provider, missing
/// from the vault, or without its required cookies), so the card and the
/// details offer "Paste a new session" for it. Only for providers that
/// declare a session.
pub fn offers_new_paste(report: &crate::Report) -> bool {
    const PASTE_AGAIN: &[&str] = &[
        "user_session.rejected",
        "user_session.not_found",
        "user_session.invalid_reference",
        "query.session.missing_cookies",
    ];
    (!descriptors(report.provider).is_empty()
        || !crate::secret_input::secret_inputs(report.provider).is_empty())
        && report.status != crate::Status::Ready
        && report
            .notes_l10n
            .first()
            .is_some_and(|note| PASTE_AGAIN.contains(&note.id.as_ref()))
}

/// SES-02: what the read-only test query of a pasted session
/// ([`crate::Monitor::test_session`]) says about saving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCheck {
    /// The query answered with the pasted session.
    Passed,
    /// The provider could not be reached (network failure or timeout):
    /// nothing is known about the session, so the user may explicitly save
    /// it anyway.
    Unreachable,
    /// The provider rejected the session, answered with an incompatible
    /// schema or failed otherwise, or the draft cannot be queried: the
    /// session is never saved.
    Rejected,
}

impl SessionCheck {
    /// The check of a test report. A transport failure is the query's own
    /// `query.transport` or `query.timeout` note, which the report carries
    /// first.
    pub fn of(report: &crate::Report) -> Self {
        let unreachable = report.notes_l10n.first().is_some_and(|note| {
            matches!(
                note.id.as_ref(),
                crate::providers::TRANSPORT_NOTE | crate::providers::TIMEOUT_NOTE
            )
        });
        match report.status {
            crate::Status::Ready => Self::Passed,
            crate::Status::Error if unreachable => Self::Unreachable,
            _ => Self::Rejected,
        }
    }
}

/// SES-02: proof that the pasted sessions of one save passed the test
/// before save, or that the user explicitly chose "Save anyway" after a
/// transport failure. Only [`clear_for_save`] makes one, and
/// [`stage_pasted`] writes nothing without it.
#[derive(Debug)]
pub struct SaveClearance {
    _private: (),
}

#[cfg(test)]
impl SaveClearance {
    pub(crate) fn for_tests() -> Self {
        Self { _private: () }
    }
}

/// SES-02: why a pasted session was not saved. `save_anyway` is set only
/// after a transport failure: the front end may then offer "Save anyway".
#[derive(Debug, Clone)]
pub struct SaveRefused {
    pub note: Localized,
    pub save_anyway: bool,
}

/// The cause of a failed test, for the refusal: the query's own note (no
/// body, URL or value ever reaches it) or else the status.
fn test_cause(report: &crate::Report) -> Localized {
    report
        .notes_l10n
        .first()
        .cloned()
        .unwrap_or_else(|| crate::text::status_l10n(report.status))
}

/// SES-02: the policy of the test before save. A passed test clears the
/// save; a transport failure clears it only when the user explicitly chose
/// `save_anyway`; a rejected session, an incompatible schema or any other
/// failure never does.
pub fn clear_for_save(
    report: &crate::Report,
    save_anyway: bool,
) -> Result<SaveClearance, SaveRefused> {
    let check = SessionCheck::of(report);
    if check == SessionCheck::Passed || (check == SessionCheck::Unreachable && save_anyway) {
        return Ok(SaveClearance { _private: () });
    }
    let cause = params([("cause", ParamValue::from(test_cause(report)))]);
    Err(if check == SessionCheck::Unreachable {
        SaveRefused {
            note: Localized::with_params(
                "user_session.save.unreachable",
                cause,
                "The session could not be tested ({cause}); nothing was saved. Choose \"Save anyway\" to keep it untested, or try again.",
            ),
            save_anyway: true,
        }
    } else {
        SaveRefused {
            note: Localized::with_params(
                "user_session.save.rejected",
                cause,
                "The session test failed ({cause}); nothing was saved. Paste a valid session and try again.",
            ),
            save_anyway: false,
        }
    })
}

/// SES-02: the test before save of `account` (the draft about to be saved)
/// with its pasted `sessions`: one read-only query through `monitor`, then
/// [`clear_for_save`]. Without pasted sessions there is nothing to test and
/// no clearance is needed (`Ok(None)`).
pub async fn test_before_save(
    monitor: &crate::Monitor,
    account: &Account,
    sessions: &[ReducedSession],
    save_anyway: bool,
) -> Result<Option<SaveClearance>, SaveRefused> {
    if sessions.is_empty() {
        return Ok(None);
    }
    let report = test_pasted(monitor, account, sessions).await;
    clear_for_save(&report, save_anyway).map(Some)
}

/// `account` reading only the session `slot`: every other session slot of
/// its provider is dropped (vault reference and variable), so an adapter
/// that prefers another session (MiMo's `session_env` over
/// `platform_session_env`) queries with this one.
fn only_slot(account: &Account, slot: &str) -> Account {
    let mut probe = account.clone();
    for other in descriptors(account.provider) {
        if other.slot != slot {
            probe.params.remove(other.slot);
        }
    }
    if let Some(session) = probe.session.as_mut() {
        session.slots.retain(|kept| kept == slot);
        if session.slots.is_empty() {
            probe.session = None;
        }
    }
    probe
}

/// SES-02: the RF3 test of each pasted session on its own (one read-only
/// query per slot, with the account's other sessions left out), so a
/// working session of another slot never vouches for a paste and a broken
/// one never blocks it. The report is the first rejected one, else the
/// first unreachable one, else the last passed one; nothing is stored.
pub async fn test_pasted(
    monitor: &crate::Monitor,
    account: &Account,
    sessions: &[ReducedSession],
) -> crate::Report {
    let mut worst: Option<(SessionCheck, crate::Report)> = None;
    for session in sessions {
        let probe = only_slot(account, session.slot);
        let report = monitor
            .test_session(&probe, std::slice::from_ref(session))
            .await;
        let check = SessionCheck::of(&report);
        let rank = |check: SessionCheck| match check {
            SessionCheck::Rejected => 2,
            SessionCheck::Unreachable => 1,
            SessionCheck::Passed => 0,
        };
        let replace = worst.as_ref().is_none_or(|(kept, _)| {
            rank(check) > rank(*kept)
                || (check == SessionCheck::Passed && *kept == SessionCheck::Passed)
        });
        if replace {
            worst = Some((check, report));
        }
        if check == SessionCheck::Rejected {
            break;
        }
    }
    match worst {
        Some((_, report)) => report,
        None => monitor.test_session(account, sessions).await,
    }
}

/// SES-02: a save of pasted sessions without the test before save.
pub fn untested() -> Localized {
    Localized::fixed(
        "user_session.save.untested",
        "Test the pasted session before saving it; nothing was saved.",
    )
}

// ------------------------------------------------------------------- RF2.2

/// Overwrite `text` before freeing it. Best effort: copies an earlier
/// reallocation left behind belong to the allocator.
pub fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

/// A paste reduced to the cookies one session slot accepts: the `Cookie`
/// value to store (accepted names only, in the descriptor's order, numbered
/// fragments reassembled), which names it kept and how many other cookies
/// were discarded. `Debug` never shows the value, and dropping it
/// overwrites the buffer.
pub struct ReducedSession {
    slot: &'static str,
    cookie: String,
    kept: Vec<&'static str>,
    discarded: usize,
    expires_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for ReducedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReducedSession")
            .field("slot", &self.slot)
            .field("kept", &self.kept)
            .field("discarded", &self.discarded)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Drop for ReducedSession {
    fn drop(&mut self) {
        wipe(&mut self.cookie);
    }
}

impl ReducedSession {
    pub fn slot(&self) -> &'static str {
        self.slot
    }

    /// The reduced `Cookie` value, for a query or the vault only.
    pub(crate) fn cookie(&self) -> &str {
        &self.cookie
    }

    /// Accepted cookie names the paste carried, in the descriptor's order.
    pub fn kept(&self) -> &[&'static str] {
        &self.kept
    }

    /// Cookies of the paste whose name the descriptor does not accept.
    pub fn discarded(&self) -> usize {
        self.discarded
    }

    /// RF3.2: the session's known expiry: the earliest date a kept
    /// required cookie showed (any kept cookie when none is required).
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }

    /// What the paste step shows: names, counts and the expiry, never a
    /// value.
    pub fn summary(&self, now: DateTime<Utc>) -> Localized {
        let kept = self.kept.join(", ");
        let validity = match self.expires_at {
            Some(at) => expiry(at, now).label,
            None => Localized::fixed("user_session.expiry.unknown", "Validity unknown."),
        };
        Localized::with_params(
            "user_session.paste.summary",
            params([
                ("kept", ParamValue::from(kept)),
                ("discarded", ParamValue::from(self.discarded)),
                ("expiry", ParamValue::from(validity)),
            ]),
            "Kept: {kept}. Discarded: {discarded}. {expiry}",
        )
    }
}

/// One `name=value` of a paste, with the expiry its row showed.
struct Pair {
    name: String,
    value: String,
    expires_at: Option<DateTime<Utc>>,
}

impl Drop for Pair {
    fn drop(&mut self) {
        wipe(&mut self.value);
    }
}

/// Every `name=value` of a `;`-separated cookie list.
fn cookie_list(list: &str, out: &mut Vec<Pair>) {
    for item in list
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let (name, value) = item.split_once('=').unwrap_or((item, ""));
        out.push(Pair {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
            expires_at: None,
        });
    }
}

/// A `Cookie:` line or a bare `name=value; ...` list, possibly among other
/// request headers, which are skipped.
fn header_pairs(text: &str) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match line.split_once(':') {
            Some((name, value)) if is_header_name(name) => {
                if name.trim().eq_ignore_ascii_case("cookie") {
                    cookie_list(value, &mut pairs);
                }
            }
            _ => cookie_list(line, &mut pairs),
        }
    }
    pairs
}

/// `Name` of a `Name: value` header line; a cookie (`a=b:c`) never is one.
fn is_header_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Rows of a cookie table copied from the browser's inspector: name, value
/// and further columns, one of which may be the expiry.
fn table_pairs(text: &str) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for line in text.lines() {
        let columns: Vec<&str> = line.split('\t').map(str::trim).collect();
        let (Some(name), Some(value)) = (columns.first(), columns.get(1)) else {
            continue;
        };
        let header = name.eq_ignore_ascii_case("name") && value.eq_ignore_ascii_case("value");
        if name.is_empty() || header {
            continue;
        }
        pairs.push(Pair {
            name: name.to_string(),
            value: value.to_string(),
            expires_at: columns[2..].iter().find_map(|column| parse_date(column)),
        });
    }
    pairs
}

/// An inspector expiry: RFC 3339 (Chromium) or RFC 2822 (Firefox, Safari).
/// "Session" and sizes are no date.
fn parse_date(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .or_else(|_| DateTime::parse_from_rfc2822(text))
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// The words of a copied `cURL` command line (POSIX quoting, `$'…'` and
/// backslash line continuations).
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                open = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                open = true;
                while let Some(c) = chars.next() {
                    match c {
                        '\'' => break,
                        '\\' => match chars.next() {
                            Some('n') => word.push('\n'),
                            Some('t') => word.push('\t'),
                            Some(other) => word.push(other),
                            None => {}
                        },
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                open = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&next @ ('"' | '\\' | '$' | '`')) => {
                                chars.next();
                                word.push(next);
                            }
                            Some('\n') => {
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') | None => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(other) => {
                    open = true;
                    word.push(other);
                }
            },
            c if c.is_whitespace() => {
                if open {
                    words.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            c => {
                open = true;
                word.push(c);
            }
        }
    }
    if open {
        words.push(word);
    }
    words
}

/// The cookies of a copied `cURL`: `-H 'Cookie: …'` and `-b`/`--cookie`
/// lists (a `-b` naming a file carries no cookie).
fn curl_pairs(text: &str) -> Vec<Pair> {
    let mut words = shell_words(text);
    let mut pairs = Vec::new();
    let mut next_is: Option<&str> = None;
    for word in &words {
        if let Some(flag) = next_is.take() {
            curl_value(flag, word, &mut pairs);
            continue;
        }
        match word.as_str() {
            "-H" | "--header" => next_is = Some("header"),
            "-b" | "--cookie" => next_is = Some("cookie"),
            other => {
                if let Some(value) = other.strip_prefix("--header=") {
                    curl_value("header", value, &mut pairs);
                } else if let Some(value) = other.strip_prefix("--cookie=") {
                    curl_value("cookie", value, &mut pairs);
                } else if let Some(value) = other.strip_prefix("-H") {
                    curl_value("header", value, &mut pairs);
                } else if let Some(value) = other.strip_prefix("-b") {
                    curl_value("cookie", value, &mut pairs);
                }
            }
        }
    }
    words.iter_mut().for_each(wipe);
    pairs
}

fn curl_value(flag: &str, value: &str, pairs: &mut Vec<Pair>) {
    match flag {
        "header" => {
            if let Some((name, list)) = value.split_once(':') {
                if name.trim().eq_ignore_ascii_case("cookie") {
                    cookie_list(list, pairs);
                }
            }
        }
        _ if value.contains('=') => cookie_list(value, pairs),
        _ => {}
    }
}

/// Where one accepted name occurs in a paste: the indices of its whole
/// pairs and, by part number, of its fragments.
type Occurrences = (Vec<usize>, BTreeMap<u32, Vec<usize>>);

/// `paste` with every line break as `\n`: a terminal delivers a pasted line
/// break as a carriage return, which `str::lines` does not split on. The
/// capacity never grows, so [`wipe`] reaches the whole copy.
fn unix_lines(paste: &str) -> String {
    let mut text = String::with_capacity(paste.len());
    let mut chars = paste.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => text.push('\n'),
            c => text.push(c),
        }
    }
    text
}

/// The `name=value` pairs of a paste in any of the accepted formats.
fn paste_pairs(paste: &str) -> Vec<Pair> {
    let mut lines = unix_lines(paste);
    let text = lines.trim_start_matches('\u{feff}').trim();
    let pairs = if text
        .get(..5)
        .is_some_and(|start| start.eq_ignore_ascii_case("curl "))
    {
        curl_pairs(text)
    } else if text.contains('\t') {
        table_pairs(text)
    } else {
        header_pairs(text)
    };
    wipe(&mut lines);
    pairs
}

fn paste_error(id: &'static str, name: &str, fallback: &'static str) -> Localized {
    Localized::with_param(id, "name", name, fallback)
}

fn duplicate(name: &str) -> Localized {
    paste_error(
        "user_session.paste.duplicate",
        name,
        "The cookie {name} appears more than once with different values; copy from a single request.",
    )
}

/// The one value `indices` of `pairs` agree on.
fn single_value<'p>(
    pairs: &'p [Pair],
    indices: &[usize],
    name: &str,
) -> Result<&'p Pair, Localized> {
    let first = &pairs[indices[0]];
    if indices.iter().all(|&i| pairs[i].value == first.value) {
        Ok(first)
    } else {
        Err(duplicate(name))
    }
}

/// The earliest of two optional dates.
fn earliest(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// RF2.2: reduce a paste (a `Cookie:` line, a `name=value; …` list, an
/// inspector cookie table or a copied `cURL`) to the cookies the `slot` of
/// `provider` accepts; everything else is discarded before anything is
/// stored. Cookies split into numbered parts (`name.0`, `name.1`, …) are
/// reassembled in order. Errors name cookies, never a value: a required
/// cookie missing, a part missing, a cookie repeated with different
/// values, an empty or invalid value, or a session already expired at
/// `now`.
pub fn reduce(
    provider: Provider,
    slot: &str,
    paste: &str,
    now: DateTime<Utc>,
) -> Result<ReducedSession, Localized> {
    let descriptor = &vault_descriptor(provider, slot).ok_or_else(invalid_reference)?;
    if descriptor.delivery == Delivery::Secret {
        return reduce_secret(descriptor, paste);
    }
    let pairs = paste_pairs(paste);
    if pairs.is_empty() {
        return Err(Localized::fixed(
            "user_session.paste.empty",
            "No cookie in the pasted text; copy the Cookie header, the request as cURL or the cookie table.",
        ));
    }
    let accepted: Vec<&'static str> = descriptor
        .required
        .iter()
        .chain(descriptor.optional)
        .copied()
        .collect();
    // Per accepted name: the pairs carrying it whole and its numbered parts.
    let mut found: BTreeMap<&'static str, Occurrences> = BTreeMap::new();
    let mut reduced = ReducedSession {
        slot: descriptor.slot,
        cookie: String::new(),
        kept: Vec::new(),
        discarded: 0,
        expires_at: None,
    };
    for (index, pair) in pairs.iter().enumerate() {
        if let Some(name) = accepted.iter().find(|name| **name == pair.name) {
            found.entry(name).or_default().0.push(index);
            continue;
        }
        let part = pair.name.rsplit_once('.').and_then(|(base, part)| {
            let name = accepted.iter().find(|name| **name == base)?;
            let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            Some((*name, part.parse::<u32>().ok().filter(|_| digits)?))
        });
        match part {
            Some((name, part)) => found
                .entry(name)
                .or_default()
                .1
                .entry(part)
                .or_default()
                .push(index),
            None => reduced.discarded += 1,
        }
    }
    let bounds_expiry =
        |name: &str| descriptor.required.is_empty() || descriptor.required.contains(&name);
    for name in &accepted {
        let Some((whole, parts)) = found.get(name) else {
            continue;
        };
        let mut value = String::new();
        let mut expires_at = None;
        if parts.is_empty() {
            let pair = single_value(&pairs, whole, name)?;
            value.push_str(&pair.value);
            for &i in whole {
                expires_at = earliest(expires_at, pairs[i].expires_at);
            }
        } else {
            if !whole.is_empty() {
                return Err(duplicate(name));
            }
            for (expected, (part, indices)) in parts.iter().enumerate() {
                if *part != expected as u32 {
                    return Err(Localized::with_params(
                        "user_session.paste.fragment_missing",
                        params([
                            ("name", ParamValue::from(*name)),
                            ("part", ParamValue::from(expected)),
                        ]),
                        "The cookie {name} came in parts and part {part} is missing; copy every part.",
                    ));
                }
                let pair = single_value(&pairs, indices, name)?;
                value.push_str(&pair.value);
                for &i in indices {
                    expires_at = earliest(expires_at, pairs[i].expires_at);
                }
            }
        }
        if value.is_empty() || value.chars().any(|c| c.is_control() || c == ';') {
            wipe(&mut value);
            return Err(paste_error(
                "user_session.paste.invalid_value",
                name,
                "The value of the cookie {name} is empty or has invalid characters; copy it again.",
            ));
        }
        if !reduced.cookie.is_empty() {
            reduced.cookie.push_str("; ");
        }
        reduced.cookie.push_str(name);
        reduced.cookie.push('=');
        reduced.cookie.push_str(&value);
        wipe(&mut value);
        reduced.kept.push(name);
        if bounds_expiry(name) {
            reduced.expires_at = earliest(reduced.expires_at, expires_at);
        }
    }
    let missing: Vec<&str> = descriptor
        .required
        .iter()
        .copied()
        .filter(|name| !reduced.kept.contains(name))
        .collect();
    if !missing.is_empty() {
        let names = missing.join(", ");
        return Err(Localized::with_param(
            "user_session.paste.missing",
            "names",
            names.as_str(),
            "This session's cookies are missing: {names}.",
        ));
    }
    if reduced.kept.is_empty() {
        let names = accepted.join(", ");
        return Err(Localized::with_param(
            "user_session.paste.none_accepted",
            "names",
            names.as_str(),
            "No cookie of this session in the pasted text; it uses: {names}.",
        ));
    }
    if descriptor.jwt_expiry && reduced.expires_at.is_none() {
        reduced.expires_at = descriptor.required.first().and_then(|name| {
            let prefix = format!("{name}=");
            reduced
                .cookie
                .split("; ")
                .find_map(|item| item.strip_prefix(prefix.as_str()))
                .and_then(jwt_expiry)
        });
    }
    if let Some(at) = reduced.expires_at.filter(|at| *at <= now) {
        let date = at.format("%Y-%m-%d").to_string();
        return Err(Localized::with_param(
            "user_session.paste.expired",
            "date",
            date.as_str(),
            "This session expired on {date}; sign in to the site again and paste a new one.",
        ));
    }
    Ok(reduced)
}

/// The `exp` of a JWT, read without verifying the signature (it only
/// dates the session for the expiry chip).
pub(crate) fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    use base64::Engine as _;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let exp = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_i64()?;
    DateTime::from_timestamp(exp, 0)
}

/// PRD 0036 RF2.1: a pasted password or key is kept whole (trimmed); it
/// must be non-empty, bounded and free of control characters. Errors never
/// echo the value.
fn reduce_secret(descriptor: &SessionDescriptor, paste: &str) -> Result<ReducedSession, Localized> {
    let mut value = paste.trim().to_string();
    if value.is_empty() || value.len() > MAX_SECRET_LEN || value.chars().any(char::is_control) {
        wipe(&mut value);
        return Err(Localized::fixed(
            "user_session.paste.secret_invalid",
            "The pasted value is empty, too long or has invalid characters; copy it again.",
        ));
    }
    Ok(ReducedSession {
        slot: descriptor.slot,
        cookie: value,
        kept: Vec::new(),
        discarded: 0,
        expires_at: None,
    })
}

/// Whether `slot` of `provider` holds a web session (cookies), as opposed
/// to a password or key: only a rejected session reads "session expired".
pub(crate) fn is_cookie_slot(provider: Provider, slot: &str) -> bool {
    descriptor(provider, slot).is_some_and(|d| d.delivery == Delivery::CookieHeader)
}

// ------------------------------------------------------------------- RF3.2

/// A session expiring sooner than this is near its end: the card warns
/// and the alerts (PRD 0008) notify it.
pub const EXPIRY_WARNING: TimeDelta = TimeDelta::days(3);

/// How long a session with a known expiry still lasts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionExpiry {
    pub expires_at: DateTime<Utc>,
    /// Whole days left at `now`; negative once expired.
    pub days: i64,
    /// Under [`EXPIRY_WARNING`] (or expired).
    pub soon: bool,
    /// "Expira em N dias", "Expira em menos de um dia", "Sessão expirada".
    pub label: Localized,
}

/// The expiry of a session ending at `expires_at`, seen at `now`.
pub fn expiry(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> SessionExpiry {
    let left = expires_at - now;
    let days = left.num_days();
    let label = if left <= TimeDelta::zero() {
        Localized::fixed("user_session.expiry.expired", "Session expired.")
    } else if days == 0 {
        Localized::fixed(
            "user_session.expiry.hours",
            "Session expires in less than a day.",
        )
    } else if days == 1 {
        Localized::fixed("user_session.expiry.one_day", "Session expires in 1 day.")
    } else {
        Localized::with_param(
            "user_session.expiry.days",
            "days",
            ParamValue::from(days as u64),
            "Session expires in {days} days.",
        )
    };
    SessionExpiry {
        expires_at,
        days,
        soon: left < EXPIRY_WARNING,
        label,
    }
}

/// RF3.2: the expiry of the session `report`'s query used, when known.
pub fn report_expiry(report: &crate::Report, now: DateTime<Utc>) -> Option<SessionExpiry> {
    report.session_expires_at.map(|at| expiry(at, now))
}

// ------------------------------------------------------------------- RF4

/// RF4.1: the `UserSession` level and the risk warning, as the card and the
/// paste step show them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionNotice {
    /// `access.credential.user_session`: "Sessão do usuário".
    pub level: Localized,
    /// What a full login session means and where Cuotai keeps it.
    pub warning: Localized,
}

fn vault_warning() -> Localized {
    Localized::fixed(
        "user_session.warning",
        "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service.",
    )
}

fn notice_with(warning: Localized) -> SessionNotice {
    SessionNotice {
        level: crate::access::Credential::UserSession.label(),
        warning,
    }
}

/// RF4.1: what the paste step of `provider` shows above its slots; `None`
/// for a provider that accepts no session.
pub fn paste_notice(provider: Provider) -> Option<SessionNotice> {
    (!descriptors(provider).is_empty()).then(|| notice_with(vault_warning()))
}

/// RF4.1: what the card of `account` shows while its query uses a web
/// session: kept in the vault or, before a migration, read from the
/// environment variable it names. `None` for an account without one.
pub fn notice(account: &Account) -> Option<SessionNotice> {
    // PRD 0047: an API key kept in the vault is no login session.
    let web_session = account.session.as_ref().is_some_and(|session| {
        session
            .slots
            .iter()
            .any(|slot| descriptor(account.provider, slot).is_some())
    });
    if web_session {
        return Some(notice_with(vault_warning()));
    }
    descriptors(account.provider)
        .iter()
        .any(|descriptor| {
            account
                .params
                .get(descriptor.slot)
                .is_some_and(|env| !env.trim().is_empty())
        })
        .then(|| {
            notice_with(Localized::fixed(
                "user_session.warning_env",
                "This is a full login session. Cuotai reads it from an environment variable and uses it only to query this service; move it to the system vault in the account wizard.",
            ))
        })
}

/// Shown once a forgotten session was saved.
pub fn forgotten_notice() -> Localized {
    Localized::fixed(
        "user_session.forget.done",
        "Session forgotten: the system vault entry was deleted and the TOML no longer keeps the reference.",
    )
}

/// RF4.2: "forget this session" for the account at `index`: every vault
/// entry of its `[accounts.session]` is deleted and the staged
/// configuration drops the reference. Persist [`StagedSession::config`]
/// next; [`rollback`] puts the deleted sessions back when that fails. A
/// slot whose entry is already gone only loses its reference.
pub async fn stage_forget(
    oauth: &OAuthSession,
    config: &Config,
    index: usize,
) -> Result<StagedSession, Localized> {
    let account = config.accounts.get(index).ok_or_else(no_account)?;
    let session = account.session.as_ref().ok_or_else(|| {
        Localized::fixed(
            "user_session.forget.nothing",
            "This account keeps no session in the system vault.",
        )
    })?;
    let mut updated = config.clone();
    updated.accounts[index].session = None;
    updated
        .validate()
        .map_err(|error| crate::edit_error::config_error_note(&error))?;
    let mut written = Vec::with_capacity(session.slots.len());
    for slot in &session.slots {
        // An invalid reference names no vault entry.
        let Some(key) = storage_key(account.provider, session, slot) else {
            continue;
        };
        let deleted = match oauth.load_by_key(&key).await {
            Ok(Some(deleted)) => deleted,
            Ok(None) => continue,
            Err(error) => {
                let _ = undo_writes(oauth, written).await;
                return Err(error.l10n());
            }
        };
        if let Err(error) = oauth.delete_by_key(key.clone()).await {
            let _ = undo_writes(oauth, written).await;
            return Err(error.l10n_for_purge());
        }
        written.push((key, Some(deleted)));
    }
    Ok(StagedSession {
        config: updated,
        written,
    })
}

/// FIX-06 (2026-10-21 review): the vault entries an edit leaves without a
/// reference. Every `(provider, credential_ref, slot)` of `before` that no
/// account of `after` still references — the account was removed, or a
/// provider switch dropped its `[accounts.session]` — has its vault entry
/// deleted, since nothing could ever read or forget it again.
///
/// `staged` is the vault half the caller already holds for `after` (pasted
/// sessions, [`stage_pasted`]), or `None`; the deletions join it, so one
/// [`rollback`] puts every deleted session back and undoes the paste when
/// persisting `after` fails. The result is `None` when there was nothing to
/// stage at all. On error nothing is left changed: this call's deletions
/// are undone and `staged` is rolled back too (the caller aborts the save).
/// Errors never carry a session value.
pub async fn stage_dropped(
    oauth: &OAuthSession,
    before: &Config,
    after: &Config,
    staged: Option<StagedSession>,
) -> Result<Option<StagedSession>, Localized> {
    let kept: BTreeSet<(&str, &str, &str)> = after
        .accounts
        .iter()
        .filter_map(|account| Some((account.provider.slug(), account.session.as_ref()?)))
        .flat_map(|(provider, session)| {
            session
                .slots
                .iter()
                .map(move |slot| (provider, session.credential_ref.as_str(), slot.as_str()))
        })
        .collect();
    let mut written: Vec<(String, Option<OAuthToken>)> = Vec::new();
    for account in &before.accounts {
        let Some(session) = &account.session else {
            continue;
        };
        for slot in &session.slots {
            let reference = (
                account.provider.slug(),
                session.credential_ref.as_str(),
                slot.as_str(),
            );
            if kept.contains(&reference) {
                continue;
            }
            // An invalid reference names no vault entry.
            let Some(key) = storage_key(account.provider, session, slot) else {
                continue;
            };
            let failure = match oauth.load_by_key(&key).await {
                Ok(None) => continue,
                Ok(Some(deleted)) => match oauth.delete_by_key(key.clone()).await {
                    Ok(()) => {
                        written.push((key, Some(deleted)));
                        continue;
                    }
                    Err(error) => error.l10n_for_purge(),
                },
                Err(error) => error.l10n(),
            };
            let _ = undo_writes(oauth, written).await;
            if let Some(staged) = staged {
                let _ = rollback(oauth, staged).await;
            }
            return Err(failure);
        }
    }
    Ok(match staged {
        Some(mut staged) => {
            staged.written.extend(written);
            Some(staged)
        }
        None if written.is_empty() => None,
        None => Some(StagedSession {
            config: after.clone(),
            written,
        }),
    })
}

#[cfg(test)]
#[path = "user_session_tests.rs"]
mod tests;
