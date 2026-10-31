//! PRD 0029 RF6 (FIX-07): an index of the vault entries Cuotai created.
//!
//! [`TokenStore`] can load, save and delete, but not list, so an entry left
//! behind (an account removed by hand from the TOML, a crash between a vault
//! write and a save) was invisible. [`IndexedStore`] wraps any store and
//! records each key it saves, with the time, in
//! `<OS local data dir>/cuotai/vault-index.json` (`0600`); a delete removes
//! the key. The index never holds a value. A missing or corrupt index reads
//! as empty and an index that cannot be written is ignored: the credential
//! operation itself never fails because of it.
//!
//! The store sees only opaque keys, so what an entry belongs to (kind,
//! provider, account id) is resolved against the configuration when it is
//! audited ([`audit`]): an indexed key no configured account references is
//! an orphan, and [`prune`] deletes orphans.

use super::{OAuthError, OAuthSession, OAuthToken, TokenStore};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Version of `vault-index.json`.
const SCHEMA: u32 = 1;

/// `<OS local data dir>/cuotai/vault-index.json`, or `None` without one.
pub fn default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai").join("vault-index.json"))
}

/// The index of the debug-only SQLite store at `database`: beside it, as
/// `<database>.vault-index.json`, apart from the OS vault's index.
pub fn development_path(database: &Path) -> PathBuf {
    let mut name = database.file_name().unwrap_or_default().to_os_string();
    name.push(".vault-index.json");
    database.with_file_name(name)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct IndexFile {
    schema: u32,
    /// Storage key → when Cuotai first wrote it.
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    created_at: DateTime<Utc>,
}

fn read(path: &Path) -> IndexFile {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<IndexFile>(&bytes).ok())
        .filter(|file| file.schema == SCHEMA)
        .unwrap_or_default()
}

fn write(path: &Path, file: &IndexFile) {
    let file = IndexFile {
        schema: SCHEMA,
        entries: file.entries.clone(),
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&file) {
        let _ = crate::private_fs::write_file(path, &bytes);
    }
}

/// A [`TokenStore`] that keeps the index of the keys it saved.
pub struct IndexedStore {
    inner: Arc<dyn TokenStore>,
    path: PathBuf,
    /// Serializes read-modify-write of the index within the process.
    lock: Mutex<()>,
}

impl IndexedStore {
    pub fn new(inner: Arc<dyn TokenStore>, path: PathBuf) -> Self {
        Self {
            inner,
            path,
            lock: Mutex::new(()),
        }
    }

    fn update(&self, change: impl FnOnce(&mut IndexFile)) {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut file = read(&self.path);
        change(&mut file);
        write(&self.path, &file);
    }
}

impl TokenStore for IndexedStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        self.inner.load(id)
    }

    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        self.inner.save(id, token)?;
        self.update(|file| {
            file.entries.entry(id.to_owned()).or_insert(Entry {
                created_at: Utc::now(),
            });
        });
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        self.inner.delete(id)?;
        self.update(|file| {
            file.entries.remove(id);
        });
        Ok(())
    }
}

/// What an indexed entry is, as the configuration says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Oauth,
    Session,
}

/// One indexed vault entry. Never carries the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditEntry {
    /// The opaque storage key (a UUID, not a secret).
    pub key: String,
    pub created_at: DateTime<Utc>,
    /// `None` for an orphan.
    pub kind: Option<Kind>,
    pub provider: Option<crate::Provider>,
    pub account_id: Option<String>,
    /// The account's display name.
    pub account: Option<String>,
    /// No configured account references it.
    pub orphan: bool,
    /// The vault still holds it (checked with a load whose value is
    /// dropped unread); `false` only when it is known to be gone, so an
    /// entry the vault failed to read counts as held.
    pub present: bool,
}

/// The keys `config` references, with what they belong to.
fn references(config: &crate::Config) -> BTreeMap<String, (Kind, &crate::Account)> {
    let mut keys = BTreeMap::new();
    for account in &config.accounts {
        if let Some(key) = crate::account_edit::account_storage_key(account) {
            keys.insert(key, (Kind::Oauth, account));
        }
        for key in crate::user_session::account_session_keys(account) {
            keys.insert(key, (Kind::Session, account));
        }
    }
    keys
}

/// The entries of the index at `path`, matched against `config`.
pub async fn audit(session: &OAuthSession, path: &Path, config: &crate::Config) -> Vec<AuditEntry> {
    let index = read(path);
    let references = references(config);
    let mut entries = Vec::with_capacity(index.entries.len());
    for (key, entry) in index.entries {
        let present = !matches!(session.load_by_key(&key).await, Ok(None));
        let reference = references.get(&key);
        entries.push(AuditEntry {
            created_at: entry.created_at,
            kind: reference.map(|(kind, _)| *kind),
            provider: reference.map(|(_, account)| account.provider),
            account_id: reference.map(|(_, account)| account.state_key()),
            account: reference.map(|(_, account)| account.display_name().into_owned()),
            orphan: reference.is_none(),
            present,
            key,
        });
    }
    entries
}

