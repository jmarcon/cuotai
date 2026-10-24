//! Behavior-level tests for the assembly side (PRD 0021 RF2–RF4): artifact
//! manifest + checksums, download verification, signing-state merge,
//! release-notes rendering and the updater manifest.

use std::path::{Path, PathBuf};

use xtask::manifest::{self, Manifest};
use xtask::signing::{
    self, ArtifactSigning, ChecksumSigning, ChecksumSigningMethod, Fragment, SigningMethod,
    SigningStatus, UpdaterState,
};
use xtask::updater::PlatformArchive;
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
                updater_signature: None,
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
        // An available updater with no signature at all is not a usable state.
        UpdaterState {
            available: true,
            artifacts: vec!["Cuotai.app.tar.gz".into()],
            reason: None,
        },
        // Listing the same archive twice is a pipeline error.
        UpdaterState {
            available: true,
            artifacts: vec![
                "Cuotai.app.tar.gz".into(),
                "Cuotai.app.tar.gz".into(),
                "Cuotai.app.tar.gz.sig".into(),
            ],
            reason: None,
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

/// A fragment reporting exactly the files one job owns: the named updater
/// artifacts are updater-signed, the rest of that job's files keep
/// checksum-signature trust.
///
/// A name listed as `<payload>.sig` is the detached updater proof of the
/// payload beside it: the payload keeps its own method (Authenticode on
/// Windows, checksum-signature on Linux) and gains `updater_signature`,
/// which is what makes one artifact carry two proofs without ever being
/// reported twice.
fn updater_signed_fragment(
    manifest: &Manifest,
    owned: &[String],
    updater_artifacts: &[String],
) -> Fragment {
    let claims = |name: &str| updater_artifacts.iter().any(|claimed| claimed == name);
    Fragment {
        artifacts: manifest
            .artifacts
            .iter()
            // A detached `.sig` is never an artifact record of its own: the
            // payload's record carries it as `updater_signature`.
            .filter(|artifact| owned.contains(&artifact.name) && !artifact.name.ends_with(".sig"))
            .map(|artifact| ArtifactSigning {
                name: artifact.name.clone(),
                signed: claims(&artifact.name),
                method: if claims(&artifact.name) {
                    SigningMethod::TauriUpdaterKey
                } else {
                    SigningMethod::ChecksumSignature
                },
                status: if claims(&artifact.name) {
                    SigningStatus::Signed
                } else {
                    SigningStatus::Unsigned
                },
                note: None,
                updater_signature: claims(&format!("{}.sig", artifact.name))
                    .then(|| format!("{}.sig", artifact.name)),
            })
            .collect(),
        updater: (!updater_artifacts.is_empty()).then(|| UpdaterState {
            available: true,
            artifacts: updater_artifacts.to_vec(),
            reason: None,
        }),
    }
}

/// The same, for a job that produced no updater artifact: it reports only its
/// own files and states why the updater has nothing from it.
fn owned_fragment(manifest: &Manifest, owned: &[String], updater_artifacts: &[String]) -> Fragment {
    let mut fragment = updater_signed_fragment(manifest, owned, updater_artifacts);
    if updater_artifacts.is_empty() {
        fragment.updater = Some(UpdaterState {
            available: false,
            artifacts: Vec::new(),
            reason: Some("this run produced no updater artifact".into()),
        });
    }
    fragment
}

#[test]
fn signing_state_unions_the_updater_artifacts_of_every_platform() {
    let v = VERSION;
    let dir = tempfile::tempdir().unwrap();
    // `latest.json` is not itself an updater artifact: it is published and
    // authenticated through the signed `SHA256SUMS`.
    let signed = [
        "Cuotai.app.tar.gz".to_string(),
        "Cuotai.app.tar.gz.sig".to_string(),
        format!("Cuotai_{v}_amd64.AppImage"),
        format!("Cuotai_{v}_amd64.AppImage.sig"),
        format!("Cuotai_{v}_x64-setup.exe"),
        format!("Cuotai_{v}_x64-setup.exe.sig"),
    ];
    let payloads = [
        "latest.json".to_string(),
        format!("Cuotai_{v}_universal.dmg"),
        "Cuotai.app.tar.gz".to_string(),
        "Cuotai.app.tar.gz.sig".to_string(),
        format!("Cuotai_{v}_x64-setup.exe"),
        format!("Cuotai_{v}_x64-setup.exe.sig"),
        format!("Cuotai_{v}_x64_en-US.msi"),
        format!("Cuotai_{v}_amd64.AppImage"),
        format!("Cuotai_{v}_amd64.AppImage.sig"),
        format!("Cuotai_{v}_amd64.deb"),
        format!("Cuotai_{v}_amd64.rpm"),
        format!("cuotai-{v}-aarch64-apple-darwin.tar.gz"),
        format!("cuotai-{v}-x86_64-apple-darwin.tar.gz"),
        format!("cuotai-{v}-x86_64-unknown-linux-gnu.tar.gz"),
        format!("cuotai-{v}-x86_64-unknown-linux-musl.tar.gz"),
        format!("cuotai-{v}-x86_64-pc-windows-msvc.zip"),
        "sbom.cdx.json".to_string(),
    ];
    write_dist(dir.path(), &payloads);
    let (manifest, _) = manifest::build_manifest(dir.path(), VERSION, Channel::Stable).unwrap();

    // Each platform job reports only the files it owns, as the real fragments do;
    // the merged state is the union and `latest.json` keeps checksum trust.
    let macos_names = vec![
        "Cuotai.app.tar.gz".to_string(),
        "Cuotai.app.tar.gz.sig".to_string(),
    ];
    let windows_names = vec![
        format!("Cuotai_{v}_x64-setup.exe"),
        format!("Cuotai_{v}_x64-setup.exe.sig"),
        format!("Cuotai_{v}_x64_en-US.msi"),
    ];
    let linux_names = vec![
        format!("Cuotai_{v}_amd64.AppImage"),
        format!("Cuotai_{v}_amd64.AppImage.sig"),
        format!("Cuotai_{v}_amd64.deb"),
        format!("Cuotai_{v}_amd64.rpm"),
    ];

    // The macOS job updater-signs the archive itself; the Windows job adds
    // the detached signature beside an installer that keeps its Authenticode
    // record; Linux adds the detached signature beside a checksum-signed
    // package. Only the entries each job owns are reported.
    let macos = owned_fragment(&manifest, &macos_names, &macos_names);
    let windows = updater_signed_fragment(
        &manifest,
        &windows_names,
        &[
            format!("Cuotai_{v}_x64-setup.exe"),
            format!("Cuotai_{v}_x64-setup.exe.sig"),
        ],
    );
    let linux = updater_signed_fragment(
        &manifest,
        &linux_names,
        &[
            format!("Cuotai_{v}_amd64.AppImage"),
            format!("Cuotai_{v}_amd64.AppImage.sig"),
        ],
    );
    // The Windows installer is Authenticode-signed and the Linux package keeps
    // checksum-signature trust, so both carry their own method and gain the
    // detached signature as a second proof; only the macOS archive is signed
    // in place by the updater key.
    let mut windows = windows;
    if let Some(installer) = windows
        .artifacts
        .iter_mut()
        .find(|entry| entry.name == format!("Cuotai_{v}_x64-setup.exe"))
    {
        installer.method = SigningMethod::Authenticode;
        installer.signed = true;
        installer.status = SigningStatus::Signed;
    }
    let mut linux = linux;
    if let Some(appimage) = linux
        .artifacts
        .iter_mut()
        .find(|entry| entry.name == format!("Cuotai_{v}_amd64.AppImage"))
    {
        appimage.method = SigningMethod::ChecksumSignature;
        appimage.signed = false;
        appimage.status = SigningStatus::Unsigned;
    }
    let platform_owned = [
        macos_names.clone(),
        windows_names.clone(),
        linux_names.clone(),
    ];
    let rest: Vec<String> = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.name.clone())
        .filter(|name| !platform_owned.iter().any(|owned| owned.contains(name)))
        .collect();
    let assemble = owned_fragment(&manifest, &rest, &[]);

    let state = signing::merge(
        VERSION,
        Channel::Stable,
        vec![macos.clone(), windows, linux, assemble],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .unwrap();

    assert!(state.updater.available);
    assert_eq!(state.updater.artifacts, signed);
    // Three updater proofs, on three artifacts: the macOS archive is signed in
    // place, and the Windows installer and the Linux AppImage each carry a
    // detached `.sig` beside their own method (Authenticode and
    // checksum-signature). No artifact is reported twice.
    assert_eq!(
        state
            .artifacts
            .iter()
            .filter(|entry| entry.method == SigningMethod::TauriUpdaterKey)
            .count(),
        1
    );
    assert_eq!(
        state
            .artifacts
            .iter()
            .filter(|entry| entry.updater_signature.is_some())
            .count(),
        3
    );
    let installer = state
        .artifacts
        .iter()
        .find(|entry| entry.name == format!("Cuotai_{v}_x64-setup.exe"))
        .expect("installer record");
    assert_eq!(installer.method, SigningMethod::Authenticode);
    assert_eq!(
        installer.updater_signature.as_deref(),
        Some(format!("Cuotai_{v}_x64-setup.exe.sig").as_str())
    );
    assert!(
        !state
            .artifacts
            .iter()
            .any(|entry| entry.name.ends_with(".sig")),
        "a detached signature is the updater proof of its payload, not an artifact of its own"
    );

    // One platform reporting the same file twice cannot merge.
    assert!(signing::merge(
        VERSION,
        Channel::Stable,
        vec![macos.clone(), macos.clone()],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .is_err());

    // A fragment claiming the updater is available without producing anything
    // is refused even when its platform peers are honest.
    let mut lying = unsigned_fragment(&manifest);
    lying.updater = Some(UpdaterState {
        available: true,
        artifacts: Vec::new(),
        reason: None,
    });
    assert!(signing::merge(
        VERSION,
        Channel::Stable,
        vec![lying],
        &manifest,
        ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "unsigned".into(),
        },
    )
    .is_err());
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
    assert!(body.contains("--bundle SHA256SUMS.sigstore.json"));
    assert!(!body.contains("SHA256SUMS.sig "));
    assert!(body.contains(
        r"--certificate-identity-regexp '^https://github\.com/owner/cuotai/\.github/workflows/release\.yml@refs/tags/v'"
    ));
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
    let archives = vec![PlatformArchive::new("darwin-aarch64-app", &archive).unwrap()];
    let json = xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes here",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archives,
        &["darwin-aarch64-app"],
    )
    .unwrap();
    assert_eq!(json["version"], "0.68.0");
    assert_eq!(json["pub_date"], "2026-10-21T12:00:00Z");
    let url = "https://github.com/owner/cuotai/releases/download/v0.68.0/Cuotai.app.tar.gz";
    assert_eq!(json["platforms"]["darwin-aarch64-app"]["url"], url);
    assert_eq!(json["platforms"]["darwin-aarch64"]["url"], url);
    assert_eq!(
        json["platforms"]["darwin-aarch64-app"]["signature"],
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
        &archives,
        &["darwin-aarch64-app"],
    )
    .is_err());
}

