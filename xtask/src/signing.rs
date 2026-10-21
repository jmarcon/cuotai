//! Validated signing state for release artifacts (PRD 0021 RF3.4).

use crate::manifest::Manifest;
use crate::{err, Channel, Error, Res, Version};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path};

pub const SIGNING_STATE_SCHEMA: u32 = 1;

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
        let names: HashSet<&str> = updater.artifacts.iter().map(String::as_str).collect();
        if names.len() != updater.artifacts.len()
            || names != HashSet::from(["Cuotai.app.tar.gz", "Cuotai.app.tar.gz.sig"])
        {
            return err(
                "available updater must list exactly Cuotai.app.tar.gz and Cuotai.app.tar.gz.sig",
            );
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

fn validate_updater_state(updater: &UpdaterState, artifacts: &[ArtifactSigning]) -> Res<()> {
    validate_updater_shape(updater)?;
    let updater_names = ["Cuotai.app.tar.gz", "Cuotai.app.tar.gz.sig", "latest.json"];
    if updater.available {
        for name in updater_names {
            let Some(entry) = artifacts.iter().find(|entry| entry.name == name) else {
                return err(format!(
                    "updater is available but '{name}' has no signing state"
                ));
            };
            if name == "latest.json" {
                if entry.method != SigningMethod::ChecksumSignature {
                    return err("latest.json must use checksum-signature trust");
                }
            } else if entry.method != SigningMethod::TauriUpdaterKey
                || !entry.signed
                || entry.status != SigningStatus::Signed
            {
                return err(format!(
                    "updater artifact '{name}' is not signed by the updater key"
                ));
            }
        }
    } else if artifacts
        .iter()
        .any(|entry| updater_names.contains(&entry.name.as_str()))
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
    let mut updater = None;
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
            if updater.replace(candidate).is_some() {
                return err("two fragments report the updater state");
            }
        }
    }

    for artifact in &manifest.artifacts {
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

    let updater = updater.unwrap_or_else(|| UpdaterState {
        available: false,
        artifacts: Vec::new(),
        reason: Some("no updater artifacts produced in this run".into()),
    });
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
