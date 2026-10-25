//! PRD 0017 RF4: consent and integrity of a script extension.
//!
//! Every extension is born **disabled**: only an [`Approved`] — which exists
//! only through [`Consent::approve`], after the user was shown
//! [`disclosure`] and confirmed explicitly — can be run by
//! [`crate::extensions::runner::Runner`]. [`Consent`] stores, per extension
//! id, the SHA-256 of the manifest, of the program and of the whole folder
//! ([`super::tree::scan`]) and the approved set of environment variable
//! names, written atomically to a private file outside the extensions'
//! directory ([`Consent::file_for`]); [`Consent::check`] re-reads the files
//! and compares the digests, so any change to anything in the folder
//! disables the extension until a new approval.
//! [`Approved::verify`] repeats that comparison, so a run never starts over
//! files that changed since the approval. Cuotai never installs, downloads
//! or updates an extension (RF4.3).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Manifest, MANIFEST_FILE};
use crate::text::{params, Localized};

/// The file of the stored approvals, in its own private folder
/// ([`CONSENT_DIR`]) beside the extensions' directory, never inside it.
pub const CONSENT_FILE: &str = "extensions.json";

/// The private (`0700` on Unix) folder of [`CONSENT_FILE`], a sibling of
/// the extensions' directory: `<OS local data dir>/cuotai/consent`.
pub const CONSENT_DIR: &str = "consent";

/// Where releases up to 0.77.0 kept the approvals: `consent.json` inside
/// the extensions' directory itself. Read once and moved by
/// [`Consent::for_root`].
pub const LEGACY_CONSENT_FILE: &str = "consent.json";

/// The SHA-256 (lowercase hex) of the manifest, the program and the whole
/// folder ([`super::tree::scan`]) at the moment of approval, with the
/// environment variable names the user approved. A record written before
/// the folder digest existed reads with an empty `tree_sha256`, which never
/// matches: such an extension shows as changed and needs a new approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approval {
    manifest_sha256: String,
    program_sha256: String,
    #[serde(default)]
    tree_sha256: String,
    env: Vec<String>,
    /// PRD 0028 RF7.4: the names of the manifest's `[[secrets]]` (never a
    /// value), so a revocation can delete their vault entries even after
    /// the folder is gone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    secrets: Vec<String>,
}

impl Approval {
    /// The digests of `manifest`'s files **as they are right now**, and only
    /// when the manifest on disk still is the text this struct was parsed
    /// from and the program still starts with the interpreter line the
    /// disclosure showed: reading and comparing both happens before hashing,
    /// so a cached [`Manifest`] can never approve or verify files that
    /// changed afterwards. A folder that [`super::tree::scan`] refuses (on
    /// Unix, one other users could change) has no digests at all.
    fn of(manifest: &Manifest) -> Option<Self> {
        let on_disk = std::fs::read_to_string(manifest.dir.join(MANIFEST_FILE)).ok()?;
        if on_disk != manifest.source {
            return None;
        }
        if super::tree::shebang(&manifest.program) != manifest.interpreter {
            return None;
        }
        Some(Self {
            manifest_sha256: sha256_bytes(on_disk.as_bytes()),
            program_sha256: sha256_file(&manifest.program)?,
            tree_sha256: super::tree::scan(&manifest.dir).ok()?,
            env: manifest.env.clone(),
            secrets: manifest
                .secrets
                .iter()
                .map(|secret| secret.name.clone())
                .collect(),
        })
    }

    /// The part of the vault key of this approval's secrets: the first 16
    /// hex digits of the manifest digest, so a manifest that declares other
    /// secrets never reads the values entered for an earlier one.
    pub(crate) fn secret_scope(&self) -> String {
        self.manifest_sha256.chars().take(16).collect()
    }

    /// Whether `self` and `other` hash the same files (the approved
    /// variables aside).
    fn same_files(&self, other: &Self) -> bool {
        self.manifest_sha256 == other.manifest_sha256
            && self.program_sha256 == other.program_sha256
            && self.tree_sha256 == other.tree_sha256
    }
}

