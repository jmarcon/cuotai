//! Tauri updater manifest (`latest.json`, PRD 0021 RF2 / PRD 0018 RF1.3).
//! Only generated when the updater archive carries a real signature from the
//! Tauri updater private key; the key is never used for anything else.

use std::path::Path;

use crate::{err, Error, Res, Version};

/// Builds the updater manifest for the universal macOS archive.
///
/// `archive` is the path to `Cuotai.app.tar.gz`; the signature is read from
/// the sibling `<archive>.sig` written by `cargo tauri signer sign`.
/// `pub_date` is the tagged commit's timestamp normalized to RFC 3339 by the
/// caller, so rerunning the same source revision stays deterministic.
pub fn latest_json(
    version: &Version,
    notes: &str,
    pub_date: &str,
    repo: &str,
    tag: &str,
    archive: &Path,
) -> Res<serde_json::Value> {
    if !valid_repo_slug(repo) {
        return err(format!("repository slug '{repo}' is not 'owner/name'"));
    }
    if !valid_pub_date(pub_date) {
        return err(format!(
            "pub_date '{pub_date}' is not RFC 3339 (YYYY-MM-DDTHH:MM:SSZ)"
        ));
    }
    let name = archive
        .file_name()
        .ok_or_else(|| Error::Msg(format!("{}: not a file name", archive.display())))?
        .to_string_lossy()
        .into_owned();
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
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{name}");
    let platform = serde_json::json!({ "signature": signature, "url": url });
    Ok(serde_json::json!({
        "version": version.to_string(),
        "notes": notes.trim(),
        "pub_date": pub_date,
        "platforms": {
            "darwin-aarch64": platform,
            "darwin-x86_64": platform,
        }
    }))
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
