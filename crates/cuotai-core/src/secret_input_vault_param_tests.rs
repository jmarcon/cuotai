//! Re-validation PRD 0047 V-03 and S-03 (G-0047-4): suggested variable
//! names fold accented letters, and an edit never writes a variable for a
//! slot the vault already holds.

use crate::account_edit::{
    build_account, build_account_with_secrets, suggest_env_for, valid_env_name, AccountDraft,
    DraftField, DraftValue,
};
use crate::user_session::SessionConfig;
use crate::{Account, Config, Provider};

/// V-03: "Café Ação-2" reads as `CAFE_ACAO_2`, not `CAF_A_O_2`.
#[test]
fn a_suggested_name_keeps_accented_letters_readable() {
    let config = Config {
        accounts: vec![Account {
            name: "home".into(),
            provider: Provider::Groq,
            credential_env: Some("GROQ_API_KEY".into()),
            ..Default::default()
        }],
        ..Config::default()
    };
    let suggested = suggest_env_for(Provider::Groq, &config, "Café Ação-2", None);
    assert_eq!(suggested, "GROQ_API_KEY_CAFE_ACAO_2");
    assert!(valid_env_name(&suggested));
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "Straße Œuvre Æsir ÑÜ", None),
        "GROQ_API_KEY_STRASSE_OEUVRE_AESIR_NU"
    );
    // Letters outside the table still become underscores.
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "東京 work", None),
        "GROQ_API_KEY_WORK"
    );
}

fn mistral_in_vault() -> Config {
    Config {
        accounts: vec![Account {
            name: "Mistral".into(),
            provider: Provider::Mistral,
            credential_env: Some("MISTRAL_API_KEY".into()),
            session: Some(SessionConfig {
                credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
                slots: ["admin_key_env".to_string()].into(),
            }),
            ..Default::default()
        }],
        ..Config::default()
    }
}

fn draft<'a>(fields: &'a [DraftField]) -> AccountDraft<'a> {
    AccountDraft {
        provider: Provider::Mistral,
        name: "Mistral",
        credential: "MISTRAL_API_KEY",
        auth_mode: crate::account_edit::AUTH_MODE_CREDENTIAL,
        oauth_mode: false,
        oauth_existing: None,
        oauth_authorized: None,
        oauth_login_params: Default::default(),
        existing_params: None,
        credential_params: None,
        fields,
        interval_text: "",
        group_text: None,
    }
}

/// S-03: the vault wins over `params.admin_key_env` at query time, so an
/// edit that types a variable there does not write it.
#[test]
fn an_edit_never_writes_a_variable_the_vault_overrides() {
    let config = mistral_in_vault();
    let fields = [DraftField {
        key: "admin_key_env",
        value: DraftValue::Text("OTHER".into()),
    }];
    for account in [
        build_account(&draft(&fields), &config, Some(0)).unwrap(),
        build_account_with_secrets(&draft(&fields), &config, Some(0), &[]).unwrap(),
    ] {
        assert!(!account.params.contains_key("admin_key_env"));
        assert_eq!(account.credential_env.as_deref(), Some("MISTRAL_API_KEY"));
        assert_eq!(account.session, config.accounts[0].session);
    }
    // A new account (nothing in the vault) still writes the variable.
    let none = Config {
        accounts: Vec::new(),
        ..Config::default()
    };
    let new = build_account(&draft(&fields), &none, None).unwrap();
    assert_eq!(
        new.params.get("admin_key_env").map(String::as_str),
        Some("OTHER")
    );
}
