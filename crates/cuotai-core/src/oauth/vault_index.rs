//! PRD 0029 RF6 (FIX-07): an index of the vault entries Cuotai created.
//!
//! [`TokenStore`] can load, save and delete, but not list, so an entry left
//! behind (an account removed by hand from the TOML, a crash between a vault
//! write and a save) was invisible. [`IndexedStore`] wraps any store and
//! records each key it saves, with the time, in
//! `<OS local data dir>/cuotai/vault-index.json` (`0600`); a delete removes
//! the key. The index never holds a value. A missing or corrupt index reads
//! as empty and a write that keeps failing after a few attempts is logged
//! and ignored: the credential operation itself never fails because of it.
//!
//! # Delete only on proof (2026-11 re-validation F1, F2, R-01)
//!
//! One index serves every configuration of the OS user, and its entries
//! belong to three kinds of owning file: the TOML (OAuth sign-ins and web
//! sessions), `desktop.json` (notification channels) and the extensions'
//! `consent.json` (extension secrets). So each row records, when it is
//! written ([`TokenStore::save_tagged`]), its [`Kind`] and the
//! [`crate::config_file::ConfigIdentity`] digest of its owning file
//! (`owners`, a set: a copied `credential_id` is shared by two configs).
//! An audit ([`audit`]) re-reads the owning files from disk and gives each
//! entry a [`Status`]; only [`Status::Unused`] is pruned in bulk, and only
//! when its kind is known, its sole owner is the file loaded now, that file
//! was read cleanly and it does not reference the entry. Anything Cuotai
//! cannot prove unused is kept and listed with its reason.
//!
//! Schema 2 of the index adds `kind`, `owners` and `legacy`. A schema-1 row
//! reads as `legacy` ("written before ownership"); a row gains an owner
//! only by positive proof: a tagged write, or an audit in which the cleanly
//! read owning file references it. An index of a newer schema is never
//! written and all its entries are [`Status::Unclassified`]. Every write is
//! one locked read-modify-write ([`crate::private_fs::update_locked`]), so
//! the CLI and the desktop can run at once.

use super::{OAuthError, OAuthSession, OAuthToken, TokenStore};
use crate::config_file::{ConfigFile, ConfigIdentity, ResolvedPath};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Version of `vault-index.json`.
const SCHEMA: u32 = 2;

/// `<OS local data dir>/cuotai/vault-index.json`, or `None` without one.
pub fn default_path() -> Option<PathBuf> {
    crate::data_dir::root().map(|dir| dir.join("vault-index.json"))
}

/// The index of the debug-only SQLite store at `database`: beside it, as
/// `<database>.vault-index.json`, apart from the OS vault's index.
pub fn development_path(database: &Path) -> PathBuf {
    let mut name = database.file_name().unwrap_or_default().to_os_string();
    name.push(".vault-index.json");
    database.with_file_name(name)
}

/// What an indexed entry is. Recorded when it is written; adding a variant
/// fails to compile until [`owning_source`] gives it a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Oauth,
    Session,
    /// PRD 0044: a notification channel's address or signing key.
    Channel,
    /// PRD 0028 RF7.4: a declared secret of an approved extension.
    ExtensionSecret,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
            Self::Session => "session",
            Self::Channel => "channel",
            Self::ExtensionSecret => "extension_secret",
        }
    }

    /// A kind this build knows; `None` for one a newer Cuotai wrote.
    fn parse(text: &str) -> Option<Self> {
        [
            Self::Oauth,
            Self::Session,
            Self::Channel,
            Self::ExtensionSecret,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }
}

/// What a tagged write records: the entry's kind and, when the writer
/// knows it, the digest of its owning file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTag {
    pub kind: Kind,
    pub owner: Option<String>,
}

impl EntryTag {
    /// A tag without an owner (the owner is proven later by an audit).
    pub fn kind(kind: Kind) -> Self {
        Self { kind, owner: None }
    }

