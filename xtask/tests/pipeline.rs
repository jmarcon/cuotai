//! Behavior-level tests for the assembly side (PRD 0021 RF2–RF4): artifact
//! manifest + checksums, download verification, signing-state merge,
//! release-notes rendering and the updater manifest.

use std::path::{Path, PathBuf};

use xtask::manifest::{self, Manifest};
use xtask::signing::{
    self, ArtifactSigning, ChecksumSigning, ChecksumSigningMethod, Fragment, SigningMethod,
    SigningStatus, UpdaterState,
};
use xtask::{Channel, Version};

const VERSION: &str = "0.68.0";

/// The required stable names for the fixture version.
fn required_names() -> Vec<String> {
    let v = VERSION;
    [
        format!("Cuotai_{v}_universal.dmg"),
        format!("Cuotai_{v}_x64-setup.exe"),
        format!("Cuotai_{v}_x64_en-US.msi"),
        format!("Cuotai_{v}_amd64.AppImage"),
        format!("Cuotai_{v}_amd64.deb"),
        format!("Cuotai_{v}_amd64.rpm"),
        format!("cuotai-{v}-aarch64-apple-darwin.tar.gz"),
        format!("cuotai-{v}-x86_64-apple-darwin.tar.gz"),
        format!("cuotai-{v}-x86_64-unknown-linux-gnu.tar.gz"),
        format!("cuotai-{v}-x86_64-unknown-linux-musl.tar.gz"),
        format!("cuotai-{v}-x86_64-pc-windows-msvc.zip"),
        "sbom.cdx.json".into(),
    ]
    .into_iter()
    .collect()
}

fn write_dist(dir: &Path, names: &[String]) {
    for name in names {
        std::fs::write(dir.join(name), format!("content of {name}")).unwrap();
    }
}

fn dist_with_required() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    write_dist(dir.path(), &required_names());
    let path = dir.path().to_path_buf();
    (dir, path)
}

#[test]
fn manifest_classifies_and_hashes_all_required() {
    let (_d, dir) = dist_with_required();
    let (manifest, sums) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.artifacts.len(), required_names().len());
    // SHA256SUMS is sha256sum-compatible and sorted by name.
    let mut lines: Vec<&str> = sums.lines().collect();
    assert_eq!(lines.len(), manifest.artifacts.len());
    let mut sorted = lines.clone();
    sorted.sort_by_key(|l| l.split("  ").nth(1).unwrap().to_string());
    lines.sort();
    let dmg = manifest
        .artifacts
        .iter()
        .find(|a| a.name == "Cuotai_0.68.0_universal.dmg")
        .unwrap();
    assert_eq!(dmg.kind, "desktop-dmg");
    assert_eq!(dmg.platform, "macos");
    assert_eq!(dmg.sha256.len(), 64);
    assert!(sums.contains(&format!("{}  Cuotai_0.68.0_universal.dmg", dmg.sha256)));
    // Optional artifacts are reported, not silently dropped.
    assert!(manifest
        .missing_optional
        .contains(&"Cuotai_0.68.0_arm64-setup.exe".to_string()));
    assert!(manifest
        .missing_optional
        .contains(&"cuotai-0.68.0-aarch64-unknown-linux-gnu.tar.gz".to_string()));
}

#[test]
fn manifest_fails_on_missing_required() {
    let (_d, dir) = dist_with_required();
    std::fs::remove_file(dir.join("Cuotai_0.68.0_universal.dmg")).unwrap();
    let err = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap_err();
    assert!(
        err.to_string().contains("Cuotai_0.68.0_universal.dmg"),
        "{err}"
    );
}

#[test]
fn manifest_fails_on_unknown_file() {
    let (_d, dir) = dist_with_required();
    std::fs::write(dir.join("random.txt"), "x").unwrap();
    let err = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap_err();
    assert!(err.to_string().contains("unknown file"), "{err}");
}

#[test]
fn manifest_regeneration_is_idempotent() {
    let (_d, dir) = dist_with_required();
    let (first, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&first).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("SHA256SUMS"), "stale\n").unwrap();
    let (second, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    assert_eq!(first, second);
}

