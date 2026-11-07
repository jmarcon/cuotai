//! PRD 0051 RF6.4: `cargo xtask compat-check`.
//!
//! The compatibility fixtures live in
//! `crates/cuotai-core/tests/fixtures/compat/<contract>/major-<M>-revision-<R>/`,
//! each with a `manifest.json`. This check, which needs no network:
//!
//! - refuses a fixture directory without a manifest, a data file the
//!   manifest does not list, or a listed file whose SHA-256 differs;
//! - refuses a manifest whose directory name, `expected` value or `.bak`
//!   expectation (`backup` exactly for `older`) is wrong, or whose frozen
//!   `reader` is not dispatched by the core's compatibility matrix;
//! - with a base revision, refuses any change to or removal of a fixture
//!   file that exists at the base: shipped fixtures are immutable, a new
//!   revision is a new directory.
//!
//! The matrix itself (`cuotai-core`'s `compat_matrix` test) runs with the
//! workspace tests on Linux, macOS and Windows; `--run-matrix` runs it here.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{err, Res};

/// The fixtures, relative to the repository root.
pub const FIXTURES: &str = "crates/cuotai-core/tests/fixtures/compat";
/// The core module that dispatches every frozen reader.
pub const MATRIX: &str = "crates/cuotai-core/src/compat_tests.rs";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[allow(dead_code)]
    contract: String,
    major: u32,
    revision: u32,
    writer: String,
    expected: String,
    reader: String,
    #[serde(default)]
    backup: bool,
    files: BTreeMap<String, String>,
}

/// Check every manifest under `root` against the matrix source `matrix`.
/// Returns how many fixtures were checked.
pub fn check(root: &Path, matrix: &str) -> Res<usize> {
    let mut problems = Vec::new();
    let mut count = 0;
    let mut contracts: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
    contracts.sort_by_key(|entry| entry.path());
    for contract in contracts {
        let contract = contract.path();
        if !contract.is_dir() {
            problems.push(format!(
                "{}: a file outside a fixture directory",
                contract.display()
            ));
            continue;
        }
        let mut fixtures: Vec<_> = std::fs::read_dir(&contract)?.collect::<Result<_, _>>()?;
        fixtures.sort_by_key(|entry| entry.path());
        for fixture in fixtures {
            let dir = fixture.path();
            count += 1;
            if let Err(problem) = check_fixture(&dir, matrix) {
                problems.push(format!("{}: {problem}", dir.display()));
            }
        }
    }
    if problems.is_empty() {
        Ok(count)
    } else {
        err(format!("compatibility fixtures:\n{}", problems.join("\n")))
    }
}

fn check_fixture(dir: &Path, matrix: &str) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("a file outside a fixture directory".into());
    }
    let text = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|_| "undeclared fixture: no manifest.json".to_owned())?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| format!("invalid manifest.json: {e}"))?;
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    let want = format!("major-{}-revision-{}", manifest.major, manifest.revision);
    if name != want {
        return Err(format!("the directory should be named {want}"));
    }
    if manifest.writer.trim().is_empty() {
        return Err("no writer release".into());
    }
    if !["current", "newer_revision", "newer_major", "older"].contains(&manifest.expected.as_str())
    {
        return Err(format!("unknown expected value {}", manifest.expected));
    }
    if manifest.backup != (manifest.expected == "older") {
        return Err("a schema-changing (older) fixture needs backup: true, and only it".into());
    }
    if !matrix.contains(&format!("\"{}\" =>", manifest.reader)) {
        return Err(format!(
            "the frozen reader {} is not in the matrix",
            manifest.reader
        ));
    }
    let mut on_disk = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name != "manifest.json" {
            on_disk.push(name);
        }
    }
    on_disk.sort();
    let listed: Vec<String> = manifest.files.keys().cloned().collect();
    if on_disk != listed {
        return Err(format!(
            "undeclared or missing files: on disk {on_disk:?}, listed {listed:?}"
        ));
    }
    for (file, hash) in &manifest.files {
        let bytes = std::fs::read(dir.join(file)).map_err(|e| e.to_string())?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if &actual != hash {
            return Err(format!("{file} changed (sha256 {actual}, manifest {hash})"));
        }
    }
    Ok(())
}