    /// A tag owned by the file at `path` (its [`ConfigIdentity`] digest).
    pub fn owned_by(kind: Kind, path: &Path) -> Self {
        Self {
            kind,
            owner: owner_of(path),
        }
    }
}

/// The owner id of the file at `path`: its [`ConfigIdentity`] digest, the
/// same value `export::v1` publishes as `config_identity`, so the index
/// never holds a path.
fn owner_of(path: &Path) -> Option<String> {
    ConfigIdentity::of_path(path)
        .ok()
        .map(|identity| identity.digest())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct IndexFile {
    schema: u32,
    /// Storage key → what Cuotai knows of it.
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    created_at: DateTime<Utc>,
    /// The [`Kind`] as written; kept as text so a kind of a newer Cuotai
    /// survives a rewrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    /// Owner ids ([`owner_of`]) proven by a tagged write or an audit.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    owners: BTreeSet<String>,
    /// A row of schema 1, written before kinds and owners were recorded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    legacy: bool,
}

impl Entry {
    fn new() -> Self {
        Self {
            created_at: Utc::now(),
            kind: None,
            owners: BTreeSet::new(),
            legacy: false,
        }
    }

    fn stamp(&mut self, kind: Kind, owner: Option<&str>) -> bool {
        let before = (self.kind.clone(), self.owners.len(), self.legacy);
        self.kind = Some(kind.as_str().to_owned());
        if let Some(owner) = owner {
            self.owners.insert(owner.to_owned());
        }
        self.legacy = false;
        before != (self.kind.clone(), self.owners.len(), self.legacy)
    }
}

/// The index as read.
enum Index {
    /// Schema 1 (migrated in memory) or 2, or no index yet.
    Known(IndexFile),
    /// A newer schema: listed as unclassified and never written.
    Newer(Vec<(String, DateTime<Utc>)>),
}

fn parse(bytes: Option<&[u8]>) -> Index {
    #[derive(Deserialize)]
    struct Head {
        schema: u32,
    }
    let Some(bytes) = bytes else {
        return Index::Known(IndexFile::default());
    };
    match serde_json::from_slice::<Head>(bytes) {
        Ok(head) if head.schema > SCHEMA => {
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap_or_default();
            let entries = value
                .get("entries")
                .and_then(serde_json::Value::as_object)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|(key, entry)| {
                            let at = entry
                                .get("created_at")
                                .cloned()
                                .and_then(|at| serde_json::from_value(at).ok())
                                .unwrap_or_default();
                            (key.clone(), at)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Index::Newer(entries)
        }
        Ok(head) => match serde_json::from_slice::<IndexFile>(bytes) {
            Ok(mut file) => {
                if head.schema < SCHEMA {
                    for entry in file.entries.values_mut() {
                        *entry = Entry {
                            created_at: entry.created_at,
                            kind: None,
                            owners: BTreeSet::new(),
                            legacy: true,
                        };
                    }
                }
                Index::Known(file)
            }
            Err(_) => Index::Known(IndexFile::default()),
        },
        // A corrupt index reads as empty (and is replaced on the next write).
        Err(_) => Index::Known(IndexFile::default()),
    }
}

fn read(path: &Path) -> Index {
    parse(std::fs::read(path).ok().as_deref())
}

/// How many times [`update`] tries a write that failed for a reason that
/// may pass (another writer held the lock past the bounded wait, or an I/O
/// step such as a rename a scanner blocked).
const UPDATE_ATTEMPTS: usize = 3;

/// One locked read-modify-write of the index, shared by every Cuotai
/// process: the migration to schema 2 happens on the first write, in the
/// same atomic replace. An index of a newer schema is left alone. A busy
/// lock or an I/O failure is tried again up to [`UPDATE_ATTEMPTS`] times,
/// since a lost write is a credential missing from the list and from
/// `--vault-prune`; what still fails is logged, never returned (the index
/// never blocks a credential operation).
fn update(path: &Path, change: impl Fn(&mut IndexFile)) {
    use crate::diagnostics::logging::{event, Field, Level};
    use crate::private_fs::UpdateError;
    for attempt in 1..=UPDATE_ATTEMPTS {
        let result =
            crate::private_fs::update_locked(path, |bytes| match parse(bytes.as_deref()) {
                Index::Newer(_) => Err(()),
                Index::Known(mut file) => {
                    change(&mut file);
                    file.schema = SCHEMA;
                    serde_json::to_vec_pretty(&file).map_err(|_| ())
                }
            });
        let (reason, lasting) = match result {
            Ok(()) | Err(UpdateError::Refused(())) => return,
            Err(UpdateError::NotPrivate) => ("not_private", true),
            Err(UpdateError::Busy) => ("busy", false),
            Err(UpdateError::Io(_)) => ("io", false),
        };
        if lasting || attempt == UPDATE_ATTEMPTS {
            event(
                Level::Warn,
                "vault_index.write_failed",
                &[("reason", Field::Text(reason))],
            );
            return;
        }
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

    fn update(&self, change: impl Fn(&mut IndexFile)) {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        update(&self.path, change);
    }
}

impl TokenStore for IndexedStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        self.inner.load(id)
    }

    /// An untagged write: a new row has no kind, so it is never pruned in
    /// bulk; an existing row keeps what it knew.
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        self.inner.save(id, token)?;
        self.update(|file| {
            file.entries.entry(id.to_owned()).or_insert_with(Entry::new);
        });
        Ok(())
    }

    fn save_tagged(&self, id: &str, token: &OAuthToken, tag: &EntryTag) -> Result<(), OAuthError> {
        self.inner.save(id, token)?;
        self.update(|file| {
            file.entries
                .entry(id.to_owned())
                .or_insert_with(Entry::new)
                .stamp(tag.kind, tag.owner.as_deref());
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

// ---------------------------------------------------------------------------
// The owning files.

/// Which owning file decides about an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceId {
    Config,
    Prefs,
    Consent,
}

/// The one rule per kind: which file owns it. Exhaustive on purpose.
fn owning_source(kind: Kind) -> SourceId {
    match kind {
        Kind::Oauth | Kind::Session => SourceId::Config,
        Kind::Channel => SourceId::Prefs,
        Kind::ExtensionSecret => SourceId::Consent,
    }
}

/// Where the files that own vault entries are.
#[derive(Debug, Clone)]
pub struct SourcePaths {
    /// The configuration judged against, re-opened from disk each time.
    pub config: ResolvedPath,
    /// `desktop.json` (PRD 0044 channels).
    pub prefs: Option<PathBuf>,
    /// The extensions' approvals (PRD 0028 RF7.4).
    pub consent: Option<PathBuf>,
}

impl SourcePaths {
    /// `config` with the OS paths of `desktop.json` and the approvals.
    pub fn for_config(config: ResolvedPath) -> Self {
        Self {
            config,
            prefs: crate::notify_prefs::settings_path(),
            consent: crate::extensions::default_dir()
                .map(|root| crate::extensions::consent::Consent::file_for(&root)),
        }
    }

    /// The paths of the configuration `file` was loaded from.
    pub fn for_file(file: &ConfigFile) -> Self {
        Self::for_config(ResolvedPath {
            path: file.path.clone(),
            source: file.source,
        })
    }
}

/// What a reference says about an entry.
#[derive(Debug, Clone)]
struct Reference {
    kind: Kind,
    provider: Option<crate::Provider>,
    account_id: Option<String>,
    account: Option<String>,
    named: bool,
}

impl Reference {
    fn bare(kind: Kind) -> Self {
        Self {
            kind,
            provider: None,
            account_id: None,
            account: None,
            named: false,
        }
    }
}

/// One owning file as read now.
#[derive(Debug, Default)]
struct Source {
    /// Its owner id; `None` when its path cannot be resolved.
    owner: Option<String>,
    /// Read completely: present, parsed, and nothing set aside or dropped.
    clean: bool,
    refs: BTreeMap<String, Reference>,
}

/// The owning files, re-read from disk.
#[derive(Debug, Default)]
struct Sources {
    config: Source,
    prefs: Source,
    consent: Source,
}

impl Sources {
    fn read(paths: &SourcePaths) -> Self {
        Self {
            config: config_source(&paths.config),
            prefs: paths.prefs.as_deref().map(prefs_source).unwrap_or_default(),
            consent: paths
                .consent
                .as_deref()
                .map(consent_source)
                .unwrap_or_default(),
        }
    }

    fn get(&self, id: SourceId) -> &Source {
        match id {
            SourceId::Config => &self.config,
            SourceId::Prefs => &self.prefs,
            SourceId::Consent => &self.consent,
        }
    }

    fn reference(&self, key: &str) -> Option<(SourceId, &Reference)> {
        [SourceId::Config, SourceId::Prefs, SourceId::Consent]
            .into_iter()
            .find_map(|id| self.get(id).refs.get(key).map(|found| (id, found)))
    }
}

/// The keys a configuration references. A missing file (the
/// `Config::default()` fallback), a file that does not open, and one with
/// accounts set aside (PRD 0051) are not clean: an account Cuotai could not
/// read may use any entry.
fn config_source(resolved: &ResolvedPath) -> Source {
    let owner = owner_of(&resolved.path);
    let Ok(file) = ConfigFile::open(resolved.clone(), false) else {
        return Source {
            owner,
            ..Source::default()
        };
    };
    Source {
        owner,
        clean: file.exists && file.issues.is_empty(),
        refs: references(&file.config),
    }
}

fn references(config: &crate::Config) -> BTreeMap<String, Reference> {
    let mut keys = BTreeMap::new();
    for account in &config.accounts {
        let reference = |kind| Reference {
            kind,
            provider: Some(account.provider),
            account_id: Some(account.state_key()),
            account: Some(account.display_name().into_owned()),
            named: account.named(),
        };
        if let Some(key) = crate::account_edit::account_storage_key(account) {
            keys.insert(key, reference(Kind::Oauth));
        }
        for key in crate::user_session::account_session_keys(account) {
            keys.insert(key, reference(Kind::Session));
        }
    }
    keys
}

fn prefs_source(path: &Path) -> Source {
    let (channels, clean) = match crate::channels::read_checked(path) {
        Some(read) => (read.channels, read.complete),
        None => (Vec::new(), false),
    };
    let mut refs = BTreeMap::new();
    for channel in channels {
        for key in [
            crate::channels::Vault::url_key(&channel.id),
            crate::channels::Vault::secret_key(&channel.id),
        ] {
            refs.insert(key, Reference::bare(Kind::Channel));
        }
    }
    Source {
        owner: owner_of(path),
        clean,
        refs,
    }
}

fn consent_source(path: &Path) -> Source {
    let (keys, clean) = match crate::extensions::consent::Consent::at(path).live_secret_keys() {
        Ok(keys) => (keys, true),
        Err(()) => (BTreeSet::new(), false),
    };
    Source {
        owner: owner_of(path),
        clean,
        refs: keys
            .into_iter()
            .map(|key| (key, Reference::bare(Kind::ExtensionSecret)))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// The audit.

/// Why an entry is or is not pruned in bulk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Its owning file, as read now, references it.
    InUse,
    /// Proven unused: known kind, sole owner is the file loaded now, read
    /// cleanly, which does not reference it. The only bulk-pruned status.
    Unused,
    /// Another configuration, preferences or approvals file owns it.
    OtherOwner,
    /// Written before owners were recorded (or never proven) and nothing
    /// references it now.
    OwnerUnknown,
    /// Its owning file is missing, damaged, unreadable or has accounts set
    /// aside, so whether it is used cannot be told.
    Unverifiable,
    /// Its kind is unknown (an untagged write, or a newer Cuotai's).
    Unclassified,
}

/// One indexed vault entry. Never carries the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditEntry {
    /// The opaque storage key (a UUID or a channel/extension key, not a
    /// secret).
    pub key: String,
    pub created_at: DateTime<Utc>,
    /// What it is, when recorded or proven.
    pub kind: Option<Kind>,
    pub provider: Option<crate::Provider>,
    pub account_id: Option<String>,
    /// The account's display name.
    pub account: Option<String>,
    /// PRD 0029 RF3.4: whether that account has a name of its own (an
    /// unnamed one is titled by its provider once, see
    /// [`crate::display_title`]).
    pub named: bool,
    pub status: Status,
    /// `status == Unused` (kept for the `--vault-audit --json` schema 1).
    pub orphan: bool,
    /// The vault still holds it (checked with a load whose value is
    /// dropped unread); `false` only when it is known to be gone, so an
    /// entry the vault failed to read counts as held.
    pub present: bool,
}

/// The audit of the whole index against the files loaded now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The configuration file judged against.
    pub config_path: PathBuf,
    pub entries: Vec<AuditEntry>,
}

impl Plan {
    /// The keys a bulk prune deletes.
    pub fn unused_keys(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.status == Status::Unused)
            .map(|entry| entry.key.clone())
            .collect()
    }
}

/// The status of one row, with its kind and reference.
fn judge(key: &str, entry: &Entry, sources: &Sources) -> (Status, Option<Kind>, Option<Reference>) {
    if let Some((_, found)) = sources.reference(key) {
        return (Status::InUse, Some(found.kind), Some(found.clone()));
    }
    let Some(kind) = entry.kind.as_deref().and_then(Kind::parse) else {
        let status = if entry.legacy && entry.kind.is_none() {
            Status::OwnerUnknown
        } else {
            Status::Unclassified
        };
        return (status, None, None);
    };
    let source = sources.get(owning_source(kind));
    let status = match &source.owner {
        _ if !source.clean => Status::Unverifiable,
        None => Status::Unverifiable,
        Some(_) if entry.owners.is_empty() => Status::OwnerUnknown,
        Some(owner) if entry.owners.iter().any(|known| known != owner) => Status::OtherOwner,
        Some(_) => Status::Unused,
    };
    (status, Some(kind), None)
}

/// Stamp the rows the owning files reference now (positive proof only).
fn claim(index: &Path, sources: &Sources) {
    let Index::Known(file) = read(index) else {
        return;
    };
    let stale = file.entries.iter().any(|(key, entry)| {
        sources.reference(key).is_some_and(|(id, found)| {
            let owner = sources.get(id).owner.as_deref();
            entry.legacy
                || entry.kind.as_deref() != Some(found.kind.as_str())
                || owner.is_some_and(|owner| !entry.owners.contains(owner))
        })
    });
    if !stale {
        return;
    }
    update(index, |file| {
        for (key, entry) in &mut file.entries {
            if let Some((id, found)) = sources.reference(key) {
                entry.stamp(found.kind, sources.get(id).owner.as_deref());
            }
        }
    });
}

/// Record the configuration `file` as the owner of the entries it
/// references now (and their kinds), so a later audit can prove one of
/// them unused once the account is gone. Front ends call it after loading
/// a configuration; it never deletes or reads a value.
pub fn claim_config(index: &Path, file: &ConfigFile) {
    let sources = Sources {
        config: Source {
            owner: file.identity().ok().map(|identity| identity.digest()),
            clean: false,
            refs: references(&file.config),
        },
        ..Sources::default()
    };
    claim(index, &sources);
}

async fn audit_sources(session: &OAuthSession, index: &Path, sources: &Sources) -> Vec<AuditEntry> {
    claim(index, sources);
    let rows: Vec<(String, Option<Entry>, DateTime<Utc>)> = match read(index) {
        Index::Known(file) => file
            .entries
            .into_iter()
            .map(|(key, entry)| {
                let at = entry.created_at;
                (key, Some(entry), at)
            })
            .collect(),
        Index::Newer(entries) => entries
            .into_iter()
            .map(|(key, at)| (key, None, at))
            .collect(),
    };
    let mut entries = Vec::with_capacity(rows.len());
    for (key, entry, created_at) in rows {
        let present = !matches!(session.load_by_key(&key).await, Ok(None));
        let (status, kind, reference) = match &entry {
            Some(entry) => judge(&key, entry, sources),
            None => (Status::Unclassified, None, None),
        };
        // Only the account fields are read; the kind is `kind` above.
        let reference = reference.unwrap_or(Reference::bare(Kind::Oauth));
        entries.push(AuditEntry {
            created_at,
            kind,
            provider: reference.provider,
            account_id: reference.account_id,
            account: reference.account,
            named: reference.named,
            status,
            orphan: status == Status::Unused,
            present,
            key,
        });
    }
    entries
}

/// The entries of the index at `index`, judged against the owning files at
/// `paths`, re-read from disk now.
pub async fn audit(session: &OAuthSession, index: &Path, paths: &SourcePaths) -> Plan {
    let sources = Sources::read(paths);
    Plan {
        config_path: paths.config.path.clone(),
        entries: audit_sources(session, index, &sources).await,
    }
}

/// Why a confirmed key was not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// No longer in the index.
    Gone,
    /// Its status changed since the confirmation (now in use, or no longer
    /// provably unused).
    Changed(Status),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub key: String,
    pub reason: SkipReason,
}

