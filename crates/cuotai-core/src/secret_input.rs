//! PRD 0047 — secrets typed in the desktop's add-account flow.
//!
//! The desktop user never sees an environment variable: every API key,
//! token, secret or web session a provider needs is a *secret input* the
//! user types or pastes, kept in the OS credential vault through the same
//! `[accounts.session]` reference as the PRD 0016 web sessions (the TOML
//! holds only the random `credential_ref` and the slot names). The slot of
//! an input is the account field it supersedes: `credential_env` for the
//! main credential, the `*_env` parameter otherwise. Terminal users keep
//! naming variables ([`crate::account_edit::default_env`] is the default
//! per provider; any account may name another one).
//!
//! * [`secret_inputs`] lists, per provider, what the GUI asks for. The
//!   labels and help texts never mention a variable.
//! * [`SecretValue`] carries one typed value from the front end; it is
//!   never `Debug`-printed and is overwritten when dropped.
//!   [`reduce_secrets`] turns the values into the
//!   [`ReducedSession`]s the existing pipeline takes:
//!   [`user_session::test_before_save`] (SES-02) and
//!   [`user_session::stage_pasted`] (vault first, [`user_session::rollback`]
//!   when the TOML write fails).
//! * [`secret_states`] says, for an existing account, where each secret
//!   comes from today (vault, terminal environment or nowhere), and
//!   [`vault_migration_candidates`] /
//!   [`user_session::stage_vault_migration`] move the environment ones
//!   into the vault on request.
//!
//! Values never appear in the TOML, `Debug`, errors, reports or snapshots.

use crate::account_edit::{self, CredentialKind, FieldDescriptorKind};
use crate::text::{params, Localized, ParamValue};
use crate::user_session::{self, Delivery, MigrationCandidate, ReducedSession, CREDENTIAL_SLOT};
use crate::{Account, Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One secret the GUI asks for, declared by the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretInput {
    /// Vault slot and the account field it supersedes: `credential_env`
    /// or a `*_env` parameter key.
    pub slot: &'static str,
    /// What the secret is, in user terms (never a variable name).
    pub label: Localized,
    /// Where to find it and how Cuotai keeps it (never a variable name).
    pub help: Localized,
    /// Whether a new account needs it to be queried.
    pub required: bool,
    /// A browser web session: the paste is a cookie list the core reduces
    /// (PRD 0016 RF2), not the value itself.
    pub cookie: bool,
    /// The field descriptor whose visibility governs this input
    /// ([`account_edit::visible_field_indices`]); `None` for the main
    /// credential and for session slots without a field (always shown).
    pub field: Option<&'static str>,
    /// Terminal only: the variable the TUI suggests for this secret;
    /// empty when the TUI has no default. The GUI never shows it.
    pub default_env: &'static str,
}

/// Generic help of a key, token or secret input.
fn secret_help() -> Localized {
    Localized::fixed(
        "secret_input.help",
        "Paste the value. Cuotai keeps it in the system vault and sends it only to this provider; the configuration file keeps just a reference.",
    )
}

/// Label of a `*_env` parameter secret, by slot.
fn param_label(slot: &str) -> Option<&'static str> {
    Some(match slot {
        "admin_key_env" => "Admin API key (optional)",
        "token_env" => "OAuth access token (optional)",
        "session_env" => "CLI session token",
        "management_key_env" => "Management key (optional)",
        "analytics_key_env" => "Analytics key (optional)",
        "secret_env" => "AccessKey secret",
        "security_token_env" => "STS security token (optional)",
        "platform_session_env" => "Platform session",
        _ => return None,
    })
}

/// Whether `provider` asks for its main credential as a secret.
fn has_credential(provider: Provider) -> bool {
    account_edit::credential_kind(provider) == CredentialKind::EnvVar
        && !account_edit::default_env(provider).is_empty()
}

/// PRD 0047: the secrets the desktop asks for when adding or editing an
/// account of `provider`, main credential first, then the parameter
/// secrets in field order, then any further web session slot.
pub fn secret_inputs(provider: Provider) -> Vec<SecretInput> {
    let mut inputs = Vec::new();
    if has_credential(provider) {
        let declared = user_session::descriptor(provider, CREDENTIAL_SLOT);
        inputs.push(SecretInput {
            slot: CREDENTIAL_SLOT,
            label: declared.map_or_else(
                || Localized::fixed("secret_input.credential.label", "API key or access token"),
                |d| d.label(),
            ),
            help: secret_help(),
            // PRD 0036 RF2.1: a 9router instance may have login disabled.
            required: provider != Provider::NineRouter,
            cookie: false,
            field: None,
            default_env: account_edit::default_env(provider),
        });
    }
    for field in account_edit::field_descriptors(provider) {
        if !matches!(field.kind, FieldDescriptorKind::Text)
            || inputs.iter().any(|i| i.slot == field.key)
        {
            continue;
        }
        if let Some(declared) = user_session::descriptor(provider, field.key) {
            inputs.push(SecretInput {
                slot: declared.slot,
                label: declared.label(),
                help: declared.instructions(),
                required: false,
                cookie: declared.delivery == Delivery::CookieHeader,
                field: Some(field.key),
                default_env: "",
            });
            continue;
        }
        let Some(label) = param_label(field.key) else {
            continue;
        };
        inputs.push(SecretInput {
            slot: field.key,
            label: Localized::fixed(format!("secret_input.slot.{}.label", field.key), label),
            help: secret_help(),
            required: field.required,
            cookie: false,
            field: Some(field.key),
            default_env: "",
        });
    }
    for declared in user_session::descriptors(provider) {
        if inputs.iter().any(|i| i.slot == declared.slot) {
            continue;
        }
        inputs.push(SecretInput {
            slot: declared.slot,
            label: declared.label(),
            help: declared.instructions(),
            required: false,
            cookie: declared.delivery == Delivery::CookieHeader,
            field: None,
            default_env: "",
        });
    }
    inputs
}

