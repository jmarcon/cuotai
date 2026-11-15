//! Release body assembly (PRD 0021 RF4.2): the version's changelog section,
//! then the signing-state table and the verification commands, cosign first. Missing or
//! empty changelog sections already failed in `release-check`.

use crate::manifest::Manifest;
use crate::signing::{ArtifactSigning, SigningMethod, SigningState, SigningStatus};

/// Renders the GitHub Release body (Markdown).
pub fn render_body(
    changelog_body: &str,
    state: &SigningState,
    manifest: &Manifest,
    repo: &str,
) -> String {
    let mut out = String::new();
    out.push_str(changelog_body.trim());
    out.push_str("\n\n---\n\n## Downloads and signing\n\n");

    // Two separate facts per artifact (PRD 0019 re-validation, V-04/S-04):
    // whether the file itself carries a signature, and whether its hash is in
    // `SHA256SUMS` and that file is signed. A checksum entry is never shown
    // as a payload signature.
    out.push_str("| Artifact | Platform | Payload signature | Checksum | Notes |\n");
    out.push_str("|---|---|---|---|---|\n");
    // The main downloads first (PRD 0021 re-validation V-05), then the
    // terminal archives, then the updater and integrity files; the stable
    // sort keeps the existing order inside each kind.
    let entry_of = |name: &str| manifest.artifacts.iter().find(|a| a.name == name);
    let mut artifacts: Vec<&ArtifactSigning> = state.artifacts.iter().collect();
    artifacts.sort_by_key(|artifact| kind_rank(entry_of(&artifact.name).map(|a| a.kind.as_str())));
    for artifact in artifacts {
        let entry = entry_of(&artifact.name);
        let platform = entry
            .map(|a| platform_label(&a.platform, &a.arch))
            .unwrap_or_else(|| "-".into());
        let checksum = match (entry.is_some(), state.checksums.signed) {
            (true, true) => "in signed `SHA256SUMS` (cosign)",
            (true, false) => "in unsigned `SHA256SUMS`",
            (false, _) => "not listed",
        };
        let note = artifact_note(artifact, state.checksums.signed);
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            artifact.name,
            platform,
            payload_signature(artifact),
            checksum,
            note
        ));
    }
    out.push('\n');

    if !manifest.missing_optional.is_empty() {
        out.push_str("Optional artifacts not produced in this run (an optional ARM64 build or a signing prerequisite was unavailable):\n\n");
        for name in &manifest.missing_optional {
            out.push_str(&format!("- `{name}`\n"));
        }
        out.push('\n');
    }

    if state.updater.available {
        out.push_str(
            "In-app updater artifacts (`Cuotai.app.tar.gz` for macOS, the updater signatures of the Windows and Linux installers, and `latest.json`) are attached.\n\n",
        );
    } else {
        let reason = state
            .updater
            .reason
            .as_deref()
            .unwrap_or("updater signing prerequisite not configured");
        out.push_str(&format!(
            "In-app updater artifacts were **not** produced: {reason}.\n\n"
        ));
    }

    out.push_str("## Verify your download\n\n");
    if state.checksums.signed {
        // Only a tag run of the release workflow is a release identity; a
        // dispatch rehearsal signs with its branch ref and must not match.
        let identity =
            format!("^https://github.com/{repo}/.github/workflows/release.yml@refs/tags/v")
                .replace('.', "\\.");
        out.push_str(
            "1. Check that `SHA256SUMS` was signed by this repository's release workflow (Sigstore keyless):\n\n",
        );
        out.push_str("   ```sh\n");
        out.push_str("   cosign verify-blob \\\n");
        out.push_str("     --bundle SHA256SUMS.sigstore.json \\\n");
        out.push_str(&format!(
            "     --certificate-identity-regexp '{identity}' \\\n"
        ));
        out.push_str(
            "     --certificate-oidc-issuer https://token.actions.githubusercontent.com \\\n",
        );
        out.push_str("     SHA256SUMS\n");
        out.push_str("   ```\n\n");
        out.push_str(
            "2. Check your downloads against it. A matching checksum proves nothing on its own; it counts only after step 1 passed:\n\n",
        );
        out.push_str("   ```sh\n");
        out.push_str("   shasum -a 256 -c --ignore-missing SHA256SUMS\n");
        out.push_str("   ```\n\n");
        out.push_str("3. Optionally, check the build provenance of any file:\n\n");
        out.push_str("   ```sh\n");
        out.push_str(&format!("   gh attestation verify <file> --repo {repo}\n"));
        out.push_str("   ```\n");
    } else {
        out.push_str("```sh\n");
        out.push_str("shasum -a 256 -c --ignore-missing SHA256SUMS\n");
        out.push_str("```\n\n");
        out.push_str(&format!(
            "`SHA256SUMS` is **not** signed in this workflow-dispatch rehearsal ({}). \
             Provenance attestations are also not published for dispatch runs; these \
             local checksums provide no external authenticity claim.\n",
            state.checksums.status
        ));
    }
    out
}

