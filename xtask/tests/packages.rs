//! Package-manager manifest rendering (PRD 0021 RF5.6): deterministic
//! renders from a fixture manifest, structural validation per channel, and
//! the optional-ARM64 and signing-state branches.

use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::manifest::{self, Manifest};
use xtask::packages::{self, RenderRequest};
use xtask::signing::{
    self, ArtifactSigning, ChecksumSigning, ChecksumSigningMethod, SigningMethod, SigningState,
    SigningStatus, UpdaterState,
};
use xtask::Channel;

const VERSION: &str = "0.68.1";
const TAG: &str = "v0.68.1";
const REPO: &str = "jmarcon/cuotai";

const CASK: &str = "homebrew/Casks/cuotai-app.rb";
const FORMULA: &str = "homebrew/Formula/cuotai.rb";
const WINGET_INSTALLER: &str = "winget/Cuotai.Cuotai.installer.yaml";
const SCOOP: &str = "scoop/bucket/cuotai.json";
const PKGBUILD: &str = "aur/PKGBUILD";
const SRCINFO: &str = "aur/.SRCINFO";

fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging")
}

/// The dry-run shape of run 39691203704: every required artifact, optional
/// ARM64 legs absent, no updater assets.
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

fn optional_names() -> Vec<String> {
    let v = VERSION;
    [
        format!("Cuotai_{v}_arm64-setup.exe"),
        format!("cuotai-{v}-aarch64-pc-windows-msvc.zip"),
        format!("Cuotai_{v}_aarch64.deb"),
    ]
    .into_iter()
    .collect()
}

fn manifest_with(extra: &[String]) -> Manifest {
    let dir = tempfile::tempdir().unwrap();
    for name in required_names().iter().chain(extra.iter()) {
        std::fs::write(dir.path().join(name), format!("payload of {name}")).unwrap();
    }
    let (manifest, _) = manifest::build_manifest(dir.path(), VERSION, Channel::Stable).unwrap();
    manifest
}

fn artifact_sha(manifest: &Manifest, name: &str) -> String {
    manifest
        .artifacts
        .iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("manifest has no {name}"))
        .sha256
        .clone()
}

fn signing_state(dmg_signed: bool) -> SigningState {
    SigningState {
        schema: signing::SIGNING_STATE_SCHEMA,
        version: VERSION.into(),
        channel: Channel::Stable,
        artifacts: vec![
            ArtifactSigning {
                name: format!("Cuotai_{VERSION}_universal.dmg"),
                signed: dmg_signed,
                method: if dmg_signed {
                    SigningMethod::DeveloperId
                } else {
                    SigningMethod::AdHoc
                },
                status: if dmg_signed {
                    SigningStatus::Signed
                } else {
                    SigningStatus::Unsigned
                },
                note: None,
                updater_signature: None,
            },
            ArtifactSigning {
                name: format!("Cuotai_{VERSION}_x64-setup.exe"),
                signed: false,
                method: SigningMethod::None,
                status: SigningStatus::Unsigned,
                note: None,
                updater_signature: None,
            },
        ],
        checksums: ChecksumSigning {
            signed: false,
            method: ChecksumSigningMethod::None,
            status: "dry-run: signing skipped".into(),
        },
        updater: UpdaterState {
            available: false,
            artifacts: vec![],
            reason: Some("no updater key in a dry run".into()),
        },
    }
}

fn request(manifest: Manifest, signing: Option<SigningState>) -> RenderRequest {
    RenderRequest {
        manifest,
        signing,
        repo: REPO.into(),
        tag: TAG.into(),
    }
}

fn render(manifest: Manifest, signing: Option<SigningState>) -> Vec<packages::Rendered> {
    packages::render_all(&templates_dir(), &request(manifest, signing)).unwrap()
}

fn content_of(rendered: &[packages::Rendered], path: &str) -> String {
    rendered
        .iter()
        .find(|r| r.path == Path::new(path))
        .unwrap_or_else(|| panic!("no rendered {path}"))
        .content
        .clone()
}

