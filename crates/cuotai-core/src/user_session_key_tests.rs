//! PRD 0047 V-01: a vault entry that is gone is named by its slot kind.

use super::*;
use crate::oauth::{OAuthError, TokenStore};
use crate::{Monitor, Status};
use std::sync::Arc;

/// A vault with nothing in it.
struct EmptyStore;
impl TokenStore for EmptyStore {
    fn load(&self, _id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        Ok(None)
    }
    fn save(&self, _id: &str, _token: &OAuthToken) -> Result<(), OAuthError> {
        Ok(())
    }
    fn delete(&self, _id: &str) -> Result<(), OAuthError> {
        Ok(())
    }
}

struct NoEnv;
impl Credentials for NoEnv {
    fn get(&self, _name: &str) -> Option<String> {
        None
    }
}

async fn first_note(provider: Provider, slot: &str) -> (Status, String) {
    let account = Account {
        id: None,
        name: "Vault".into(),
        provider,
        session: Some(SessionConfig {
            credential_ref: uuid::Uuid::new_v4().to_string(),
            slots: [slot.to_string()].into(),
        }),
        ..Default::default()
    };
    let oauth = OAuthSession::new(Arc::new(EmptyStore), Arc::new(NoEnv)).unwrap();
    let monitor = Monitor::with_login_session(
        Config {
            accounts: vec![account],
            ..Config::default()
        },
        Arc::new(NoEnv),
        Arc::new(oauth),
    )
    .unwrap();
    let report = monitor.refresh_account(0).await;
    (report.status, report.notes_l10n[0].id.to_string())
}

#[tokio::test]
async fn a_missing_vault_key_is_called_a_key() {
    let (status, note) = first_note(Provider::Groq, CREDENTIAL_SLOT).await;
    assert_eq!(status, Status::NeedsConfiguration);
    assert_eq!(note, "user_session.key_not_found");

    let (status, note) = first_note(Provider::XiaomiMimo, "platform_session_env").await;
    assert_eq!(status, Status::NeedsConfiguration);
    assert_eq!(note, "user_session.not_found");
}