#[test]
fn verify_download_roundtrip_and_corruption() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    assert_eq!(
        manifest::verify_download(&dir, &manifest).unwrap(),
        manifest.artifacts.len()
    );
    std::fs::write(dir.join("sbom.cdx.json"), "tampered").unwrap();
    let err = manifest::verify_download(&dir, &manifest).unwrap_err();
    assert!(err.to_string().contains("sbom.cdx.json"), "{err}");
}

#[test]
fn verify_download_fails_on_empty_asset() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let name = &manifest.artifacts[0].name;
    let content = std::fs::read(dir.join(name)).unwrap();
    // Same length as before so only the zero-size rule can trip: truncate.
    drop(content);
    std::fs::write(dir.join(name), "").unwrap();
    let err = manifest::verify_download(&dir, &manifest).unwrap_err();
    assert!(err.to_string().contains("empty"), "{err}");
}

#[test]
fn manifest_load_rejects_unsafe_duplicate_and_unknown_names() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let path = dir.join("candidate.json");

    for bad_name in [
        "../outside-artifact",
        "/absolute-artifact",
        "nested/artifact",
        r"nested\artifact",
        "unknown-artifact",
    ] {
        let mut candidate = manifest.clone();
        candidate.artifacts[0].name = bad_name.into();
        std::fs::write(&path, serde_json::to_vec(&candidate).unwrap()).unwrap();
        assert!(
            manifest::load(&path).is_err(),
            "accepted unsafe or unknown name {bad_name:?}"
        );
    }

    let mut duplicate = manifest.clone();
    duplicate.artifacts[1] = duplicate.artifacts[0].clone();
    std::fs::write(&path, serde_json::to_vec(&duplicate).unwrap()).unwrap();
    let err = manifest::load(&path).unwrap_err();
    assert!(err.to_string().contains("duplicate"), "{err}");
}

#[test]
fn verify_download_rejects_unknown_assets() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    std::fs::write(dir.join("unexpected.bin"), "unexpected").unwrap();
    let err = manifest::verify_download(&dir, &manifest).unwrap_err();
    assert!(err.to_string().contains("unknown release asset"), "{err}");
}

#[cfg(unix)]
#[test]
fn verify_download_rejects_symlinked_assets() {
    use std::os::unix::fs::symlink;

    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let name = &manifest.artifacts[0].name;
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("artifact");
    std::fs::write(&outside, std::fs::read(dir.join(name)).unwrap()).unwrap();
    std::fs::remove_file(dir.join(name)).unwrap();
    symlink(&outside, dir.join(name)).unwrap();
    let err = manifest::verify_download(&dir, &manifest).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
}

fn unsigned_fragment(manifest: &Manifest) -> Fragment {
    Fragment {
        artifacts: manifest
            .artifacts
            .iter()
            .map(|a| ArtifactSigning {
                name: a.name.clone(),
                signed: false,
                method: if a.platform == "linux" || a.kind == "cli-archive" || a.kind == "sbom" {
                    SigningMethod::ChecksumSignature
                } else {
                    SigningMethod::None
                },
                status: SigningStatus::Unsigned,
                note: None,
            })
            .collect(),
        updater: None,
    }
}

#[test]
fn signing_state_merges_and_resolves_checksum_trust() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();

    // With a signed SHA256SUMS, checksum-signature artifacts become signed.
    let state = signing::merge(
        VERSION,
        Channel::Stable,
        vec![unsigned_fragment(&manifest)],
        &manifest,
        ChecksumSigning {
            signed: true,
            method: ChecksumSigningMethod::SigstoreKeyless,
            status: "signed".into(),
        },
    )
    .unwrap();
    let cli = state
        .artifacts
        .iter()
        .find(|a| a.name == "cuotai-0.68.0-x86_64-unknown-linux-gnu.tar.gz")
        .unwrap();
    assert!(cli.signed);
    let dmg = state
        .artifacts
        .iter()
        .find(|a| a.name == "Cuotai_0.68.0_universal.dmg")
        .unwrap();
    assert!(!dmg.signed);
    assert!(!state.updater.available);

    // Without it, they are honestly unsigned.
    let state = signing::merge(
        VERSION,
        Channel::Stable,
        vec![unsigned_fragment(&manifest)],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "dry-run: not signed".into(),
        },
    )
    .unwrap();
    let cli = state
        .artifacts
        .iter()
        .find(|a| a.name == "cuotai-0.68.0-x86_64-unknown-linux-gnu.tar.gz")
        .unwrap();
    assert!(!cli.signed);
    assert_eq!(cli.status, SigningStatus::Unsigned);
    assert!(cli.note.as_deref().unwrap().contains("dry-run"));
}