/// What [`prune_confirmed`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PruneOutcome {
    pub deleted: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// Delete the vault entry `entry` and its index row.
async fn forget(
    session: &OAuthSession,
    index: &Path,
    entry: &AuditEntry,
) -> Result<(), crate::text::Localized> {
    if entry.present {
        session
            .delete_by_key(entry.key.clone())
            .await
            .map_err(|error| error.l10n_for_purge())?;
    }
    // A delete through an `IndexedStore` already left the index; an entry
    // gone from the vault (or a store without the index) leaves it here.
    update(index, |file| {
        file.entries.remove(&entry.key);
    });
    Ok(())
}

/// Delete exactly the `confirmed` keys that are still [`Status::Unused`]
/// when the owning files are read again from disk now; every other key is
/// skipped and reported. Stops at the first vault error, which never
/// carries a value.
pub async fn prune_confirmed(
    session: &OAuthSession,
    index: &Path,
    paths: &SourcePaths,
    confirmed: &[String],
) -> Result<PruneOutcome, crate::text::Localized> {
    let fresh = audit(session, index, paths).await;
    let mut outcome = PruneOutcome::default();
    let mut seen = BTreeSet::new();
    for key in confirmed {
        if !seen.insert(key) {
            continue;
        }
        match fresh.entries.iter().find(|entry| &entry.key == key) {
            None => outcome.skipped.push(Skipped {
                key: key.clone(),
                reason: SkipReason::Gone,
            }),
            Some(entry) if entry.status == Status::Unused => {
                forget(session, index, entry).await?;
                outcome.deleted.push(key.clone());
            }
            Some(entry) => outcome.skipped.push(Skipped {
                key: key.clone(),
                reason: SkipReason::Changed(entry.status),
            }),
        }
    }
    Ok(outcome)
}

/// Delete one indexed entry the user chose by its key, whatever its status
/// except [`Status::InUse`] (judged again now). Returns the status it had.
pub async fn remove_one(
    session: &OAuthSession,
    index: &Path,
    paths: &SourcePaths,
    key: &str,
) -> Result<Status, crate::text::Localized> {
    let fresh = audit(session, index, paths).await;
    let entry = fresh.entries.iter().find(|entry| entry.key == key).ok_or(
        crate::text::Localized::fixed(
            "vault.remove.not_indexed",
            "That key is not in the list of stored credentials.",
        ),
    )?;
    if entry.status == Status::InUse {
        return Err(crate::text::Localized::fixed(
            "vault.remove.in_use",
            "That credential is in use; remove its account or channel instead.",
        ));
    }
    forget(session, index, entry).await?;
    Ok(entry.status)
}

#[cfg(test)]
#[path = "vault_index_tests.rs"]
mod tests;