#[test]
fn updater_manifest_refuses_unknown_platforms_and_missing_required_keys() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("Cuotai.app.tar.gz");
    std::fs::write(&archive, "archive bytes").unwrap();
    std::fs::write(
        dir.path().join("Cuotai.app.tar.gz.sig"),
        "dW50cnVzdGVkIHNpZw==\n",
    )
    .unwrap();

    // Keys the plugin never looks up, or installers no bundle produces.
    for key in [
        "plan9-x86_64-app",
        "darwin-sparc-app",
        "darwin-x86_64-deb",
        "windows-x86_64-appimage",
        "linux-x86_64-rpm-deb",
        "",
    ] {
        assert!(
            PlatformArchive::new(key, &archive).is_err(),
            "{key} must not reach the manifest"
        );
    }

    let archives = vec![PlatformArchive::new("darwin-aarch64-app", &archive).unwrap()];
    // A required platform that this release did not produce fails instead of
    // shipping a manifest that would strand those users.
    for missing in ["darwin-x86_64-app", "windows-x86_64-nsis", "linux-x86_64"] {
        let err = xtask::updater::latest_json(
            &Version::parse(VERSION).unwrap(),
            "Notes",
            "2026-10-21T12:00:00Z",
            "owner/cuotai",
            "v0.68.0",
            &archives,
            &[missing],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains(missing), "{missing}: {err}");
    }

    // No archive at all is never a manifest.
    assert!(xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &[],
        &[],
    )
    .is_err());
}

