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
//!
//! Session values never appear in the TOML, `Debug`, errors or reports.
//!
//! [`TokenStore`]: crate::oauth::TokenStore

use crate::oauth::{OAuthSession, OAuthToken};
use crate::text::{params, Localized, ParamValue};
use crate::{Account, Config, Credentials, Provider};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// How the session travels to its hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// As the request's `Cookie` header (`name=value; name=value`).
    CookieHeader,
}

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
    pub delivery: Delivery,
    /// `user_session.slot.*` id and Portuguese fallback of [`Self::label`].
    #[serde(skip)]
    label: (&'static str, &'static str),
}

impl SessionDescriptor {
    /// What this session is, for pickers and consent texts.
    pub fn label(&self) -> Localized {
        Localized::fixed(self.label.0, self.label.1)
    }
}

/// Alibaba console login cookies the Coding Plan and Token Plan adapters
/// read (CSRF, anonymous id and `sec_token`) plus the Aliyun login family.
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
    delivery: Delivery::CookieHeader,
    label: (
        "user_session.slot.xiaomi_mimo.session_env",
        "Sessão da conta Xiaomi (quota semanal do MiMo Desktop)",
    ),
};

const XIAOMI_PLATFORM: SessionDescriptor = SessionDescriptor {
    slot: "platform_session_env",
    required: &["api-platform_serviceToken", "userId"],
    optional: &[],
    hosts: &["platform.xiaomimimo.com"],
    delivery: Delivery::CookieHeader,
    label: (
        "user_session.slot.xiaomi_mimo.platform_session_env",
        "Sessão web de platform.xiaomimimo.com (saldo e Token Plan)",
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
    delivery: Delivery::CookieHeader,
    label: (
        "user_session.slot.alibaba_coding_plan.session_env",
        "Sessão do console Alibaba (Coding Plan)",
    ),
};

const ALIBABA_TOKEN_PLAN: SessionDescriptor = SessionDescriptor {
    slot: "session_env",
    required: &[],
    optional: ALIBABA_COOKIES,
    hosts: &["home.qwencloud.com", "cs-data.qwencloud.com"],
    delivery: Delivery::CookieHeader,
    label: (
        "user_session.slot.alibaba_token_plan.session_env",
        "Sessão do console Qwen Cloud (Token Plan)",
    ),
};

/// The sessions `provider` accepts; empty when it has none.
pub fn descriptors(provider: Provider) -> &'static [SessionDescriptor] {
    match provider {
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
                .all(|slot| descriptor(provider, slot).is_some())
    }
}

/// Vault key of one slot: bound to the reference, the provider and the slot.
fn storage_key(provider: Provider, config: &SessionConfig, slot: &str) -> Option<String> {
    let namespace = uuid::Uuid::parse_str(&config.credential_ref).ok()?;
    let identity = serde_json::to_vec(&("user_session", provider, slot)).ok()?;
    Some(uuid::Uuid::new_v5(&namespace, &identity).to_string())
}

/// The vault payload: the session is the token; nothing else is stored.
fn token(value: String) -> OAuthToken {
    OAuthToken {
        access_token: value,
        refresh_token: None,
        expires_at: None,
        metadata: BTreeMap::new(),
    }
}

fn not_found() -> Localized {
    Localized::fixed(
        "user_session.not_found",
        "Sessão não encontrada no cofre do sistema; informe a sessão novamente no assistente da conta.",
    )
}

fn invalid_reference() -> Localized {
    Localized::fixed(
        "user_session.invalid_reference",
        "Referência de sessão inválida em [accounts.session]; informe a sessão novamente no assistente da conta.",
    )
}

