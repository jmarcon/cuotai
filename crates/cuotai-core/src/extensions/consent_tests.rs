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

/// RF4.5: an approval issued before a revocation can never be admitted,
/// while an approval taken after it can. The guard makes the decision and
/// the process spawn atomic with respect to `revoke`.
#[test]
fn a_revocation_invalidates_the_approvals_issued_before_it() {
    let rig = Rig::new();
    let approved = rig.consent.approve(&rig.manifest()).unwrap();
    {
        let guard = approved
            .admit(rig.consent.shared())
            .expect("an approved run may start");
        drop(guard);
    }

    assert!(rig.consent.revoke("proxy").unwrap(), "revoked");
    // Re-approving and revoking again: the second approval is older than
    // the current generation once the revoke lands.
    let approved = rig.consent.approve(&rig.manifest()).unwrap();
    assert!(rig.consent.revoke("proxy").unwrap());
    assert!(
        approved.admit(rig.consent.shared()).is_none(),
        "an approval revoked after it was issued never starts a process"
    );

    let fresh = rig.consent.approve(&rig.manifest()).unwrap();
    assert!(
        fresh.admit(rig.consent.shared()).is_some(),
        "a new approval admits"
    );
}

/// RF4.5: the generation is per extension id, so revoking one leaves
/// another's approval valid.
#[test]
fn revoking_one_extension_leaves_another_approved() {
    let rig = Rig::new();
    Rig::with_manifest(&rig.root, "other", &MANIFEST.replace("proxy", "other"));
    let mine = rig.consent.approve(&rig.manifest()).unwrap();
    let theirs = rig
        .consent
        .approve(&discover(&rig.root).get("other").unwrap().clone())
        .unwrap();

    rig.consent.revoke("other").unwrap();
    assert!(
        mine.admit(rig.consent.shared()).is_some(),
        "the other extension's revocation does not touch this approval"
    );
    assert!(theirs.admit(rig.consent.shared()).is_none());
    assert!(!rig.consent.has_approval("other"));
    assert!(rig.consent.has_approval("proxy"));
}