#[test]
fn updater_manifest_is_https_only() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("Cuotai_0.68.0_amd64.AppImage");
    std::fs::write(&archive, "appimage bytes").unwrap();
    std::fs::write(
        dir.path().join("Cuotai_0.68.0_amd64.AppImage.sig"),
        "c2lnbmF0dXJl\n",
    )
    .unwrap();
    let archives = vec![PlatformArchive::with_keys(&["linux-x86_64-appimage"], &archive).unwrap()];
    let json = xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archives,
        &["linux-x86_64-appimage"],
    )
    .unwrap();
    for target in ["linux-x86_64", "linux-x86_64-appimage"] {
        let url = json["platforms"][target]["url"].as_str().unwrap();
        assert!(url.starts_with("https://"), "{target}: {url}");
    }
    // The generic key is added automatically next to the installer key.
    assert_eq!(
        json["platforms"]["linux-x86_64-appimage"]["signature"],
        "c2lnbmF0dXJl"
    );

    // A signature that is not base64 is refused rather than published.
    std::fs::write(
        dir.path().join("Cuotai_0.68.0_amd64.AppImage.sig"),
        "not base64!\n",
    )
    .unwrap();
    assert!(xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archives,
        &["linux-x86_64-appimage"],
    )
    .is_err());
}

