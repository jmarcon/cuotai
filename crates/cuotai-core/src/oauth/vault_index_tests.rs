//! Tests of [`super`]: the index, its schema-2 migration, and "delete only on
//! proof" (2026-11 re-validation F1, F2, R-01). Fake values only.

use super::*;
use crate::config_file::PathSource;
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

fn oauth_account(name: &str, credential_id: &str) -> Account {
    let mut oauth = crate::oauth::OAuthConfig::new(String::new());
    oauth.credential_id = credential_id.into();
    Account {
        name: name.into(),
        provider: Provider::ChatGpt,
        oauth: Some(oauth),
        ..Default::default()
    }
}

const WORK: &str = "4be708d8-7073-4d01-85ba-3306cc3cf087";
const HOME: &str = "0d7e2a52-55a7-4b8e-9d0c-6f1f3b9e2a11";

/// A sandbox: one index, one memory vault, a configuration, `desktop.json`
/// and the approvals file, all in a temporary folder.
struct World {
    dir: tempfile::TempDir,
    index: PathBuf,
    memory: Arc<Memory>,
    store: Arc<dyn TokenStore>,
    session: OAuthSession,
    paths: SourcePaths,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("vault-index.json");
        let memory = Arc::new(Memory::default());
        let store: Arc<dyn TokenStore> = Arc::new(IndexedStore::new(memory.clone(), index.clone()));
        let paths = SourcePaths {
            config: ResolvedPath {
                path: dir.path().join("cuotai.toml"),
                source: PathSource::System,
            },
            prefs: Some(dir.path().join("desktop.json")),
            consent: Some(dir.path().join("consent").join("extensions.json")),
        };
        Self {
            session: session(store.clone()),
            dir,
            index,
            memory,
            store,
            paths,
        }
    }

    fn config_path(&self) -> &Path {
        &self.paths.config.path
    }

    fn prefs(&self) -> &Path {
        self.paths.prefs.as_deref().unwrap()
    }

    fn consent(&self) -> &Path {
        self.paths.consent.as_deref().unwrap()
    }

    /// Write the configuration at `path` with `accounts`.
    fn write_config(&self, path: &Path, accounts: Vec<Account>) -> ConfigFile {
        let _ = std::fs::remove_file(path);
        let mut file = ConfigFile::load(Some(path.to_owned()), true).unwrap();
        let config = Config {
            accounts,
            ..file.config.clone()
        };
        file.save(config).unwrap();
        file
    }

    /// Store a login of `account` as the login commit does (kind only).
    fn store_login(&self, account: &Account) -> String {
        let key = crate::account_edit::account_storage_key(account).unwrap();
        self.store
            .save_tagged(&key, &token("FAKE-LOGIN"), &EntryTag::kind(Kind::Oauth))
            .unwrap();
        key
    }

    async fn plan(&self) -> Plan {
        audit(&self.session, &self.index, &self.paths).await
    }

    async fn status(&self, key: &str) -> Status {
        self.plan()
            .await
            .entries
            .into_iter()
            .find(|entry| entry.key == key)
            .expect("the key is indexed")
            .status
    }

    async fn prune(&self, keys: &[String]) -> PruneOutcome {
        prune_confirmed(&self.session, &self.index, &self.paths, keys)
            .await
            .unwrap()
    }

    fn held(&self, key: &str) -> bool {
        self.memory.load(key).unwrap().is_some()
    }

    fn raw_index(&self) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(&self.index).unwrap()).unwrap()
    }
}

#[tokio::test]
async fn the_index_follows_save_and_delete_and_never_holds_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault-index.json");
    let memory = Arc::new(Memory::default());
    let store = IndexedStore::new(memory.clone(), path.clone());
    store.save("key-a", &token("SECRET-A")).unwrap();
    store
        .save_tagged("key-b", &token("SECRET-B"), &EntryTag::kind(Kind::Session))
        .unwrap();
    store.delete("key-a").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("key-b") && !text.contains("key-a"), "{text}");
    assert!(
        text.contains("\"schema\": 2") && text.contains("\"session\""),
        "{text}"
    );
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

