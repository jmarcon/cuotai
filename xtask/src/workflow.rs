//! Static policy checks for GitHub Actions workflows (PRD 0021 RF9.2/RF9.5).
//! The guard intentionally knows the two repository workflows: changing a
//! permission, secret owner or release target requires an explicit policy
//! update instead of silently widening the pipeline.

use std::collections::BTreeSet;
use std::path::Path;

use serde_yaml::{Mapping, Value};

use crate::{err, Error, Res};

const RELEASE_HEAVY_JOBS: &[&str] = &[
    "desktop-macos",
    "desktop-windows",
    "desktop-windows-arm64",
    "desktop-linux",
    "desktop-linux-arm64",
    "cli",
    "sbom",
];

const SECRET_OWNERS: &[(&str, &str, &str)] = &[
    (
        "APPLE_CERTIFICATE",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_CERTIFICATE_PASSWORD",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_SIGNING_IDENTITY",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_SIGNING_IDENTITY",
        "desktop-macos",
        "Sign and verify app payload and DMG container",
    ),
    (
        "APPLE_SIGNING_IDENTITY",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "APPLE_API_KEY",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_API_KEY_ID",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_API_ISSUER",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_API_KEY",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "APPLE_API_KEY_ID",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "APPLE_API_ISSUER",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "APPLE_ID",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_PASSWORD",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_TEAM_ID",
        "desktop-macos",
        "Apple signing keychain (complete tag credentials only)",
    ),
    (
        "APPLE_ID",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "APPLE_PASSWORD",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "APPLE_TEAM_ID",
        "desktop-macos",
        "Notarize and staple app and DMG (complete tag credentials only)",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-macos",
        "Collect finished artifacts and updater archive",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-macos",
        "Collect finished artifacts and updater archive",
    ),
    (
        "WINDOWS_CERTIFICATE",
        "desktop-windows",
        "Sign and verify application executable (tag runs, when configured)",
    ),
    (
        "WINDOWS_CERTIFICATE_PASSWORD",
        "desktop-windows",
        "Sign and verify application executable (tag runs, when configured)",
    ),
    (
        "WINDOWS_CERTIFICATE",
        "desktop-windows",
        "Sign and verify installers and write signing fragment",
    ),
    (
        "WINDOWS_CERTIFICATE_PASSWORD",
        "desktop-windows",
        "Sign and verify installers and write signing fragment",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-windows",
        "Updater-sign the installers",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-windows",
        "Updater-sign the installers",
    ),
    (
        "WINDOWS_CERTIFICATE",
        "desktop-windows-arm64",
        "Sign and verify application executable (tag runs, when configured)",
    ),
    (
        "WINDOWS_CERTIFICATE_PASSWORD",
        "desktop-windows-arm64",
        "Sign and verify application executable (tag runs, when configured)",
    ),
    (
        "WINDOWS_CERTIFICATE",
        "desktop-windows-arm64",
        "Sign and verify installer and write signing fragment",
    ),
    (
        "WINDOWS_CERTIFICATE_PASSWORD",
        "desktop-windows-arm64",
        "Sign and verify installer and write signing fragment",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-windows-arm64",
        "Updater-sign the ARM64 installer (PRD 0018 RF1.1)",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-windows-arm64",
        "Updater-sign the ARM64 installer (PRD 0018 RF1.1)",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-linux",
        "Build AppImage, deb and rpm",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-linux",
        "Build AppImage, deb and rpm",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-linux-arm64",
        "Build AppImage, deb and rpm (ARM64)",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-linux-arm64",
        "Build AppImage, deb and rpm (ARM64)",
    ),
    (
        "GITHUB_TOKEN",
        "publish",
        "Create or update the draft and upload assets",
    ),
    (
        "GITHUB_TOKEN",
        "publish",
        "Verify the complete draft assembly (RF4.4, pre-publish)",
    ),
    ("GITHUB_TOKEN", "publish", "Publish"),
    (
        "GITHUB_TOKEN",
        "verify-public",
        "Download and verify the complete public assembly",
    ),
    (
        "HOMEBREW_TAP_TOKEN",
        "channel-homebrew",
        "Push the rendered manifests to the tap (tag runs only)",
    ),
    (
        "WINGET_SUBMIT_TOKEN",
        "channel-winget",
        "Open a winget-pkgs pull request (tag runs, opt-in)",
    ),
    (
        "SCOOP_BUCKET_TOKEN",
        "channel-scoop",
        "Push the manifest to the bucket (tag runs only)",
    ),
    (
        "CARGO_REGISTRY_TOKEN",
        "channel-cargo",
        "Publish cuotai-core, then cuotai-tui (tag runs only)",
    ),
    (
        "AUR_SSH_PRIVATE_KEY",
        "channel-aur",
        "Push PKGBUILD and .SRCINFO over SSH (tag runs only)",
    ),
];

