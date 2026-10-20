//! Non-secret account identity for frontends: badges, e-mail and a copyable
//! identifier, derived from `Account::params` (public metadata projected by
//! the core at login). Lives in the core so frontends never branch on
//! `Provider` to decide what identifies an account.

use crate::account_edit::{self, FieldDescriptorKind};
use crate::text::Localized;
use crate::{Account, Provider, Status};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BadgeTone {
    Accent,
    Info,
    Good,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Badge {
    pub tone: BadgeTone,
    pub text: Localized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CopyValue {
    pub label: Localized,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AccountIdentity {
    pub email: Option<String>,
    pub badges: Vec<Badge>,
    pub copy: Option<CopyValue>,
}

/// The account's e-mail (public login metadata in `params`), if known.
pub fn email(account: &Account) -> Option<&str> {
    param(account, "email")
}

fn param<'a>(account: &'a Account, key: &str) -> Option<&'a str> {
    account
        .params
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

/// Identity summary of `account` after a report with `status`.
pub fn account_identity(account: &Account, status: Status) -> AccountIdentity {
    let param = |key: &str| param(account, key);
    let mut identity = AccountIdentity {
        email: email(account).map(str::to_owned),
        ..AccountIdentity::default()
    };
    for (key, tone) in [
        ("auth_method", BadgeTone::Accent),
        ("region", BadgeTone::Info),
    ] {
        if let Some(value) = param(key) {
            identity.badges.push(Badge {
                tone,
                text: value_label(account.provider, key, value),
            });
        }
    }
    if account.oauth.is_some() && status == Status::Ready {
        identity.badges.push(Badge {
            tone: BadgeTone::Good,
            text: Localized::fixed("identity.session_active", "sessão ativa"),
        });
    }
    identity.copy = param("profile_arn").map(|value| CopyValue {
        label: Localized::fixed("identity.profile_arn", "ARN do perfil"),
        value: value.to_owned(),
    });
    identity
}

/// Localized label of a known choice value; other values are shown verbatim.
fn value_label(provider: Provider, key: &str, value: &str) -> Localized {
    let shared = account_edit::field_descriptors(provider)
        .iter()
        .map(|descriptor| (descriptor, true));
    let native_only = account_edit::native_option_descriptors(provider)
        .filter(|descriptor| {
            !account_edit::field_descriptors(provider)
                .iter()
                .any(|field| field.key == descriptor.key)
        })
        .map(|descriptor| (descriptor, false));
    for (descriptor, is_field) in shared.chain(native_only) {
        let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
            continue;
        };
        if descriptor.key != key {
            continue;
        }
        if let Some(option) = choice.options.iter().find(|option| option.value == value) {
            let id = if is_field {
                account_edit::field_option_id(provider, key, value)
            } else {
                account_edit::native_option_option_id(provider, key, value)
            };
            return Localized::fixed(id, option.label);
        }
    }
    Localized::with_param(
        "identity.value",
        "value",
        value.to_owned(),
        value.to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn account(provider: Provider, params: &[(&str, &str)]) -> Account {
        Account {
            name: "conta".into(),
            provider,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..Account::default()
        }
    }

    #[test]
    fn known_choice_values_use_localized_option_labels_and_others_stay_verbatim() {
        let kiro = account(
            Provider::Kiro,
            &[
                ("auth_method", "idc"),
                ("region", "eu-west-1"),
                ("profile_arn", "arn:x"),
            ],
        );
        let identity = account_identity(&kiro, Status::Error);
        assert_eq!(identity.badges.len(), 2);
        assert_eq!(
            identity.badges[0].text.id,
            "field.kiro.auth_method.option.idc"
        );
        assert_eq!(
            identity.badges[0].text.resolve("en-US"),
            "IAM Identity Center"
        );
        assert_eq!(identity.badges[1].text.resolve("en-US"), "eu-west-1");
        assert_eq!(identity.copy.unwrap().value, "arn:x");
        assert_eq!(identity.email, None);
    }

    #[test]
    fn active_session_badge_only_for_oauth_accounts_with_fresh_data() {
        let mut oauth = account(Provider::ChatGpt, &[]);
        oauth.oauth = Some(crate::oauth::OAuthConfig::new(String::new()));
        assert!(account_identity(&oauth, Status::Ready)
            .badges
            .iter()
            .any(|badge| badge.tone == BadgeTone::Good));
        assert!(account_identity(&oauth, Status::AuthenticationError)
            .badges
            .is_empty());
        assert!(
            account_identity(&account(Provider::Groq, &[]), Status::Ready)
                .badges
                .is_empty()
        );
    }
}