/// True when the machine has the linter; the structural checks in
/// `packages.rs` always run, these are the channels' own linters on top.
fn linter_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn renders_every_channel_deterministically_from_one_manifest() {
    let manifest = manifest_with(&[]);
    let signing = Some(signing_state(false));
    let first = render(manifest.clone(), signing.clone());
    let second = render(manifest.clone(), signing);

    assert_eq!(first, second, "same inputs must render identical bytes");
    let paths: Vec<String> = first.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(
        paths,
        vec![
            "homebrew/Casks/cuotai-app.rb",
            "homebrew/Formula/cuotai.rb",
            "winget/Cuotai.Cuotai.yaml",
            "winget/Cuotai.Cuotai.installer.yaml",
            "winget/Cuotai.Cuotai.locale.en-US.yaml",
            "scoop/bucket/cuotai.json",
            "aur/PKGBUILD",
            "aur/.SRCINFO",
        ]
    );
    for file in &first {
        assert!(
            !file.content.contains("{{"),
            "{} still holds a placeholder",
            file.path.display()
        );
    }

    // Every channel carries the manifest's checksums and the verified tag's
    // release URL; nothing is recomputed or typed by hand.
    let dmg_sha = artifact_sha(&manifest, &format!("Cuotai_{VERSION}_universal.dmg"));
    let cli_arm_sha = artifact_sha(
        &manifest,
        &format!("cuotai-{VERSION}-aarch64-apple-darwin.tar.gz"),
    );
    let cli_x64_sha = artifact_sha(
        &manifest,
        &format!("cuotai-{VERSION}-x86_64-apple-darwin.tar.gz"),
    );
    let nsis_sha = artifact_sha(&manifest, &format!("Cuotai_{VERSION}_x64-setup.exe"));
    let zip_sha = artifact_sha(
        &manifest,
        &format!("cuotai-{VERSION}-x86_64-pc-windows-msvc.zip"),
    );
    let deb_sha = artifact_sha(&manifest, &format!("Cuotai_{VERSION}_amd64.deb"));
    let url = |name: &str| format!("https://github.com/{REPO}/releases/download/{TAG}/{name}");

    let cask = content_of(&first, CASK);
    assert!(cask.contains(&format!("version \"{VERSION}\"")));
    assert!(cask.contains(&format!("sha256 \"{dmg_sha}\"")));
    // Homebrew interpolates `#{version}` inside the URL, so the versioned
    // file name is assembled by Homebrew from the `version` line above.
    assert!(
        cask.contains(
            "url \"https://github.com/jmarcon/cuotai/releases/download/v0.68.1/Cuotai_#{version}_universal.dmg\""
        ),
        "{cask}"
    );
    assert!(cask.contains("app \"Cuotai.app\""));
    assert!(cask.contains("zap trash:"));

    let formula = content_of(&first, FORMULA);
    assert!(formula.contains(&format!("sha256 \"{cli_arm_sha}\"")));
    assert!(formula.contains(&format!("sha256 \"{cli_x64_sha}\"")));

    let installer = content_of(&first, WINGET_INSTALLER);
    assert!(installer.contains(&format!("InstallerSha256: {nsis_sha}")));
    assert!(installer.contains(&url(&format!("Cuotai_{VERSION}_x64-setup.exe"))));
    assert!(installer.contains("Silent: /S"));
    // PRD 0018 RF3.2: the switch that makes the installer write the marker.
    assert!(installer.contains("Custom: /WINGET=1"));

    let scoop = content_of(&first, SCOOP);
    assert!(scoop.contains(&format!("\"hash\": \"{zip_sha}\"")));

    let pkgbuild = content_of(&first, PKGBUILD);
    assert!(pkgbuild.contains(&format!("pkgver={VERSION}")));
    assert!(pkgbuild.contains(&format!("sha256sums_x86_64=('{deb_sha}')")));

    let srcinfo = content_of(&first, SRCINFO);
    assert!(srcinfo.contains(&format!("\tpkgver = {VERSION}")));
    assert!(srcinfo.contains(&format!("\tsha256sums_x86_64 = {deb_sha}")));
}

#[test]
fn cask_caveat_follows_the_signing_state() {
    let manifest = manifest_with(&[]);
    let unsigned = content_of(&render(manifest.clone(), Some(signing_state(false))), CASK);
    assert!(unsigned.contains("caveats do"));
    assert!(unsigned.contains("com.apple.quarantine"));

    let signed = content_of(&render(manifest, Some(signing_state(true))), CASK);
    assert!(
        !signed.contains("caveats do"),
        "a Developer ID signed DMG needs no Gatekeeper note"
    );

    // No signing state at all is treated as "not proven signed".
    let unknown = content_of(&render(manifest_with(&[]), None), CASK);
    assert!(unknown.contains("caveats do"));
}

#[test]
fn optional_arms_are_included_only_when_produced() {
    let manifest = manifest_with(&optional_names());
    let nsis_arm = artifact_sha(&manifest, &format!("Cuotai_{VERSION}_arm64-setup.exe"));
    let deb_arm = artifact_sha(&manifest, &format!("Cuotai_{VERSION}_aarch64.deb"));
    let rendered = render(manifest, Some(signing_state(false)));

    let installer = content_of(&rendered, WINGET_INSTALLER);
    assert!(installer.contains("Architecture: arm64"), "{installer}");
    assert!(installer.contains(&format!("InstallerSha256: {nsis_arm}")));

    let scoop: serde_json::Value = serde_json::from_str(&content_of(&rendered, SCOOP)).unwrap();
    assert!(scoop["architecture"]["arm64"]["hash"].is_string());
    assert!(scoop["autoupdate"]["architecture"]["arm64"]["url"]
        .as_str()
        .unwrap()
        .contains("aarch64-pc-windows-msvc"));

    let pkgbuild = content_of(&rendered, PKGBUILD);
    assert!(pkgbuild.contains("source_aarch64=("));
    assert!(pkgbuild.contains(&format!("sha256sums_aarch64=('{deb_arm}')")));
    assert!(pkgbuild.contains("'aarch64'"));
    let srcinfo = content_of(&rendered, SRCINFO);
    assert!(srcinfo.contains("\tarch = aarch64"));
    assert!(srcinfo.contains(&format!("\tsha256sums_aarch64 = {deb_arm}")));
}

