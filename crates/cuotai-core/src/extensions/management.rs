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
use super::runner::{Reading, Runner, SecretEnv};
use super::{discover, CatalogEntry, Manifest};
use crate::oauth::{OAuthToken, TokenStore};
use crate::text::Localized;
use crate::{Account, Config, Provider, Status};

/// The account parameter carrying a dynamic extension identity.
pub const EXTENSION_ID_PARAM: &str = "extension_id";

/// PRD 0028 RF7.1: the parameter of a built-in account that chose an
/// extension override ("Use extension override").
pub const EXTENSION_OVERRIDE_PARAM: &str = "extension_override";

/// PRD 0028 RF7.1: one account an override extension can replace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverrideTarget {
    pub index: usize,
    pub name: String,
    /// Whether the account already uses this extension.
    pub active: bool,
}

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

/// PRD 0028 RF7.4: one `[[secrets]]` entry of an extension, for the
/// extension dialogs: whether a value is stored, never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretItem {
    pub name: String,
    pub env: String,
    pub description: String,
    pub stored: bool,
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
    /// PRD 0028 RF7.1: the built-in provider this extension can replace.
    pub overrides: Option<Provider>,
    /// The accounts of that provider, each with whether it uses this
    /// extension; offered only while the extension is enabled.
    pub override_targets: Vec<OverrideTarget>,
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

    /// [`Self::at`] whose runner inherits `parent` instead of the process
    /// environment.
    #[cfg(test)]
    pub(crate) fn at_with_parent_env(
        root: impl Into<PathBuf>,
        parent: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Self {
        let mut manager = Self::at(root);
        manager.runner = Runner::with_parent_env(parent, &manager.consent);
        manager
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
                    overrides: manifest.and_then(|m| m.overrides),
                    override_targets: Vec::new(),
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
                overrides: None,
                override_targets: Vec::new(),
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
                    && (item.state != ExtensionState::Enabled
                        || (item.account.is_none() && item.overrides.is_none())),
                disable: item.approved,
                test: item.state == ExtensionState::Enabled,
                remove: item.account.is_some(),
            };
        }
        for item in &mut items {
            let (Some(provider), ExtensionState::Enabled) = (item.overrides, item.state) else {
                continue;
            };
            item.override_targets = config
                .accounts
                .iter()
                .enumerate()
                .filter(|(_, account)| account.provider == provider)
                .map(|(index, account)| OverrideTarget {
                    index,
                    name: account.name.clone(),
                    active: account.extension_override() == Some(item.id.as_str()),
                })
                .collect();
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    }

    /// PRD 0028 RF7.1: `config` with the account at `index` using (`on`) or
    /// no longer using the override extension `id`. Only an enabled
    /// extension that overrides that account's provider can be chosen;
    /// turning it off is always allowed. The frontend persists the result.
    pub fn set_override(
        &self,
        config: &Config,
        index: usize,
        id: &str,
        on: bool,
    ) -> Result<Config, Localized> {
        let account = config.accounts.get(index).ok_or_else(invalid_account)?;
        let mut updated = account.clone();
        if on {
            let manifest = self.manifest(id)?;
            if manifest.overrides != Some(account.provider) {
                return Err(not_an_override(id, account.provider));
            }
            if self.consent.status(&manifest) != consent::Status::Approved {
                return Err(consent::not_approved(id));
            }
            updated
                .params
                .insert(EXTENSION_OVERRIDE_PARAM.to_owned(), id.to_owned());
        } else {
            updated.params.remove(EXTENSION_OVERRIDE_PARAM);
        }
        config
            .upsert_account(updated, Some(index))
            .map_err(|error| Localized::fixed("extensions.action.account_invalid", error.message()))
    }

    /// PRD 0028 RF7.2: the reading of the override extension `id` for an
    /// account of `provider`, or `None` when the built-in adapter must
    /// answer: the extension is missing, does not override `provider`, was
    /// never approved, was revoked or changed since its approval.
    pub async fn run_override(
        &self,
        provider: Provider,
        id: &str,
        store: Option<Arc<dyn TokenStore>>,
    ) -> Option<Result<Checked, Localized>> {
        let manifest = discover(&self.root).get(id).cloned()?;
        if manifest.overrides != Some(provider) {
            return None;
        }
        let name = manifest.name.clone();
        let approved = self.consent.check(manifest).ok()?;
        let note = Localized::with_param(
            "extensions.override.active",
            "name",
            name.as_str(),
            format!("Lido pela extensão {name} no lugar da integração embutida."),
        );
        let result = self.run_approved(id, approved, store).await;
        Some(result.map(|mut checked| {
            checked.notes.insert(0, note);
            checked
        }))
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
        // PRD 0028 RF7.1: an override extension creates no account of its
        // own; the user picks, account by account, where it replaces the
        // built-in query.
        if manifest.overrides.is_some() {
            let rollback = self.commit(&snapshot)?;
            return Ok(EnableResult {
                config: config.clone(),
                action: ActionResult {
                    id: id.clone(),
                    message: Localized::with_param(
                        "extensions.action.enabled_override",
                        "id",
                        id.as_str(),
                        format!(
                            "Extensão {id} ativada; escolha em quais contas ela substitui a \
                             integração embutida."
                        ),
                    ),
                    account: None,
                },
                consent_rollback: rollback,
            });
        }
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

    /// RF7.4: [`Self::disable`] that also deletes the vault entries of the
    /// revoked approval's secrets. The values are deleted first and put back
    /// when the revocation cannot be saved, so a failed revocation leaves
    /// the extension exactly as it was.
    pub fn disable_with_secrets(
        &self,
        id: &str,
        store: &dyn TokenStore,
    ) -> Result<ActionResult, Localized> {
        let Some((scope, names)) = self.consent.stored_secrets(id) else {
            return self.disable(id);
        };
        let mut removed: Vec<(String, OAuthToken)> = Vec::new();
        for name in &names {
            let key = secret_key(id, &scope, name);
            let restore = |removed: &[(String, OAuthToken)]| {
                for (key, token) in removed {
                    let _ = store.save(key, token);
                }
            };
            match store.load(&key) {
                Ok(Some(token)) => {
                    if store.delete(&key).is_err() {
                        restore(&removed);
                        return Err(secret_storage());
                    }
                    removed.push((key, token));
                }
                Ok(None) => {}
                Err(_) => {
                    restore(&removed);
                    return Err(secret_storage());
                }
            }
        }
        match self.disable(id) {
            Ok(result) => Ok(result),
            Err(error) => {
                for (key, token) in &removed {
                    let _ = store.save(key, token);
                }
                Err(error)
            }
        }
    }

    /// RF7.4: the secrets the current manifest of `id` declares, each with
    /// whether the vault holds a value for the files as they are now.
    pub fn secrets(&self, id: &str, store: &dyn TokenStore) -> Result<Vec<SecretItem>, Localized> {
        let manifest = self.manifest(id)?;
        let scope = self
            .consent
            .snapshot(&manifest)
            .map(|approval| approval.secret_scope());
        Ok(manifest
            .secrets
            .iter()
            .map(|secret| SecretItem {
                name: secret.name.clone(),
                env: secret.env.clone(),
                description: secret.description.clone(),
                stored: scope.as_deref().is_some_and(|scope| {
                    matches!(
                        store.load(&secret_key(id, scope, &secret.name)),
                        Ok(Some(_))
                    )
                }),
            })
            .collect())
    }

    /// RF7.4: keep the value of one declared secret of an **approved**
    /// extension in the vault, keyed by the extension id, the approval's
    /// manifest digest and the secret's name. The value is never written to
    /// the TOML, the manifest, the consent record or any log.
    pub fn set_secret(
        &self,
        id: &str,
        name: &str,
        value: &str,
        store: &dyn TokenStore,
    ) -> Result<ActionResult, Localized> {
        let manifest = self.manifest(id)?;
        let approved = self
            .consent
            .check(manifest)
            .map_err(|manifest| consent::not_approved(&manifest.id))?;
        let secret = approved
            .manifest()
            .secrets
            .iter()
            .find(|secret| secret.name == name)
            .ok_or_else(|| unknown_secret(id, name))?;
        let value = value.trim();
        if value.is_empty() {
            return Err(empty_secret(&secret.name));
        }
        let token = OAuthToken {
            access_token: value.to_owned(),
            refresh_token: None,
            expires_at: None,
            metadata: BTreeMap::new(),
        };
        store
            .save(&secret_key(id, &approved.secret_scope(), name), &token)
            .map_err(|_| secret_storage())?;
        // A run that failed for the missing value may go again at once.
        self.runner.forget(id);
        Ok(ActionResult {
            id: id.to_owned(),
            message: Localized::with_param(
                "extensions.action.secret_saved",
                "name",
                name,
                format!("Segredo {name} guardado no cofre do sistema."),
            ),
            account: None,
        })
    }

    /// Explicit test of an enabled extension. It uses the same shared Runner
    /// as recurring account refreshes and records only normalized output.
    pub async fn test_reading(&self, id: &str) -> Result<Reading, Localized> {
        self.test_reading_with(id, None).await
    }

    /// [`Self::test_reading`] with the vault of the extension's secrets.
    pub async fn test_reading_with(
        &self,
        id: &str,
        store: Option<Arc<dyn TokenStore>>,
    ) -> Result<Reading, Localized> {
        let manifest = self.manifest(id)?;
        let approved = self
            .consent
            .check(manifest)
            .map_err(|manifest| consent::not_approved(&manifest.id))?;
        let secrets = load_secrets(&approved, store).await?;
        let reading = self.runner.run_with(&approved, &secrets).await;
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
        self.run_account_with(account, None).await
    }

    /// [`Self::run_account`] with the vault of the extension's secrets
    /// (RF7.4); without one, an extension that declares secrets is not run.
    pub async fn run_account_with(
        &self,
        account: &Account,
        store: Option<Arc<dyn TokenStore>>,
    ) -> Result<Checked, Localized> {
        let id = account.extension_id().ok_or_else(invalid_account)?;
        let result = self.run_id(id, store).await;
        if let Err(note) = &result {
            let mut runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            let state = runtime.entry(id.to_owned()).or_default();
            state.error = Some(note.clone());
            state.checked = None;
        }
        result
    }

    async fn run_id(
        &self,
        id: &str,
        store: Option<Arc<dyn TokenStore>>,
    ) -> Result<Checked, Localized> {
        let manifest = self.manifest(id)?;
        let approved = self.consent.check(manifest).map_err(|manifest| {
            match self.consent.status(&manifest) {
                consent::Status::Changed => changed(&manifest.id),
                consent::Status::NeverApproved | consent::Status::Approved => {
                    consent::not_approved(&manifest.id)
                }
            }
        })?;
        self.run_approved(id, approved, store).await
    }

    /// One scheduled run of an already approved extension: the cached
    /// reading between due runs, otherwise a new run with its secrets.
    async fn run_approved(
        &self,
        id: &str,
        approved: consent::Approved,
        store: Option<Arc<dyn TokenStore>>,
    ) -> Result<Checked, Localized> {
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
        let secrets = load_secrets(&approved, store).await?;
        let reading = self.runner.run_with(&approved, &secrets).await;
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

/// RF7.4: the vault key of one secret: `ext_<id>_<scope>_<name>`, within
/// the store's identifier alphabet and length (64 + 16 + 32 + 7 < 128).
fn secret_key(id: &str, scope: &str, name: &str) -> String {
    format!("ext_{id}_{scope}_{name}")
}

/// Every declared secret of `approved`, read from `store` off the async
/// threads; a missing value (or no vault at all) stops the run before any
/// process starts.
async fn load_secrets(
    approved: &consent::Approved,
    store: Option<Arc<dyn TokenStore>>,
) -> Result<SecretEnv, Localized> {
    let manifest = approved.manifest();
    if manifest.secrets.is_empty() {
        return Ok(SecretEnv::default());
    }
    let Some(store) = store else {
        return Err(missing_secret(&manifest.id, &manifest.secrets[0].name));
    };
    let id = manifest.id.clone();
    let scope = approved.secret_scope();
    let wanted: Vec<(String, String)> = manifest
        .secrets
        .iter()
        .map(|secret| (secret.name.clone(), secret.env.clone()))
        .collect();
    tokio::task::spawn_blocking(move || {
        let mut env = SecretEnv::default();
        for (name, var) in wanted {
            match store.load(&secret_key(&id, &scope, &name)) {
                Ok(Some(token)) => env.push(var, token.access_token),
                Ok(None) => return Err(missing_secret(&id, &name)),
                Err(_) => return Err(secret_storage()),
            }
        }
        Ok(env)
    })
    .await
    .map_err(|_| secret_storage())?
}

fn missing_secret(id: &str, name: &str) -> Localized {
    Localized::with_params(
        "extensions.action.secret_missing",
        crate::text::params([("id", id.into()), ("name", name.into())]),
        format!(
            "A extensão {id} precisa do segredo {name}; informe-o na tela de extensões. \
             Nenhum programa foi executado."
        ),
    )
}

fn unknown_secret(id: &str, name: &str) -> Localized {
    Localized::with_params(
        "extensions.action.secret_unknown",
        crate::text::params([("id", id.into()), ("name", name.into())]),
        format!("A extensão {id} não declara o segredo {name}."),
    )
}

fn empty_secret(name: &str) -> Localized {
    Localized::with_param(
        "extensions.action.secret_empty",
        "name",
        name,
        format!("Informe um valor para o segredo {name}."),
    )
}

fn secret_storage() -> Localized {
    Localized::fixed(
        "extensions.action.secret_storage",
        "Não foi possível acessar o cofre do sistema para os segredos da extensão.",
    )
}

fn not_an_override(id: &str, provider: Provider) -> Localized {
    Localized::with_params(
        "extensions.action.not_an_override",
        crate::text::params([("id", id.into()), ("provider", provider.label().into())]),
        format!(
            "A extensão {id} não substitui a integração {}.",
            provider.label()
        ),
    )
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
