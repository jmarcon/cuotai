//! Tauri updater manifest (`latest.json`, PRD 0018 RF1.3/RF1.4, PRD 0021
//! RF2). The manifest is only generated from updater archives that carry a
//! real signature from the Tauri updater private key; the key is never used
//! for anything else.
//!
//! One manifest serves every platform: `tauri-plugin-updater` looks up
//! `{os}-{arch}-{installer}` first and falls back to `{os}-{arch}`, so the
//! document carries a generic entry per architecture plus the
//! installer-specific entries whose bytes differ (NSIS vs MSI, AppImage vs
//! deb/rpm).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{err, Error, Res, Version};

/// Operating systems the updater plugin reports (`{os}` of a manifest key).
const OSES: [&str; 3] = ["darwin", "linux", "windows"];
/// Architectures the updater plugin reports (`{arch}` of a manifest key).
const ARCHES: [&str; 2] = ["aarch64", "x86_64"];
/// Installer names the plugin appends to the manifest key, per operating
/// system (`{installer}` of a manifest key, `installer_for_bundle_type`), in
/// preference order: the first one owns the generic `{os}-{arch}` fallback, so
/// which artifact an updater that cannot identify the running bundle downloads
/// never depends on the order the pipeline passed its arguments in.
const INSTALLERS: [(&str, &[&str]); 3] = [
    ("darwin", &["app"]),
    ("linux", &["appimage", "deb", "rpm"]),
    ("windows", &["nsis", "msi"]),
];

/// One updater archive and the manifest keys it answers to.
#[derive(Clone, Debug)]
pub struct PlatformArchive {
    /// Keys this archive claims: every `{os}-{arch}-{installer}` it was named
    /// for, and the generic `{os}-{arch}` keys the caller named explicitly.
    pub keys: Vec<String>,
    /// Path to the signed updater archive; its sibling `<ext>.sig` is required.
    pub archive: PathBuf,
    /// The generic `{os}-{arch}` keys this archive offers, with their
    /// preference: `0` when the caller named the key, otherwise the rank of the
    /// installer it would serve through the fallback.
    generics: Vec<(String, usize)>,
}

impl PlatformArchive {
    /// Both keys one archive answers to: the installer-specific
    /// `{os}-{arch}-{installer}` key and the generic `{os}-{arch}` fallback the
    /// plugin looks up when it cannot identify the running bundle type.
    pub fn new(key: &str, archive: &Path) -> Res<Self> {
        Self::with_keys(&[key], archive)
    }

    /// Explicit key list, for a release where one architecture is served by
    /// archives with different bytes per installer (`nsis` + `msi`,
    /// `appimage` + `deb` + `rpm`), and for one archive that answers for
    /// several architectures (the universal macOS app).
    pub fn with_keys(wanted: &[&str], archive: &Path) -> Res<Self> {
        if wanted.is_empty() {
            return err("an updater archive must serve at least one platform key");
        }
        let mut keys: Vec<String> = Vec::with_capacity(wanted.len());
        let mut generics: Vec<(String, usize)> = Vec::new();
        for key in wanted {
            let (os, arch, installer) = split_platform_key(key)?;
            if !keys.iter().any(|other| other == key) {
                keys.push((*key).to_string());
            }
            let (generic, rank) = match installer {
                // The caller decided which archive serves the generic key.
                None => ((*key).to_string(), 0),
                Some(installer) => (
                    generic_key(os, arch),
                    INSTALLERS
                        .iter()
                        .find(|(name, _)| *name == os)
                        .and_then(|(_, installers)| {
                            installers.iter().position(|name| *name == installer)
                        })
                        .unwrap_or(usize::MAX),
                ),
            };
            generics.retain(|(existing, _)| existing != &generic);
            generics.push((generic, rank));
        }
        Ok(Self {
            keys,
            archive: archive.to_path_buf(),
            generics,
        })
    }

    /// Merges another archive's keys into this one; used when the same file
    /// answers for several platforms (one universal app for both macOS
    /// architectures, one AppImage for every Linux installer key).
    pub fn merge(&mut self, other: Self) -> Res<()> {
        for key in other.keys {
            if !self.keys.contains(&key) {
                self.keys.push(key);
            }
        }
        for (generic, rank) in other.generics {
            match self.generics.iter_mut().find(|(key, _)| *key == generic) {
                Some(entry) => entry.1 = entry.1.min(rank),
                None => self.generics.push((generic, rank)),
            }
        }
        Ok(())
    }
}

