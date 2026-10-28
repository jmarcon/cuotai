//! Static policy checks for GitHub Actions workflows (PRD 0021 RF9.2/RF9.3/RF9.5).
//! The guard intentionally knows the repository workflows: changing a
//! permission, secret owner, scheduled monitor or release target requires an
//! explicit policy update instead of silently widening the pipeline.

use std::collections::BTreeSet;
use std::path::Path;

use serde_yaml::{Mapping, Value};

use crate::{err, Error, Res};

const RELEASE_JOBS: &[&str] = &[
    "check",
    "desktop-macos",
    "desktop-windows",
    "desktop-windows-arm64",
    "desktop-linux",
    "desktop-linux-arm64",
    "cli",
    "sbom",
    "assemble",
    "smoke-cli",
    "smoke-desktop-macos",
    "smoke-desktop-windows",
    "smoke-desktop-linux",
    "publish",
    "verify-public",
    "package-manifests",
    "channel-homebrew",
    "channel-winget",
    "channel-scoop",
    "channel-cargo",
    "channel-aur",
];

const RELEASE_HEAVY_JOBS: &[&str] = &[
    "desktop-macos",
    "desktop-windows",
    "desktop-windows-arm64",
    "desktop-linux",
    "desktop-linux-arm64",
    "cli",
    "sbom",
];

const RELEASE_DESKTOP_JOBS: &[&str] = &[
    "desktop-macos",
    "desktop-windows",
    "desktop-windows-arm64",
    "desktop-linux",
    "desktop-linux-arm64",
];

const CHANNEL_JOBS: &[&str] = &[
    "channel-homebrew",
    "channel-winget",
    "channel-scoop",
    "channel-cargo",
    "channel-aur",
];

/// PRD 0021 RF6: the produced-artifact smoke jobs, each with the script it
/// must run. `publish` waits for all of them.
const RELEASE_SMOKE_JOBS: &[(&str, &str)] = &[
    ("smoke-cli", "scripts/release/smoke-cli.sh"),
    (
        "smoke-desktop-macos",
        "scripts/release/smoke-desktop-macos.sh",
    ),
    (
        "smoke-desktop-windows",
        "scripts/release/smoke-desktop-windows.ps1",
    ),
    (
        "smoke-desktop-linux",
        "scripts/release/smoke-desktop-linux.sh",
    ),
];
const PUBLISH_CONDITION: &str =
    "github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')";

const HEAVY_JOB_CONDITION: &str = "(github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')) || (github.event_name == 'workflow_dispatch' && inputs.full_rehearsal)";
const RELEASE_OR_REHEARSAL_ENVIRONMENT: &str =
    "${{ github.event_name == 'push' && 'release' || 'rehearsal' }}";
const REHEARSAL_SECRET_JOBS: &[&str] = RELEASE_DESKTOP_JOBS;

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
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-macos",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-windows",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-windows",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-windows-arm64",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-windows-arm64",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-linux",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-linux",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY",
        "desktop-linux-arm64",
        "Validate required release credentials",
    ),
    (
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "desktop-linux-arm64",
        "Validate required release credentials",
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
    check_text(path, &text)?;
    // ci.yml warms the tauri-cli binary cache that release.yml restores, so
    // the two workflows must name the same version or the release legs would
    // silently compile it again on every tag.
    if path.file_name().and_then(|name| name.to_str()) == Some("ci.yml") {
        let release = path.with_file_name("release.yml");
        if release.is_file() {
            let release_text = std::fs::read_to_string(&release)
                .map_err(|e| Error::Msg(format!("cannot read {}: {e}", release.display())))?;
            check_tauri_cli_version(&text, &release_text)?;
        }
    }
    Ok(())
}

/// The workflow-level `env.TAURI_CLI_VERSION` of a workflow.
fn tauri_cli_version(text: &str) -> Res<String> {
    let root: Value = serde_yaml::from_str(text)
        .map_err(|e| Error::Msg(format!("invalid workflow YAML: {e}")))?;
    let root = mapping(&root, "workflow root")?;
    let env = mapping(need(root, "env", "workflow root")?, "workflow env")?;
    scalar(env, "TAURI_CLI_VERSION")
        .map(str::to_owned)
        .map_or_else(|| err("workflow env has no TAURI_CLI_VERSION"), Ok)
}