/// An ephemeral updater key pair, generated the way `cargo tauri signer`
/// does: `minisign` keys with the signature bound to a version-bearing
/// trusted comment. Nothing here is a production key.
struct EphemeralSigner {
    public_key: minisign::PublicKey,
    secret_key: minisign::SecretKey,
}

impl EphemeralSigner {
    fn generate() -> Self {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Self {
            public_key: pair.pk,
            secret_key: pair.sk,
        }
    }

    /// Writes `<archive>.sig` exactly like `tauri-cli`'s `sign_file`: the
    /// base64 of the minisign signature box, whose trusted comment carries the
    /// timestamp, the file name and the app version.
    fn sign(&self, archive: &Path, version: &str) {
        let name = archive.file_name().unwrap().to_string_lossy().to_string();
        let trusted = format!("timestamp:1700000000\tfile:{name}\tversion:{version}");
        let bytes = std::fs::read(archive).unwrap();
        let signature = minisign::sign(
            None,
            &self.secret_key,
            std::io::Cursor::new(bytes),
            Some(&trusted),
            Some("signature from tauri secret key"),
        )
        .unwrap();
        let sig_path = archive.with_extension(format!(
            "{}.sig",
            archive.extension().unwrap().to_string_lossy()
        ));
        std::fs::write(sig_path, base64_encode(signature.to_string().as_bytes())).unwrap();
    }

    /// The same check `tauri-plugin-updater` performs before installing: the
    /// manifest's `signature` field must verify against the embedded public
    /// key over the downloaded bytes.
    fn verify(&self, archive: &Path, signature: &str) -> Result<(), String> {
        let decoded = base64_decode(signature)?;
        let box_ = minisign::SignatureBox::from_string(&decoded)
            .map_err(|e| format!("signature is not a minisign signature box: {e}"))?;
        let bytes = std::fs::read(archive).map_err(|e| e.to_string())?;
        minisign::verify(
            &self.public_key,
            &box_,
            std::io::Cursor::new(bytes),
            true,
            false,
            true,
        )
        .map_err(|e| format!("signature does not verify: {e}"))
    }

    fn pubkey_base64(&self) -> String {
        self.public_key.to_base64()
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(text: &str) -> Result<String, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|e| format!("signature is not base64: {e}"))
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|e| format!("base64 did not decode to text: {e}"))
        })
}

#[test]
fn updater_manifest_validates_against_an_ephemeral_real_signature() {
    let dir = tempfile::tempdir().unwrap();
    let signer = EphemeralSigner::generate();

    // One release touching every updatable target: the universal macOS
    // archive, the Windows installers and the Linux packages.
    let payloads: [(&str, &[&str]); 5] = [
        (
            "Cuotai.app.tar.gz",
            &["darwin-aarch64-app", "darwin-x86_64-app"],
        ),
        ("Cuotai_0.68.0_x64-setup.exe", &["windows-x86_64-nsis"]),
        ("Cuotai_0.68.0_x64_en-US.msi", &["windows-x86_64-msi"]),
        ("Cuotai_0.68.0_amd64.AppImage", &["linux-x86_64-appimage"]),
        ("Cuotai_0.68.0_amd64.deb", &["linux-x86_64-deb"]),
    ];
    let mut archives = Vec::new();
    for (name, keys) in payloads {
        let path = dir.path().join(name);
        std::fs::write(&path, format!("payload of {name}")).unwrap();
        signer.sign(&path, VERSION);
        archives.push(PlatformArchive::with_keys(keys, &path).unwrap());
    }

    let json = xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "### Added\n\n- Update check (PRD 0018 RF2).",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archives,
        &[
            "darwin-aarch64-app",
            "darwin-x86_64-app",
            "windows-x86_64-nsis",
            "windows-x86_64-msi",
            "linux-x86_64-appimage",
            "linux-x86_64-deb",
        ],
    )
    .unwrap();

    // The release notes are the changelog section (RF1.4, PRD 0021 RF7.5).
    assert_eq!(json["notes"], "### Added\n\n- Update check (PRD 0018 RF2).");
    assert_eq!(json["version"], "0.68.0");
    assert_eq!(json["pub_date"], "2026-10-21T12:00:00Z");

    let expected: Vec<&str> = vec![
        "darwin-aarch64",
        "darwin-aarch64-app",
        "darwin-x86_64",
        "darwin-x86_64-app",
        "linux-x86_64",
        "linux-x86_64-appimage",
        "linux-x86_64-deb",
        "windows-x86_64",
        "windows-x86_64-msi",
        "windows-x86_64-nsis",
    ];
    let platforms = json["platforms"].as_object().unwrap();
    let mut keys: Vec<&str> = platforms.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, expected, "one manifest covers every produced target");

    // Every entry: an HTTPS URL pointing at the release asset, and a signature
    // that verifies over those exact bytes with the embedded public key — the
    // check the updater plugin performs before it installs anything.
    for (key, entry) in platforms {
        let url = entry["url"].as_str().unwrap();
        assert!(
            url.starts_with("https://github.com/owner/cuotai/releases/download/v0.68.0/"),
            "{key}: {url}"
        );
        let name = url.rsplit('/').next().unwrap();
        let path = dir.path().join(name);
        signer
            .verify(&path, entry["signature"].as_str().unwrap())
            .unwrap_or_else(|e| panic!("{key}: {e}"));
    }

    // The signature is the minisign box the plugin decodes, bound to this
    // version: the embedded public key verifies it and another archive's bytes
    // do not.
    let signature = json["platforms"]["darwin-aarch64-app"]["signature"]
        .as_str()
        .unwrap();
    let box_ = minisign::SignatureBox::from_string(&base64_decode(signature).unwrap()).unwrap();
    assert!(box_.trusted_comment().unwrap().contains("version:0.68.0"));
    assert_eq!(
        box_.keynum(),
        minisign::PublicKey::from_base64(&signer.pubkey_base64())
            .unwrap()
            .keynum()
    );
    let tampered = dir.path().join("tampered");
    std::fs::write(&tampered, "payload of Cuotai.app.tar.gz but edited").unwrap();
    assert!(signer.verify(&tampered, signature).is_err());
}

