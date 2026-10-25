//! UI-neutral extension discovery, consent, account construction and execution.
//!
//! Frontends render this module's values and route explicit user actions back
//! here; manifest, integrity, enablement, account and scheduling policy stays
//! in core.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::consent::{self, Approval, Consent};
use super::output::Checked;
use super::runner::{Reading, Runner};
use super::{discover, CatalogEntry, Manifest};
use crate::text::Localized;
use crate::{Account, Config, Provider, Status};

/// The account parameter carrying a dynamic extension identity.
pub const EXTENSION_ID_PARAM: &str = "extension_id";

/// The disclosures awaiting a confirmation, behind one lock: issuance and
/// consumption never take it in different orders, and nothing else is
/// remembered, so a spent or invented token is simply absent.
#[derive(Debug, Default)]
struct Registry {
    /// Insertion order, so the oldest disclosure is the one evicted when the
    /// cap is reached (never whichever UUID sorts first).
    order: VecDeque<String>,
    pending: BTreeMap<String, Snapshot>,
}

impl Registry {
    const MAX_PENDING: usize = 64;

    fn insert(&mut self, token: String, snapshot: Snapshot) {
        // At most MAX_PENDING: drop the oldest first, then add.
        while self.pending.len() >= Self::MAX_PENDING {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.pending.remove(&oldest);
        }
        self.pending.insert(token.clone(), snapshot);
        self.order.push_back(token);
    }

    fn remove(&mut self, token: &str) -> Option<Snapshot> {
        let snapshot = self.pending.remove(token);
        if snapshot.is_some() {
            self.order.retain(|held| held != token);
        }
        snapshot
    }
}

/// The key a manager is cached and built under: the canonical form of the
/// directory when it exists, otherwise its nearest existing parent's
/// canonical form with the missing tail appended. `..`, `.`, a trailing
/// slash and a symlinked ancestor therefore all resolve to one manager
/// (RF5.8).
fn normalized_root(root: PathBuf) -> PathBuf {
    let resolved = if let Ok(canonical) = root.canonicalize() {
        canonical
    } else {
        let mut missing = Vec::new();
        let mut existing = root.as_path();
        loop {
            if let Ok(canonical) = existing.canonicalize() {
                let mut resolved = canonical;
                for name in missing.iter().rev() {
                    resolved.push(name);
                }
                break resolved;
            }
            let Some(parent) = existing.parent() else {
                break root.clone();
            };
            if let Some(name) = existing.file_name() {
                missing.push(name.to_os_string());
            }
            existing = parent;
        }
    };
    lexical_normalize(resolved)
}

/// Normalize `.` and `..` without requiring the missing suffix to exist.
fn lexical_normalize(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// One manager per live extensions directory per process, held weakly: a
/// rebuilt monitor reuses the live manager (so the runner, its cap and the
/// schedules are shared) and nothing is retained after the last user of a
/// directory drops it (RF5.8).
static SHARED: Mutex<BTreeMap<PathBuf, std::sync::Weak<Manager>>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionState {
    Disabled,
    Enabled,
    Changed,
    Missing,
    /// The folder exists but its manifest is not usable (RF5: listed with
    /// its reason so the user sees why nothing can be enabled).
    Invalid,
}

impl ExtensionState {
    pub fn label(self) -> Localized {
        match self {
            Self::Disabled => Localized::fixed("extensions.state.disabled", "Desativada"),
            Self::Enabled => Localized::fixed("extensions.state.enabled", "Ativada"),
            Self::Changed => Localized::fixed(
                "extensions.state.changed",
                "Alterada — requer nova aprovação",
            ),
            Self::Missing => {
                Localized::fixed("extensions.state.missing", "Arquivos não encontrados")
            }
            Self::Invalid => Localized::fixed(
                "extensions.state.invalid",
                "Manifesto inválido — nada pode ser ativado",
            ),
        }
    }

    /// Whether enabling is offered: never for a missing folder or a
    /// manifest that does not read.
    pub fn can_enable(self) -> bool {
        !matches!(self, Self::Missing | Self::Invalid)
    }
}

/// An opaque handle to the exact files a disclosure showed (RF5.3). Only
/// [`Manager::enable`] understands it, it never leaves the process and it is
/// single use: it binds the approval to the manifest and program bytes the
/// user actually reviewed, so files changing between the disclosure and the
/// confirmation fail closed instead of being approved unseen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// Opaque, unforgeable token carrying the digests of the disclosed
    /// manifest and program plus the issuing generation. Core owns every
    /// meaning in it; a frontend only passes it back untouched.
    token: String,
    /// The extension id at issue time, echoed so a frontend can find the
    /// account slot to update. It authorizes nothing: the token decides.
    #[serde(default, skip_serializing)]
    cached_id: Option<String>,
}