/// Session values of `account`, by slot, read from the vault for a query.
/// The values never leave the query's credentials.
pub(crate) async fn load(
    oauth: &OAuthSession,
    account: &Account,
) -> Result<Vec<(&'static str, String)>, crate::providers::QueryError> {
    let Some(config) = &account.session else {
        return Ok(Vec::new());
    };
    let mut sessions = Vec::with_capacity(config.slots.len());
    for slot in &config.slots {
        let (Some(descriptor), Some(key)) = (
            descriptor(account.provider, slot),
            storage_key(account.provider, config, slot),
        ) else {
            return Err(crate::providers::QueryError {
                status: crate::Status::NeedsConfiguration,
                note: invalid_reference(),
            });
        };
        let stored =
            oauth
                .load_by_key(&key)
                .await
                .map_err(|error| crate::providers::QueryError {
                    status: crate::Status::Error,
                    note: error.l10n(),
                })?;
        match stored {
            Some(token) if !token.access_token.trim().is_empty() => {
                sessions.push((descriptor.slot, token.access_token));
            }
            _ => {
                return Err(crate::providers::QueryError {
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
        "Sessão movida para o cofre do sistema; o TOML guarda só a referência. Remova a variável do seu ambiente quando quiser.",
    )
}

/// The sessions of `account` still read from an environment variable that
/// [`stage_migration`] would move to the vault.
pub fn migration_candidates(account: &Account) -> Vec<MigrationCandidate> {
    descriptors(account.provider)
        .iter()
        .filter_map(|descriptor| {
            let env = account.params.get(descriptor.slot)?.trim();
            if env.is_empty() {
                return None;
            }
            let label = descriptor.label();
            let hosts = descriptor.hosts.join(", ");
            let consent = Localized::with_params(
                "user_session.migrate.consent",
                params([
                    ("label", ParamValue::from(label.clone())),
                    ("env", ParamValue::from(env.to_string())),
                    ("hosts", ParamValue::from(hosts.clone())),
                ]),
                format!(
                    "{}: copiar o valor de {env} para o cofre do sistema e tirar {env} do TOML. A sessão só é enviada a {hosts}; a variável continua no seu ambiente até você removê-la.",
                    label.fallback
                ),
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

/// The vault half of a migration: the sessions were written, the TOML not
/// yet. Persist [`StagedMigration::config`] next, or undo the vault writes
/// with [`rollback_migration`] when that fails.
pub struct StagedMigration {
    config: Config,
    /// Each written vault key with the session it replaced, if any.
    written: Vec<(String, Option<OAuthToken>)>,
}

impl StagedMigration {
    /// The configuration to persist: the account references the vault and
    /// no longer names the migrated variables.
    pub fn config(&self) -> &Config {
        &self.config
    }
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
) -> Result<StagedMigration, Localized> {
    let account = config.accounts.get(index).ok_or_else(|| {
        Localized::fixed(
            "user_session.migrate.no_account",
            "Conta inexistente; reabra o assistente.",
        )
    })?;
    let candidates = migration_candidates(account);
    if candidates.is_empty() {
        return Err(Localized::fixed(
            "user_session.migrate.nothing",
            "Esta conta não referencia nenhuma sessão em variável de ambiente.",
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
                    format!(
                        "A variável {} está ausente ou vazia neste processo; exporte-a antes de migrar.",
                        candidate.env
                    ),
                )
            })?;
        values.push((candidate.slot, value));
    }
    let mut migrated = account.clone();
    let mut session = migrated.session.take().unwrap_or_else(SessionConfig::fresh);
    for (slot, _) in &values {
        migrated.params.remove(*slot);
        session.slots.insert(slot.to_string());
    }
    migrated.session = Some(session);
    let mut updated = config.clone();
    updated.accounts[index] = migrated;
    updated
        .validate()
        .map_err(|error| crate::edit_error::config_error_note(&error))?;
    let session = updated.accounts[index].session.as_ref().expect("set above");
    let mut written = Vec::with_capacity(values.len());
    for (slot, value) in values {
        let key = storage_key(account.provider, session, slot).ok_or_else(invalid_reference)?;
        let replaced = match oauth.load_by_key(&key).await {
            Ok(replaced) => replaced,
            Err(error) => {
                let _ = undo_writes(oauth, written).await;
                return Err(error.l10n());
            }
        };
        if let Err(error) = oauth.restore_by_key(key.clone(), token(value)).await {
            let _ = undo_writes(oauth, written).await;
            return Err(error.l10n());
        }
        written.push((key, replaced));
    }
    Ok(StagedMigration {
        config: updated,
        written,
    })
}

/// Undo the vault writes of a [`StagedMigration`] after its configuration
/// could not be persisted: new entries are deleted and a replaced session
/// comes back, so the account reads what it read before.
pub async fn rollback_migration(
    oauth: &OAuthSession,
    staged: StagedMigration,
) -> Result<(), Localized> {
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

#[cfg(test)]
#[path = "user_session_tests.rs"]
mod tests;