#[test]
fn updater_manifest_rejects_two_archives_for_one_platform_key() {
    let dir = tempfile::tempdir().unwrap();
    let signer = EphemeralSigner::generate();
    let appimage = dir.path().join("Cuotai_0.68.0_amd64.AppImage");
    std::fs::write(&appimage, "appimage").unwrap();
    signer.sign(&appimage, VERSION);
    let copy = dir.path().join("Cuotai_0.68.0_amd64.AppImage.copy");
    std::fs::write(&copy, "appimage").unwrap();
    signer.sign(&copy, VERSION);

    // Two different files claiming the same platform key is a pipeline error,
    // not a coin toss on which one the updater downloads.
    let archives = vec![
        PlatformArchive::new("linux-x86_64-appimage", &appimage).unwrap(),
        PlatformArchive::new("linux-x86_64-appimage", &copy).unwrap(),
    ];
    let err = xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archives,
        &["linux-x86_64-appimage"],
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("linux-x86_64-appimage"), "{err}");
}

#[test]
fn updater_manifest_generic_key_answers_with_the_default_installer() {
    let dir = tempfile::tempdir().unwrap();
    let signer = EphemeralSigner::generate();
    // The MSI is passed before the NSIS installer: the generic
    // `windows-x86_64` entry must still be the NSIS installer, whatever the
    // argument order was.
    let msi = dir.path().join("Cuotai_0.68.0_x64_en-US.msi");
    std::fs::write(&msi, "msi").unwrap();
    signer.sign(&msi, VERSION);
    let nsis = dir.path().join("Cuotai_0.68.0_x64-setup.exe");
    std::fs::write(&nsis, "nsis").unwrap();
    signer.sign(&nsis, VERSION);

    let archives = vec![
        PlatformArchive::new("windows-x86_64-msi", &msi).unwrap(),
        PlatformArchive::new("windows-x86_64-nsis", &nsis).unwrap(),
    ];
    let json = xtask::updater::latest_json(
        &Version::parse(VERSION).unwrap(),
        "Notes",
        "2026-10-21T12:00:00Z",
        "owner/cuotai",
        "v0.68.0",
        &archives,
        &["windows-x86_64-nsis", "windows-x86_64-msi"],
    )
    .unwrap();
    let generic = json["platforms"]["windows-x86_64"]["url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        generic.ends_with("Cuotai_0.68.0_x64-setup.exe"),
        "{generic}"
    );
    assert!(json["platforms"]["windows-x86_64-msi"]["url"]
        .as_str()
        .unwrap()
        .ends_with("Cuotai_0.68.0_x64_en-US.msi"));
    assert!(json["platforms"]["windows-x86_64-nsis"]["url"]
        .as_str()
        .unwrap()
        .ends_with("Cuotai_0.68.0_x64-setup.exe"));
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