#[test]
fn signing_state_requires_full_coverage() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let mut fragment = unsigned_fragment(&manifest);
    fragment.artifacts.pop();
    let err = signing::merge(
        VERSION,
        Channel::Stable,
        vec![fragment],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no signing state"), "{err}");
}

#[test]
fn signing_state_requires_manifest_channel_match() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let err = signing::merge(
        VERSION,
        Channel::Prerelease,
        vec![unsigned_fragment(&manifest)],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("channel"), "{err}");
}

#[test]
fn signing_state_rejects_dishonest_signed_flag() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let mut fragment = unsigned_fragment(&manifest);
    // The DMG uses method "none" here; claiming it signed must fail loudly.
    fragment
        .artifacts
        .iter_mut()
        .find(|a| a.name == "Cuotai_0.68.0_universal.dmg")
        .unwrap()
        .status = SigningStatus::Signed;
    let err = signing::merge(
        VERSION,
        Channel::Stable,
        vec![fragment],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("contradictory"), "{err}");
}

#[test]
fn signing_state_rejects_every_method_flag_status_inversion() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let invalid = [
        (true, SigningMethod::None, SigningStatus::Signed),
        (true, SigningMethod::None, SigningStatus::Unsigned),
        (false, SigningMethod::DeveloperId, SigningStatus::Unsigned),
        (true, SigningMethod::AdHoc, SigningStatus::Signed),
        (false, SigningMethod::Authenticode, SigningStatus::Unsigned),
        (
            false,
            SigningMethod::TauriUpdaterKey,
            SigningStatus::Unavailable,
        ),
    ];
    for (signed, method, status) in invalid {
        let mut fragment = unsigned_fragment(&manifest);
        let entry = fragment
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.name == "Cuotai_0.68.0_universal.dmg")
            .unwrap();
        entry.signed = signed;
        entry.method = method;
        entry.status = status;
        let result = signing::merge(
            VERSION,
            Channel::Stable,
            vec![fragment],
            &manifest,
            ChecksumSigning {
                signed: false,
                method: ChecksumSigningMethod::None,
                status: "unsigned".into(),
            },
        );
        assert!(
            result.is_err(),
            "accepted signed={signed}, method={method}, status={status}"
        );
    }
}

#[test]
fn signing_state_rejects_checksum_availability_contradictions() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    for checksums in [
        ChecksumSigning {
            signed: true,
            method: ChecksumSigningMethod::None,
            status: "signed".into(),
        },
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::SigstoreKeyless,
            status: "unavailable".into(),
        },
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "signed".into(),
        },
        ChecksumSigning {
            signed: true,
            method: ChecksumSigningMethod::SigstoreKeyless,
            status: "unavailable".into(),
        },
    ] {
        assert!(signing::merge(
            VERSION,
            Channel::Stable,
            vec![unsigned_fragment(&manifest)],
            &manifest,
            checksums,
        )
        .is_err());
    }
}

#[test]
fn signing_state_load_revalidates_invariants() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let state = signing::merge(
        VERSION,
        Channel::Stable,
        vec![unsigned_fragment(&manifest)],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .unwrap();
    let mut json = serde_json::to_value(state).unwrap();
    json["artifacts"][0]["signed"] = true.into();
    json["artifacts"][0]["method"] = "none".into();
    json["artifacts"][0]["status"] = "signed".into();
    let path = dir.join("signing-state.json");
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let err = signing::load(&path).unwrap_err();
    assert!(err.to_string().contains("contradictory"), "{err}");
}

#[test]
fn signing_state_validates_updater_artifacts_exist() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let mut fragment = unsigned_fragment(&manifest);
    fragment.updater = Some(UpdaterState {
        available: true,
        artifacts: vec!["Cuotai.app.tar.gz".into(), "Cuotai.app.tar.gz.sig".into()],
        reason: None,
    });
    let err = signing::merge(
        VERSION,
        Channel::Stable,
        vec![fragment],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not in the manifest"), "{err}");
}

