//! PRD 0017 RF4: an extension is born disabled; approval persists the
//! SHA-256 of the manifest and the program and the approved `env` names
//! atomically; a change to either file disables it until a new approval.

use super::*;
use crate::extensions::discover;
use std::fs;

const MANIFEST: &str = r#"
id = "proxy"
name = "Proxy balance"
version = "1.2.0"
schema = 1
command = ["./run.sh"]
interval_seconds = 300
env = ["PROXY_TOKEN", "PROXY_URL"]
"#;

/// A sandbox: the extensions `root` with one extension and its `consent`.
struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    consent: Consent,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("extensions");
        Self::with_manifest(&root, "proxy", MANIFEST);
        let consent = Consent::at(dir.path().join("consent.json"));
        Self {
            _dir: dir,
            root,
            consent,
        }
    }

    /// A manifest and its program under `root/name`.
    fn with_manifest(root: &Path, name: &str, manifest: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        let program = dir.join("run.sh");
        fs::write(&program, "#!/bin/sh\nprintf '{}'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn manifest(&self) -> Manifest {
        let discovery = discover(&self.root);
        assert!(discovery.rejected.is_empty(), "{:?}", discovery.rejected);
        discovery.get("proxy").expect("discovered").clone()
    }
}

/// The stored record of `rig`, as written on disk.
fn stored(rig: &Rig) -> serde_json::Value {
    let text = fs::read_to_string(&rig.consent.file).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn an_extension_is_born_disabled() {
    let rig = Rig::new();
    let manifest = rig.manifest();
    assert!(rig.consent.check(manifest).is_err(), "never approved");
}

#[test]
fn approval_persists_the_digests_and_the_env_and_enables_the_run() {
    let rig = Rig::new();
    let manifest = rig.manifest();
    let approved = rig.consent.approve(&manifest).unwrap();
    assert_eq!(approved.env(), &["PROXY_TOKEN", "PROXY_URL"]);
    assert!(approved.verify());

    let record = stored(&rig);
    let entry = &record["approved"]["proxy"];
    assert_eq!(entry["manifest_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(entry["program_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        entry["env"],
        serde_json::json!(["PROXY_TOKEN", "PROXY_URL"])
    );

    // A fresh Consent over the same file sees the approval.
    let again = Consent::at(rig.consent.file.clone());
    let approved = again
        .check(rig.manifest())
        .map_err(|_| "still approved")
        .unwrap();
    assert_eq!(approved.env(), &["PROXY_TOKEN", "PROXY_URL"]);
}

#[test]
fn a_changed_program_disables_until_a_new_approval() {
    let rig = Rig::new();
    rig.consent.approve(&rig.manifest()).unwrap();

    let manifest = rig.manifest();
    fs::write(
        manifest.dir.join("run.sh"),
        "#!/bin/sh\nprintf '{\"schema\":1}'\n",
    )
    .unwrap();
    assert!(
        rig.consent.check(rig.manifest()).is_err(),
        "changed program"
    );
    assert!(
        rig.consent.approve(&rig.manifest()).is_ok(),
        "approved again"
    );
    assert!(rig.consent.check(rig.manifest()).is_ok());
}

#[test]
fn a_changed_manifest_disables_until_a_new_approval() {
    let rig = Rig::new();
    rig.consent.approve(&rig.manifest()).unwrap();

    let changed = MANIFEST.replace("interval_seconds = 300", "interval_seconds = 120");
    let manifest = rig.manifest();
    fs::write(manifest.dir.join(MANIFEST_FILE), changed).unwrap();
    assert!(
        rig.consent.check(rig.manifest()).is_err(),
        "changed manifest"
    );
    rig.consent.approve(&rig.manifest()).unwrap();
    assert!(rig.consent.check(rig.manifest()).is_ok());
}

#[test]
fn a_damaged_record_holds_no_approval() {
    let rig = Rig::new();
    rig.consent.approve(&rig.manifest()).unwrap();
    fs::write(&rig.consent.file, b"not json").unwrap();
    assert!(rig.consent.check(rig.manifest()).is_err());
}

#[test]
fn the_approved_env_never_exceeds_what_the_manifest_asks() {
    let rig = Rig::new();
    rig.consent.approve(&rig.manifest()).unwrap();
    // A record crafted with a name the manifest never asked for.
    let mut record: serde_json::Value = stored(&rig);
    record["approved"]["proxy"]["env"] = serde_json::json!(["PROXY_TOKEN", "SNEAKY"]);
    fs::write(&rig.consent.file, serde_json::to_vec(&record).unwrap()).unwrap();
    let approved = rig
        .consent
        .check(rig.manifest())
        .map_err(|_| "approved")
        .unwrap();
    assert_eq!(approved.env(), &["PROXY_TOKEN"]);
}

#[test]
fn the_disclosure_is_deterministic_and_complete() {
    let rig = Rig::new();
    let manifest = rig.manifest();
    let once = disclosure(&manifest);
    assert_eq!(once, disclosure(&manifest), "deterministic");
    let text: Vec<String> = once.iter().map(|line| line.text().to_string()).collect();
    let joined = text.join("\n");
    assert!(joined.contains("proxy"), "{joined}");
    assert!(joined.contains("Proxy balance 1.2.0"), "{joined}");
    assert!(joined.contains(r#"comando: ["./run.sh"]"#), "{joined}");
    assert!(
        joined.contains(&manifest.dir.display().to_string()),
        "{joined}"
    );
    assert!(joined.contains("PROXY_TOKEN, PROXY_URL"), "{joined}");
    assert!(joined.contains("300 s"), "{joined}");
    assert!(joined.contains("15 s"), "{joined}");
    assert!(joined.contains("revise o código"), "{joined}");

    let quiet = MANIFEST
        .replace("env = [\"PROXY_TOKEN\", \"PROXY_URL\"]\n", "")
        .replace("id = \"proxy\"", "id = \"quiet\"");
    Rig::with_manifest(&rig.root, "quiet", &quiet);
    let discovery = discover(&rig.root);
    let quiet = discovery.get("quiet").unwrap();
    let text: Vec<String> = disclosure(quiet)
        .iter()
        .map(|line| line.text().to_string())
        .collect();
    assert!(
        text.iter()
            .any(|line| line.contains("não pede nenhuma variável")),
        "{text:?}"
    );
}