/// Checks one workflow file against Cuotai's least-privilege and free-tier policy.
pub fn check_file(path: &Path) -> Res<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Msg(format!("cannot read {}: {e}", path.display())))?;
    check_text(path, &text)
}

fn check_text(path: &Path, text: &str) -> Res<()> {
    let root: Value = serde_yaml::from_str(text)
        .map_err(|e| Error::Msg(format!("{}: invalid workflow YAML: {e}", path.display())))?;
    let root = mapping(&root, "workflow root")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    check_default_permissions(root)?;
    check_triggers(name, root)?;
    for (key, value) in root {
        if string(key, "workflow key")? != "jobs" {
            reject_secret_references("workflow level", Some(value))?;
        }
    }

    let jobs = mapping(need(root, "jobs", "workflow root")?, "jobs")?;
    for (job_key, job_value) in jobs {
        let job_name = string(job_key, "job id")?;
        let job = mapping(job_value, &format!("job {job_name}"))?;
        check_timeout(job_name, job)?;
        check_job_permissions(job_name, job)?;
        check_job_secrets(job_name, job)?;
        check_action_pins(job_name, job)?;
    }

    if name == "release.yml" {
        check_release_policy(jobs)?;
    }
    Ok(())
}

fn check_default_permissions(root: &Mapping) -> Res<()> {
    let permissions = mapping(
        need(root, "permissions", "workflow root")?,
        "workflow permissions",
    )?;
    if permissions.len() != 1 || scalar(permissions, "contents") != Some("read") {
        return err("workflow permissions must be exactly 'contents: read'");
    }
    Ok(())
}

fn check_triggers(name: &str, root: &Mapping) -> Res<()> {
    let on = need(root, "on", "workflow root")?;
    let on = mapping(on, "workflow triggers")?;
    if on.contains_key(Value::String("pull_request_target".into())) {
        return err("pull_request_target is forbidden because it can expose secrets to fork code");
    }
    if name == "release.yml" {
        let actual: BTreeSet<&str> = on
            .keys()
            .map(|key| string(key, "trigger"))
            .collect::<Res<_>>()?;
        let expected = BTreeSet::from(["push", "workflow_dispatch"]);
        if actual != expected {
            return err("release.yml may trigger only on tag pushes and workflow_dispatch");
        }
        let push = mapping(
            need(on, "push", "release triggers")?,
            "release push trigger",
        )?;
        if push.contains_key(Value::String("branches".into()))
            || scalar_sequence(push, "tags")? != ["v*"]
        {
            return err("release.yml push trigger must be limited to v* tags");
        }
    }
    Ok(())
}

fn check_timeout(job_name: &str, job: &Mapping) -> Res<()> {
    match job.get(Value::String("timeout-minutes".into())) {
        Some(Value::Number(number)) if number.as_u64().is_some_and(|value| value > 0) => Ok(()),
        _ => err(format!(
            "job {job_name} must have a positive timeout-minutes"
        )),
    }
}

fn check_job_permissions(job_name: &str, job: &Mapping) -> Res<()> {
    let expected: &[(&str, &str)] = match job_name {
        "assemble" => &[
            ("contents", "read"),
            ("id-token", "write"),
            ("attestations", "write"),
        ],
        "publish" => &[("contents", "write"), ("attestations", "read")],
        "verify-public" => &[("contents", "read"), ("attestations", "read")],
        _ => &[],
    };
    let permissions = job.get(Value::String("permissions".into()));
    if expected.is_empty() {
        if permissions.is_some() {
            return err(format!(
                "job {job_name} must inherit the read-only workflow permissions"
            ));
        }
        return Ok(());
    }
    let permissions = mapping(
        permissions.ok_or_else(|| {
            Error::Msg(format!(
                "job {job_name} is missing its explicit permissions"
            ))
        })?,
        &format!("permissions for job {job_name}"),
    )?;
    let exact = permissions.len() == expected.len()
        && expected
            .iter()
            .all(|(permission, access)| scalar(permissions, permission) == Some(*access));
    if !exact {
        return err(format!(
            "job {job_name} permissions are broader than its checked policy"
        ));
    }
    if job_name == "assemble" {
        let has_attestation = steps(job)?.iter().any(|step| {
            scalar(step, "uses")
                .is_some_and(|uses| uses.starts_with("actions/attest-build-provenance@"))
        });
        if !has_attestation {
            return err("assemble grants id-token: write without an attestation step");
        }
    }
    Ok(())
}