#[test]
fn missing_artifact_fails_the_owning_channel() {
    let mut manifest = manifest_with(&[]);
    manifest.artifacts.retain(|a| a.kind != "desktop-dmg");
    let err = packages::render_all(&templates_dir(), &request(manifest, None)).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("channel homebrew"), "{message}");
    assert!(message.contains("desktop-dmg"), "{message}");
}

#[test]
fn tag_and_repo_must_match_the_verified_release() {
    let manifest = manifest_with(&[]);
    let mut wrong_tag = request(manifest.clone(), None);
    wrong_tag.tag = "v9.9.9".into();
    let err = packages::render_all(&templates_dir(), &wrong_tag).unwrap_err();
    assert!(err.to_string().contains("does not match manifest version"));

    let mut wrong_repo = request(manifest, None);
    wrong_repo.repo = "not a slug".into();
    let err = packages::render_all(&templates_dir(), &wrong_repo).unwrap_err();
    assert!(err.to_string().contains("invalid repository slug"));
}

#[test]
fn template_placeholders_must_all_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let src = templates_dir();
    for file in [
        "homebrew/Casks/cuotai-app.rb.tmpl",
        "homebrew/Formula/cuotai.rb.tmpl",
        "winget/Cuotai.Cuotai.yaml.tmpl",
        "winget/Cuotai.Cuotai.installer.yaml.tmpl",
        "winget/Cuotai.Cuotai.locale.en-US.yaml.tmpl",
        "scoop/bucket/cuotai.json.tmpl",
        "aur/PKGBUILD.tmpl",
    ] {
        let target = dir.path().join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let content = std::fs::read_to_string(src.join(file)).unwrap();
        std::fs::write(
            &target,
            content.replace("{{VERSION}}", "{{VERSION}} {{BOGUS}}"),
        )
        .unwrap();
    }
    let err = packages::render_all(dir.path(), &request(manifest_with(&[]), None)).unwrap_err();
    assert!(err.to_string().contains("unresolved placeholder"), "{err}");
}

#[test]
fn rendered_files_pass_the_channels_own_syntax_checks_when_available() {
    let rendered = render(manifest_with(&[]), Some(signing_state(false)));
    let out = tempfile::tempdir().unwrap();
    packages::write_rendered(out.path(), &rendered).unwrap();

    // The writer creates the channel directories and the exact file set.
    for file in &rendered {
        assert_eq!(
            std::fs::read_to_string(out.path().join(&file.path)).unwrap(),
            file.content
        );
    }

    if linter_available("ruby") {
        for file in [CASK, FORMULA] {
            let path = out.path().join(file);
            let output = Command::new("ruby")
                .args(["-c", path.to_str().unwrap()])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "ruby -c failed for {file}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    if linter_available("bash") {
        let path = out.path().join(PKGBUILD);
        let output = Command::new("bash")
            .args(["-n", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "bash -n failed for the PKGBUILD: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// The AUR rejects a push whose `PKGBUILD` and `.SRCINFO` disagree, and a
/// missing checksum blocks the build: every architecture that has a source
/// must carry the same number of checksums, and every checksum must appear
/// in the generated `.SRCINFO`.
#[test]
fn aur_pkgbuild_and_srcinfo_agree() {
    let rendered = render(manifest_with(&optional_names()), Some(signing_state(false)));
    let pkgbuild = content_of(&rendered, PKGBUILD);
    let srcinfo = content_of(&rendered, SRCINFO);
    for arch in ["x86_64", "aarch64"] {
        let sources = pkgbuild
            .lines()
            .find_map(|l| l.strip_prefix(&format!("source_{arch}=(")))
            .unwrap_or_else(|| panic!("no source_{arch} in the PKGBUILD"));
        let sums = pkgbuild
            .lines()
            .find_map(|l| l.strip_prefix(&format!("sha256sums_{arch}=(")))
            .unwrap_or_else(|| panic!("no sha256sums_{arch} in the PKGBUILD"));
        assert_eq!(
            sources.matches("https://").count(),
            sums.split('\'').filter(|s| s.len() == 64).count(),
            "source_{arch} and sha256sums_{arch} have different entry counts"
        );
        for sum in sums.split('\'').filter(|s| s.len() == 64) {
            assert!(
                srcinfo.contains(&format!("\tsha256sums_{arch} = {sum}")),
                ".SRCINFO does not carry {sum} for {arch}"
            );
        }
    }
}

#[test]
fn load_inputs_accepts_a_directory_and_rejects_a_mismatched_signing_state() {
    let manifest = manifest_with(&[]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    let (loaded, signing) = packages::load_inputs(dir.path()).unwrap();
    assert_eq!(loaded, manifest);
    assert!(signing.is_none());

    let mut state = signing_state(false);
    state.version = "9.9.9".into();
    std::fs::write(
        dir.path().join("signing-state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();
    let err = packages::load_inputs(dir.path()).unwrap_err();
    assert!(err.to_string().contains("does not match manifest version"));
}