/// SHA-256 (lowercase hex) of `bytes`.
fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// SHA-256 (lowercase hex) of the file at `path`, streamed; `None` when it
/// cannot be read.
fn sha256_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).ok()?;
    Some(format!("{:x}", hasher.finalize()))
}

/// The stored approvals, by extension id. A record that cannot be read or
/// parsed holds no approval.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    approved: BTreeMap<String, Approval>,
    /// PRD 0028 RF7.4: the secret scopes of manifests replaced by a newer
    /// approval, per extension id (scope → secret names), whose vault
    /// entries are deleted once the new manifest's value is stored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    retired: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

/// The approvals the user gave, in the local data directory; injectable so
/// tests never touch OS directories.
///
/// Clones share one file, one revocation generation and one admission guard,
/// so every frontend and every rebuilt [`crate::Monitor`] in the process
/// agrees on when an approval was revoked (RF4.5).
#[derive(Debug, Clone)]
pub struct Consent {
    file: PathBuf,
    shared: Arc<Shared>,
}

/// The admission guard and the per-id revocation generations, in one lock:
/// the decision to start a process, the check of an approval and a
/// revocation are all serialized by it, and a generation can only be read
/// while the same lock that a revocation takes is held.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    /// Per extension id: bumped by every revocation of that id, so revoking
    /// one extension never invalidates another's approval.
    generations: BTreeMap<String, u64>,
}

/// What one committed [`Consent::approve_snapshot`] did, so a caller that
/// cannot persist its own change can hand it back to
/// [`Consent::rollback`]. `Unchanged` is an explicit no-op (nothing was
/// written, nothing to undo), `Created` removes the record again and
/// `Replaced` restores exactly the previous one. The handle also carries
/// what the transaction installed and the generation it established, so an
/// older handle can never undo a newer approval. Frontends never infer this.
#[derive(Debug, PartialEq, Eq)]
pub struct Rollback {
    id: String,
    change: Change,
    applied: Approval,
    applied_generation: u64,
}

/// The effect of one approval transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// The stored record already was exactly this: no write happened.
    Unchanged,
    /// There was no record; the transaction added one.
    Created,
    /// The transaction replaced this exact record.
    Replaced(Approval),
}

/// Safe approval state for frontends. Digests and environment values never
/// leave this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NeverApproved,
    Approved,
    Changed,
}

/// An extension approved exactly as it is now: the only input
/// [`crate::extensions::runner::Runner::run`] takes. Carrying the approved
/// digests, it can prove ([`Self::verify`]) that the manifest and the
/// program are still the ones the user saw.
#[derive(Debug, Clone)]
pub struct Approved {
    manifest: Manifest,
    digests: Approval,
    /// The approved names that the manifest still asks for (a change to
    /// `env` changes the manifest digest, so this filter is only a second
    /// line of defense).
    env: Vec<String>,
    /// The revocation generation of this extension's id when the approval
    /// was issued: a newer one means it was revoked and may not start.
    generation: u64,
}

impl Approved {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The environment variable names approved for the run, a subset of
    /// the manifest's `env`.
    pub fn env(&self) -> &[String] {
        &self.env
    }

    /// RF7.4: the vault scope of this approval's secrets.
    pub(crate) fn secret_scope(&self) -> String {
        self.digests.secret_scope()
    }

    /// `true` while the manifest and the program on disk are still the
    /// approved ones.
    pub fn verify(&self) -> bool {
        Approval::of(&self.manifest).is_some_and(|current| current == self.digests)
    }

    /// Hold the admission guard while starting the process, refusing an
    /// approval whose generation is older than its own extension id's
    /// current one: after [`Consent::revoke`] returns, no process of a
    /// revoked extension can start. The guard is taken (and dropped) around
    /// the synchronous spawn, so a run already inside its process keeps
    /// running to its own end, exactly like any other in-flight query.
    pub(crate) fn admit<'a>(&self, shared: &'a Shared) -> Option<std::sync::MutexGuard<'a, State>> {
        let state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let current = state
            .generations
            .get(&self.manifest.id)
            .copied()
            .unwrap_or_default();
        (self.generation == current).then_some(state)
    }
}

