//! Format contracts of the release hand-off files (PRD 0021 RF6.3):
//! `SHA256SUMS`, `signing-state.json` and `latest.json`.
//!
//! Each file is produced by the same `xtask` subcommands the release workflow
//! runs, from a deterministic artifact set, and compared byte for byte with a
//! committed golden copy under `tests/fixtures/release-formats/`. A change to
//! any of these formats therefore breaks CI here, where it is reviewed, rather
//! than a user's `shasum -c`, a package-manager renderer or an installed
//! updater. Each golden copy is also checked against an independent schema of
//! what its consumers read, and parsed and re-serialized without loss.
//!
//! After an intended format change, regenerate the golden copies with
//! `CUOTAI_REGENERATE_RELEASE_FORMATS=1 cargo test -p xtask --test release_formats`
//! and review the diff (and every consumer named below) in the same commit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::Digest;

const VERSION: &str = "1.2.3";
const TAG: &str = "v1.2.3";
const REPO: &str = "jmarcon/cuotai";
const PUB_DATE: &str = "2026-10-24T12:00:00Z";
/// A syntactically valid detached updater signature. Real signatures are
/// verified in `pipeline.rs`; this file pins the shape only.
const FAKE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGZvcm1hdCBmaXh0dXJlIG9ubHkK";

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/release-formats")
}

/// Compares `actual` with the golden copy, or rewrites it on request.
fn assert_golden(name: &str, actual: &str) {
    let path = golden_dir().join(name);
    if std::env::var_os("CUOTAI_REGENERATE_RELEASE_FORMATS").is_some() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    // A checkout with `core.autocrlf` must not turn into a format change.
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run with CUOTAI_REGENERATE_RELEASE_FORMATS=1",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    assert!(
        expected == actual,
        "{name} drifted from its golden copy {}.\n\
         If the format change is intended, regenerate with \
         CUOTAI_REGENERATE_RELEASE_FORMATS=1 and update its consumers.\n\
         --- expected\n{expected}\n--- actual\n{actual}",
        path.display()
    );
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(golden_dir().join(name))
        .unwrap()
        .replace("\r\n", "\n")
}