impl Candidate {
    /// The extension this candidate was issued for, when the frontend still
    /// knows it (a round-tripped candidate keeps nothing else).
    pub fn id(&self) -> Option<&str> {
        self.cached_id.as_deref()
    }

    /// The token core issued; opaque everywhere else.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

/// What a disclosure shows plus the opaque handle that binds the approval to
/// it. A frontend renders `lines` and hands `candidate` back to
/// [`Manager::enable`] only after an explicit confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disclosure {
    pub lines: Vec<Localized>,
    pub candidate: Candidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountIdentity {
    pub index: usize,
    pub name: String,
    pub key: String,
}

/// The actions core allows for one row; both frontends render exactly these
/// and decide nothing themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExtensionActions {
    /// Show the disclosure and, once confirmed, enable (creates/updates the
    /// account).
    pub enable: bool,
    /// Revoke the approval.
    pub disable: bool,
    /// Run it once through the shared runner.
    pub test: bool,
    /// The ordinary account removal is available.
    pub remove: bool,
}

impl ExtensionActions {
    /// No action at all (a row with nothing stored).
    pub const NONE: Self = Self {
        enable: false,
        disable: false,
        test: false,
        remove: false,
    };
}

/// One row ready for a terminal or desktop extension manager.
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionItem {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub state: ExtensionState,
    pub state_label: Localized,
    /// Whether an approval for this id is stored, whatever its digests.
    pub approved: bool,
    /// What the user may do with this row, decided by core.
    pub actions: ExtensionActions,
    pub last_run: Option<DateTime<Utc>>,
    pub error: Option<Localized>,
    pub catalog: Option<CatalogEntry>,
    pub account: Option<AccountIdentity>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub id: String,
    pub message: Localized,
    pub account: Option<AccountIdentity>,
}

#[derive(Debug)]
pub struct EnableResult {
    pub config: Config,
    pub action: ActionResult,
    /// Exactly what the approval step changed; a frontend that cannot
    /// persist the configuration hands this back to
    /// [`Consent::rollback`](super::consent::Consent::rollback), so a
    /// pre-existing approval is restored rather than revoked.
    pub consent_rollback: consent::Rollback,
}

#[derive(Debug, Clone, Default)]
struct RuntimeState {
    last_run: Option<DateTime<Utc>>,
    error: Option<Localized>,
    checked: Option<Checked>,
}

/// One process-wide extension module. Clones share the same runner, global
/// two-process cap, per-id schedules and safe last-run state.
pub struct Manager {
    root: PathBuf,
    consent: Consent,
    runner: Runner,
    runtime: Mutex<BTreeMap<String, RuntimeState>>,
    /// Disclosures awaiting an explicit confirmation, and the tokens already
    /// spent, behind one lock.
    registry: Mutex<Registry>,
}

/// The exact files a disclosure showed, kept in core until the frontend
/// confirms (RF5.3).
#[derive(Debug, Clone)]
struct Snapshot {
    id: String,
    manifest: Manifest,
    /// The digests of the reviewed manifest and program, written verbatim on
    /// confirmation.
    approval: Approval,
}

