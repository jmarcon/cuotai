//! PRD 0017 RF4: consent and integrity of a script extension.
//!
//! Every extension is born **disabled**: only an [`Approved`] — which exists
//! only through [`Consent::approve`], after the user was shown
//! [`disclosure`] and confirmed explicitly — can be run by
//! [`crate::extensions::runner::Runner`]. [`Consent`] stores, per extension
//! id, the SHA-256 of the manifest and of the program and the approved set
//! of environment variable names, written atomically; [`Consent::check`]
//! re-reads the files and compares the digests, so any change to the
//! manifest or the program disables the extension until a new approval.
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

/// The file of the stored approvals, inside the extensions' directory.
pub const CONSENT_FILE: &str = "consent.json";

/// The SHA-256 (lowercase hex) of the manifest and the program at the
/// moment of approval, with the environment variable names the user
/// approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approval {
    manifest_sha256: String,
    program_sha256: String,
    env: Vec<String>,
}

impl Approval {
    /// The digests of `manifest`'s files **as they are right now**, and only
    /// when the manifest on disk still is the text this struct was parsed
    /// from: reading the file and comparing it byte for byte happens before
    /// hashing, so a cached [`Manifest`] can never approve or verify a file
    /// that changed afterwards.
    fn of(manifest: &Manifest) -> Option<Self> {
        let on_disk = std::fs::read_to_string(manifest.dir.join(MANIFEST_FILE)).ok()?;
        if on_disk != manifest.source {
            return None;
        }
        Some(Self {
            manifest_sha256: sha256_bytes(on_disk.as_bytes()),
            program_sha256: sha256_file(&manifest.program)?,
            env: manifest.env.clone(),
        })
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

    /// `<OS local data dir>/cuotai/extensions/consent.json`; without a
    /// local data directory nothing can be approved.
    pub fn in_data_dir() -> Result<Self, Localized> {
        dirs::data_local_dir()
            .map(|dir| Self::at(dir.join("cuotai").join("extensions").join(CONSENT_FILE)))
            .ok_or_else(consent_storage)
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
        if stored.manifest_sha256 != current.manifest_sha256
            || stored.program_sha256 != current.program_sha256
        {
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
        crate::hooks::write_atomically(&self.file, &bytes).map_err(|_| consent_storage())?;
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
        crate::hooks::write_atomically(&self.file, &bytes).map_err(|_| consent_storage())?;
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
            crate::hooks::write_atomically(&self.file, &bytes).map_err(|_| consent_storage())?;
        }
        if removed {
            let generation = state.generations.entry(id.to_owned()).or_default();
            *generation = generation.saturating_add(1);
        }
        Ok(removed)
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
        format!(
            "A extensão {} ({name}) executa um programa local com os seus poderes de usuário:",
            manifest.id
        ),
    )];
    let command = serde_json::to_string(&manifest.command).unwrap_or_default();
    lines.push(Localized::with_param(
        "extensions.consent.command",
        "command",
        command.as_str(),
        format!("comando: {command}"),
    ));
    let dir = manifest.dir.display().to_string();
    lines.push(Localized::with_param(
        "extensions.consent.directory",
        "dir",
        dir.as_str(),
        format!("pasta: {dir}"),
    ));
    if manifest.env.is_empty() {
        lines.push(Localized::fixed(
            "extensions.consent.env_none",
            "não pede nenhuma variável de ambiente",
        ));
    } else {
        let env = manifest.env.join(", ");
        lines.push(Localized::with_param(
            "extensions.consent.env",
            "env",
            env.as_str(),
            format!("variáveis de ambiente pedidas: {env}"),
        ));
    }
    lines.push(Localized::with_params(
        "extensions.consent.interval",
        params([
            ("seconds", manifest.interval_seconds.into()),
            ("timeout", manifest.timeout_seconds.into()),
        ]),
        format!(
            "executa a cada {} s, com limite de {} s por execução",
            manifest.interval_seconds, manifest.timeout_seconds
        ),
    ));
    lines.push(Localized::fixed(
        "extensions.consent.risk",
        "O Cuotai entrega somente os valores das variáveis pedidas que ele próprio herdou; \
         não entrega o cofre nem credenciais de outras contas e não restringe a rede nem os \
         arquivos: revise o código antes de aprovar.",
    ));
    lines
}

/// The word that approves the disclosure in a terminal.
pub fn confirm_answer() -> Localized {
    Localized::fixed("extensions.consent.answer", "sim")
}

/// The question after the disclosure, asking for [`confirm_answer`].
pub fn confirm_question(answer: &str) -> Localized {
    Localized::with_param(
        "extensions.consent.question",
        "answer",
        answer,
        format!(
            "Para aprovar exatamente esta extensão como ela está agora, digite \"{answer}\" e \
             pressione Enter (qualquer outra resposta recusa):"
        ),
    )
}

/// The user answered anything but [`confirm_answer`].
pub fn declined() -> Localized {
    Localized::fixed(
        "extensions.consent.declined",
        "Extensão não aprovada; ela não foi executada.",
    )
}

/// The extension was never approved, or its manifest or program changed
/// since the approval, and nobody is there to approve it.
pub fn not_approved(id: &str) -> Localized {
    Localized::with_param(
        "extensions.consent.not_approved",
        "id",
        id,
        format!(
            "A extensão {id} ainda não foi aprovada, ou seu manifesto ou programa mudou desde a \
             aprovação; revise e aprove em um terminal com cuotai --extension-check {id}."
        ),
    )
}

/// The files changed between the disclosure and the confirmation (RF5.3).
pub(crate) fn snapshot_changed(id: &str) -> Localized {
    Localized::with_param(
        "extensions.action.changed",
        "id",
        id,
        format!("A extensão {id} mudou desde a aprovação; nenhum programa foi executado."),
    )
}

/// Another approval for this id landed after the one this handle undoes, so
/// the rollback refuses to overwrite it.
pub(crate) fn rollback_conflict(id: &str) -> Localized {
    Localized::with_param(
        "extensions.action.rollback_conflict",
        "id",
        id,
        format!(
            "A aprovação da extensão {id} mudou de novo; a operação foi desfeita até aqui, \
             mas a aprovação mais recente foi mantida."
        ),
    )
}

/// Approvals cannot be recorded (no local data directory, or it cannot be
/// written).
pub fn consent_storage() -> Localized {
    Localized::fixed(
        "extensions.consent.storage",
        "Não foi possível registrar a aprovação da extensão no diretório de dados local.",
    )
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
