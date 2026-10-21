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
struct Approval {
    manifest_sha256: String,
    program_sha256: String,
    env: Vec<String>,
}

impl Approval {
    /// The digests of the files of `manifest` as they are now; `None` when
    /// one of them cannot be read.
    fn of(manifest: &Manifest) -> Option<Self> {
        Some(Self {
            manifest_sha256: sha256_file(&manifest.dir.join(MANIFEST_FILE))?,
            program_sha256: sha256_file(&manifest.program)?,
            env: manifest.env.clone(),
        })
    }
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
#[derive(Debug, Clone)]
pub struct Consent {
    file: PathBuf,
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
}

impl Consent {
    pub fn at(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// `<OS local data dir>/cuotai/extensions/consent.json`; without a
    /// local data directory nothing can be approved.
    pub fn in_data_dir() -> Result<Self, Localized> {
        dirs::data_local_dir()
            .map(|dir| Self::at(dir.join("cuotai").join("extensions").join(CONSENT_FILE)))
            .ok_or_else(consent_storage)
    }

    /// `manifest` as an [`Approved`] when its manifest and program still
    /// match the stored digests; otherwise `manifest` back (boxed: it is
    /// large and this is the cold path), to disclose and approve again.
    /// Unreadable files are never approved.
    pub fn check(&self, manifest: Manifest) -> Result<Approved, Box<Manifest>> {
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
        Ok(Approved {
            manifest,
            digests: current,
            env,
        })
    }

    /// Record that the user approved `manifest` exactly as it is now:
    /// the digests of the manifest and the program and the `env` names it
    /// asks for, written atomically.
    pub fn approve(&self, manifest: &Manifest) -> Result<Approved, Localized> {
        let digests = Approval::of(manifest).ok_or_else(consent_storage)?;
        let mut record = self.read();
        record.approved.insert(manifest.id.clone(), digests.clone());
        let bytes = serde_json::to_vec(&record).map_err(|_| consent_storage())?;
        crate::hooks::write_atomically(&self.file, &bytes).map_err(|_| consent_storage())?;
        Ok(Approved {
            env: digests.env.clone(),
            manifest: manifest.clone(),
            digests,
        })
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
        "O Cuotai não entrega credenciais a ela e não restringe sua rede nem seus arquivos: \
         revise o código antes de aprovar.",
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