/// RF4.5: clones share one revocation state, so a frontend and a rebuilt
/// monitor agree on whether an approval is still valid.
#[test]
fn clones_share_one_revocation_state() {
    let rig = Rig::new();
    let other = rig.consent.clone();
    let approved = rig.consent.approve(&rig.manifest()).unwrap();
    assert!(other.check(rig.manifest()).is_ok());
    other.revoke("proxy").unwrap();
    assert!(
        approved.admit(other.shared()).is_none(),
        "a revocation through any clone invalidates the approval"
    );
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
    assert!(joined.contains(r#"command: ["./run.sh"]"#), "{joined}");
    assert!(
        joined.contains(&manifest.dir.display().to_string()),
        "{joined}"
    );
    assert!(joined.contains("PROXY_TOKEN, PROXY_URL"), "{joined}");
    assert!(joined.contains("300 s"), "{joined}");
    assert!(joined.contains("15 s"), "{joined}");
    assert!(joined.contains("review the code"), "{joined}");

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
            .any(|line| line.contains("asks for no environment variable")),
        "{text:?}"
    );
}

/// FIX-04: the digest covers the whole folder, so any file beside the
/// program — changed, added (hidden ones too) or removed — disables the
/// extension until a new approval.
#[test]
fn any_file_of_the_folder_disables_until_a_new_approval() {
    let rig = Rig::new();
    let dir = rig.manifest().dir;
    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::write(dir.join("lib").join("helper.sh"), "echo one\n").unwrap();
    rig.consent.approve(&rig.manifest()).unwrap();
    assert!(rig.consent.check(rig.manifest()).is_ok());

    fs::write(dir.join("lib").join("helper.sh"), "echo two\n").unwrap();
    assert_eq!(rig.consent.status(&rig.manifest()), Status::Changed);
    rig.consent.approve(&rig.manifest()).unwrap();

    fs::write(dir.join(".hidden"), "x").unwrap();
    assert!(rig.consent.check(rig.manifest()).is_err(), "an added file");
    rig.consent.approve(&rig.manifest()).unwrap();

    fs::remove_file(dir.join(".hidden")).unwrap();
    assert!(rig.consent.check(rig.manifest()).is_err(), "a removed file");
    rig.consent.approve(&rig.manifest()).unwrap();
    assert!(rig.consent.check(rig.manifest()).is_ok());
}

/// FIX-04: the digest is stable (the same folder hashes the same, whatever
/// the order its entries were created in).
#[test]
fn the_folder_digest_is_stable() {
    let one = tempfile::tempdir().unwrap();
    let two = tempfile::tempdir().unwrap();
    for (dir, order) in [(one.path(), ["a", "b", "c"]), (two.path(), ["c", "a", "b"])] {
        for name in order {
            fs::write(dir.join(name), name).unwrap();
        }
    }
    let one = crate::extensions::tree::scan(&one.path().canonicalize().unwrap()).unwrap();
    let two = crate::extensions::tree::scan(&two.path().canonicalize().unwrap()).unwrap();
    assert_eq!(one, two);
    assert_eq!(one.len(), 64);
}

/// FIX-04: a record written before the folder digest existed (no
/// `tree_sha256`) reads, but never matches: the extension shows as changed
/// and asks for a new approval.
#[test]
fn an_approval_from_before_the_folder_digest_asks_again() {
    let rig = Rig::new();
    rig.consent.approve(&rig.manifest()).unwrap();
    let mut record = stored(&rig);
    let entry = record["approved"]["proxy"].as_object_mut().unwrap();
    assert_eq!(entry["tree_sha256"].as_str().unwrap().len(), 64);
    entry.remove("tree_sha256");
    fs::write(&rig.consent.file, serde_json::to_vec(&record).unwrap()).unwrap();

    assert!(rig.consent.has_approval("proxy"));
    assert_eq!(rig.consent.status(&rig.manifest()), Status::Changed);
    assert!(rig.consent.check(rig.manifest()).is_err());
    rig.consent.approve(&rig.manifest()).unwrap();
    assert_eq!(rig.consent.status(&rig.manifest()), Status::Approved);
}

/// FIX-04: the disclosure names the interpreter a script starts, and the
/// approval binds to it: a program whose `#!` line changed after the
/// disclosure cannot be approved with it.
#[test]
fn the_disclosure_shows_the_interpreter_and_the_approval_binds_to_it() {
    let rig = Rig::new();
    let manifest = rig.manifest();
    assert_eq!(manifest.interpreter.as_deref(), Some("/bin/sh"));
    let lines = disclosure(&manifest);
    let line = lines
        .iter()
        .find(|line| line.id.as_ref() == "extensions.consent.interpreter")
        .expect("an interpreter line");
    assert!(line.text().contains("/bin/sh"), "{}", line.text());
    assert!(line.resolve("en-US").contains("/bin/sh"));

    fs::write(
        manifest.dir.join("run.sh"),
        "#!/usr/bin/env python3\nprint('{}')\n",
    )
    .unwrap();
    assert!(
        rig.consent.approve(&manifest).is_err(),
        "a stale disclosure"
    );
    let fresh = rig.manifest();
    assert_eq!(fresh.interpreter.as_deref(), Some("/usr/bin/env python3"));
    assert!(rig.consent.approve(&fresh).is_ok());
}

/// A native program (no `#!`) has no interpreter line.
#[test]
fn a_native_program_has_no_interpreter_line() {
    let rig = Rig::new();
    let manifest = rig.manifest();
    fs::write(manifest.dir.join("run.sh"), b"\x7fELF\x02\x01").unwrap();
    let manifest = rig.manifest();
    assert_eq!(manifest.interpreter, None);
    assert!(disclosure(&manifest)
        .iter()
        .all(|line| line.id.as_ref() != "extensions.consent.interpreter"));
}

/// FIX-03: the approvals live outside the extensions' directory, in
/// `<parent>/consent/extensions.json`.
#[test]
fn the_approvals_live_beside_the_extensions_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("extensions");
    assert_eq!(
        Consent::file_for(&root),
        dir.path().join("consent").join("extensions.json")
    );
    if let Some(default) = crate::extensions::default_dir() {
        assert!(Consent::file_for(&default)
            .ends_with(Path::new("cuotai").join("consent").join("extensions.json")));
    }
}