impl Consent {
    pub fn at(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            shared: Arc::new(Shared::default()),
        }
    }

    /// The approvals of the default extensions' directory
    /// ([`super::default_dir`]): `<OS local data dir>/cuotai/consent/
    /// extensions.json`, migrated from the legacy location as
    /// [`Self::for_root`] does; without a local data directory nothing can
    /// be approved.
    pub fn in_data_dir() -> Result<Self, Localized> {
        super::default_dir()
            .map(|root| Self::for_root(&root))
            .ok_or_else(consent_storage)
    }

    /// The approvals of the extensions under `root`, kept in
    /// [`Self::file_for`] (outside `root`, so nothing copied into the
    /// extensions' directory can replace them). A legacy
    /// `<root>/consent.json` is moved there once: read, written to the new
    /// file (`0600` in a `0700` folder on Unix) and removed. When the new
    /// file already exists it wins and the legacy one is only removed; a
    /// legacy file that does not parse holds no approval and is left alone.
    pub fn for_root(root: &Path) -> Self {
        let consent = Self::at(Self::file_for(root));
        // Best effort: a failed move keeps the legacy file for the next
        // start, and an approval missing meanwhile only asks again.
        let _ = consent.migrate(&root.join(LEGACY_CONSENT_FILE));
        consent
    }

    /// `<parent of root>/consent/extensions.json` (for the default `root`,
    /// `<OS local data dir>/cuotai/consent/extensions.json`).
    pub fn file_for(root: &Path) -> PathBuf {
        root.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(root)
            .join(CONSENT_DIR)
            .join(CONSENT_FILE)
    }

    /// The approvals file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Move the legacy record at `legacy` to [`Self::file`] (see
    /// [`Self::for_root`]). `Ok(true)` when something was moved.
    fn migrate(&self, legacy: &Path) -> io::Result<bool> {
        let Ok(meta) = std::fs::symlink_metadata(legacy) else {
            return Ok(false);
        };
        if std::fs::symlink_metadata(&self.file).is_ok() {
            if meta.is_file() || meta.file_type().is_symlink() {
                std::fs::remove_file(legacy)?;
            }
            return Ok(false);
        }
        if !meta.is_file() {
            return Ok(false);
        }
        let bytes = std::fs::read(legacy)?;
        if serde_json::from_slice::<Record>(&bytes).is_err() {
            return Ok(false);
        }
        crate::private_fs::write_file(&self.file, &bytes)?;
        std::fs::remove_file(legacy)?;
        Ok(true)
    }

    /// Whether `manifest` is approved as it exists now. A damaged record is
    /// treated as never approved; unreadable or changed files are changed
    /// when a record exists, and are never runnable.
    pub fn status(&self, manifest: &Manifest) -> Status {
        let stored = self.read().approved.get(&manifest.id).cloned();
        let Some(stored) = stored else {
            return Status::NeverApproved;
        };
        match Approval::of(manifest) {
            Some(current) if current == stored => Status::Approved,
            _ => Status::Changed,
        }
    }

    /// The digest snapshot of `manifest` exactly as it stands: the manifest
    /// text it was parsed from plus its program bytes. Core-owned and
    /// opaque (the digests never leave this module).
    pub(crate) fn snapshot(&self, manifest: &Manifest) -> Option<Approval> {
        Approval::of(manifest)
    }

    /// `manifest` as an [`Approved`] when its manifest and program still
    /// match the stored digests; otherwise `manifest` back (boxed: it is
    /// large and is the cold path), to disclose and approve again.
    /// Unreadable files are never approved.
    pub fn check(&self, manifest: Manifest) -> Result<Approved, Box<Manifest>> {
        // The guard is held across the read, the digest comparison and the
        // stamping of this id's generation: a revoke cannot slip between
        // them and leave a revoked approval stamped as current.
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let stored = self.read().approved.get(&manifest.id).cloned();
        let current = Approval::of(&manifest);
        let (Some(stored), Some(current)) = (stored, current) else {
            return Err(Box::new(manifest));
        };
        if !stored.same_files(&current) {
            return Err(Box::new(manifest));
        }
        let env = stored
            .env
            .iter()
            .filter(|name| manifest.env.contains(*name))
            .cloned()
            .collect();
        let generation = state
            .generations
            .get(&manifest.id)
            .copied()
            .unwrap_or_default();
        Ok(Approved {
            manifest,
            digests: current,
            env,
            generation,
        })
    }

    /// RF5.3: store exactly the reviewed digests, and only while the files
    /// on disk still hash to them. The comparison and the write share the
    /// admission guard, so nothing else can be approved in between.
    ///
    /// Replacing a *different* approval bumps this id's generation, so an
    /// [`Approved`] carrying the old digests can never start again — not
    /// even if the files later revert. The returned handle undoes exactly
    /// what this call did (nothing, an insertion, or a replacement) and is
    /// what a caller uses when its own persistence fails.
    pub(crate) fn approve_snapshot(
        &self,
        manifest: &Manifest,
        reviewed: &Approval,
    ) -> Result<Rollback, Localized> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if Approval::of(manifest).as_ref() != Some(reviewed) {
            return Err(snapshot_changed(manifest.id.as_str()));
        }
        let mut record = self.read();
        let id = manifest.id.clone();
        match record.approved.get(&id) {
            // Nothing changes: no write, no generation bump, and a handle
            // that is a no-op if handed back.
            Some(current) if current == reviewed => {
                let generation = state.generations.get(&id).copied().unwrap_or_default();
                return Ok(Rollback {
                    id,
                    change: Change::Unchanged,
                    applied: reviewed.clone(),
                    applied_generation: generation,
                });
            }
            _ => {}
        }
        let previous = record.approved.insert(id.clone(), reviewed.clone());
        if let Some(old) = previous
            .as_ref()
            .filter(|old| !old.secrets.is_empty() && old.secret_scope() != reviewed.secret_scope())
        {
            record
                .retired
                .entry(id.clone())
                .or_default()
                .insert(old.secret_scope(), old.secrets.clone());
        }
        // Compute the generation while holding the state lock, but publish it
        // only after the consent file has committed. A failed write must not
        // mutate in-memory admission state.
        let applied_generation = state
            .generations
            .get(&id)
            .copied()
            .unwrap_or_default()
            .saturating_add(1);
        let bytes = serde_json::to_vec(&record).map_err(|_| consent_storage())?;
        crate::private_fs::write_file(&self.file, &bytes).map_err(|_| consent_storage())?;
        state.generations.insert(id.clone(), applied_generation);
        Ok(Rollback {
            change: match previous {
                None => Change::Created,
                Some(previous) => Change::Replaced(previous),
            },
            applied: reviewed.clone(),
            applied_generation,
            id,
        })
    }

    /// Undo one [`Self::approve_snapshot`] **only while nothing newer
    /// happened**: the record for this id must still be exactly the one that
    /// call wrote, and this id's generation the one it established. An older
    /// handle therefore never overwrites a newer approval — it reports a
    /// conflict instead.
    pub fn rollback(&self, rollback: Rollback) -> Result<(), Localized> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // `Unchanged` never wrote anything, so nothing to undo.
        if matches!(&rollback.change, Change::Unchanged) {
            return Ok(());
        }
        let current = self.read().approved.get(&rollback.id).cloned();
        let generation = state
            .generations
            .get(&rollback.id)
            .copied()
            .unwrap_or_default();
        if current.as_ref() != Some(&rollback.applied) || generation != rollback.applied_generation
        {
            return Err(rollback_conflict(rollback.id.as_str()));
        }
        let mut record = self.read();
        match &rollback.change {
            Change::Replaced(previous) => {
                record
                    .approved
                    .insert(rollback.id.clone(), previous.clone());
            }
            Change::Created => {
                record.approved.remove(&rollback.id);
            }
            Change::Unchanged => unreachable!("handled above"),
        }
        let bytes = serde_json::to_vec(&record).map_err(|_| consent_storage())?;
        crate::private_fs::write_file(&self.file, &bytes).map_err(|_| consent_storage())?;
        // Generations are monotonic. Restoring the previous record receives
        // a fresh identity, so neither this handle nor an older Approved can
        // become valid again through an ABA cycle.
        state
            .generations
            .insert(rollback.id.clone(), generation.saturating_add(1));
        Ok(())
    }

    /// Approve the files as they are now (CLI approval of a disclosure just
    /// shown). Frontends that disclosed earlier use
    /// [`Self::approve_snapshot`], which binds to the reviewed bytes.
    pub fn approve(&self, manifest: &Manifest) -> Result<Approved, Localized> {
        let digests = self.snapshot(manifest).ok_or_else(consent_storage)?;
        let rollback = self.approve_snapshot(manifest, &digests)?;
        Ok(Approved {
            env: digests.env.clone(),
            manifest: manifest.clone(),
            // Stamped from the transaction that just committed, never from a
            // separate sample of the same lock.
            generation: rollback.applied_generation,
            digests,
        })
    }

    /// Revoke `id` atomically. This never removes discovery/install files.
    ///
    /// The write and the per-id generation bump happen under the admission
    /// guard, so no process of this extension can start once this returns,
    /// and no other extension's approval is affected.
    pub fn revoke(&self, id: &str) -> Result<bool, Localized> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut record = self.read();
        let removed = record.approved.remove(id).is_some();
        if removed {
            let bytes = serde_json::to_vec(&record).map_err(|_| consent_storage())?;
            crate::private_fs::write_file(&self.file, &bytes).map_err(|_| consent_storage())?;
        }
        if removed {
            let generation = state.generations.entry(id.to_owned()).or_default();
            *generation = generation.saturating_add(1);
        }
        Ok(removed)
    }

    /// RF7.4: the vault scope and the secret names of the stored approval of
    /// `id`, whatever the files on disk are now.
    pub(crate) fn stored_secrets(&self, id: &str) -> Option<(String, Vec<String>)> {
        self.read()
            .approved
            .get(id)
            .map(|approval| (approval.secret_scope(), approval.secrets.clone()))
    }

    /// Whether an approval for `id` exists on disk, whatever its digests.
    /// Frontends use it to know whether disabling still has something to
    /// revoke, including for a row whose files are gone or unreadable.
    pub fn has_approval(&self, id: &str) -> bool {
        self.read().approved.contains_key(id)
    }

    /// The shared guard every revocation and every process start coordinate
    /// on; clones of one [`Consent`] share it.
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// An unreadable or damaged record holds no approval.
    /// PRD 0028 RF7.4: the retired scopes of `id` that still hold a value
    /// for the secret `name`; they are forgotten here, so the caller deletes
    /// their vault entries once (after the new value is stored).
    pub(crate) fn take_retired_secret(&self, id: &str, name: &str) -> Vec<String> {
        let _state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut record = self.read();
        let Some(scopes) = record.retired.get_mut(id) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for (scope, names) in scopes.iter_mut() {
            if let Some(at) = names.iter().position(|known| known == name) {
                names.remove(at);
                found.push(scope.clone());
            }
        }
        scopes.retain(|_, names| !names.is_empty());
        if scopes.is_empty() {
            record.retired.remove(id);
        }
        if !found.is_empty() {
            if let Ok(bytes) = serde_json::to_vec(&record) {
                let _ = crate::private_fs::write_file(&self.file, &bytes);
            }
        }
        found
    }

    fn read(&self) -> Record {
        std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
}