impl Manager {
    /// A manager for tests and single-shot commands: its own runner and
    /// consent file under `root`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = normalized_root(root.into());
        let consent = Consent::for_root(&root);
        Self::build(root, consent)
    }

    fn build(root: PathBuf, consent: Consent) -> Self {
        let runner = Runner::with_consent(&consent);
        Self {
            consent,
            root,
            runner,
            runtime: Mutex::new(BTreeMap::new()),
            registry: Mutex::new(Registry::default()),
        }
    }

    /// The one manager for `root` while one is alive: the same runner, the
    /// same two-process cap, the same schedules and the same revocation
    /// guard across every rebuilt [`crate::Monitor`] (RF5.8). A dropped
    /// manager (its directory gone, its last monitor replaced) leaves only a
    /// prunable weak entry, never a retained runner, environment or
    /// disclosure.
    pub fn shared(root: impl Into<PathBuf>) -> Arc<Self> {
        let root = normalized_root(root.into());
        let mut cache = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        cache.retain(|_, live| live.strong_count() > 0);
        if let Some(found) = cache.get(&root).and_then(std::sync::Weak::upgrade) {
            return found;
        }
        let consent = Consent::for_root(&root);
        let manager = Arc::new(Self::build(root.clone(), consent));
        cache.insert(root, Arc::downgrade(&manager));
        manager
    }

    /// [`Self::shared`] for the default location, or `None` without one.
    pub fn default_shared() -> Option<Arc<Self>> {
        super::default_dir().map(Self::shared)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Discovered extensions, folders whose manifest did not read, and
    /// configured accounts whose install folder is gone. Ordering is
    /// deterministic by id.
    pub fn list(&self, config: &Config) -> Vec<ExtensionItem> {
        let discovery = discover(&self.root);
        let runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner);
        let accounts: BTreeMap<&str, (usize, &Account)> = config
            .accounts
            .iter()
            .enumerate()
            .filter_map(|(index, account)| account.extension_id().map(|id| (id, (index, account))))
            .collect();
        let catalog: BTreeMap<String, CatalogEntry> = discovery
            .catalog()
            .into_iter()
            .map(|entry| (entry.id.clone(), entry))
            .collect();
        let mut ids: BTreeSet<String> = discovery
            .extensions
            .iter()
            .map(|manifest| manifest.id.clone())
            .collect();
        ids.extend(accounts.keys().map(|id| (*id).to_owned()));
        let manifests: BTreeMap<&str, &Manifest> = discovery
            .extensions
            .iter()
            .map(|manifest| (manifest.id.as_str(), manifest))
            .collect();
        let mut items: Vec<ExtensionItem> = ids
            .into_iter()
            .map(|id| {
                let manifest = manifests.get(id.as_str()).copied();
                let account = accounts
                    .get(id.as_str())
                    .map(|(index, account)| AccountIdentity {
                        index: *index,
                        name: account.name.clone(),
                        key: account.key(),
                    });
                let state = match manifest {
                    None => ExtensionState::Missing,
                    Some(manifest) => match self.consent.status(manifest) {
                        consent::Status::NeverApproved => ExtensionState::Disabled,
                        consent::Status::Approved => ExtensionState::Enabled,
                        consent::Status::Changed => ExtensionState::Changed,
                    },
                };
                let run = runtime.get(&id);
                ExtensionItem {
                    name: manifest.map_or_else(|| id.clone(), |m| m.name.clone()),
                    version: manifest.map_or_else(String::new, |m| m.version.clone()),
                    description: manifest.and_then(|m| m.description.clone()),
                    state,
                    state_label: state.label(),
                    approved: state == ExtensionState::Enabled,
                    actions: ExtensionActions::NONE,
                    last_run: run.and_then(|state| state.last_run),
                    error: run.and_then(|state| state.error.clone()),
                    catalog: catalog.get(&id).cloned(),
                    account,
                    id,
                }
            })
            .collect();
        // RF5.4/RF5.7: a folder whose manifest did not read is shown with
        // its reason, never silently dropped and never enableable. A folder
        // already listed (a configured account of the same id) is merged
        // into that row instead of duplicated.
        let mut reasons: BTreeMap<String, Localized> = BTreeMap::new();
        for rejected in &discovery.rejected {
            let key = rejected
                .id
                .clone()
                .or_else(|| {
                    rejected
                        .dir
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            reasons
                .entry(key)
                .and_modify(|existing| {
                    let both = format!("{}; {}", existing.fallback, rejected.reason.fallback);
                    *existing = Localized::fixed("extensions.error.rejected_many", both);
                })
                .or_insert_with(|| rejected.reason.clone());
        }
        for item in &mut items {
            if let Some(reason) = reasons.get(&item.id) {
                item.state = ExtensionState::Invalid;
                item.state_label = item.state.label();
                item.catalog = None;
                item.error = Some(reason.clone());
            }
        }
        for (id, reason) in reasons {
            if items.iter().any(|item| item.id == id) {
                continue;
            }
            let state = ExtensionState::Invalid;
            items.push(ExtensionItem {
                id: id.clone(),
                name: id.clone(),
                version: String::new(),
                description: None,
                state,
                state_label: state.label(),
                approved: false,
                actions: ExtensionActions::NONE,
                last_run: runtime.get(&id).and_then(|state| state.last_run),
                error: Some(reason),
                catalog: None,
                account: None,
            });
        }
        for item in &mut items {
            // RF5.4/RF5.7: revoking stays possible while any approval
            // exists, even for a row whose files changed, vanished or
            // stopped parsing; the row's actions are core's answer, so no
            // frontend re-derives them from strings.
            item.approved = self.consent.has_approval(&item.id);
            item.actions = ExtensionActions {
                enable: item.state.can_enable()
                    && (item.state != ExtensionState::Enabled || item.account.is_none()),
                disable: item.approved,
                test: item.state == ExtensionState::Enabled,
                remove: item.account.is_some(),
            };
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    }

    /// RF5.3: what the user must see before approving, plus the opaque
    /// handle [`Self::enable`] will only accept for exactly these files.
    pub fn disclosure(&self, id: &str) -> Result<Disclosure, Localized> {
        let manifest = self.manifest(id)?;
        let snapshot = self.snapshot(&manifest)?;
        let token = self.issue(snapshot);
        Ok(Disclosure {
            lines: consent::disclosure(&manifest),
            candidate: Candidate {
                token,
                cached_id: Some(id.to_owned()),
            },
        })
    }

    /// Whether the current manifest and program are already approved.
    pub fn is_enabled(&self, id: &str) -> Result<bool, Localized> {
        let manifest = self.manifest(id)?;
        Ok(self.consent.status(&manifest) == consent::Status::Approved)
    }

    /// Approve exactly the disclosed files without touching configuration
    /// (the one-shot CLI check). This is a complete transaction: once
    /// consent commits, old runtime/backoff state is cleared immediately.
    /// Configuration-bearing frontends use [`Self::enable`] instead.
    pub fn approve(&self, candidate: &Candidate, confirmed: bool) -> Result<(), Localized> {
        if !confirmed {
            return Err(consent::declined());
        }
        let snapshot = self.consume(candidate.token())?;
        let rollback = self.commit(&snapshot)?;
        self.confirm_commit(&snapshot.id);
        drop(rollback);
        Ok(())
    }

    /// Consume and undo one [`Self::enable`] whose frontend persistence
    /// failed. A rollback handle is single-use and cannot be replayed.
    pub fn rollback(&self, rollback: consent::Rollback) -> Result<(), Localized> {
        self.consent.rollback(rollback)
    }

    /// How many runs are queued for one of the runner's permits (tests
    /// synchronize on this instead of sleeping).
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        self.runner.waiting()
    }

    /// Approve exactly the disclosed files and build/update the matching
    /// account through `Config::upsert_account`. `confirmed` must originate
    /// from an explicit frontend confirmation of `candidate`.
    pub fn enable(
        &self,
        candidate: &Candidate,
        confirmed: bool,
        config: &Config,
    ) -> Result<EnableResult, Localized> {
        if !confirmed {
            return Err(consent::declined());
        }
        let snapshot = self.consume(candidate.token())?;
        let id = snapshot.id.clone();
        let manifest = snapshot.manifest.clone();
        let editing = config
            .accounts
            .iter()
            .position(|account| account.extension_id() == Some(&id));
        let account = extension_account(&manifest, config, editing);
        let updated = config.upsert_account(account, editing).map_err(|error| {
            Localized::fixed("extensions.action.account_invalid", error.message())
        })?;
        let rollback = self.commit(&snapshot)?;
        let index = editing.unwrap_or_else(|| updated.accounts.len() - 1);
        // Read the identity out of the new configuration before it moves.
        let identity = AccountIdentity {
            index,
            name: updated.accounts[index].name.clone(),
            key: updated.accounts[index].key(),
        };
        Ok(EnableResult {
            config: updated,
            action: ActionResult {
                id: id.clone(),
                message: Localized::with_param(
                    "extensions.action.enabled",
                    "id",
                    id.as_str(),
                    format!("Extensão {id} ativada e conta criada."),
                ),
                account: Some(identity),
            },
            consent_rollback: rollback,
        })
    }

    /// Revoke approval. The account and installed files stay; normal account
    /// removal remains the only way to remove the account.
    pub fn disable(&self, id: &str) -> Result<ActionResult, Localized> {
        self.consent.revoke(id)?;
        Ok(ActionResult {
            id: id.to_owned(),
            message: Localized::with_param(
                "extensions.action.disabled",
                "id",
                id,
                format!("Extensão {id} desativada; a aprovação foi revogada."),
            ),
            account: None,
        })
    }

    /// Explicit test of an enabled extension. It uses the same shared Runner
    /// as recurring account refreshes and records only normalized output.
    pub async fn test_reading(&self, id: &str) -> Result<Reading, Localized> {
        let manifest = self.manifest(id)?;
        let approved = self
            .consent
            .check(manifest)
            .map_err(|manifest| consent::not_approved(&manifest.id))?;
        let reading = self.runner.run(&approved).await;
        self.record(id, &reading.checked);
        Ok(reading)
    }

    pub async fn test(&self, id: &str) -> Result<Checked, Localized> {
        Ok(self.test_reading(id).await?.checked)
    }

    /// Reading for a normal Monitor refresh. Missing, disabled and changed
    /// extensions are rejected before process creation. Runs inside the
    /// manifest schedule; between due runs the last normalized reading is
    /// reused, never raw process output.
    pub async fn run_account(&self, account: &Account) -> Result<Checked, Localized> {
        let id = account.extension_id().ok_or_else(invalid_account)?;
        let result = self.run_id(id).await;
        if let Err(note) = &result {
            let mut runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            let state = runtime.entry(id.to_owned()).or_default();
            state.error = Some(note.clone());
            state.checked = None;
        }
        result
    }

    async fn run_id(&self, id: &str) -> Result<Checked, Localized> {
        let manifest = self.manifest(id)?;
        let approved = self.consent.check(manifest).map_err(|manifest| {
            match self.consent.status(&manifest) {
                consent::Status::Changed => changed(&manifest.id),
                consent::Status::NeverApproved | consent::Status::Approved => {
                    consent::not_approved(&manifest.id)
                }
            }
        })?;
        if !self.runner.next_run_in(id, Instant::now()).is_zero() {
            if let Some(checked) = self
                .runtime
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(id)
                .and_then(|state| state.checked.clone())
            {
                return Ok(checked);
            }
        }
        let reading = self.runner.run(&approved).await;
        let checked = reading.checked;
        self.record(id, &checked);
        Ok(checked)
    }

    fn manifest(&self, id: &str) -> Result<Manifest, Localized> {
        discover(&self.root)
            .get(id)
            .cloned()
            .ok_or_else(|| missing(id))
    }

    /// The manifest and program digests as they are now, computed from the
    /// exact manifest text discovery validated.
    fn snapshot(&self, manifest: &Manifest) -> Result<Snapshot, Localized> {
        Ok(Snapshot {
            id: manifest.id.clone(),
            manifest: manifest.clone(),
            approval: self
                .consent
                .snapshot(manifest)
                .ok_or_else(consent::consent_storage)?,
        })
    }

    /// Keep the snapshot under an unguessable token. One lock holds the
    /// pending disclosures, so issuance and consumption cannot deadlock.
    /// At most [`Registry::MAX_PENDING`] stay, and the oldest disclosure is
    /// the one dropped.
    fn issue(&self, snapshot: Snapshot) -> String {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let token = uuid::Uuid::new_v4().to_string();
        registry.insert(token.clone(), snapshot);
        token
    }

    /// Take the snapshot a token stands for, exactly once: the token is
    /// removed as it is taken, so a spent or invented one is simply absent.
    fn consume(&self, token: &str) -> Result<Snapshot, Localized> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry.remove(token).ok_or_else(expired_candidate)
    }

    /// Persist exactly the reviewed digests (RF5.3, through
    /// [`Consent::approve_snapshot`]: the verification and the write share
    /// the consent's own admission guard, and a replacement retires the
    /// previous approval). The returned handle says exactly what changed,
    /// so a frontend whose own persistence fails can hand it to
    /// [`Consent::rollback`] and never has to infer anything.
    ///
    /// The cached failure and backoff are **not** touched here: they are
    /// cleared by [`Self::confirm_commit`], once the frontend's
    /// configuration write succeeded, so a failed save leaves the previous
    /// version's error and schedule exactly as they were (RF5.10).
    fn commit(&self, snapshot: &Snapshot) -> Result<consent::Rollback, Localized> {
        self.consent
            .approve_snapshot(&snapshot.manifest, &snapshot.approval)
    }

    /// The frontend's configuration is saved: the newly approved version
    /// replaces the old one's cached failure and backoff, so it runs at
    /// once. Called only after persistence succeeded.
    pub fn confirm_commit(&self, id: &str) {
        self.runner.forget(id);
        self.runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    fn record(&self, id: &str, checked: &Checked) {
        let error = (checked.status != Status::Ready)
            .then(|| checked.notes.first().cloned())
            .flatten();
        self.runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id.to_owned(),
                RuntimeState {
                    last_run: Some(Utc::now()),
                    error,
                    checked: Some(checked.clone()),
                },
            );
    }
}