/// FIX-03: a legacy `<root>/consent.json` moves once to the new file and
/// is removed; a new file that already exists wins; a damaged legacy file
/// is left alone and approves nothing.
#[test]
fn the_legacy_record_moves_once_to_the_private_file() {
    let rig = Rig::new();
    rig.consent.approve(&rig.manifest()).unwrap();
    let legacy = rig.root.join(LEGACY_CONSENT_FILE);
    fs::copy(&rig.consent.file, &legacy).unwrap();

    let moved = Consent::for_root(&rig.root);
    assert!(!legacy.exists(), "the legacy file is removed");
    assert!(moved.file().exists());
    assert!(moved.has_approval("proxy"));
    assert_eq!(moved.status(&rig.manifest()), Status::Approved);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(moved.file()), 0o600);
        assert_eq!(mode(moved.file().parent().unwrap()), 0o700);
    }

    // The new file wins over a legacy one that reappears.
    fs::write(&legacy, br#"{"approved":{}}"#).unwrap();
    let again = Consent::for_root(&rig.root);
    assert!(!legacy.exists());
    assert!(again.has_approval("proxy"), "the new record was kept");

    // A damaged legacy file with no new record is left alone.
    fs::remove_file(again.file()).unwrap();
    fs::write(&legacy, b"not json").unwrap();
    let damaged = Consent::for_root(&rig.root);
    assert!(legacy.exists());
    assert!(!damaged.file().exists());
    assert!(!damaged.has_approval("proxy"));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn rejected_id(rig: &Rig) -> String {
        let discovery = discover(&rig.root);
        assert!(discovery.get("proxy").is_none());
        discovery.rejected[0].reason.id.to_string()
    }

    /// FIX-04: a link inside the folder is hashed by its text, not followed;
    /// one leading out of the folder (or nowhere) is refused.
    #[test]
    fn links_must_stay_inside_the_folder() {
        let rig = Rig::new();
        let dir = rig.manifest().dir;
        std::os::unix::fs::symlink("run.sh", dir.join("alias")).unwrap();
        rig.consent.approve(&rig.manifest()).unwrap();
        assert!(rig.consent.check(rig.manifest()).is_ok());

        fs::remove_file(dir.join("alias")).unwrap();
        std::os::unix::fs::symlink(MANIFEST_FILE, dir.join("alias")).unwrap();
        assert!(
            rig.consent.check(rig.manifest()).is_err(),
            "a retargeted link"
        );

        fs::remove_file(dir.join("alias")).unwrap();
        let outside = rig._dir.path().join("outside.sh");
        fs::write(&outside, "echo out\n").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("alias")).unwrap();
        assert_eq!(rejected_id(&rig), "extensions.error.tree_escapes");

        fs::remove_file(dir.join("alias")).unwrap();
        std::os::unix::fs::symlink("missing", dir.join("alias")).unwrap();
        assert_eq!(rejected_id(&rig), "extensions.error.tree_escapes");
    }

    /// FIX-05: a folder (or anything in it) writable by the group or by
    /// others is refused at discovery, and an approved one that becomes so
    /// no longer verifies, so no run starts over files another user could
    /// swap.
    #[test]
    fn a_folder_others_can_write_is_refused() {
        let rig = Rig::new();
        let manifest = rig.manifest();
        let approved = rig.consent.approve(&manifest).unwrap();
        assert!(approved.verify());

        fs::set_permissions(&manifest.dir, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(!approved.verify(), "the run would be refused");
        assert_eq!(rejected_id(&rig), "extensions.error.tree_exposed");

        fs::set_permissions(&manifest.dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(
            manifest.dir.join(MANIFEST_FILE),
            fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        assert!(!approved.verify());
        assert_eq!(rejected_id(&rig), "extensions.error.tree_exposed");

        fs::set_permissions(
            manifest.dir.join(MANIFEST_FILE),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(approved.verify(), "fixed permissions verify again");
    }

    /// FIX-04: special files (here a FIFO, which would block a read) are
    /// refused, never opened.
    #[test]
    fn a_special_file_is_refused() {
        let rig = Rig::new();
        let dir = rig.manifest().dir;
        let fifo = std::ffi::CString::new(dir.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(rejected_id(&rig), "extensions.error.tree_escapes");
    }
}