/// Builds the updater manifest for every platform this release produced.
///
/// `archives` are the collected updater archives; each needs its sibling
/// `.sig` written by `cargo tauri signer sign`, and a missing signature is an
/// error: an updater archive is never advertised unsigned.
///
/// `required` names the platforms a complete release must carry. An optional
/// leg that produced nothing (no ARM64 runner, no updater key) is simply
/// absent, never fabricated; a required one fails the release.
///
/// `pub_date` is the tagged commit's timestamp normalized to RFC 3339 by the
/// caller, so rerunning the same source revision stays deterministic.
pub fn latest_json(
    version: &Version,
    notes: &str,
    pub_date: &str,
    repo: &str,
    tag: &str,
    archives: &[PlatformArchive],
    required: &[&str],
) -> Res<serde_json::Value> {
    if !valid_repo_slug(repo) {
        return err(format!("repository slug '{repo}' is not 'owner/name'"));
    }
    if !valid_pub_date(pub_date) {
        return err(format!(
            "pub_date '{pub_date}' is not RFC 3339 (YYYY-MM-DDTHH:MM:SSZ)"
        ));
    }
    if archives.is_empty() {
        return err("no updater archive was collected; latest.json is not generated");
    }

    let mut platforms: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for entry in archives {
        let value = platform_value(repo, tag, &entry.archive)?;
        for key in &entry.keys {
            insert_key(&mut platforms, key, value.clone())?;
        }
    }

    // The generic `{os}-{arch}` entry of an architecture served by several
    // installers goes to the best-ranked one (NSIS before MSI, AppImage before
    // deb/rpm): that is what an updater unable to identify the running bundle
    // type downloads, and it must not depend on argument order.
    let mut owners: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (index, entry) in archives.iter().enumerate() {
        for (key, rank) in &entry.generics {
            match owners.get(key) {
                Some((best, _)) if *best <= *rank => {}
                _ => {
                    owners.insert(key.clone(), (*rank, index));
                }
            }
        }
    }
    for (key, (_, index)) in owners {
        insert_key(
            &mut platforms,
            &key,
            platform_value(repo, tag, &archives[index].archive)?,
        )?;
    }

    // A required platform may be named generic or installer-specific; either
    // way the architecture must be served.
    for key in required {
        split_platform_key(key)?;
        let generic = generic_platform_key(key)?;
        if !platforms.contains_key(&generic) {
            return err(format!(
                "required platform '{key}' is missing from the updater manifest"
            ));
        }
    }

    Ok(serde_json::json!({
        "version": version.to_string(),
        "notes": notes.trim(),
        "pub_date": pub_date,
        "platforms": platforms,
    }))
}

/// The `{signature, url}` entry one archive contributes to the manifest.
fn platform_value(repo: &str, tag: &str, archive: &Path) -> Res<serde_json::Value> {
    let name = archive
        .file_name()
        .ok_or_else(|| Error::Msg(format!("{}: not a file name", archive.display())))?
        .to_string_lossy()
        .into_owned();
    Ok(serde_json::json!({
        "signature": read_signature(archive)?,
        "url": download_url(repo, tag, &name)?,
    }))
}

fn insert_key(
    platforms: &mut BTreeMap<String, serde_json::Value>,
    key: &str,
    value: serde_json::Value,
) -> Res<()> {
    if platforms.insert(key.to_string(), value).is_some() {
        return err(format!(
            "two updater archives claim the manifest key '{key}'"
        ));
    }
    Ok(())
}

/// The HTTPS download URL of a release asset (PRD 0018 RF4.3: the manifest
/// never points at plain HTTP).
pub fn download_url(repo: &str, tag: &str, name: &str) -> Res<String> {
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{name}");
    if !url.starts_with("https://") {
        return err(format!("refusing a non-HTTPS updater URL: {url}"));
    }
    Ok(url)
}

/// Reads `<archive>.sig`, the file `cargo tauri signer sign` writes next to
/// the artifact. It is the base64 of the minisign signature box, exactly what
/// the plugin's `signature` field carries.
fn read_signature(archive: &Path) -> Res<String> {
    let sig_path = match archive.extension() {
        Some(ext) => archive.with_extension(format!("{}.sig", ext.to_string_lossy())),
        None => archive.with_extension("sig"),
    };
    let signature = std::fs::read_to_string(&sig_path).map_err(|e| {
        Error::Msg(format!(
            "updater signature {} is missing ({e}); the updater archive requires the \
             Tauri updater private key",
            sig_path.display()
        ))
    })?;
    let signature = signature.trim();
    if signature.is_empty() {
        return err(format!("updater signature {} is empty", sig_path.display()));
    }
    if !signature
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        return err(format!(
            "updater signature {} is not base64",
            sig_path.display()
        ));
    }
    Ok(signature.to_string())
}

/// Splits a manifest key into `(os, arch, installer)`, where the installer is
/// absent for the generic `{os}-{arch}` fallback, and validates every part: an
/// unknown operating system, architecture or installer is refused before it can
/// reach the manifest.
fn split_platform_key(key: &str) -> Res<(&str, &str, Option<&str>)> {
    let mut parts = key.split('-');
    let (os, arch, installer) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(os), Some(arch), None, None) => (os, arch, None),
        (Some(os), Some(arch), Some(installer), None) => (os, arch, Some(installer)),
        _ => {
            return err(format!(
                "platform key '{key}' is not '{{os}}-{{arch}}' or '{{os}}-{{arch}}-{{installer}}'"
            ))
        }
    };
    if !OSES.contains(&os) {
        return err(format!(
            "platform key '{key}' names operating system '{os}', which the updater never reports"
        ));
    }
    if !ARCHES.contains(&arch) {
        return err(format!(
            "platform key '{key}' names architecture '{arch}', which the updater never reports"
        ));
    }
    if let Some(installer) = installer {
        let installers = INSTALLERS
            .iter()
            .find(|(name, _)| *name == os)
            .map(|(_, installers)| *installers)
            .unwrap_or_default();
        if !installers.contains(&installer) {
            return err(format!(
                "platform key '{key}' names installer '{installer}', which no {os} bundle produces"
            ));
        }
    }
    Ok((os, arch, installer))
}

/// The generic `{os}-{arch}` fallback of a manifest key, the entry the plugin
/// uses when it cannot identify the running bundle type.
fn generic_platform_key(key: &str) -> Res<String> {
    let (os, arch, _) = split_platform_key(key)?;
    Ok(generic_key(os, arch))
}

fn generic_key(os: &str, arch: &str) -> String {
    format!("{os}-{arch}")
}

fn valid_repo_slug(repo: &str) -> bool {
    let mut parts = repo.split('/');
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if !a.is_empty() && !b.is_empty())
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

fn valid_pub_date(text: &str) -> bool {
    // YYYY-MM-DDTHH:MM:SSZ — the workflow normalizes the source revision's
    // fixed commit timestamp into this UTC form.
    let bytes = text.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || b.is_ascii_digit())
}