fn xtask(args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "xtask {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

/// The required payloads of a release, with deterministic bytes.
fn required_payloads() -> Vec<String> {
    let v = VERSION;
    vec![
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
        "sbom.cdx.json".to_string(),
    ]
}

/// The updater payloads of a signed release: each gets a detached `.sig`.
fn updater_payloads() -> Vec<String> {
    let v = VERSION;
    vec![
        "Cuotai.app.tar.gz".to_string(),
        format!("Cuotai_{v}_x64-setup.exe"),
        format!("Cuotai_{v}_x64_en-US.msi"),
        format!("Cuotai_{v}_amd64.AppImage"),
        format!("Cuotai_{v}_amd64.deb"),
        format!("Cuotai_{v}_amd64.rpm"),
    ]
}

fn cli_fragment(name: &str) -> serde_json::Value {
    serde_json::json!({"artifacts": [{
        "name": name, "signed": false, "method": "checksum-signature", "status": "unsigned",
        "note": "unsigned payload; integrity is authenticated through SHA256SUMS when its signature is available"
    }]})
}

/// The per-job fragments, shaped exactly as the release scripts write them.
fn write_fragments(dir: &Path, signed: bool) {
    let v = VERSION;
    let dmg = format!("Cuotai_{v}_universal.dmg");
    let exe = format!("Cuotai_{v}_x64-setup.exe");
    let msi = format!("Cuotai_{v}_x64_en-US.msi");
    let linux = [
        format!("Cuotai_{v}_amd64.AppImage"),
        format!("Cuotai_{v}_amd64.deb"),
        format!("Cuotai_{v}_amd64.rpm"),
    ];
    let unavailable =
        "complete Tauri updater signing credentials are not configured in the release environment";
    let fragments: Vec<(&str, serde_json::Value)> = if signed {
        vec![
            (
                "macos.json",
                serde_json::json!({
                    "artifacts": [
                        {"name": dmg, "signed": true, "method": "developer-id", "status": "signed+notarized",
                         "note": "Developer ID signatures; app payload and DMG notarized, stapled and Gatekeeper-validated"},
                        {"name": "Cuotai.app.tar.gz", "signed": true, "method": "tauri-updater-key",
                         "status": "signed", "updater_signature": "Cuotai.app.tar.gz.sig"}
                    ],
                    "updater": {"available": true, "artifacts": ["Cuotai.app.tar.gz", "Cuotai.app.tar.gz.sig"]}
                }),
            ),
            (
                "windows.json",
                serde_json::json!({
                    "artifacts": [
                        {"name": exe, "signed": true, "method": "authenticode", "status": "signed",
                         "updater_signature": format!("{exe}.sig")},
                        {"name": msi, "signed": true, "method": "authenticode", "status": "signed",
                         "updater_signature": format!("{msi}.sig")}
                    ],
                    "updater": {"available": true,
                                "artifacts": [exe, format!("{exe}.sig"), msi, format!("{msi}.sig")]}
                }),
            ),
            (
                "linux-x64.json",
                serde_json::json!({
                    "artifacts": linux.iter().map(|name| serde_json::json!({
                        "name": name, "signed": false, "method": "checksum-signature", "status": "unsigned",
                        "note": "unsigned payload; integrity is authenticated through SHA256SUMS when its signature is available",
                        "updater_signature": format!("{name}.sig")
                    })).collect::<Vec<_>>(),
                    "updater": {"available": true, "artifacts": linux.iter()
                        .flat_map(|name| [name.clone(), format!("{name}.sig")]).collect::<Vec<_>>()}
                }),
            ),
            ("assemble.json", cli_fragment("latest.json")),
        ]
    } else {
        vec![
            (
                "macos.json",
                serde_json::json!({
                    "artifacts": [{"name": dmg, "signed": false, "method": "ad-hoc", "status": "unsigned",
                        "note": "app payload and DMG container are ad-hoc signed but have no trusted identity; Gatekeeper requires a manual override"}],
                    "updater": {"available": false, "artifacts": [],
                        "reason": "complete Tauri updater signing credentials not configured or dry-run"}
                }),
            ),
            (
                "windows.json",
                serde_json::json!({
                    "artifacts": ([exe.clone(), msi.clone()]).iter().map(|name| serde_json::json!({
                        "name": name, "signed": false, "method": "none", "status": "unsigned",
                        "note": "no Windows certificate configured; the installed application and installer have no Authenticode trust"
                    })).collect::<Vec<_>>(),
                    "updater": {"available": false, "artifacts": [], "reason": unavailable}
                }),
            ),
            (
                "linux-x64.json",
                serde_json::json!({
                    "artifacts": linux.iter().map(|name| serde_json::json!({
                        "name": name, "signed": false, "method": "checksum-signature", "status": "unsigned",
                        "note": "unsigned payload; integrity is authenticated through SHA256SUMS when its signature is available"
                    })).collect::<Vec<_>>(),
                    "updater": {"available": false, "artifacts": [], "reason": unavailable}
                }),
            ),
            ("assemble.json", serde_json::json!({"artifacts": []})),
        ]
    };
    for (name, value) in fragments {
        write(dir, name, &serde_json::to_string_pretty(&value).unwrap());
    }
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
    ] {
        let name = format!("cuotai-{v}-{target}.tar.gz");
        write(
            dir,
            &format!("cli-{name}.json"),
            &cli_fragment(&name).to_string(),
        );
    }
    let zip = format!("cuotai-{v}-x86_64-pc-windows-msvc.zip");
    write(
        dir,
        "cli-x86_64-pc-windows-msvc.json",
        &cli_fragment(&zip).to_string(),
    );
    write(dir, "sbom.json", &cli_fragment("sbom.cdx.json").to_string());
}

/// The produced hand-off files of one run.
struct Assembly {
    _root: tempfile::TempDir,
    dist: PathBuf,
}

impl Assembly {
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dist.join(name)).unwrap()
    }
}