/// Two stores on one index (the CLI and the desktop) never lose a row:
/// every write is one locked read-modify-write.
#[tokio::test]
async fn two_instances_writing_at_once_keep_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault-index.json");
    let memory = Arc::new(Memory::default());
    let threads: Vec<_> = (0..2)
        .map(|n| {
            let store = IndexedStore::new(memory.clone(), path.clone());
            std::thread::spawn(move || {
                for i in 0..20 {
                    store
                        .save_tagged(
                            &format!("k{n}-{i}"),
                            &token("t"),
                            &EntryTag::kind(Kind::Oauth),
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let Index::Known(file) = read(&path) else {
        panic!("a known schema");
    };
    assert_eq!(file.entries.len(), 40);
}

/// The common case still works: an account removed by hand leaves an entry
/// the configuration owned, which is proven unused and pruned.
#[tokio::test]
async fn an_account_removed_by_hand_is_unused_and_pruned() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let home = oauth_account("Home", HOME);
    let work_key = world.store_login(&work);
    let home_key = world.store_login(&home);
    world.write_config(world.config_path(), vec![work.clone(), home]);
    let plan = world.plan().await;
    let used = plan.entries.iter().find(|e| e.key == work_key).unwrap();
    assert_eq!(
        (used.status, used.kind, used.provider, used.orphan),
        (
            Status::InUse,
            Some(Kind::Oauth),
            Some(Provider::ChatGpt),
            false
        )
    );
    assert_eq!(used.account.as_deref(), Some("Work"));
    assert!(!serde_json::to_string(&plan).unwrap().contains("FAKE"));

    world.write_config(world.config_path(), vec![work]);
    let plan = world.plan().await;
    assert_eq!(plan.unused_keys(), vec![home_key.clone()]);
    let outcome = world.prune(&plan.unused_keys()).await;
    assert_eq!(outcome.deleted, vec![home_key.clone()]);
    assert!(!world.held(&home_key) && world.held(&work_key));
    assert_eq!(world.plan().await.entries.len(), 1);
}

/// A failed read is not a missing entry: the vault may still hold it, so
/// prune deletes it, or keeps it indexed when the vault refuses.
#[tokio::test]
async fn an_unused_entry_the_vault_cannot_read_is_never_forgotten() {
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
    for delete_works in [true, false] {
        let mut world = World::new();
        let vault = Arc::new(Unreadable {
            inner: Memory::default(),
            delete_works,
        });
        let store: Arc<dyn TokenStore> =
            Arc::new(IndexedStore::new(vault.clone(), world.index.clone()));
        world.session = session(store.clone());
        let work = oauth_account("Work", WORK);
        let key = crate::account_edit::account_storage_key(&work).unwrap();
        store
            .save_tagged(&key, &token("OLD"), &EntryTag::kind(Kind::Oauth))
            .unwrap();
        world.write_config(world.config_path(), vec![work]);
        world.plan().await;
        world.write_config(world.config_path(), Vec::new());
        let plan = world.plan().await;
        assert!(plan.entries[0].present);
        assert_eq!(plan.entries[0].status, Status::Unused);

        let pruned = prune_confirmed(
            &world.session,
            &world.index,
            &world.paths,
            &plan.unused_keys(),
        )
        .await;
        let held = vault.inner.load(&key).unwrap().is_some();
        let indexed = world.raw_index()["entries"].get(&key).is_some();
        if delete_works {
            assert_eq!(pruned.unwrap().deleted.len(), 1);
            assert!(!held && !indexed);
        } else {
            assert!(pruned.is_err());
            assert!(held && indexed, "held {held}, indexed {indexed}");
        }
    }
}

/// An approvals file with extension `id` approved (manifest digest
/// `digest`) declaring `secrets`, plus `retired` scopes.
fn write_consent(path: &Path, id: &str, digest: &str, secrets: &[&str], retired: &[(&str, &str)]) {
    let mut scopes = serde_json::Map::new();
    for (scope, name) in retired {
        scopes.insert((*scope).into(), serde_json::json!([name]));
    }
    let mut record = serde_json::json!({
        "approved": { id: {
            "manifest_sha256": digest,
            "program_sha256": "00",
            "tree_sha256": "00",
            "env": [],
            "secrets": secrets,
        }},
    });
    if !scopes.is_empty() {
        record["retired"] = serde_json::json!({ id: scopes });
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
}

fn store_secret(world: &World, key: &str) {
    world
        .store
        .save_tagged(
            key,
            &token("FAKE-SECRET"),
            &EntryTag::owned_by(Kind::ExtensionSecret, world.consent()),
        )
        .unwrap();
}

/// F1: an extension secret of a stored approval, and one of a retired
/// scope kept until the new value is stored, are in use; one no approval
/// or retired scope names is the only one pruned.
#[tokio::test]
async fn extension_secrets_of_approvals_and_retired_scopes_are_in_use() {
    let world = World::new();
    world.write_config(world.config_path(), Vec::new());
    let digest = "ab".repeat(32);
    let scope = &digest[..16];
    let old_scope = "cd".repeat(8);
    // An id with `_`, so nothing may parse the key back.
    let id = "my_ext";
    write_consent(
        world.consent(),
        id,
        &digest,
        &["token"],
        &[(&old_scope, "token")],
    );
    let live = crate::extensions::management::secret_key(id, scope, "token");
    let retired = crate::extensions::management::secret_key(id, &old_scope, "token");
    let stale = crate::extensions::management::secret_key(id, &"ef".repeat(8), "token");
    for key in [&live, &retired, &stale] {
        store_secret(&world, key);
    }
    let plan = world.plan().await;
    assert_eq!(world.status(&live).await, Status::InUse);
    assert_eq!(world.status(&retired).await, Status::InUse);
    assert_eq!(plan.unused_keys(), vec![stale.clone()]);
    let all: Vec<String> = plan.entries.iter().map(|e| e.key.clone()).collect();
    let outcome = world.prune(&all).await;
    assert_eq!(outcome.deleted, vec![stale]);
    assert!(world.held(&live) && world.held(&retired));
}

/// F2: an entry another configuration owns is never pruned from this one.
#[tokio::test]
async fn an_entry_of_another_configuration_is_never_pruned() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let key = world.store_login(&work);
    let other = world.dir.path().join("other.toml");
    let other_file = world.write_config(&other, vec![work]);
    claim_config(&world.index, &other_file);
    world.write_config(world.config_path(), Vec::new());
    assert_eq!(world.status(&key).await, Status::OtherOwner);
    let outcome = world.prune(std::slice::from_ref(&key)).await;
    assert!(outcome.deleted.is_empty());
    assert_eq!(
        outcome.skipped[0].reason,
        SkipReason::Changed(Status::OtherOwner)
    );
    assert!(world.held(&key));
    // A key shared by both configurations keeps both owners: unused in
    // one, it is still the other's.
    let shared = oauth_account("Shared", HOME);
    let shared_key = world.store_login(&shared);
    world.write_config(world.config_path(), vec![shared.clone()]);
    world.plan().await;
    let other_file = world.write_config(&other, vec![shared]);
    claim_config(&world.index, &other_file);
    world.write_config(world.config_path(), Vec::new());
    assert_eq!(world.status(&shared_key).await, Status::OtherOwner);
}

/// F2: with no configuration file (the empty fallback) nothing is proven
/// unused.
#[tokio::test]
async fn a_missing_configuration_proves_nothing_unused() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let key = world.store_login(&work);
    world.write_config(world.config_path(), vec![work]);
    world.plan().await;
    std::fs::remove_file(world.config_path()).unwrap();
    let plan = world.plan().await;
    assert_eq!(plan.entries[0].status, Status::Unverifiable);
    assert!(plan.unused_keys().is_empty());
    assert!(world
        .prune(std::slice::from_ref(&key))
        .await
        .deleted
        .is_empty());
    assert!(world.held(&key));
}

/// F2: an account the reader set aside (PRD 0051) may use any entry.
#[tokio::test]
async fn a_configuration_with_accounts_set_aside_proves_nothing_unused() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let key = world.store_login(&work);
    world.write_config(world.config_path(), vec![work]);
    world.plan().await;
    // Work is gone; a table this release cannot read is set aside.
    let text = "[[accounts]]\nname = \"Later\"\nprovider = \"future_provider\"\n";
    std::fs::write(world.config_path(), text).unwrap();
    let file = ConfigFile::open(world.paths.config.clone(), false).unwrap();
    assert_eq!(file.issues.len(), 1, "the table is set aside");
    assert_eq!(world.status(&key).await, Status::Unverifiable);
}

/// R-01: a damaged `desktop.json`, or one with a dropped channel row,
/// turns no channel entry into an unused one.
#[tokio::test]
async fn a_damaged_preferences_file_keeps_channel_entries() {
    let world = World::new();
    world.write_config(world.config_path(), Vec::new());
    let id = "0123456789abcdef";
    let channel = serde_json::json!({
        "id": id, "kind": "webhook", "label": "Phone", "host": "example.com",
        "created_at": "2026-01-01T00:00:00Z", "signed": true,
    });
    let document = serde_json::json!({ "channels": [channel] });
    std::fs::write(world.prefs(), serde_json::to_vec(&document).unwrap()).unwrap();
    let tag = EntryTag::owned_by(Kind::Channel, world.prefs());
    let secret = crate::channels::Vault::secret_key(id);
    world
        .store
        .save_tagged(&secret, &token("FAKE-HMAC"), &tag)
        .unwrap();
    let gone = crate::channels::Vault::url_key("fedcba9876543210");
    world
        .store
        .save_tagged(&gone, &token("FAKE-URL"), &tag)
        .unwrap();
    assert_eq!(world.status(&secret).await, Status::InUse);
    assert_eq!(world.status(&gone).await, Status::Unused);

    std::fs::write(world.prefs(), "{ broken").unwrap();
    let plan = world.plan().await;
    assert!(
        plan.entries
            .iter()
            .all(|e| e.status == Status::Unverifiable),
        "{plan:?}"
    );
    assert!(world
        .prune(&[secret.clone(), gone.clone()])
        .await
        .deleted
        .is_empty());

    // A row Cuotai would not have written is dropped by the reader, so the
    // read is incomplete.
    let bad = serde_json::json!({
        "id": "NOT-HEX", "kind": "ntfy", "label": "x", "host": "h",
        "created_at": "2026-01-01T00:00:00Z",
    });
    let document = serde_json::json!({ "channels": [bad] });
    std::fs::write(world.prefs(), serde_json::to_vec(&document).unwrap()).unwrap();
    assert_eq!(world.status(&gone).await, Status::Unverifiable);
    assert!(world.held(&secret) && world.held(&gone));
}

/// R-01: a damaged or missing approvals file keeps every extension secret.
#[tokio::test]
async fn a_damaged_approvals_file_keeps_extension_secrets() {
    let world = World::new();
    world.write_config(world.config_path(), Vec::new());
    let key = crate::extensions::management::secret_key("ext", &"ab".repeat(8), "token");
    store_secret(&world, &key);
    assert_eq!(world.status(&key).await, Status::Unverifiable, "missing");
    std::fs::create_dir_all(world.consent().parent().unwrap()).unwrap();
    std::fs::write(world.consent(), "{ broken").unwrap();
    assert_eq!(world.status(&key).await, Status::Unverifiable, "damaged");
    assert!(world
        .prune(std::slice::from_ref(&key))
        .await
        .deleted
        .is_empty());
    assert!(world.held(&key));
}

/// F2: prune deletes the confirmed list, judged again from disk, not
/// whatever is unused at that moment.
#[tokio::test]
async fn prune_deletes_only_confirmed_keys_still_unused() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let home = oauth_account("Home", HOME);
    let work_key = world.store_login(&work);
    let home_key = world.store_login(&home);
    world.write_config(world.config_path(), vec![work.clone(), home.clone()]);
    world.plan().await;
    world.write_config(world.config_path(), vec![home]);
    let confirmed = world.plan().await.unused_keys();
    assert_eq!(confirmed, vec![work_key.clone()]);
    // The file changes after the confirmation: Work is back, Home is gone.
    world.write_config(world.config_path(), vec![work]);
    let outcome = world
        .prune(&[confirmed[0].clone(), "never-indexed".into()])
        .await;
    assert!(outcome.deleted.is_empty(), "{outcome:?}");
    assert_eq!(
        outcome.skipped,
        vec![
            Skipped {
                key: work_key.clone(),
                reason: SkipReason::Changed(Status::InUse)
            },
            Skipped {
                key: "never-indexed".into(),
                reason: SkipReason::Gone
            },
        ]
    );
    // Home became unused but was never confirmed: it stays.
    assert!(world.held(&work_key) && world.held(&home_key));
}

/// An untagged key, an unknown-shape key and a kind of a newer Cuotai are
/// never unused; they are removed only one by one, and an entry in use is
/// refused.
#[tokio::test]
async fn an_entry_of_unknown_kind_is_never_an_orphan() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let work_key = world.store_login(&work);
    world.write_config(world.config_path(), vec![work]);
    world.store.save("xyz_untagged", &token("t")).unwrap();
    update(&world.index, |file| {
        let mut entry = Entry::new();
        entry.kind = Some("future_kind".into());
        entry.owners.insert("someone".into());
        file.entries.insert("future-key".into(), entry);
    });
    world.memory.save("future-key", &token("t")).unwrap();
    let plan = world.plan().await;
    for key in ["xyz_untagged", "future-key"] {
        assert_eq!(world.status(key).await, Status::Unclassified);
    }
    assert!(plan.unused_keys().is_empty());
    let all: Vec<String> = plan.entries.iter().map(|e| e.key.clone()).collect();
    assert!(world.prune(&all).await.deleted.is_empty());
    // The kind a newer Cuotai wrote survives a rewrite of the index.
    assert_eq!(
        world.raw_index()["entries"]["future-key"]["kind"],
        "future_kind"
    );

    let removed = remove_one(&world.session, &world.index, &world.paths, "xyz_untagged").await;
    assert_eq!(removed.unwrap(), Status::Unclassified);
    assert!(!world.held("xyz_untagged"));
    let refused = remove_one(&world.session, &world.index, &world.paths, &work_key).await;
    assert_eq!(refused.unwrap_err().id, "vault.remove.in_use");
    let unknown = remove_one(&world.session, &world.index, &world.paths, "nope").await;
    assert_eq!(unknown.unwrap_err().id, "vault.remove.not_indexed");
    assert!(world.held(&work_key));
}