/// Delete every orphan of [`audit`]; returns how many were deleted. An
/// orphan already gone from the vault only leaves the index. Stops at the
/// first vault error, which never carries a value.
pub async fn prune(
    session: &OAuthSession,
    path: &Path,
    config: &crate::Config,
) -> Result<usize, crate::text::Localized> {
    let mut deleted = 0;
    for entry in audit(session, path, config).await {
        if !entry.orphan {
            continue;
        }
        if entry.present {
            session
                .delete_by_key(entry.key.clone())
                .await
                .map_err(|error| error.l10n_for_purge())?;
            deleted += 1;
        }
        // A delete through an `IndexedStore` already left the index; an
        // entry gone from the vault (or a store without the index) leaves
        // it here.
        let mut file = read(path);
        if file.entries.remove(&entry.key).is_some() {
            write(path, &file);
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Account, Config, Provider};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, OAuthToken>>);
    impl TokenStore for Memory {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
            self.0.lock().unwrap().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), OAuthError> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    fn token(value: &str) -> OAuthToken {
        OAuthToken {
            access_token: value.into(),
            refresh_token: None,
            expires_at: None,
            metadata: BTreeMap::new(),
        }
    }

    fn session(store: Arc<dyn TokenStore>) -> OAuthSession {
        struct None_;
        impl crate::Credentials for None_ {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
        }
        OAuthSession::new(store, Arc::new(None_)).unwrap()
    }

    fn oauth_account(name: &str) -> Account {
        let mut oauth = crate::oauth::OAuthConfig::new(String::new());
        oauth.credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087".into();
        Account {
            name: name.into(),
            provider: Provider::ChatGpt,
            oauth: Some(oauth),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn the_index_follows_save_and_delete_and_never_holds_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault-index.json");
        let memory = Arc::new(Memory::default());
        let store = IndexedStore::new(memory.clone(), path.clone());
        store.save("key-a", &token("SECRET-A")).unwrap();
        store.save("key-b", &token("SECRET-B")).unwrap();
        store.delete("key-a").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("key-b") && !text.contains("key-a"), "{text}");
        assert!(!text.contains("SECRET"), "{text}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn a_corrupt_index_never_blocks_a_credential_operation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault-index.json");
        std::fs::write(&path, "{not json").unwrap();
        let memory = Arc::new(Memory::default());
        let store = IndexedStore::new(memory.clone(), path.clone());
        store.save("key-a", &token("t")).unwrap();
        assert!(memory.load("key-a").unwrap().is_some());
        assert!(std::fs::read_to_string(&path).unwrap().contains("key-a"));
        // An index that cannot be written (its directory is a file) is
        // ignored as well.
        let blocked = dir.path().join("file");
        std::fs::write(&blocked, "").unwrap();
        let store = IndexedStore::new(memory.clone(), blocked.join("vault-index.json"));
        store.save("key-b", &token("t")).unwrap();
        store.delete("key-b").unwrap();
    }

    /// A store that holds entries it cannot read back.
    struct Unreadable {
        inner: Memory,
        delete_works: bool,
    }
    impl TokenStore for Unreadable {
        fn load(&self, _: &str) -> Result<Option<OAuthToken>, OAuthError> {
            Err(OAuthError::Storage(crate::text::Localized::fixed(
                "test.vault_locked",
                "Vault unavailable.",
            )))
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
            self.inner.save(id, token)
        }
        fn delete(&self, id: &str) -> Result<(), OAuthError> {
            if self.delete_works {
                self.inner.delete(id)
            } else {
                self.load(id).map(|_| ())
            }
        }
    }

    /// A failed read is not a missing entry: the vault may still hold it, so
    /// `prune` must delete it, or keep it indexed when the vault refuses.
    #[tokio::test]
    async fn an_orphan_the_vault_cannot_read_is_never_forgotten() {
        for delete_works in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("vault-index.json");
            let vault = Arc::new(Unreadable {
                inner: Memory::default(),
                delete_works,
            });
            let store: Arc<dyn TokenStore> =
                Arc::new(IndexedStore::new(vault.clone(), path.clone()));
            store.save("left-behind", &token("OLD")).unwrap();
            let session = session(store);
            let config = Config::default();
            assert!(audit(&session, &path, &config).await[0].present);

            let pruned = prune(&session, &path, &config).await;
            let held = vault.inner.load("left-behind").unwrap().is_some();
            let indexed = read(&path).entries.contains_key("left-behind");
            if delete_works {
                assert_eq!(pruned.unwrap(), 1);
                assert!(!held && !indexed);
            } else {
                assert!(pruned.is_err());
                assert!(held && indexed, "held {held}, indexed {indexed}");
            }
        }
    }

    #[tokio::test]
    async fn an_orphan_after_a_manual_edit_is_listed_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault-index.json");
        let memory = Arc::new(Memory::default());
        let store: Arc<dyn TokenStore> = Arc::new(IndexedStore::new(memory.clone(), path.clone()));
        let session = session(store.clone());
        let account = oauth_account("Work");
        let key = crate::account_edit::account_storage_key(&account).unwrap();
        store.save(&key, &token("SECRET")).unwrap();
        store.save("left-behind", &token("OLD")).unwrap();
        let config = Config {
            accounts: vec![account],
            ..Config::default()
        };
        let entries = audit(&session, &path, &config).await;
        assert_eq!(entries.len(), 2);
        let used = entries.iter().find(|e| e.key == key).unwrap();
        assert_eq!(
            (used.kind, used.provider, used.orphan, used.present),
            (Some(Kind::Oauth), Some(Provider::ChatGpt), false, true)
        );
        assert_eq!(used.account.as_deref(), Some("Work"));
        let orphan = entries.iter().find(|e| e.key == "left-behind").unwrap();
        assert!(orphan.orphan && orphan.present && orphan.kind.is_none());
        assert!(!serde_json::to_string(&entries).unwrap().contains("SECRET"));

        assert_eq!(prune(&session, &path, &config).await.unwrap(), 1);
        assert!(memory.load("left-behind").unwrap().is_none());
        assert!(memory.load(&key).unwrap().is_some());
        let entries = audit(&session, &path, &config).await;
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].orphan);
    }
}