/// Runs the assembly steps of `release.yml` in their order: the updater
/// manifest (signed runs only), `release-manifest` (manifest + SHA256SUMS),
/// then `signing-state`.
fn assemble(signed: bool) -> Assembly {
    let root = tempfile::tempdir().unwrap();
    let dist = root.path().join("dist");
    let fragments = root.path().join("fragments");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::create_dir_all(&fragments).unwrap();
    for name in required_payloads() {
        write(&dist, &name, &format!("payload bytes of {name}\n"));
    }
    if signed {
        write(
            &dist,
            "Cuotai.app.tar.gz",
            "payload bytes of Cuotai.app.tar.gz\n",
        );
        for name in updater_payloads() {
            write(
                &dist,
                &format!("{name}.sig"),
                &format!("{FAKE_SIGNATURE}\n"),
            );
        }
        let notes = root.path().join("notes.txt");
        std::fs::write(&notes, "### Added\n\n- Format fixture.\n").unwrap();
        let v = VERSION;
        let platform = |key: &str, name: &str| format!("{key}={}", path(&dist.join(name)));
        let platforms = [
            platform("darwin-aarch64-app", "Cuotai.app.tar.gz"),
            platform("darwin-x86_64-app", "Cuotai.app.tar.gz"),
            platform("windows-x86_64-nsis", &format!("Cuotai_{v}_x64-setup.exe")),
            platform("windows-x86_64-msi", &format!("Cuotai_{v}_x64_en-US.msi")),
            platform(
                "linux-x86_64-appimage",
                &format!("Cuotai_{v}_amd64.AppImage"),
            ),
            platform("linux-x86_64-deb", &format!("Cuotai_{v}_amd64.deb")),
            platform("linux-x86_64-rpm", &format!("Cuotai_{v}_amd64.rpm")),
        ];
        let latest = dist.join("latest.json");
        let mut args = vec![
            "updater-manifest",
            "--version",
            VERSION,
            "--repo",
            REPO,
            "--tag",
            TAG,
            "--notes-file",
            path(&notes),
            "--pub-date",
            PUB_DATE,
        ];
        for platform in &platforms {
            args.extend(["--platform", platform.as_str()]);
        }
        for required in [
            "darwin-aarch64-app",
            "darwin-x86_64-app",
            "windows-x86_64-nsis",
            "linux-x86_64-appimage",
        ] {
            args.extend(["--require", required]);
        }
        args.extend(["--out", path(&latest)]);
        xtask(&args);
    }
    write_fragments(&fragments, signed);
    xtask(&[
        "release-manifest",
        "--artifacts-dir",
        path(&dist),
        "--version",
        VERSION,
        "--channel",
        "stable",
        "--out-dir",
        path(&dist),
    ]);
    let state = dist.join("signing-state.json");
    let mut args = vec![
        "signing-state",
        "--version",
        VERSION,
        "--channel",
        "stable",
        "--manifest",
    ];
    let manifest = dist.join("manifest.json");
    args.push(path(&manifest));
    args.extend(["--fragments-dir", path(&fragments)]);
    if signed {
        args.extend(["--checksums", "signed"]);
    } else {
        args.extend([
            "--checksums",
            "unavailable",
            "--checksums-status",
            "dry-run: checksum signature not produced (dispatch runs never publish)",
        ]);
    }
    args.extend(["--out", path(&state)]);
    xtask(&args);
    Assembly { _root: root, dist }
}

