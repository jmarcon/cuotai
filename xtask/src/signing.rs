//! Validated signing state for release artifacts (PRD 0021 RF3.4).

use crate::manifest::Manifest;
use crate::{err, Channel, Error, Res, Version};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path};

pub const SIGNING_STATE_SCHEMA: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SigningMethod {
    DeveloperId,
    AdHoc,
    Authenticode,
    TauriUpdaterKey,
    ChecksumSignature,
    None,
}

impl fmt::Display for SigningMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::DeveloperId => "developer-id",
            Self::AdHoc => "ad-hoc",
            Self::Authenticode => "authenticode",
            Self::TauriUpdaterKey => "tauri-updater-key",
            Self::ChecksumSignature => "checksum-signature",
            Self::None => "none",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SigningStatus {
    Signed,
    #[serde(rename = "signed+notarized")]
    SignedNotarized,
    Unsigned,
    Unavailable,
}

impl fmt::Display for SigningStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Signed => "signed",
            Self::SignedNotarized => "signed+notarized",
            Self::Unsigned => "unsigned",
            Self::Unavailable => "unavailable",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChecksumSigningMethod {
    SigstoreKeyless,
    None,
}

impl fmt::Display for ChecksumSigningMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::SigstoreKeyless => "sigstore-keyless",
            Self::None => "none",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSigning {
    pub name: String,
    pub signed: bool,
    pub method: SigningMethod,
    pub status: SigningStatus,
    /// The detached updater signature `<name>.sig`, when the artifact's bytes
    /// also carry the Tauri updater key (PRD 0018 RF1.1). An artifact can have
    /// two independent proofs — Authenticode (Windows) or the signed
    /// `SHA256SUMS` (Linux) plus the updater signature — and still is one
    /// artifact with one record: the payload is never listed twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updater_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecksumSigning {
    pub signed: bool,
    pub method: ChecksumSigningMethod,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdaterState {
    pub available: bool,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fragment {
    #[serde(default)]
    pub artifacts: Vec<ArtifactSigning>,
    #[serde(default)]
    pub updater: Option<UpdaterState>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningState {
    pub schema: u32,
    pub version: String,
    pub channel: Channel,
    pub artifacts: Vec<ArtifactSigning>,
    pub checksums: ChecksumSigning,
    pub updater: UpdaterState,
}

fn safe_basename(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(value)) if value == OsStr::new(name))
        && components.next().is_none()
}

fn validate_artifact(entry: &ArtifactSigning, unresolved_checksum: bool) -> Res<()> {
    if !safe_basename(&entry.name) {
        return err(format!(
            "signing artifact '{}' is not a safe basename",
            entry.name
        ));
    }
    if let Some(signature) = &entry.updater_signature {
        let expected = format!("{}.sig", entry.name);
        if signature != &expected || !safe_basename(signature) {
            return err(format!(
                "'{}' has invalid updater signature name '{signature}'",
                entry.name
            ));
        }
    }
    let valid = match entry.method {
        SigningMethod::DeveloperId => {
            entry.signed
                && matches!(
                    entry.status,
                    SigningStatus::Signed | SigningStatus::SignedNotarized
                )
        }
        SigningMethod::AdHoc => !entry.signed && entry.status == SigningStatus::Unsigned,
        SigningMethod::Authenticode | SigningMethod::TauriUpdaterKey => {
            entry.signed && entry.status == SigningStatus::Signed
        }
        SigningMethod::ChecksumSignature if unresolved_checksum => {
            !entry.signed && entry.status == SigningStatus::Unsigned
        }
        SigningMethod::ChecksumSignature => matches!(
            (entry.signed, entry.status),
            (true, SigningStatus::Signed) | (false, SigningStatus::Unsigned)
        ),
        SigningMethod::None => {
            !entry.signed
                && matches!(
                    entry.status,
                    SigningStatus::Unsigned | SigningStatus::Unavailable
                )
        }
    };
    if !valid {
        return err(format!(
            "'{}' has contradictory signing fields: signed={}, method={}, status={}",
            entry.name, entry.signed, entry.method, entry.status
        ));
    }
    Ok(())
}

fn validate_checksums(checksums: &ChecksumSigning) -> Res<()> {
    let consistent = matches!(
        (checksums.signed, checksums.method),
        (true, ChecksumSigningMethod::SigstoreKeyless) | (false, ChecksumSigningMethod::None)
    );
    if !consistent {
        return err(format!(
            "checksum signing is contradictory: signed={}, method={}",
            checksums.signed, checksums.method
        ));
    }
    let status = checksums.status.trim().to_ascii_lowercase();
    if status.is_empty()
        || (checksums.signed
            && (!status.starts_with("signed")
                || status.contains("unavailable")
                || status.contains("not signed")))
        || (!checksums.signed && status.starts_with("signed"))
    {
        return err(format!(
            "checksum signing status '{}' contradicts signed={}",
            checksums.status, checksums.signed
        ));
    }
    Ok(())
}

fn validate_updater_shape(updater: &UpdaterState) -> Res<()> {
    if updater.available {
        if updater.reason.is_some() {
            return err("updater is available but also has an unavailability reason");
        }
        if updater.artifacts.is_empty() {
            return err("available updater must list the updater artifacts it produced");
        }
        let names: HashSet<&str> = updater.artifacts.iter().map(String::as_str).collect();
        if names.len() != updater.artifacts.len() {
            return err("available updater lists the same artifact twice");
        }
        // Every listed name is an updater artifact of this run: a payload that
        // is signed only by the updater key, or a detached `.sig` of a payload
        // that keeps another method (Authenticode, checksum-signature).
        if !updater.artifacts.iter().any(|name| name.ends_with(".sig")) {
            return err("available updater lists no signature file");
        }
    } else if !updater.artifacts.is_empty()
        || updater
            .reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
    {
        return err("unavailable updater must list no artifacts and give a non-empty reason");
    }
    Ok(())
}

/// The updater proof of an artifact: the Tauri updater key either signs the
/// payload itself (`method = tauri-updater-key`, the macOS archive) or signs a
/// detached `.sig` next to it (`updater_signature`, the Windows installers and
/// the Linux packages, which keep Authenticode or checksum-signature trust).
fn updater_proof(entry: &ArtifactSigning) -> bool {
    entry.method == SigningMethod::TauriUpdaterKey || entry.updater_signature.is_some()
}

fn validate_updater_state(updater: &UpdaterState, artifacts: &[ArtifactSigning]) -> Res<()> {
    validate_updater_shape(updater)?;
    if updater.available {
        for name in &updater.artifacts {
            let Some(entry) = artifacts.iter().find(|entry| &entry.name == name) else {
                // A detached `.sig` is part of the updater state but has no
                // record of its own: the payload's record carries its proof.
                if let Some(payload) = name.strip_suffix(".sig") {
                    if let Some(payload) = artifacts.iter().find(|entry| entry.name == payload) {
                        if payload.updater_signature.as_deref() == Some(name) {
                            continue;
                        }
                    }
                }
                return err(format!(
                    "updater is available but '{name}' has no signing state"
                ));
            };
            if !updater_proof(entry) {
                return err(format!(
                    "updater artifact '{name}' is not signed by the updater key"
                ));
            }
            // An artifact whose own bytes are signed has no detached signature name;
            // when it does, the listed name must be that signature.
            if let Some(signature) = &entry.updater_signature {
                if updater.artifacts.iter().all(|listed| listed != signature) {
                    return err(format!(
                        "'{}' names updater signature '{signature}', which the updater state does not list",
                        entry.name
                    ));
                }
            }
        }
        let Some(entry) = artifacts.iter().find(|entry| entry.name == "latest.json") else {
            return err("updater is available but 'latest.json' has no signing state");
        };
        if entry.method != SigningMethod::ChecksumSignature {
            return err("latest.json must use checksum-signature trust");
        }
        for entry in artifacts {
            if updater_proof(entry) && !updater.artifacts.contains(&entry.name) {
                return err(format!(
                    "'{}' is signed by the updater key but the updater state does not list it",
                    entry.name
                ));
            }
        }
    } else if artifacts
        .iter()
        .any(|entry| entry.name == "latest.json" || updater_proof(entry))
    {
        return err("updater is unavailable but updater artifacts have signing entries");
    }
    Ok(())
}

fn validate_unique_artifacts(artifacts: &[ArtifactSigning], unresolved_checksum: bool) -> Res<()> {
    let mut names = HashSet::new();
    for artifact in artifacts {
        validate_artifact(artifact, unresolved_checksum)?;
        if !names.insert(artifact.name.as_str()) {
            return err(format!("duplicate signing state for '{}'", artifact.name));
        }
    }
    Ok(())
}

pub fn merge(
    version: &str,
    channel: Channel,
    fragments: Vec<Fragment>,
    manifest: &Manifest,
    checksums: ChecksumSigning,
) -> Res<SigningState> {
    if manifest.version != version {
        return err(format!(
            "manifest is for version {} but the signing state is for {version}",
            manifest.version
        ));
    }
    if manifest.channel != channel {
        return err(format!(
            "manifest channel {} differs from signing-state channel {}",
            manifest.channel.as_str(),
            channel.as_str()
        ));
    }
    validate_checksums(&checksums)?;

    let mut artifacts = Vec::new();
    // Every platform fragment reports the updater archives it signed, so the
    // updater state is the union of those reports: available as soon as one
    // platform produced updater artifacts, unavailable with that platform's
    // reason when none did.
    let mut updater_artifacts: Vec<String> = Vec::new();
    let mut updater_reason: Option<String> = None;
    for fragment in fragments {
        validate_unique_artifacts(&fragment.artifacts, true)?;
        if let Some(candidate) = &fragment.updater {
            validate_updater_shape(candidate)?;
        }
        for artifact in fragment.artifacts {
            if artifacts
                .iter()
                .any(|existing: &ArtifactSigning| existing.name == artifact.name)
            {
                return err(format!(
                    "two fragments report the signing state of '{}'",
                    artifact.name
                ));
            }
            artifacts.push(artifact);
        }
        if let Some(candidate) = fragment.updater {
            if candidate.available {
                for name in &candidate.artifacts {
                    if updater_artifacts.contains(name) {
                        return err(format!(
                            "two fragments report '{name}' as an updater artifact"
                        ));
                    }
                    updater_artifacts.push(name.clone());
                }
            } else if updater_reason.is_none() {
                updater_reason = candidate.reason.clone();
            }
        }
    }
    let updater = if updater_artifacts.is_empty() {
        UpdaterState {
            available: false,
            artifacts: Vec::new(),
            reason: Some(
                updater_reason
                    .unwrap_or_else(|| "no updater artifacts produced in this run".to_string()),
            ),
        }
    } else {
        updater_artifacts.sort();
        UpdaterState {
            available: true,
            artifacts: updater_artifacts,
            reason: None,
        }
    };

    // A fragment may report the detached `.sig` as part of the updater state
    // without giving the payload a second record (the payload keeps its own
    // method). The merged state carries it on the payload's record, so the
    // published state has exactly one entry per artifact.
    for entry in &mut artifacts {
        if updater_proof(entry) {
            continue;
        }
        let signature = format!("{}.sig", entry.name);
        if updater.artifacts.iter().any(|name| name == &signature) {
            entry.updater_signature = Some(signature);
        }
    }

    for artifact in &manifest.artifacts {
        // A detached `.sig` is covered by the updater proof of the payload it
        // belongs to, so it needs no record of its own — only its payload.
        if artifact.name.ends_with(".sig")
            && artifacts
                .iter()
                .any(|entry| entry.updater_signature.as_deref() == Some(artifact.name.as_str()))
        {
            continue;
        }
        if !artifacts.iter().any(|entry| entry.name == artifact.name) {
            return err(format!(
                "no signing state for '{}'; every artifact must be reported",
                artifact.name
            ));
        }
    }
    for entry in &artifacts {
        if !manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.name == entry.name)
        {
            return err(format!(
                "signing state reported for '{}', which is not in the manifest",
                entry.name
            ));
        }
    }

    for entry in &mut artifacts {
        if entry.method == SigningMethod::ChecksumSignature {
            if checksums.signed {
                entry.signed = true;
                entry.status = SigningStatus::Signed;
            } else {
                entry.signed = false;
                entry.status = SigningStatus::Unsigned;
                let reason = &checksums.status;
                entry.note = Some(match entry.note.take() {
                    Some(note) => format!("{note}; checksum signature unavailable: {reason}"),
                    None => format!("checksum signature unavailable: {reason}"),
                });
            }
        }
        validate_artifact(entry, false)?;
        if entry.method == SigningMethod::ChecksumSignature && entry.signed != checksums.signed {
            return err(format!(
                "'{}' contradicts checksum-signature availability",
                entry.name
            ));
        }
    }

    for name in &updater.artifacts {
        if !manifest
            .artifacts
            .iter()
            .any(|artifact| &artifact.name == name)
        {
            return err(format!("updater artifact '{name}' is not in the manifest"));
        }
    }
    validate_updater_state(&updater, &artifacts)?;

    artifacts.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(SigningState {
        schema: SIGNING_STATE_SCHEMA,
        version: version.to_string(),
        channel,
        artifacts,
        checksums,
        updater,
    })
}

