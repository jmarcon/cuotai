//! Release body assembly (PRD 0021 RF4.2): the version's changelog section,
//! then the signing-state table and the verification commands. Missing or
//! empty changelog sections already failed in `release-check`.

use crate::manifest::Manifest;
use crate::signing::SigningState;

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

    out.push_str("| Artifact | Platform | Signed | Method / status |\n");
    out.push_str("|---|---|---|---|\n");
    for artifact in &state.artifacts {
        let signed = if artifact.signed { "yes" } else { "**no**" };
        let platform = manifest
            .artifacts
            .iter()
            .find(|a| a.name == artifact.name)
            .map(|a| format!("{}/{}", a.platform, a.arch))
            .unwrap_or_else(|| "-".into());
        let note = artifact
            .note
            .as_deref()
            .map(|n| format!(" — {n}"))
            .unwrap_or_default();
        // A second, independent proof of the same bytes: the detached updater
        // signature beside the package (PRD 0018 RF1.1).
        let updater = artifact
            .updater_signature
            .as_deref()
            .map(|sig| format!(" (+ `{sig}` by the updater key)"))
            .unwrap_or_default();
        out.push_str(&format!(
            "| `{}` | {} | {} | {}, {}{}{} |\n",
            artifact.name, platform, signed, artifact.method, artifact.status, updater, note
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
    out.push_str("```sh\n");
    out.push_str("shasum -a 256 -c SHA256SUMS\n");
    if state.checksums.signed {
        out.push_str(&format!(
            "gh attestation verify Cuotai_{}_universal.dmg --repo {repo}\n",
            state.version
        ));
    }
    out.push_str("```\n\n");
    if state.checksums.signed {
        out.push_str(
            "`SHA256SUMS` is signed with Sigstore keyless signing from this repository's GitHub Actions identity:\n\n",
        );
        out.push_str("```sh\n");
        out.push_str("cosign verify-blob \\\n");
        out.push_str("  --bundle SHA256SUMS.sigstore.json \\\n");
        // Only a tag run of the release workflow is a release identity; a
        // dispatch rehearsal signs with its branch ref and must not match.
        let identity =
            format!("^https://github.com/{repo}/.github/workflows/release.yml@refs/tags/v")
                .replace('.', "\\.");
        out.push_str(&format!(
            "  --certificate-identity-regexp '{identity}' \\\n"
        ));
        out.push_str(
            "  --certificate-oidc-issuer https://token.actions.githubusercontent.com \\\n",
        );
        out.push_str("  SHA256SUMS\n");
        out.push_str("```\n");
    } else {
        out.push_str(&format!(
            "`SHA256SUMS` is **not** signed in this workflow-dispatch rehearsal ({}). \
             Provenance attestations are also not published for dispatch runs; these \
             local checksums provide no external authenticity claim.\n",
            state.checksums.status
        ));
    }
    out
}