/// The fixture files changed or removed since `base` (`git diff
/// --name-status`): a shipped fixture is never edited.
pub fn changed_since(repo: &Path, base: &str) -> Res<Vec<String>> {
    let out = crate::git::run(
        repo,
        &[
            "diff",
            "--name-status",
            "--no-renames",
            base,
            "--",
            FIXTURES,
        ],
    )?;
    Ok(out
        .lines()
        .filter(|line| !line.starts_with('A'))
        .filter(|line| in_fixture_dir(line))
        .map(str::to_owned)
        .collect())
}

/// Whether a `git diff --name-status` line names a file inside a
/// `major-<M>-revision-<R>` directory: files from before the layout (the
/// 0.83.0 `config/proposed.toml`) were never under this rule.
fn in_fixture_dir(line: &str) -> bool {
    line.split('/').any(|part| {
        part.strip_prefix("major-")
            .and_then(|rest| rest.split_once("-revision-"))
            .is_some_and(|(major, revision)| {
                !major.is_empty()
                    && !revision.is_empty()
                    && major.bytes().all(|b| b.is_ascii_digit())
                    && revision.bytes().all(|b| b.is_ascii_digit())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATRIX_SOURCE: &str = "\"n_minus_1::thing\" => check(),";

    fn fixture(root: &Path, data: &[u8], expected: &str, backup: bool) -> std::path::PathBuf {
        let dir = root.join("thing.json").join("major-1-revision-0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("thing.json"), data).unwrap();
        let manifest = serde_json::json!({
            "contract": "thing.json",
            "major": 1,
            "revision": 0,
            "writer": "vNEXT",
            "expected": expected,
            "reader": "n_minus_1::thing",
            "backup": backup,
            "files": {"thing.json": format!("{:x}", Sha256::digest(data))},
        });
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        dir
    }

    #[test]
    fn a_declared_fixture_passes() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), b"{}", "current", false);
        assert_eq!(check(root.path(), MATRIX_SOURCE).unwrap(), 1);
    }

    #[test]
    fn an_undeclared_fixture_or_file_fails() {
        let root = tempfile::tempdir().unwrap();
        let dir = fixture(root.path(), b"{}", "current", false);
        std::fs::write(dir.join("extra.json"), b"{}").unwrap();
        assert!(check(root.path(), MATRIX_SOURCE).is_err());
        std::fs::remove_file(dir.join("extra.json")).unwrap();
        std::fs::create_dir_all(root.path().join("thing.json").join("major-1-revision-1")).unwrap();
        let error = check(root.path(), MATRIX_SOURCE).unwrap_err().to_string();
        assert!(error.contains("no manifest.json"), "{error}");
    }

    #[test]
    fn a_mutated_fixture_fails() {
        let root = tempfile::tempdir().unwrap();
        let dir = fixture(root.path(), b"{}", "current", false);
        std::fs::write(dir.join("thing.json"), b"{\"x\":1}").unwrap();
        let error = check(root.path(), MATRIX_SOURCE).unwrap_err().to_string();
        assert!(error.contains("changed"), "{error}");
    }

    #[test]
    fn an_older_fixture_needs_its_backup_and_a_reader() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), b"{}", "older", false);
        assert!(check(root.path(), MATRIX_SOURCE).is_err());
        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), b"{}", "older", true);
        assert!(check(root.path(), MATRIX_SOURCE).is_ok());
        assert!(check(root.path(), "no reader here").is_err());
    }

    #[test]
    fn only_files_of_fixture_directories_are_immutable() {
        assert!(in_fixture_dir(
            "M\tcrates/cuotai-core/tests/fixtures/compat/x/major-1-revision-0/x.json"
        ));
        assert!(!in_fixture_dir(
            "D\tcrates/cuotai-core/tests/fixtures/compat/config/proposed.toml"
        ));
        assert!(!in_fixture_dir("M\tcompat/x/major-a-revision-0/x"));
    }

    /// The repository's own fixtures pass.
    #[test]
    fn the_repository_fixtures_pass() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let matrix = std::fs::read_to_string(repo.join(MATRIX)).unwrap();
        assert!(check(&repo.join(FIXTURES), &matrix).unwrap() >= 20);
    }
}