#[test]
fn signing_state_rejects_updater_availability_contradictions() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let invalid = [
        UpdaterState {
            available: false,
            artifacts: vec!["Cuotai.app.tar.gz".into()],
            reason: Some("not configured".into()),
        },
        UpdaterState {
            available: false,
            artifacts: Vec::new(),
            reason: None,
        },
        UpdaterState {
            available: true,
            artifacts: vec!["Cuotai.app.tar.gz".into(), "Cuotai.app.tar.gz.sig".into()],
            reason: Some("contradiction".into()),
        },
    ];
    for updater in invalid {
        let mut fragment = unsigned_fragment(&manifest);
        fragment.updater = Some(updater);
        assert!(signing::merge(
            VERSION,
            Channel::Stable,
            vec![fragment],
            &manifest,
            ChecksumSigning {
                signed: false,
                method: ChecksumSigningMethod::None,
                status: "unsigned".into(),
            },
        )
        .is_err());
    }
}

#[test]
fn release_notes_combine_changelog_signing_and_verification() {
    let (_d, dir) = dist_with_required();
    let (manifest, _) = manifest::build_manifest(&dir, VERSION, Channel::Stable).unwrap();
    let state = signing::merge(
        VERSION,
        Channel::Stable,
        vec![unsigned_fragment(&manifest)],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "dry-run: not signed".into(),
        },
    )
    .unwrap();
    let body = xtask::notes::render_body(
        "### Added\n\n- Something new (PRD 0001 RF1).",
        &state,
        &manifest,
        "owner/cuotai",
    );
    assert!(body.contains("- Something new (PRD 0001 RF1)."));
    assert!(body.contains("| Artifact | Platform | Signed | Method / status |"));
    assert!(body.contains("**no**"));
    assert!(body.contains("shasum -a 256 -c SHA256SUMS"));
    assert!(!body.contains("gh attestation verify"));
    assert!(body.contains("Cuotai_0.68.0_arm64-setup.exe")); // missing optional listed
    assert!(body.contains("updater artifacts were **not** produced"));
    assert!(body.contains("Provenance attestations are also not published"));
    assert!(!body.contains("cosign verify-blob"));

    // Signed checksums switch the verification block to cosign.
    let state = signing::merge(
        VERSION,
        Channel::Stable,
        vec![unsigned_fragment(&manifest)],
        &manifest,
        ChecksumSigning {
            signed: true,
            method: ChecksumSigningMethod::SigstoreKeyless,
            status: "signed".into(),
        },
    )
    .unwrap();
    let body = xtask::notes::render_body("### Added\n\n- X.", &state, &manifest, "owner/cuotai");
    assert!(body.contains("cosign verify-blob"));
    assert!(body.contains("gh attestation verify"));
    assert!(body.contains("certificate-oidc-issuer https://token.actions.githubusercontent.com"));
}

#[test]
fn updater_manifest_uses_the_real_signature_file() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("Cuotai.app.tar.gz");
    std::fs::write(&archive, "archive bytes").unwrap();
    std::fs::write(
        dir.path().join("Cuotai.app.tar.gz.sig"),
        "dW50cnVzdGVkIHNpZw==\n",
    )
    .unwrap();
    let json = xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes here",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archive,
    )
    .unwrap();
    assert_eq!(json["version"], "0.68.0");
    assert_eq!(json["pub_date"], "2026-10-21T12:00:00Z");
    let url = "https://github.com/owner/cuotai/releases/download/v0.68.0/Cuotai.app.tar.gz";
    assert_eq!(json["platforms"]["darwin-aarch64"]["url"], url);
    assert_eq!(
        json["platforms"]["darwin-x86_64"]["signature"],
        "dW50cnVzdGVkIHNpZw=="
    );

    // No signature file → honest failure instead of a fake manifest.
    std::fs::remove_file(dir.path().join("Cuotai.app.tar.gz.sig")).unwrap();
    assert!(xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archive,
    )
    .is_err());
}