pub fn load_fragment(path: &Path) -> Res<Fragment> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Msg(format!("cannot read {}: {e}", path.display())))?;
    let fragment: Fragment = serde_json::from_str(&text).map_err(|e| Error::Json {
        context: path.display().to_string(),
        source: e,
    })?;
    validate_unique_artifacts(&fragment.artifacts, true)
        .map_err(|e| Error::Msg(format!("{}: {e}", path.display())))?;
    if let Some(updater) = &fragment.updater {
        validate_updater_shape(updater)
            .map_err(|e| Error::Msg(format!("{}: {e}", path.display())))?;
    }
    Ok(fragment)
}

pub fn load(path: &Path) -> Res<SigningState> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Msg(format!("cannot read {}: {e}", path.display())))?;
    let state: SigningState = serde_json::from_str(&text).map_err(|e| Error::Json {
        context: path.display().to_string(),
        source: e,
    })?;
    if state.schema != SIGNING_STATE_SCHEMA {
        return err(format!(
            "{}: schema {} is not supported (expected {SIGNING_STATE_SCHEMA})",
            path.display(),
            state.schema
        ));
    }
    Version::parse(&state.version)
        .map_err(|e| Error::Msg(format!("{}: invalid version: {e}", path.display())))?;
    validate_checksums(&state.checksums)
        .map_err(|e| Error::Msg(format!("{}: {e}", path.display())))?;
    validate_unique_artifacts(&state.artifacts, false)
        .map_err(|e| Error::Msg(format!("{}: {e}", path.display())))?;
    for artifact in &state.artifacts {
        if artifact.method == SigningMethod::ChecksumSignature
            && artifact.signed != state.checksums.signed
        {
            return err(format!(
                "{}: '{}' contradicts checksum-signature availability",
                path.display(),
                artifact.name
            ));
        }
    }
    validate_updater_state(&state.updater, &state.artifacts)
        .map_err(|e| Error::Msg(format!("{}: {e}", path.display())))?;
    Ok(state)
}
