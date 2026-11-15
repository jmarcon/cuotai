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
    for artifact in &state.artifacts {
        let entry = manifest.artifacts.iter().find(|a| a.name == artifact.name);
        let platform = entry
            .map(|a| format!("{}/{}", a.platform, a.arch))
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
        out.push_str("Optional artifacts not produced in this run (no ARM64 runner or no signing prerequisite):\n\n");
        for name in &manifest.missing_optional {
            out.push_str(&format!("- `{name}`\n"));
        }
        out.push('\n');
    }

    if state.updater.available {
        out.push_str(
            "In-app updater artifacts (`Cuotai.app.tar.gz` for macOS, the updater signatures of the Windows and Linux installers, and `latest.json`) are attached for PRD 0018.\n\n",
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
        (SigningMethod::TauriUpdaterKey, SigningStatus::Signed) => {
            "updater key, for in-app updates"
        }
        _ => "**none**",
    };
    match artifact.updater_signature.as_deref() {
        // A second, independent proof of the same bytes: the detached updater
        // signature beside the package (PRD 0018 RF1.1).
        Some(sig) if artifact.method == SigningMethod::TauriUpdaterKey => {
            format!("updater key, for in-app updates (`{sig}`)")
        }
        Some(sig) if base == "**none**" => {
            format!("updater key only, for in-app updates (`{sig}`)")
        }
        Some(sig) => format!("{base} + updater key (`{sig}`)"),
        None => base.to_string(),
    }
}

/// The artifact's own note, without the hedges the two columns now state.
fn artifact_note(artifact: &ArtifactSigning, checksums_signed: bool) -> String {
    match artifact.note.as_deref() {
        Some(_) if checksums_signed && artifact.method == SigningMethod::ChecksumSignature => {
            String::new()
        }
        Some(note) => note.to_string(),
        None => String::new(),
    }
}