/// What signs the file's own bytes, independent of `SHA256SUMS`.
fn payload_signature(artifact: &ArtifactSigning) -> String {
    let base = match (artifact.method, artifact.status) {
        (SigningMethod::DeveloperId, SigningStatus::SignedNotarized) => "Developer ID + notarized",
        (SigningMethod::DeveloperId, SigningStatus::Signed) => "Developer ID (not notarized)",
        (SigningMethod::AdHoc, _) => "ad-hoc only (not trusted by Gatekeeper)",
        (SigningMethod::Authenticode, SigningStatus::Signed) => "Authenticode",
        (SigningMethod::TauriUpdaterKey, SigningStatus::Signed) => "updater key for in-app updates",
        _ => "**none**",
    };
    match artifact.updater_signature.as_deref() {
        // A second, independent proof of the same bytes: the detached updater
        // signature beside the package (PRD 0018 RF1.1).
        Some(sig) if artifact.method == SigningMethod::TauriUpdaterKey => {
            format!("updater key for in-app updates (`{sig}`)")
        }
        // V-05: a reader scanning this column must see that the installer
        // itself carries no publisher signature.
        Some(sig) if base == "**none**" => {
            format!("**none** (no Authenticode); updater key for in-app updates (`{sig}`)")
        }
        Some(sig) => format!("{base} + updater key for in-app updates (`{sig}`)"),
        None => base.to_string(),
    }
}

/// Display order of an artifact kind: what most people download first.
fn kind_rank(kind: Option<&str>) -> u8 {
    match kind {
        Some("desktop-dmg") => 0,
        Some("desktop-installer-nsis") => 1,
        Some("desktop-installer-msi") => 2,
        Some("desktop-appimage") => 3,
        Some("desktop-deb") => 4,
        Some("desktop-rpm") => 5,
        Some("cli-archive") => 6,
        Some("desktop-updater-archive") => 7,
        Some("desktop-updater-signature") => 8,
        Some("updater-manifest") => 9,
        Some("sbom") => 10,
        _ => 11,
    }
}

/// `macOS (universal)`, `Windows x64`, `Linux amd64` or `any`.
fn platform_label(platform: &str, arch: &str) -> String {
    let os = match platform {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "any" => return "any".into(),
        other => other,
    };
    if arch == "universal" {
        format!("{os} (universal)")
    } else {
        format!("{os} {arch}")
    }
}

/// The artifact's own note, without the hedges the two columns now state.
fn artifact_note(artifact: &ArtifactSigning, checksums_signed: bool) -> String {
    if artifact.name == "Cuotai.app.tar.gz" && artifact.note.is_none() {
        return "the same app as the DMG, for in-app updates".into();
    }
    match artifact.note.as_deref() {
        Some(_) if checksums_signed && artifact.method == SigningMethod::ChecksumSignature => {
            String::new()
        }
        Some(note) => note.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(method: SigningMethod, sig: Option<&str>) -> ArtifactSigning {
        ArtifactSigning {
            name: "Cuotai_1.0.0_x64-setup.exe".into(),
            signed: false,
            method,
            status: SigningStatus::Unsigned,
            updater_signature: sig.map(str::to_string),
            note: None,
        }
    }

    #[test]
    fn an_installer_without_authenticode_reads_none_first() {
        let sig = Some("Cuotai_1.0.0_x64-setup.exe.sig");
        assert_eq!(
            payload_signature(&artifact(SigningMethod::None, sig)),
            "**none** (no Authenticode); updater key for in-app updates (`Cuotai_1.0.0_x64-setup.exe.sig`)"
        );
        assert_eq!(
            payload_signature(&artifact(SigningMethod::None, None)),
            "**none**"
        );
    }

    #[test]
    fn platforms_and_kinds_read_like_a_download_page() {
        assert_eq!(platform_label("macos", "universal"), "macOS (universal)");
        assert_eq!(platform_label("windows", "x64"), "Windows x64");
        assert_eq!(platform_label("any", "any"), "any");
        assert!(kind_rank(Some("desktop-dmg")) < kind_rank(Some("cli-archive")));
        assert!(kind_rank(Some("cli-archive")) < kind_rank(Some("updater-manifest")));
        assert!(kind_rank(Some("sbom")) < kind_rank(None));
    }
}