fn sha256(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn sha256sums_is_byte_stable_and_coreutils_compatible() {
    let run = assemble(true);
    let sums = run.read("SHA256SUMS");
    assert_golden("SHA256SUMS", &sums);

    // Grammar consumed by `shasum -a 256 -c`, `sha256sum -c`,
    // scripts/release/smoke-checksum.sh and verify-release.sh: one
    // `<64 lowercase hex>  <basename>` line per artifact, LF-terminated,
    // sorted by name, unique, never listing the generated metadata.
    assert!(sums.ends_with('\n') && !sums.contains('\r'));
    let mut names = Vec::new();
    for line in sums.lines() {
        let (digest, name) = line.split_at(64);
        assert!(digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        let name = name.strip_prefix("  ").expect("two-space separator");
        assert!(!name.is_empty() && !name.contains(['/', '\\', ' ']));
        // Every line verifies against the bytes in the assembly.
        assert_eq!(
            sha256(&std::fs::read(run.dist.join(name)).unwrap()),
            digest,
            "{name}"
        );
        names.push(name.to_string());
    }
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted, "SHA256SUMS must be sorted and unique");
    for generated in [
        "SHA256SUMS",
        "manifest.json",
        "signing-state.json",
        "SHA256SUMS.sigstore.json",
    ] {
        assert!(!names.iter().any(|n| n == generated), "{generated} listed");
    }
    assert!(names.iter().any(|n| n == "latest.json"));

    // SHA256SUMS and manifest.json describe the same files with the same digests.
    let manifest: serde_json::Value = serde_json::from_str(&run.read("manifest.json")).unwrap();
    let from_manifest: BTreeMap<String, String> = manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap().to_string(),
                a["sha256"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let from_sums: BTreeMap<String, String> = sums
        .lines()
        .map(|line| (line[66..].to_string(), line[..64].to_string()))
        .collect();
    assert_eq!(from_manifest, from_sums);
}

/// What the consumers of `signing-state.json` read: `xtask release-notes`
/// and `package-manifests` (through `signing::load`), the channel gates, and
/// the smoke scripts (`jq '.artifacts[] | select(.name == …) | .method'`,
/// `smoke-desktop-windows.ps1` reading `method`/`signed`). Unknown keys fail.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct SigningStateSchema {
    schema: u32,
    version: String,
    channel: String,
    artifacts: Vec<ArtifactSchema>,
    checksums: ChecksumsSchema,
    updater: UpdaterSchema,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ArtifactSchema {
    name: String,
    signed: bool,
    method: String,
    status: String,
    #[serde(default)]
    updater_signature: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ChecksumsSchema {
    signed: bool,
    method: String,
    status: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct UpdaterSchema {
    available: bool,
    artifacts: Vec<String>,
    #[serde(default)]
    reason: Option<String>,
}

fn check_signing_state_schema(text: &str) -> SigningStateSchema {
    let state: SigningStateSchema = serde_json::from_str(text).unwrap();
    assert_eq!(state.schema, xtask::signing::SIGNING_STATE_SCHEMA);
    assert_eq!(state.version, VERSION);
    assert!(["stable", "prerelease"].contains(&state.channel.as_str()));
    let methods = BTreeSet::from([
        "developer-id",
        "ad-hoc",
        "authenticode",
        "tauri-updater-key",
        "checksum-signature",
        "none",
    ]);
    let statuses = BTreeSet::from(["signed", "signed+notarized", "unsigned", "unavailable"]);
    for artifact in &state.artifacts {
        assert!(
            methods.contains(artifact.method.as_str()),
            "{}",
            artifact.method
        );
        assert!(
            statuses.contains(artifact.status.as_str()),
            "{}",
            artifact.status
        );
        if let Some(signature) = &artifact.updater_signature {
            assert_eq!(signature, &format!("{}.sig", artifact.name));
        }
    }
    assert!(["sigstore-keyless", "none"].contains(&state.checksums.method.as_str()));
    // Every published payload has exactly one record.
    let names: Vec<&str> = state.artifacts.iter().map(|a| a.name.as_str()).collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len());
    state
}

fn assert_round_trips(name: &str, text: &str) {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), text).unwrap();
    let state = xtask::signing::load(file.path()).unwrap();
    assert_eq!(
        serde_json::to_string_pretty(&state).unwrap() + "\n",
        text,
        "{name} does not survive load and re-serialization"
    );
}

#[test]
fn signing_state_of_a_signed_tag_is_stable_and_round_trips() {
    let run = assemble(true);
    let text = run.read("signing-state.json");
    assert_golden("signing-state.signed.json", &text);
    let state = check_signing_state_schema(&text);
    assert!(state.checksums.signed && state.updater.available);
    let by_name = |name: &str| {
        state
            .artifacts
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    assert_eq!(
        by_name(&format!("Cuotai_{VERSION}_universal.dmg")).method,
        "developer-id"
    );
    assert_eq!(
        by_name(&format!("Cuotai_{VERSION}_x64-setup.exe")).method,
        "authenticode"
    );
    assert!(by_name(&format!("cuotai-{VERSION}-x86_64-unknown-linux-gnu.tar.gz")).signed);
    assert_round_trips(
        "signing-state.signed.json",
        &golden("signing-state.signed.json"),
    );
}

#[test]
fn signing_state_of_a_rehearsal_is_stable_and_round_trips() {
    let run = assemble(false);
    let text = run.read("signing-state.json");
    assert_golden("signing-state.rehearsal.json", &text);
    let state = check_signing_state_schema(&text);
    assert!(!state.checksums.signed && !state.updater.available);
    assert!(state.updater.reason.is_some());
    assert!(state.artifacts.iter().all(|a| !a.signed));
    assert_round_trips(
        "signing-state.rehearsal.json",
        &golden("signing-state.rehearsal.json"),
    );
}

#[test]
fn signing_state_schema_rejects_drift() {
    let text = golden("signing-state.signed.json");
    let file = tempfile::NamedTempFile::new().unwrap();
    // A new top-level key, a renamed field and a schema bump all fail to load.
    for (from, to) in [
        ("\"schema\": 2,", "\"schema\": 2,\n  \"extra\": true,"),
        ("\"updater_signature\"", "\"updater_sig\""),
        ("\"schema\": 2,", "\"schema\": 3,"),
    ] {
        let drifted = text.replacen(from, to, 1);
        assert_ne!(drifted, text, "fixture lost {from}");
        std::fs::write(file.path(), &drifted).unwrap();
        assert!(xtask::signing::load(file.path()).is_err(), "accepted {to}");
    }
}

/// The multi-platform manifest `tauri-plugin-updater` 2 reads from
/// `releases/latest/download/latest.json` (PRD 0018 RF1.3): a semver
/// `version`, `notes`, an RFC 3339 `pub_date` and `platforms` keyed
/// `{os}-{arch}[-{installer}]`, each with exactly `signature` and `url`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct LatestSchema {
    version: String,
    notes: String,
    pub_date: String,
    platforms: BTreeMap<String, PlatformSchema>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformSchema {
    signature: String,
    url: String,
}

#[test]
fn latest_json_is_stable_and_matches_the_updater_contract() {
    let run = assemble(true);
    let text = run.read("latest.json");
    assert_golden("latest.json", &text);

    let latest: LatestSchema = serde_json::from_str(&text).unwrap();
    semver::Version::parse(&latest.version).unwrap();
    assert_eq!(latest.version, VERSION);
    assert_eq!(latest.pub_date, PUB_DATE);
    assert!(!latest.notes.is_empty());
    let keys: BTreeSet<&str> = latest.platforms.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "darwin-aarch64",
            "darwin-aarch64-app",
            "darwin-x86_64",
            "darwin-x86_64-app",
            "linux-x86_64",
            "linux-x86_64-appimage",
            "linux-x86_64-deb",
            "linux-x86_64-rpm",
            "windows-x86_64",
            "windows-x86_64-msi",
            "windows-x86_64-nsis",
        ])
    );
    let prefix = format!("https://github.com/{REPO}/releases/download/{TAG}/");
    for (key, platform) in &latest.platforms {
        let asset = platform
            .url
            .strip_prefix(&prefix)
            .unwrap_or_else(|| panic!("{key}: {} is not a release asset URL", platform.url));
        // The URL names an asset of this release and the signature is that
        // asset's detached `.sig`, byte for byte (trimmed).
        let signature = std::fs::read_to_string(run.dist.join(format!("{asset}.sig"))).unwrap();
        assert_eq!(platform.signature, signature.trim(), "{key}");
        assert!(run.read("SHA256SUMS").contains(&format!("  {asset}\n")));
    }
    // The generic fallback of each architecture is the preferred installer.
    assert_eq!(
        latest.platforms["windows-x86_64"].url,
        latest.platforms["windows-x86_64-nsis"].url
    );
    assert_eq!(
        latest.platforms["linux-x86_64"].url,
        latest.platforms["linux-x86_64-appimage"].url
    );

    // Lossless parse and re-serialization of the golden copy.
    let golden_text = golden("latest.json");
    let _: LatestSchema = serde_json::from_str(&golden_text).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(
        serde_json::to_string_pretty(&parsed).unwrap() + "\n",
        golden_text
    );
}