/// The static slot name of `slot` when `provider` can keep it in the vault.
pub(crate) fn secret_input_slot(provider: Provider, slot: &str) -> Option<&'static str> {
    if slot == CREDENTIAL_SLOT {
        return has_credential(provider).then_some(CREDENTIAL_SLOT);
    }
    account_edit::field_descriptors(provider)
        .iter()
        .find(|field| field.key == slot && param_label(slot).is_some())
        .map(|field| field.key)
}

/// One secret typed in the GUI, by slot. Never `Debug`-printed with its
/// value; the value is overwritten when dropped.
#[derive(Deserialize)]
pub struct SecretValue {
    pub slot: String,
    pub value: String,
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretValue")
            .field("slot", &self.slot)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        user_session::wipe(&mut self.value);
    }
}

fn unknown_slot() -> Localized {
    Localized::fixed(
        "secret_input.unknown_slot",
        "This provider does not take that secret; reopen the account form.",
    )
}

fn duplicate_slot() -> Localized {
    Localized::fixed(
        "secret_input.duplicate_slot",
        "The same secret was sent twice; reopen the account form.",
    )
}

/// PRD 0047: the typed `values` of an account of `provider`, checked
/// against [`secret_inputs`] and reduced for the vault (a cookie session
/// keeps only its declared cookies; a key is kept whole, trimmed). Blank
/// values are skipped (the stored secret, if any, stays). Errors never
/// carry a value.
pub fn reduce_secrets(
    provider: Provider,
    values: &[SecretValue],
) -> Result<Vec<ReducedSession>, Localized> {
    let inputs = secret_inputs(provider);
    let mut seen = BTreeSet::new();
    let mut reduced = Vec::with_capacity(values.len());
    for value in values {
        if value.value.trim().is_empty() {
            continue;
        }
        let input = inputs
            .iter()
            .find(|input| input.slot == value.slot)
            .ok_or_else(unknown_slot)?;
        if !seen.insert(input.slot) {
            return Err(duplicate_slot());
        }
        reduced.push(user_session::reduce(
            provider,
            input.slot,
            &value.value,
            chrono::Utc::now(),
        )?);
    }
    Ok(reduced)
}

/// Where a secret of a saved account comes from today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    /// Kept in the system vault.
    Vault,
    /// Read from an environment variable set up in the terminal. The GUI
    /// shows it read-only and offers [`user_session::stage_vault_migration`].
    Environment,
    /// Not set.
    Unset,
}

/// The state of one [`SecretInput`] of a saved account. Holds no value and
/// no variable name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretState {
    pub slot: &'static str,
    pub source: SecretSource,
    /// GUI wording of the state, in user terms.
    pub label: Localized,
}

fn state_label(source: &SecretSource) -> Localized {
    match source {
        SecretSource::Vault => {
            Localized::fixed("secret_input.state.vault", "Saved in the system vault")
        }
        SecretSource::Environment => Localized::fixed(
            "secret_input.state.environment",
            "Using an environment variable (configured in the terminal)",
        ),
        SecretSource::Unset => Localized::fixed("secret_input.state.unset", "Not set"),
    }
}

/// PRD 0047: where each secret input of `account` comes from. A main
/// credential the account never named reads `Unset` (the TUI's default
/// variable is a terminal convention, not a GUI state).
pub fn secret_states(account: &Account) -> Vec<SecretState> {
    let vault: BTreeSet<&str> = account
        .session
        .iter()
        .flat_map(|session| session.slots.iter().map(String::as_str))
        .collect();
    secret_inputs(account.provider)
        .into_iter()
        .map(|input| {
            let source = if vault.contains(input.slot) {
                SecretSource::Vault
            } else if user_session::slot_env(account, input.slot)
                .is_some_and(|env| !env.trim().is_empty())
            {
                SecretSource::Environment
            } else {
                SecretSource::Unset
            };
            SecretState {
                slot: input.slot,
                label: state_label(&source),
                source,
            }
        })
        .collect()
}

/// PRD 0047: every secret of `account` read from an environment variable
/// that [`user_session::stage_vault_migration`] would move to the vault —
/// web sessions and API keys alike. The consent text speaks of the
/// terminal setup, never of the variable; `env` (the name, needed to read
/// the value) is for the core, not for display.
pub fn vault_migration_candidates(account: &Account) -> Vec<MigrationCandidate> {
    // OAuth accounts keep their token in the vault already.
    if account.oauth.is_some() {
        return Vec::new();
    }
    secret_inputs(account.provider)
        .into_iter()
        .filter_map(|input| {
            let env = user_session::slot_env(account, input.slot)?.trim();
            if env.is_empty() {
                return None;
            }
            let consent = Localized::with_params(
                "secret_input.migrate.consent",
                params([("label", ParamValue::from(input.label.clone()))]),
                "{label}: Cuotai copies the value it reads today from the terminal setup into the system vault. From then on this account works without that setup; the configuration file keeps just a reference.",
            );
            let hosts = user_session::descriptor(account.provider, input.slot)
                .map_or(&[][..], |d| d.hosts);
            Some(MigrationCandidate {
                slot: input.slot,
                env: env.to_string(),
                label: input.label,
                hosts,
                consent,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "secret_input_tests.rs"]
mod tests;