fn check_tauri_cli_version(ci_text: &str, release_text: &str) -> Res<()> {
    let ci = tauri_cli_version(ci_text)?;
    let release = tauri_cli_version(release_text)?;
    if ci != release {
        return err(format!(
            "ci.yml warms the tauri-cli {ci} cache but release.yml installs tauri-cli {release}; keep TAURI_CLI_VERSION equal in both"
        ));
    }
    Ok(())
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
        check_job_permissions(name, job_name, job)?;
        check_job_secrets(job_name, job)?;
        check_action_pins(job_name, job)?;
    }

    if name == "release.yml" {
        check_release_policy(jobs)?;
        check_smoke_policy(root, jobs)?;
    }
    if name == "ci.yml" {
        check_ci_result_policy(jobs)?;
        // PRD 0039 RF2.2: a push is gated from its previous tip through the
        // fail-closed `--before` resolution, never a hand-made fallback range.
        if !text.contains("changelog-gate --before \"$BEFORE\"") {
            return err(
                "ci.yml must run the commit gate on pushes with `changelog-gate --before \"$BEFORE\"`",
            );
        }
    }
    if name == "external-rules.yml" {
        check_external_rules_policy(jobs, text)?;
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

        let dispatch = mapping(
            need(on, "workflow_dispatch", "release triggers")?,
            "workflow_dispatch trigger",
        )?;
        let inputs = mapping(
            need(dispatch, "inputs", "workflow_dispatch trigger")?,
            "workflow_dispatch inputs",
        )?;
        let full_rehearsal = mapping(
            need(inputs, "full_rehearsal", "workflow_dispatch inputs")?,
            "full_rehearsal input",
        )?;
        if boolean(full_rehearsal, "default") != Some(false)
            || boolean(full_rehearsal, "required") != Some(false)
            || scalar(full_rehearsal, "type") != Some("boolean")
        {
            return err("full_rehearsal must be an optional boolean defaulting to false");
        }
    }
    if name == "ci.yml" {
        // PRD 0039 RF8.1: pull requests, pushes to main and manual dispatch;
        // never tags (release.yml owns them) nor other branches.
        let actual: BTreeSet<&str> = on
            .keys()
            .map(|key| string(key, "trigger"))
            .collect::<Res<_>>()?;
        let expected = BTreeSet::from(["pull_request", "push", "workflow_dispatch"]);
        if actual != expected {
            return err(
                "ci.yml must trigger exactly on pull_request, push (main) and workflow_dispatch",
            );
        }
        let push = mapping(need(on, "push", "ci triggers")?, "ci push trigger")?;
        if push.len() != 1 || scalar_sequence(push, "branches")? != ["main"] {
            return err("ci.yml push trigger must be limited to the main branch (no tags)");
        }
    }
    if name == "advisories.yml" {
        let actual: BTreeSet<&str> = on
            .keys()
            .map(|key| string(key, "trigger"))
            .collect::<Res<_>>()?;
        let expected = BTreeSet::from(["schedule", "workflow_dispatch"]);
        if actual != expected {
            return err("advisories.yml may trigger only weekly and by workflow_dispatch");
        }
        let schedule = need(on, "schedule", "advisories triggers")?
            .as_sequence()
            .ok_or_else(|| Error::Msg("advisories schedule must be a sequence".into()))?;
        if schedule.len() != 1 {
            return err("advisories.yml must have exactly one weekly schedule");
        }
        let schedule = mapping(&schedule[0], "advisories schedule")?;
        if scalar(schedule, "cron") != Some("41 6 * * 3") {
            return err("advisories.yml must run weekly on Wednesday");
        }
    }
    if name == "external-rules.yml" {
        let actual: BTreeSet<&str> = on
            .keys()
            .map(|key| string(key, "trigger"))
            .collect::<Res<_>>()?;
        let expected = BTreeSet::from(["schedule", "workflow_dispatch"]);
        if actual != expected {
            return err("external-rules.yml may trigger only monthly and by workflow_dispatch");
        }
        let schedule = need(on, "schedule", "external-rules triggers")?
            .as_sequence()
            .ok_or_else(|| Error::Msg("external-rules schedule must be a sequence".into()))?;
        if schedule.len() != 1 {
            return err("external-rules.yml must have exactly one monthly schedule");
        }
        let schedule = mapping(&schedule[0], "external-rules schedule")?;
        if scalar(schedule, "cron") != Some("17 8 1 * *") {
            return err("external-rules.yml must run monthly on the first day");
        }
    }
    Ok(())
}