fn extension_account(manifest: &Manifest, config: &Config, editing: Option<usize>) -> Account {
    let collision = config.accounts.iter().enumerate().any(|(index, account)| {
        Some(index) != editing && account.name.eq_ignore_ascii_case(&manifest.name)
    });
    let name = if collision {
        format!("{} ({})", manifest.name, manifest.id)
    } else {
        manifest.name.clone()
    };
    Account {
        // PRD 0029 RF1.3: re-adding keeps the edited account's id.
        id: Some(
            editing
                .and_then(|index| config.accounts.get(index))
                .map(Account::id)
                .unwrap_or_else(crate::AccountId::random),
        ),
        name,
        provider: Provider::Extension,
        credential_env: None,
        oauth: None,
        session: None,
        params: BTreeMap::from([(EXTENSION_ID_PARAM.to_owned(), manifest.id.clone())]),
        refresh_seconds: Some(manifest.interval_seconds),
        group: None,
    }
}

fn invalid_account() -> Localized {
    Localized::fixed(
        "extensions.action.invalid_account",
        "A conta de extensão não tem uma identidade válida.",
    )
}

/// A confirmation token that was never issued, already used or dropped: the
/// frontend must show the disclosure again (RF5.3).
fn expired_candidate() -> Localized {
    Localized::fixed(
        "extensions.action.candidate_expired",
        "Esta confirmação não vale mais; revise a extensão de novo antes de aprová-la.",
    )
}

pub(crate) fn missing(id: &str) -> Localized {
    Localized::with_param(
        "extensions.action.missing",
        "id",
        id,
        format!("A extensão {id} não foi encontrada; nenhum programa foi executado."),
    )
}

fn changed(id: &str) -> Localized {
    Localized::with_param(
        "extensions.action.changed",
        "id",
        id,
        format!("A extensão {id} mudou desde a aprovação; nenhum programa foi executado."),
    )
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;