/// RF4.1: what the user approves, one line each and always in the same
/// order: what the extension is, its exact command (as a JSON array, so
/// quotes, spaces and control characters stay visible), its directory, the
/// environment variable names it asks for, its interval and time limit and
/// the residual risk.
pub fn disclosure(manifest: &Manifest) -> Vec<Localized> {
    let name = format!("{} {}", manifest.name, manifest.version);
    let mut lines = vec![Localized::with_params(
        "extensions.consent.header",
        params([
            ("id", manifest.id.as_str().into()),
            ("name", name.as_str().into()),
        ]),
        "The extension {id} ({name}) runs a local program with your user's powers:",
    )];
    let command = serde_json::to_string(&manifest.command).unwrap_or_default();
    lines.push(Localized::with_param(
        "extensions.consent.command",
        "command",
        command.as_str(),
        "command: {command}",
    ));
    // FIX-04: a script names the interpreter the kernel will start; the
    // user approves that program too.
    if let Some(interpreter) = &manifest.interpreter {
        lines.push(Localized::with_param(
            "extensions.consent.interpreter",
            "interpreter",
            interpreter.as_str(),
            "interpreter (the program's #! line): {interpreter}",
        ));
    }
    let dir = manifest.dir.display().to_string();
    lines.push(Localized::with_param(
        "extensions.consent.directory",
        "dir",
        dir.as_str(),
        "directory: {dir}",
    ));
    if manifest.env.is_empty() {
        lines.push(Localized::fixed(
            "extensions.consent.env_none",
            "it asks for no environment variable",
        ));
    } else {
        let env = manifest.env.join(", ");
        lines.push(Localized::with_param(
            "extensions.consent.env",
            "env",
            env.as_str(),
            "environment variables it asks for: {env}",
        ));
    }
    // PRD 0028 RF7.4: each secret by name, variable and description; the
    // value is asked for later and kept in the OS vault.
    for secret in &manifest.secrets {
        lines.push(Localized::with_params(
            "extensions.consent.secret",
            params([
                ("name", secret.name.as_str().into()),
                ("env", secret.env.as_str().into()),
                ("description", secret.description.as_str().into()),
            ]),
            "secret {name} (variable {env}), kept in the system vault: {description}",
        ));
    }
    lines.push(Localized::with_params(
        "extensions.consent.interval",
        params([
            ("seconds", manifest.interval_seconds.into()),
            ("timeout", manifest.timeout_seconds.into()),
        ]),
        "it runs every {seconds} s, with a {timeout} s limit each time",
    ));
    lines.push(Localized::fixed(
        "extensions.consent.risk",
        "Cuotai passes only the requested values it inherited; it passes neither vault data nor other accounts' credentials and restricts neither network nor files: review the code before approving it.",
    ));
    lines
}