fn check_job_secrets(job_name: &str, job: &Mapping) -> Res<()> {
    for (key, value) in job {
        if string(key, "job key")? != "steps" {
            reject_secret_references(&format!("job {job_name}"), Some(value))?;
        }
    }
    let mut uses_environment_secret = false;
    for step in steps(job)? {
        let step_name = scalar(step, "name").unwrap_or("unnamed step");
        for (key, value) in step {
            if string(key, "step key")? != "env" && !secret_references(value).is_empty() {
                return err(format!(
                    "job {job_name}, step {step_name}: secrets are allowed only in the step env block"
                ));
            }
        }
        let Some(env) = step.get(Value::String("env".into())) else {
            continue;
        };
        for secret in secret_references(env) {
            let owned = SECRET_OWNERS
                .iter()
                .any(|owner| owner.0 == secret && owner.1 == job_name && owner.2 == step_name);
            if !owned {
                return err(format!(
                    "secret {secret} is not owned by job {job_name}, step {step_name}"
                ));
            }
            uses_environment_secret |= secret != "GITHUB_TOKEN";
        }
    }
    if uses_environment_secret && scalar(job, "environment") != Some("release") {
        return err(format!(
            "job {job_name} must read its owned secrets from the release environment"
        ));
    }
    Ok(())
}

fn check_action_pins(job_name: &str, job: &Mapping) -> Res<()> {
    for step in steps(job)? {
        let Some(uses) = scalar(step, "uses") else {
            continue;
        };
        if uses.starts_with("./") {
            continue;
        }
        let Some((_, revision)) = uses.rsplit_once('@') else {
            return err(format!("job {job_name}: action {uses} has no revision"));
        };
        if revision.len() != 40
            || !revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return err(format!(
                "job {job_name}: action {uses} must be pinned to a full lowercase commit SHA"
            ));
        }
    }
    Ok(())
}

fn check_release_policy(jobs: &Mapping) -> Res<()> {
    for job_name in RELEASE_HEAVY_JOBS {
        let job = mapping(
            need(jobs, job_name, "release jobs")?,
            &format!("job {job_name}"),
        )?;
        let condition = scalar(job, "if").unwrap_or_default();
        if !condition.contains("github.event_name == 'push'")
            || !condition.contains("startsWith(github.ref, 'refs/tags/')")
        {
            return err(format!(
                "heavy job {job_name} must run only for a pushed tag"
            ));
        }
    }

    let cli = mapping(need(jobs, "cli", "release jobs")?, "cli job")?;
    check_cli_matrix(cli)?;
    for (name, runner, optional) in [
        ("desktop-macos", "macos-latest", false),
        ("desktop-windows", "windows-latest", false),
        ("desktop-windows-arm64", "windows-11-arm", true),
        ("desktop-linux", "ubuntu-22.04", false),
        ("desktop-linux-arm64", "ubuntu-24.04-arm", true),
    ] {
        let job = mapping(need(jobs, name, "release jobs")?, &format!("job {name}"))?;
        if scalar(job, "runs-on") != Some(runner)
            || boolean(job, "continue-on-error").unwrap_or(false) != optional
        {
            return err(format!(
                "release desktop target {name} drifted from the bounded RF2 matrix"
            ));
        }
    }
    Ok(())
}

fn check_cli_matrix(job: &Mapping) -> Res<()> {
    let strategy = mapping(need(job, "strategy", "cli job")?, "cli strategy")?;
    let matrix = mapping(need(strategy, "matrix", "cli strategy")?, "cli matrix")?;
    let include = need(matrix, "include", "cli matrix")?
        .as_sequence()
        .ok_or_else(|| Error::Msg("cli matrix include must be a sequence".into()))?;
    let actual: BTreeSet<(String, bool)> = include
        .iter()
        .map(|row| {
            let row = mapping(row, "cli matrix row")?;
            Ok((
                scalar(row, "target")
                    .ok_or_else(|| Error::Msg("cli matrix row has no target".into()))?
                    .to_string(),
                boolean(row, "optional").unwrap_or(false),
            ))
        })
        .collect::<Res<_>>()?;
    let expected = BTreeSet::from([
        ("aarch64-apple-darwin".into(), false),
        ("x86_64-apple-darwin".into(), false),
        ("x86_64-unknown-linux-gnu".into(), false),
        ("x86_64-unknown-linux-musl".into(), false),
        ("x86_64-pc-windows-msvc".into(), false),
        ("aarch64-unknown-linux-gnu".into(), true),
        ("aarch64-pc-windows-msvc".into(), true),
    ]);
    if actual != expected || include.len() != expected.len() {
        return err("cli targets drifted from the bounded RF2 matrix");
    }
    Ok(())
}