fn workspace_bom(root: &str, crates: &[&str], shared: &[&str]) -> String {
    let mut components: Vec<serde_json::Value> = crates
        .iter()
        .chain(shared.iter())
        .map(|name| {
            serde_json::json!({
                "bom-ref": format!("pkg:{name}"),
                "type": "library",
                "name": name,
                "version": "1.2.3",
            })
        })
        .collect();
    components.sort_by_key(|c| c["bom-ref"].as_str().unwrap().to_string());
    // One dependency entry per bom-ref, as cargo-cyclonedx emits: the shared
    // libraries have no dependencies, the workspace crates depend on them.
    let dependencies: Vec<serde_json::Value> = components
        .iter()
        .map(|c| {
            let reference = c["bom-ref"].as_str().unwrap().to_string();
            let depends_on: Vec<String> = if crates
                .iter()
                .any(|crate_name| format!("pkg:{crate_name}") == reference)
            {
                shared.iter().map(|s| format!("pkg:{s}")).collect()
            } else {
                Vec::new()
            };
            serde_json::json!({"ref": reference, "dependsOn": depends_on})
        })
        .collect();
    serde_json::json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:00000000-0000-4000-8000-000000000000",
        "version": 1,
        "metadata": {
            "component": {
                "bom-ref": root,
                "type": "application",
                "name": root,
                "version": VERSION,
            }
        },
        "components": components,
        "dependencies": dependencies,
    })
    .to_string()
}

#[test]
fn aggregate_sbom_covers_both_workspaces_in_one_connected_graph() {
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("sbom-cli.json");
    let desktop = dir.path().join("sbom-desktop.json");
    // `serde_json` and `sha2` are in both graphs, `tauri` only in the desktop.
    std::fs::write(
        &cli,
        workspace_bom("cuotai-tui", &["rusqlite"], &["serde_json", "sha2"]),
    )
    .unwrap();
    std::fs::write(
        &desktop,
        workspace_bom("cuotai-desktop", &["tauri"], &["serde_json", "sha2"]),
    )
    .unwrap();

    let merged = xtask::sbom::merge(
        &[cli.as_path(), desktop.as_path()],
        &Version::parse(VERSION).unwrap(),
        "2026-10-21T09:00:00Z",
    )
    .unwrap();

    assert_eq!(merged["bomFormat"], "CycloneDX");
    assert_eq!(merged["specVersion"], "1.5");
    assert_eq!(merged["metadata"]["component"]["type"], "application");
    assert_eq!(
        merged["metadata"]["component"]["version"], VERSION,
        "the aggregate root carries the release version"
    );
    assert_eq!(merged["metadata"]["timestamp"], "2026-10-21T09:00:00Z");

    let components = merged["components"].as_array().unwrap();
    let refs: Vec<&str> = components
        .iter()
        .map(|c| c["bom-ref"].as_str().unwrap())
        .collect();
    // Both workspace roots plus the union of crates, each shared crate once,
    // and the list sorted by bom-ref.
    assert_eq!(
        refs,
        vec![
            "cuotai-desktop",
            "cuotai-tui",
            "pkg:rusqlite",
            "pkg:serde_json",
            "pkg:sha2",
            "pkg:tauri",
        ]
    );

    let dependencies = merged["dependencies"].as_array().unwrap();
    let root = dependencies
        .iter()
        .find(|d| d["ref"] == "cuotai-release")
        .expect("aggregate root entry");
    assert_eq!(
        root["dependsOn"],
        serde_json::json!(["cuotai-desktop", "cuotai-tui"]),
        "both workspaces hang off the aggregate root, so neither is unconnected"
    );
    let tauri_edge = dependencies
        .iter()
        .find(|d| d["ref"] == "pkg:tauri")
        .unwrap();
    assert_eq!(
        tauri_edge["dependsOn"],
        serde_json::json!(["pkg:serde_json", "pkg:sha2"])
    );
}

#[test]
fn aggregate_sbom_refuses_a_workspace_bom_with_an_unresolvable_edge() {
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("sbom-cli.json");
    let desktop = dir.path().join("sbom-desktop.json");
    std::fs::write(&cli, workspace_bom("cuotai-tui", &[], &["serde_json"])).unwrap();
    let mut broken: serde_json::Value =
        serde_json::from_str(&workspace_bom("cuotai-desktop", &[], &[])).unwrap();
    broken["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"ref": "pkg:absent", "dependsOn": []}));
    std::fs::write(&desktop, broken.to_string()).unwrap();

    let err = xtask::sbom::merge(
        &[cli.as_path(), desktop.as_path()],
        &Version::parse(VERSION).unwrap(),
        "2026-10-21T09:00:00Z",
    )
    .unwrap_err();
    assert!(err.to_string().contains("pkg:absent"), "{err}");
}