/// PRD 0039 RF8.6: `CI result` is the one required check, so it must wait
/// for every other job of ci.yml and run unless the whole run was cancelled
/// (a superseded run must not post a red required check; a timed-out job still
/// fails it because the run itself is not cancelled).
fn check_ci_result_policy(jobs: &Mapping) -> Res<()> {
    let result = mapping(need(jobs, "ci-result", "ci jobs")?, "ci-result job")?;
    if scalar(result, "name") != Some("CI result")
        || scalar(result, "if") != Some("${{ !cancelled() }}")
    {
        return err(
            "ci.yml job ci-result must be named 'CI result' and run with if: ${{ !cancelled() }}",
        );
    }
    let needs: BTreeSet<&str> = scalar_sequence(result, "needs")?.into_iter().collect();
    for key in jobs.keys() {
        let job = string(key, "job id")?;
        if job != "ci-result" && !needs.contains(job) {
            return err(format!(
                "ci.yml job ci-result must wait for job {job} (it is the only required check)"
            ));
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

fn check_job_permissions(workflow_name: &str, job_name: &str, job: &Mapping) -> Res<()> {
    let expected: &[(&str, &str)] = match (workflow_name, job_name) {
        ("release.yml", "assemble") => &[
            ("contents", "read"),
            ("id-token", "write"),
            ("attestations", "write"),
        ],
        ("release.yml", "publish") => &[("contents", "write"), ("attestations", "read")],
        ("release.yml", "verify-public") => &[("contents", "read"), ("attestations", "read")],
        ("external-rules.yml", "issue") => &[("issues", "write")],
        // CodeQL advanced setup: the SARIF upload needs security-events: write.
        ("codeql.yml", "analyze" | "analyze-swift") => &[
            ("security-events", "write"),
            ("contents", "read"),
            ("actions", "read"),
        ],
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
    let environment = scalar(job, "environment");
    if CHANNEL_JOBS.contains(&job_name) {
        if environment != Some("channels") {
            return err(format!(
                "channel job {job_name} must use the channels environment"
            ));
        }
    } else if environment == Some("channels") {
        return err(format!(
            "non-channel job {job_name} must not use the channels environment"
        ));
    }
    if uses_environment_secret {
        let allowed = environment == Some("release")
            || (REHEARSAL_SECRET_JOBS.contains(&job_name)
                && environment == Some(RELEASE_OR_REHEARSAL_ENVIRONMENT))
            || (CHANNEL_JOBS.contains(&job_name) && environment == Some("channels"));
        if !allowed {
            return err(format!(
                "job {job_name} must read owned secrets from its checked release, channels or secretless-rehearsal environment"
            ));
        }
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
    let actual: BTreeSet<&str> = jobs
        .keys()
        .map(|key| string(key, "release job id"))
        .collect::<Res<_>>()?;
    let expected: BTreeSet<&str> = RELEASE_JOBS.iter().copied().collect();
    if actual != expected {
        return err("release jobs drifted from the bounded RF2/RF5 policy");
    }

    let homebrew = mapping(
        need(jobs, "channel-homebrew", "release jobs")?,
        "job channel-homebrew",
    )?;
    let audit_step = steps(homebrew)?
        .into_iter()
        .find(|step| {
            scalar(step, "name")
                .is_some_and(|name| name.starts_with("Audit and install test against"))
        })
        .map_or_else(|| err("channel-homebrew has no audit step"), Ok)?;
    let audit_env = audit_step
        .get(Value::String("env".into()))
        .and_then(Value::as_mapping);
    let token = audit_env.and_then(|env| scalar(env, "HOMEBREW_GITHUB_API_TOKEN"));
    if token != Some("${{ github.token }}") {
        return err(
            "channel-homebrew audit step must set HOMEBREW_GITHUB_API_TOKEN to github.token (brew audit --online hits the unauthenticated API rate limit)",
        );
    }

    for job_name in RELEASE_HEAVY_JOBS {
        let job = mapping(
            need(jobs, job_name, "release jobs")?,
            &format!("job {job_name}"),
        )?;
        if scalar(job, "needs") != Some("check") {
            return err(format!("heavy job {job_name} must wait only for check"));
        }
        if normalized_condition(job) != HEAVY_JOB_CONDITION {
            return err(format!(
                "heavy job {job_name} must run only for a pushed tag or explicit full rehearsal"
            ));
        }
    }

    for job_name in RELEASE_DESKTOP_JOBS {
        let job = mapping(
            need(jobs, job_name, "release jobs")?,
            &format!("job {job_name}"),
        )?;
        let first_step = steps(job)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::Msg(format!("desktop job {job_name} has no steps")))?;
        if scalar(first_step, "name") != Some("Validate required release credentials") {
            return err(format!(
                "desktop job {job_name} must validate release credentials before build work"
            ));
        }
        let validation_env = mapping(
            need(first_step, "env", "release credential validation step")?,
            "release credential validation environment",
        )?;
        for (name, expected) in [
            (
                "HAS_TAURI_SIGNING_PRIVATE_KEY",
                "${{ github.event_name == 'push' && secrets.TAURI_SIGNING_PRIVATE_KEY != '' }}",
            ),
            (
                "HAS_TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
                "${{ github.event_name == 'push' && secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD != '' }}",
            ),
        ] {
            if scalar(validation_env, name) != Some(expected) {
                return err(format!(
                    "desktop job {job_name} must inspect release credentials only during pushed tag runs"
                ));
            }
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
    check_checksum_signing(jobs)
}

/// PRD 0021 RF3.2: `assemble` signs `SHA256SUMS` as a Sigstore bundle and
/// verifies it in the same step, unconditionally, so a full rehearsal runs
/// the exact commands a tag run depends on (run 40119269466 first reached a
/// tag-only signing step on a real release and failed there).
fn check_checksum_signing(jobs: &Mapping) -> Res<()> {
    let assemble = mapping(need(jobs, "assemble", "release jobs")?, "job assemble")?;
    let steps = steps(assemble)?;
    let installer = steps.iter().find(|step| {
        scalar(step, "uses").is_some_and(|uses| uses.starts_with("sigstore/cosign-installer@"))
    });
    if installer.is_none_or(|step| step.contains_key(Value::String("if".into()))) {
        return err("assemble must install cosign unconditionally (rehearsals sign too)");
    }
    let signing: Vec<&&Mapping> = steps
        .iter()
        .filter(|step| scalar(step, "run").is_some_and(|run| run.contains("cosign sign-blob")))
        .collect();
    let [step] = signing.as_slice() else {
        return err("assemble must sign SHA256SUMS in exactly one step");
    };
    let run = scalar(step, "run").unwrap_or_default();
    // The signing command itself (one line), apart from the verification
    // that follows it, which also passes `--bundle`.
    let sign_command = run
        .lines()
        .find(|line| line.contains("cosign sign-blob"))
        .unwrap_or_default();
    if step.contains_key(Value::String("if".into())) {
        return err("the SHA256SUMS signing step must run in full rehearsals too (no if:)");
    }
    if !sign_command.contains("--bundle ")
        || sign_command.trim_end().ends_with('\\')
        || run.contains("--output-signature")
        || run.contains("--output-certificate")
    {
        return err(
            "SHA256SUMS must be signed as a Sigstore bundle (--bundle), not as detached .sig/.pem",
        );
    }
    if !run.contains("cosign verify-blob")
        || !run.contains("--certificate-identity ")
        || !run.contains("--certificate-oidc-issuer https://token.actions.githubusercontent.com")
    {
        return err(
            "the SHA256SUMS signing step must verify the bundle against the workflow identity",
        );
    }
    if !run.contains("refs/heads/*) ;;") {
        return err("a rehearsal must refuse to sign SHA256SUMS from a non-branch ref");
    }
    Ok(())
}

/// PRD 0021 RF6: the smoke jobs test the assembled artifacts before anything
/// is published, on every system, without secrets, and nothing can publish
/// around them.
fn check_smoke_policy(root: &Mapping, jobs: &Mapping) -> Res<()> {
    for (job_name, script) in RELEASE_SMOKE_JOBS {
        let job = mapping(
            need(jobs, job_name, "release jobs")?,
            &format!("job {job_name}"),
        )?;
        let needs: BTreeSet<&str> = scalar_sequence(job, "needs")?.into_iter().collect();
        if needs != BTreeSet::from(["check", "assemble"]) {
            return err(format!(
                "smoke job {job_name} must wait for exactly check and assemble"
            ));
        }
        // No condition and no tolerated failure: a smoke runs whenever the
        // assembly succeeded and its failure is a failed run.
        for forbidden in ["if", "continue-on-error", "environment"] {
            if job.contains_key(Value::String(forbidden.into())) {
                return err(format!(
                    "smoke job {job_name} must not set {forbidden}: it gates publishing"
                ));
            }
        }
        let runs_script = steps(job)?.iter().any(|step| {
            scalar(step, "run").is_some_and(|run| run.contains(script))
                && !step.contains_key(Value::String("continue-on-error".into()))
                && !step.contains_key(Value::String("if".into()))
        });
        if !runs_script {
            return err(format!(
                "smoke job {job_name} must run {script} unconditionally"
            ));
        }
    }

    let publish = mapping(need(jobs, "publish", "release jobs")?, "publish job")?;
    let needs: BTreeSet<&str> = scalar_sequence(publish, "needs")?.into_iter().collect();
    for (job_name, _) in RELEASE_SMOKE_JOBS {
        if !needs.contains(job_name) {
            return err(format!(
                "publish must wait for smoke job {job_name} (PRD 0021 RF6)"
            ));
        }
    }
    // The implicit success() of this exact condition is what makes a failed
    // or skipped smoke skip publishing; always()/failure() would bypass it.
    if normalized_condition(publish) != PUBLISH_CONDITION {
        return err(
            "publish must run only on pushed tags and only after every needed job succeeded",
        );
    }

    // Every CLI target the release builds is smoked on a runner of its own.
    let cli_targets = matrix_targets(mapping(need(jobs, "cli", "release jobs")?, "cli job")?)?;
    let smoke = mapping(need(jobs, "smoke-cli", "release jobs")?, "smoke-cli job")?;
    if matrix_targets(smoke)? != cli_targets {
        return err("smoke-cli must cover exactly the targets of the cli matrix");
    }

    // Smoke containers are pinned by digest, like the actions.
    if let Some(env) = root.get(Value::String("env".into())) {
        for (key, value) in mapping(env, "workflow env")? {
            let key = string(key, "workflow env name")?;
            if !(key.starts_with("SMOKE_") && key.ends_with("_IMAGE")) {
                continue;
            }
            let image = value.as_str().unwrap_or_default();
            let pinned = image.split_once("@sha256:").is_some_and(|(name, digest)| {
                !name.is_empty()
                    && digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            });
            if !pinned {
                return err(format!(
                    "{key} must pin its container image by sha256 digest"
                ));
            }
        }
    }
    Ok(())
}

fn matrix_targets(job: &Mapping) -> Res<BTreeSet<String>> {
    let strategy = mapping(need(job, "strategy", "job")?, "strategy")?;
    let matrix = mapping(need(strategy, "matrix", "strategy")?, "matrix")?;
    need(matrix, "include", "matrix")?
        .as_sequence()
        .ok_or_else(|| Error::Msg("matrix include must be a sequence".into()))?
        .iter()
        .map(|row| {
            scalar(mapping(row, "matrix row")?, "target")
                .map(str::to_string)
                .ok_or_else(|| Error::Msg("matrix row has no target".into()))
        })
        .collect()
}

fn check_external_rules_policy(jobs: &Mapping, text: &str) -> Res<()> {
    let actual: BTreeSet<&str> = jobs
        .keys()
        .map(|key| string(key, "external-rules job id"))
        .collect::<Res<_>>()?;
    if actual != BTreeSet::from(["check", "issue"]) {
        return err("external-rules.yml jobs drifted from the checked RF9.3 design");
    }
    let issue = mapping(need(jobs, "issue", "external-rules jobs")?, "issue job")?;
    if scalar(issue, "needs") != Some("check")
        || normalized_condition(issue) != "needs.check.outputs.needs_attention == 'true'"
    {
        return err("the issue job must run only when the external check needs attention");
    }
    for required in [
        "cargo run -p xtask --locked -- external-rules-check",
        "state=open",
        "--paginate --slurp",
        "External rules or action pins need review",
        "repos/$REPOSITORY/issues/$existing",
    ] {
        if !text.contains(required) {
            return err(format!(
                "external-rules.yml is missing checked behavior: {required}"
            ));
        }
    }
    Ok(())
}

fn normalized_condition(job: &Mapping) -> &str {
    let condition = scalar(job, "if").unwrap_or_default().trim();
    condition
        .strip_prefix("${{")
        .and_then(|condition| condition.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(condition)
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
        Value::String(text) => collect_secret_references_in_text(text, references),
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

fn collect_secret_references_in_text(text: &str, references: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find("secrets") {
        let secret_start = search_from + offset;
        let identifier_end = secret_start + "secrets".len();
        let is_identifier_byte = |byte: &u8| byte.is_ascii_alphanumeric() || *byte == b'_';
        if bytes
            .get(secret_start.wrapping_sub(1))
            .is_some_and(is_identifier_byte)
            || bytes.get(identifier_end).is_some_and(is_identifier_byte)
        {
            search_from = identifier_end;
            continue;
        }

        let mut index = identifier_end;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let inside_expression = text[..secret_start]
            .rfind("${{")
            .is_some_and(|open| !text[open + 3..secret_start].contains("}}"))
            && text[index..].contains("}}");

        let name = match bytes.get(index) {
            Some(b'.') => {
                index += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                    index += 1;
                }
                let start = index;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    index += 1;
                }
                (index > start)
                    .then_some(&text[start..index])
                    .or(Some("<dynamic-secret-reference>"))
            }
            Some(b'[') => {
                index += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                    index += 1;
                }
                let Some(quote @ (b'\'' | b'"')) = bytes.get(index).copied() else {
                    references.insert("<dynamic-secret-reference>".to_string());
                    search_from = index;
                    continue;
                };
                let start = index + 1;
                index = start;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    index += 1;
                }
                if index == start || bytes.get(index) != Some(&quote) {
                    references.insert("<dynamic-secret-reference>".to_string());
                    search_from = index;
                    continue;
                }
                let end = index;
                index += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                    index += 1;
                }
                if bytes.get(index) != Some(&b']') {
                    references.insert("<dynamic-secret-reference>".to_string());
                    search_from = index;
                    continue;
                }
                Some(&text[start..end])
            }
            _ if inside_expression => Some("<dynamic-secret-reference>"),
            _ => None,
        };
        if let Some(name) = name {
            references.insert(name.to_string());
        }
        search_from = index.max(search_from + 1);
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
            "bracket-secret" => include_str!("../tests/fixtures/workflows/bracket-secret.yml"),
            "broad-permissions" => {
                include_str!("../tests/fixtures/workflows/broad-permissions.yml")
            }
            "missing-timeout" => include_str!("../tests/fixtures/workflows/missing-timeout.yml"),
            "unpinned-action" => include_str!("../tests/fixtures/workflows/unpinned-action.yml"),
            "misplaced-secret" => include_str!("../tests/fixtures/workflows/misplaced-secret.yml"),
            _ => unreachable!(),
        }
    }

    const CI_WORKFLOW: &str = include_str!("../../.github/workflows/ci.yml");
    const RELEASE_WORKFLOW: &str = include_str!("../../.github/workflows/release.yml");
    const EXTERNAL_RULES_WORKFLOW: &str =
        include_str!("../../.github/workflows/external-rules.yml");
    const CODEQL_WORKFLOW: &str = include_str!("../../.github/workflows/codeql.yml");
    const ADVISORIES_WORKFLOW: &str = include_str!("../../.github/workflows/advisories.yml");

    #[test]
    fn accepts_least_privilege_fixture() {
        assert!(check_text(Path::new("fixture.yml"), fixture("valid")).is_ok());
    }

    #[test]
    fn accepts_repository_workflows() {
        assert!(check_text(Path::new("ci.yml"), CI_WORKFLOW).is_ok());
        assert!(check_text(Path::new("release.yml"), RELEASE_WORKFLOW).is_ok());
        assert!(check_text(Path::new("advisories.yml"), ADVISORIES_WORKFLOW).is_ok());
        assert!(check_text(Path::new("external-rules.yml"), EXTERNAL_RULES_WORKFLOW).is_ok());
        assert!(check_text(Path::new("codeql.yml"), CODEQL_WORKFLOW).is_ok());
    }

    #[test]
    fn ci_and_release_install_the_same_tauri_cli() {
        assert!(super::check_tauri_cli_version(CI_WORKFLOW, RELEASE_WORKFLOW).is_ok());
        let drifted =
            RELEASE_WORKFLOW.replacen("TAURI_CLI_VERSION: \"", "TAURI_CLI_VERSION: \"9", 1);
        assert_ne!(drifted, RELEASE_WORKFLOW);
        assert!(super::check_tauri_cli_version(CI_WORKFLOW, &drifted).is_err());
        let missing = CI_WORKFLOW.replacen("TAURI_CLI_VERSION:", "UNRELATED_VERSION:", 1);
        assert!(super::check_tauri_cli_version(&missing, RELEASE_WORKFLOW).is_err());
    }

    #[test]
    fn rejects_homebrew_audit_without_api_token() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "HOMEBREW_GITHUB_API_TOKEN: ${{ github.token }}",
            "HOMEBREW_UNUSED: \"1\"",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_ci_result_that_runs_on_cancelled_or_never_runs() {
        for replacement in ["if: always()", "if: success()"] {
            let unsafe_workflow = CI_WORKFLOW.replacen("if: ${{ !cancelled() }}", replacement, 1);
            assert_ne!(unsafe_workflow, CI_WORKFLOW);
            assert!(check_text(Path::new("ci.yml"), &unsafe_workflow).is_err());
        }
    }

    #[test]
    fn rejects_codeql_broad_permissions() {
        let unsafe_workflow = CODEQL_WORKFLOW.replacen(
            "      security-events: write\n",
            "      security-events: write\n      contents: write\n",
            1,
        );
        assert!(check_text(Path::new("codeql.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_external_issue_on_a_clean_result() {
        let unsafe_workflow = EXTERNAL_RULES_WORKFLOW.replacen(
            "    if: needs.check.outputs.needs_attention == 'true'\n",
            "    if: always()\n",
            1,
        );
        assert!(check_text(Path::new("external-rules.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_external_issue_lookup_without_pagination() {
        let unsafe_workflow = EXTERNAL_RULES_WORKFLOW.replacen("--paginate --slurp ", "", 1);
        assert!(check_text(Path::new("external-rules.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_external_issue_broad_permissions() {
        let unsafe_workflow = EXTERNAL_RULES_WORKFLOW.replacen(
            "    permissions:\n      issues: write\n",
            "    permissions:\n      contents: write\n      issues: write\n",
            1,
        );
        assert!(check_text(Path::new("external-rules.yml"), &unsafe_workflow).is_err());
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

    #[test]
    fn rejects_bracket_secret_outside_an_owning_step() {
        assert!(check_text(Path::new("fixture.yml"), fixture("bracket-secret")).is_err());
    }

    #[test]
    fn rejects_pull_request_target() {
        let unsafe_workflow =
            CI_WORKFLOW.replacen("  pull_request:\n", "  pull_request_target:\n", 1);
        assert!(check_text(Path::new("ci.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn ci_runs_on_main_pushes_only_and_aggregates_every_job() {
        let tags = CI_WORKFLOW.replacen(
            "  push:\n    branches: [main]\n",
            "  push:\n    branches: [main]\n    tags: [\"v*\"]\n",
            1,
        );
        assert!(check_text(Path::new("ci.yml"), &tags).is_err());
        let every_branch = CI_WORKFLOW.replacen("  push:\n    branches: [main]\n", "  push:\n", 1);
        assert!(check_text(Path::new("ci.yml"), &every_branch).is_err());
        let open_gate = CI_WORKFLOW.replacen(
            "changelog-gate --before \"$BEFORE\" --head \"$AFTER\"",
            "changelog-gate --base \"$AFTER\" --head \"$AFTER\"",
            1,
        );
        assert!(check_text(Path::new("ci.yml"), &open_gate)
            .unwrap_err()
            .to_string()
            .contains("--before"));
        let unaggregated = CI_WORKFLOW.replacen(", release-preflight]", "]", 1);
        assert!(check_text(Path::new("ci.yml"), &unaggregated)
            .unwrap_err()
            .to_string()
            .contains("release-preflight"));
    }

    #[test]
    fn rejects_secret_job_without_release_environment() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "    environment: ${{ github.event_name == 'push' && 'release' || 'rehearsal' }}\n",
            "    environment: rehearsal\n",
            1,
        );
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_non_conjunctive_heavy_job_gate() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "  desktop-macos:\n    name: desktop (macOS universal)\n    needs: check\n    if: (github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')) || (github.event_name == 'workflow_dispatch' && inputs.full_rehearsal)",
            "  desktop-macos:\n    name: desktop (macOS universal)\n    needs: check\n    if: github.event_name == 'push' || !startsWith(github.ref, 'refs/tags/')",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_unbounded_cli_matrix() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "          - { os: windows-11-arm, target: aarch64-pc-windows-msvc, archive: zip, optional: true }",
            "          - { os: windows-11-arm, target: aarch64-pc-windows-msvc, archive: zip, optional: true }\n          - { os: ubuntu-latest, target: powerpc64le-unknown-linux-gnu, archive: tar, optional: true }",
            1,
        );
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_unchecked_release_job() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "jobs:\n",
            "jobs:\n  extra-heavy:\n    runs-on: ubuntu-latest\n    timeout-minutes: 60\n    steps:\n      - run: cargo build --workspace\n",
            1,
        );
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_heavy_job_with_an_extra_gate() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "  desktop-macos:\n    name: desktop (macOS universal)\n    needs: check\n",
            "  desktop-macos:\n    name: desktop (macOS universal)\n    needs: [check, assemble]\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_non_channel_job_in_channels_environment() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "    environment: ${{ github.event_name == 'push' && 'release' || 'rehearsal' }}\n",
            "    environment: channels\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_channel_job_in_release_environment() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "    environment: channels\n",
            "    environment: release\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn accepts_owned_named_secret_syntaxes() {
        let bracketed_workflow = RELEASE_WORKFLOW.replacen(
            "${{ secrets.HOMEBREW_TAP_TOKEN }}",
            "${{ secrets['HOMEBREW_TAP_TOKEN'] }}",
            1,
        );
        assert!(check_text(Path::new("release.yml"), &bracketed_workflow).is_ok());
    }

    #[test]
    fn rejects_dynamic_secret_reference() {
        let unsafe_workflow = fixture("valid").replacen(
            "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n",
            "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n        env:\n          TOKEN: ${{ secrets[env.SECRET_NAME] }}\n",
            1,
        );
        assert!(check_text(Path::new("fixture.yml"), &unsafe_workflow).is_err());
    }
    #[test]
    fn rejects_bare_secret_context() {
        let unsafe_workflow = fixture("valid").replacen(
            "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n",
            "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n        env:\n          TOKENS: ${{ toJSON(secrets) }}\n",
            1,
        );
        assert!(check_text(Path::new("fixture.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_unguarded_release_credential_lookup() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "${{ github.event_name == 'push' && secrets.TAURI_SIGNING_PRIVATE_KEY != '' }}",
            "${{ secrets.TAURI_SIGNING_PRIVATE_KEY != '' }}",
            1,
        );
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_desktop_build_that_validates_credentials_late() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "    steps:\n      - name: Validate required release credentials\n",
            "    steps:\n      - run: echo build work first\n      - name: Validate required release credentials\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_advisories_trigger_drift() {
        let wrong_schedule = ADVISORIES_WORKFLOW.replacen("41 6 * * 3", "41 6 * * 4", 1);
        assert_ne!(wrong_schedule, ADVISORIES_WORKFLOW);
        assert!(check_text(Path::new("advisories.yml"), &wrong_schedule).is_err());

        let extra_trigger = ADVISORIES_WORKFLOW.replacen(
            "  workflow_dispatch:\n",
            "  workflow_dispatch:\n  push:\n",
            1,
        );
        assert_ne!(extra_trigger, ADVISORIES_WORKFLOW);
        assert!(check_text(Path::new("advisories.yml"), &extra_trigger).is_err());
    }

    #[test]
    fn rejects_publish_that_does_not_wait_for_every_smoke() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "    needs: [check, assemble, smoke-cli, smoke-desktop-macos, smoke-desktop-windows, smoke-desktop-linux]\n",
            "    needs: [check, assemble, smoke-cli, smoke-desktop-macos, smoke-desktop-windows]\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow)
            .unwrap_err()
            .to_string()
            .contains("smoke-desktop-linux"));
    }

    #[test]
    fn rejects_publish_that_runs_after_a_failed_smoke() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')\n    runs-on: ubuntu-latest\n    timeout-minutes: 30\n    environment: release\n",
            "    if: always() && github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')\n    runs-on: ubuntu-latest\n    timeout-minutes: 30\n    environment: release\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }

    #[test]
    fn rejects_a_tolerated_or_conditional_smoke() {
        for (from, to) in [
            (
                "  smoke-desktop-macos:\n    name: smoke desktop (macOS)\n",
                "  smoke-desktop-macos:\n    name: smoke desktop (macOS)\n    continue-on-error: true\n",
            ),
            (
                "  smoke-desktop-linux:\n    name: smoke desktop (Linux x64)\n",
                "  smoke-desktop-linux:\n    name: smoke desktop (Linux x64)\n    if: github.event_name == 'push'\n",
            ),
            (
                "      - name: Smoke the installers (RF6.2)\n",
                "      - name: Smoke the installers (RF6.2)\n        continue-on-error: true\n",
            ),
        ] {
            let unsafe_workflow = RELEASE_WORKFLOW.replacen(from, to, 1);
            assert_ne!(unsafe_workflow, RELEASE_WORKFLOW, "{from}");
            assert!(
                check_text(Path::new("release.yml"), &unsafe_workflow).is_err(),
                "accepted {to}"
            );
        }
    }

    #[test]
    fn rejects_a_cli_target_without_smoke() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "          - { os: ubuntu-latest, target: x86_64-unknown-linux-musl, archive: tar.gz, container: alpine }\n",
            "",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow)
            .unwrap_err()
            .to_string()
            .contains("smoke-cli"));
    }

    #[test]
    fn rejects_a_smoke_image_without_digest() {
        let unsafe_workflow = RELEASE_WORKFLOW.replacen(
            "  SMOKE_ALPINE_IMAGE: \"alpine:3.23@sha256:ff7f0c727dd796aa36787bfcd9a64bf35a6ce72a73451b3359fde849b6703d63\"\n",
            "  SMOKE_ALPINE_IMAGE: \"alpine:3.23\"\n",
            1,
        );
        assert_ne!(unsafe_workflow, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow)
            .unwrap_err()
            .to_string()
            .contains("SMOKE_ALPINE_IMAGE"));
    }

    #[test]
    fn rejects_checksum_signing_skipped_in_rehearsals_or_detached() {
        let signing_step = "      - name: Sign and verify SHA256SUMS (Sigstore keyless)\n";
        let gated = RELEASE_WORKFLOW.replacen(
            signing_step,
            &format!("{signing_step}        if: needs.check.outputs.dry_run != 'true'\n"),
            1,
        );
        assert_ne!(gated, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &gated).is_err());

        let installer = "      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6 # v4.1.2\n";
        let gated_installer = RELEASE_WORKFLOW.replacen(
            &format!("{installer}      - name: Sign and verify"),
            &format!("{installer}        if: needs.check.outputs.dry_run != 'true'\n      - name: Sign and verify"),
            1,
        );
        assert_ne!(gated_installer, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &gated_installer).is_err());

        let detached = RELEASE_WORKFLOW.replacen(
            "cosign sign-blob --yes --bundle \"$bundle\" dist/SHA256SUMS",
            "cosign sign-blob --yes --output-signature dist/SHA256SUMS.sig dist/SHA256SUMS",
            1,
        );
        assert_ne!(detached, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &detached).is_err());

        // Only the signing command's own `--bundle` counts, not the verifier's.
        let unbundled = RELEASE_WORKFLOW.replacen(
            "cosign sign-blob --yes --bundle \"$bundle\" dist/SHA256SUMS",
            "cosign sign-blob --yes dist/SHA256SUMS",
            1,
        );
        assert_ne!(unbundled, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unbundled).is_err());

        let any_ref = RELEASE_WORKFLOW.replacen("              refs/heads/*) ;;\n", "", 1);
        assert_ne!(any_ref, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &any_ref).is_err());

        let unverified = RELEASE_WORKFLOW.replacen(
            "          cosign verify-blob \\\n",
            "          true \\\n",
            1,
        );
        assert_ne!(unverified, RELEASE_WORKFLOW);
        assert!(check_text(Path::new("release.yml"), &unverified).is_err());
    }

    #[test]
    fn rejects_full_rehearsal_enabled_by_default() {
        let unsafe_workflow =
            RELEASE_WORKFLOW.replacen("        default: false\n", "        default: true\n", 1);
        assert!(check_text(Path::new("release.yml"), &unsafe_workflow).is_err());
    }
}
