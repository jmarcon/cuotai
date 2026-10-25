//! PRD 0028 RF4.8: `cuotai --canary` end to end against a fixture provider
//! (a fake AWS CLI, no network, no secret), with one fixture mutated between
//! runs. It asserts the drift detector, the report, the issue draft, the
//! evidence list and `--dry-run`. CI runs it in the `canary-dry-run` job.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A private HOME with a fake `aws` whose answer follows `answer.json`.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cuotai-canary-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        let aws = dir.join("bin/aws");
        fs::write(
            &aws,
            format!("#!/bin/sh\ncat '{}'\n", dir.join("answer.json").display()),
        )
        .unwrap();
        fs::set_permissions(&aws, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            dir.join("cuotai.toml"),
            "[[accounts]]\nname = \"Bedrock\"\nprovider = \"aws_bedrock\"\nparams = { profile = \"dev\" }\n",
        )
        .unwrap();
        Self(dir)
    }

    /// What the fake AWS CLI prints from now on.
    fn answer(&self, json: &str) {
        fs::write(self.0.join("answer.json"), json).unwrap();
    }

    fn canary(&self, extra: &[&str]) -> Output {
        let home = self.0.join("home");
        // The fixture provider is metered (RF4.6): the record of the last
        // paid query would answer the next run. Each run here is a new day.
        // (`data` is XDG_DATA_HOME on Linux, `Library` is macOS's data dir.)
        let _ = fs::remove_dir_all(home.join("data"));
        let _ = fs::remove_dir_all(home.join("Library"));
        let path = format!(
            "{}:{}",
            self.0.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .arg("--canary")
            .arg("--canary-include-metered")
            .arg("--config")
            .arg(self.0.join("cuotai.toml"))
            .arg("--out")
            .arg(self.out())
            .args(extra)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("PATH", path)
            .env("CUOTAI_LANG", "en-US")
            .env_remove("CUOTAI_CONFIG")
            .env_remove("CUOTAI_UPDATE_CHECK")
            .stdin(Stdio::null())
            .output()
            .expect("run cuotai --canary")
    }

    fn out(&self) -> PathBuf {
        self.0.join("canary")
    }

    fn runs(&self) -> usize {
        fs::read_dir(self.out()).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("run-"))
                .count()
        })
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

const GOOD: &str = r#"{"ResultsByTime":[{"Total":{"UnblendedCost":{"Amount":"12.34","Unit":"USD"}},"Estimated":false}],"DimensionValueAttributes":[]}"#;
/// Still decodes, but a field Cuotai does not read is gone.
const FIELD_GONE: &str = r#"{"ResultsByTime":[{"Total":{"UnblendedCost":{"Amount":"12.34","Unit":"USD"}},"Estimated":false}]}"#;
/// No longer decodes: the cost moved to another key.
const RENAMED: &str = r#"{"ResultsByTime":[{"Total":{"Cost":{"Amount":"12.34","Unit":"USD"}},"Estimated":false}],"DimensionValueAttributes":[]}"#;

#[test]
fn drift_report_draft_and_evidence_follow_the_mutated_fixture() {
    let sandbox = Sandbox::new("drift");

    // A dry run computes and prints but writes nothing.
    sandbox.answer(GOOD);
    let dry = sandbox.canary(&["--dry-run"]);
    assert_eq!(dry.status.code(), Some(0), "{dry:?}");
    assert!(text(&dry).contains("First run"));
    assert_eq!(sandbox.runs(), 0, "--dry-run wrote a record");

    // First real run: nothing to compare with, the interface is evidence.
    let first = sandbox.canary(&[]);
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    assert_eq!(sandbox.runs(), 1);
    let verified = read(&sandbox.out().join("verified_on.json"));
    assert!(verified.contains("aws_bedrock"), "{verified}");

    // Same answer: no alert, still evidence.
    sandbox.answer(GOOD);
    let same = sandbox.canary(&["--dry-run"]);
    assert_eq!(same.status.code(), Some(0), "{same:?}");
    assert!(text(&same).contains("None."));

    // RF4.1/RF4.2: a field Cuotai never reads vanished from a good answer.
    sandbox.answer(FIELD_GONE);
    let gone = sandbox.canary(&["--dry-run"]);
    assert_eq!(gone.status.code(), Some(2), "{gone:?}");
    let report = text(&gone);
    assert!(
        report.contains("field missing: DimensionValueAttributes"),
        "{report}"
    );
    assert!(report.contains("Issue draft"), "{report}");

    // A decode failure: drift, a draft, no evidence, and no value anywhere.
    sandbox.answer(RENAMED);
    let broken = sandbox.canary(&[]);
    assert_eq!(broken.status.code(), Some(2), "{broken:?}");
    let report = text(&broken);
    assert!(report.contains("schema_changed"), "{report}");
    assert!(report.contains("Issue draft"), "{report}");
    assert!(!report.contains("12.34"), "a value reached the report");
    assert!(
        !read(&sandbox.out().join("verified_on.json")).contains("aws_bedrock"),
        "a drifted interface is not evidence"
    );
    for entry in fs::read_dir(sandbox.out()).unwrap().flatten() {
        let content = read(&entry.path());
        assert!(
            !content.contains("12.34"),
            "{:?} holds a value",
            entry.path()
        );
        assert!(
            !content.contains("Bedrock"),
            "{:?} holds the account name",
            entry.path()
        );
    }
}