/// The word that approves the disclosure in a terminal.
pub fn confirm_answer() -> Localized {
    Localized::fixed("extensions.consent.answer", "yes")
}

/// The question after the disclosure, asking for [`confirm_answer`].
pub fn confirm_question(answer: &str) -> Localized {
    Localized::with_param(
        "extensions.consent.question",
        "answer",
        answer,
        "To approve exactly this extension as it is now, type \"{answer}\" and press Enter (any other answer declines):",
    )
}

/// The user answered anything but [`confirm_answer`].
pub fn declined() -> Localized {
    Localized::fixed(
        "extensions.consent.declined",
        "Extension not approved; it was not run.",
    )
}

/// The extension was never approved, or its manifest or program changed
/// since the approval, and nobody is there to approve it.
pub fn not_approved(id: &str) -> Localized {
    Localized::with_param(
        "extensions.consent.not_approved",
        "id",
        id,
        "The extension {id} was not approved yet, or a file of its folder changed since the approval; review and approve it in a terminal with cuotai --extension-check {id}.",
    )
}

/// The files changed between the disclosure and the confirmation (RF5.3).
pub(crate) fn snapshot_changed(id: &str) -> Localized {
    Localized::with_param(
        "extensions.action.changed",
        "id",
        id,
        "The extension {id} changed since approval; no program was run.",
    )
}

/// Another approval for this id landed after the one this handle undoes, so
/// the rollback refuses to overwrite it.
pub(crate) fn rollback_conflict(id: &str) -> Localized {
    Localized::with_param(
        "extensions.action.rollback_conflict",
        "id",
        id,
        "The extension {id} was approved again after this step; the operation was undone, but the newer approval was kept.",
    )
}

/// Approvals cannot be recorded (no local data directory, or it cannot be
/// written).
pub fn consent_storage() -> Localized {
    Localized::fixed(
        "extensions.consent.storage",
        "Could not record the extension's approval in the local data directory.",
    )
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