/// Schema 1 rows read as "owner unknown"; one the configuration references
/// now is stamped with its kind and owner, atomically, in schema 2. An
/// index of a newer schema is listed as unclassified and never written.
#[tokio::test]
async fn schema_1_rows_migrate_only_by_positive_proof() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let work_key = crate::account_edit::account_storage_key(&work).unwrap();
    world.memory.save(&work_key, &token("t")).unwrap();
    world.memory.save("old-orphan", &token("t")).unwrap();
    let schema_1 = serde_json::json!({ "schema": 1, "entries": {
        work_key.clone(): { "created_at": "2026-10-01T00:00:00Z" },
        "old-orphan": { "created_at": "2026-10-01T00:00:00Z" },
    }});
    std::fs::write(&world.index, serde_json::to_vec(&schema_1).unwrap()).unwrap();
    let file = world.write_config(world.config_path(), vec![work]);

    assert_eq!(world.status(&work_key).await, Status::InUse);
    assert_eq!(world.status("old-orphan").await, Status::OwnerUnknown);
    let raw = world.raw_index();
    assert_eq!(raw["schema"], 2);
    let digest = file.identity().unwrap().digest();
    assert_eq!(raw["entries"][&work_key]["kind"], "oauth");
    assert_eq!(
        raw["entries"][&work_key]["owners"],
        serde_json::json!([digest])
    );
    assert!(raw["entries"][&work_key].get("legacy").is_none());
    assert_eq!(raw["entries"]["old-orphan"]["legacy"], true);
    assert!(world.plan().await.unused_keys().is_empty());
    assert!(world.prune(&["old-orphan".into()]).await.deleted.is_empty());
    // Removed one by one, it goes.
    let removed = remove_one(&world.session, &world.index, &world.paths, "old-orphan").await;
    assert_eq!(removed.unwrap(), Status::OwnerUnknown);

    let newer =
        r#"{"schema":3,"entries":{"k":{"created_at":"2026-10-01T00:00:00Z","shape":"new"}}}"#;
    std::fs::write(&world.index, newer).unwrap();
    world.store.save("another", &token("t")).unwrap();
    let plan = world.plan().await;
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.entries[0].status, Status::Unclassified);
    assert_eq!(std::fs::read_to_string(&world.index).unwrap(), newer);
    assert!(world.prune(&["k".into()]).await.deleted.is_empty());
}

/// Every key builder written through the index with its tag audits as
/// that kind while its owning file references it.
#[tokio::test]
async fn each_kind_written_with_its_tag_audits_as_that_kind() {
    let world = World::new();
    let work = oauth_account("Work", WORK);
    let oauth_key = world.store_login(&work);
    world.write_config(world.config_path(), vec![work]);
    let digest = "12".repeat(32);
    write_consent(world.consent(), "ext", &digest, &["token"], &[]);
    let secret = crate::extensions::management::secret_key("ext", &digest[..16], "token");
    store_secret(&world, &secret);
    let plan = world.plan().await;
    let kind = |key: &str| plan.entries.iter().find(|e| e.key == key).unwrap().kind;
    assert_eq!(kind(&oauth_key), Some(Kind::Oauth));
    assert_eq!(kind(&secret), Some(Kind::ExtensionSecret));
    assert_eq!(owning_source(Kind::Session), SourceId::Config);
    assert_eq!(owning_source(Kind::Channel), SourceId::Prefs);
}