fn steps(job: &Mapping) -> Res<Vec<&Mapping>> {
    let steps = need(job, "steps", "job")?
        .as_sequence()
        .ok_or_else(|| Error::Msg("job steps must be a sequence".into()))?;
    steps
        .iter()
        .map(|step| mapping(step, "workflow step"))
        .collect()
}

fn reject_secret_references(scope: &str, value: Option<&Value>) -> Res<()> {
    if value.is_some_and(|value| !secret_references(value).is_empty()) {
        return err(format!(
            "{scope}: secrets must be scoped to their owning step"
        ));
    }
    Ok(())
}

fn secret_references(value: &Value) -> BTreeSet<String> {
    let mut references = BTreeSet::new();
    collect_secret_references(value, &mut references);
    references
}

fn collect_secret_references(value: &Value, references: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            let mut rest = text.as_str();
            while let Some(index) = rest.find("secrets.") {
                rest = &rest[index + "secrets.".len()..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                if end > 0 {
                    references.insert(rest[..end].to_string());
                }
                rest = &rest[end..];
            }
        }
        Value::Sequence(sequence) => {
            for item in sequence {
                collect_secret_references(item, references);
            }
        }
        Value::Mapping(mapping) => {
            for (key, value) in mapping {
                collect_secret_references(key, references);
                collect_secret_references(value, references);
            }
        }
        Value::Tagged(tagged) => collect_secret_references(&tagged.value, references),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn need<'a>(mapping: &'a Mapping, key: &str, scope: &str) -> Res<&'a Value> {
    mapping
        .get(Value::String(key.into()))
        .ok_or_else(|| Error::Msg(format!("{scope}: missing {key}")))
}

fn mapping<'a>(value: &'a Value, scope: &str) -> Res<&'a Mapping> {
    value
        .as_mapping()
        .ok_or_else(|| Error::Msg(format!("{scope} must be a mapping")))
}

fn string<'a>(value: &'a Value, scope: &str) -> Res<&'a str> {
    value
        .as_str()
        .ok_or_else(|| Error::Msg(format!("{scope} must be a string")))
}

fn scalar<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a str> {
    mapping
        .get(Value::String(key.into()))
        .and_then(Value::as_str)
}

fn boolean(mapping: &Mapping, key: &str) -> Option<bool> {
    mapping
        .get(Value::String(key.into()))
        .and_then(Value::as_bool)
}

fn scalar_sequence<'a>(mapping: &'a Mapping, key: &str) -> Res<Vec<&'a str>> {
    mapping
        .get(Value::String(key.into()))
        .and_then(Value::as_sequence)
        .ok_or_else(|| Error::Msg(format!("{key} must be a sequence")))?
        .iter()
        .map(|value| string(value, key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::check_text;
    use std::path::Path;

    fn fixture(name: &str) -> &'static str {
        match name {
            "valid" => include_str!("../tests/fixtures/workflows/valid.yml"),
            "broad-permissions" => {
                include_str!("../tests/fixtures/workflows/broad-permissions.yml")
            }
            "missing-timeout" => include_str!("../tests/fixtures/workflows/missing-timeout.yml"),
            "unpinned-action" => include_str!("../tests/fixtures/workflows/unpinned-action.yml"),
            "misplaced-secret" => include_str!("../tests/fixtures/workflows/misplaced-secret.yml"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn accepts_least_privilege_fixture() {
        assert!(check_text(Path::new("fixture.yml"), fixture("valid")).is_ok());
    }

    #[test]
    fn rejects_broad_permissions() {
        assert!(check_text(Path::new("fixture.yml"), fixture("broad-permissions")).is_err());
    }

    #[test]
    fn rejects_missing_timeout() {
        assert!(check_text(Path::new("fixture.yml"), fixture("missing-timeout")).is_err());
    }

    #[test]
    fn rejects_unpinned_action() {
        assert!(check_text(Path::new("fixture.yml"), fixture("unpinned-action")).is_err());
    }

    #[test]
    fn rejects_secret_in_non_owning_step() {
        assert!(check_text(Path::new("fixture.yml"), fixture("misplaced-secret")).is_err());
    }
}
