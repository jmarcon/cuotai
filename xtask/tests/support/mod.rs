//! A throwaway repository with both workspaces, both lockfiles, the
//! changelog, a plan and the plan control at an internal version, shared by
//! the commit-gate and release tests (PRD 0039 RF2/RF4/RF5).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub const VERSION: &str = "0.79.0";
pub const DATE: &str = "2026-10-24";

pub fn write_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

pub fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-10-21T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-10-21T00:00:00Z")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub const PLAN: &str = "\
# PRD 0001 — Core

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.79.0 | Base. |
| RF2 | — | Proposed. |

- **RF1** Base.
- **RF2** Sorting.
- **RF3** Filtering.
";

pub const CONTROL: &str = "\
# Plan control (PRDs)

| Field | Value |
|---|---|
| Last updated | 2026-10-20 |
| Cuotai version at that date | 0.79.0 |

## 3. Delivered by version

| Version | Plan | Requirements delivered |
|---|---|---|
| 0.79.0 | 0001 | RF1 (base). |

## 4. Dependency graph

Nothing.
";

pub const CHANGELOG: &str = "\
# Changelog

Preface.

## [Unreleased]

## [0.79.0] - 2026-10-20

### Added

- Base (PRD 0001 RF1; 1111111).

[Unreleased]: https://github.com/o/r/compare/1111111...HEAD
[0.79.0]: https://github.com/o/r/commit/1111111
";

/// [`CHANGELOG`] with the internal versions folded into the pre-release
/// history (owner decision of 2026-10-23, PRD 0039 D11).
pub const PRE_RELEASE_CHANGELOG: &str = "\
# Changelog

Preface.

## [Unreleased]

## Pre-release history (0.1.0–0.79.0, internal builds, never published)

A summary of the internal builds.

### Added

- Base (PRD 0001 RF1).

[Unreleased]: https://github.com/o/r/compare/1111111...HEAD
";

fn write_tree(root: &Path, v: &str) {
    let req = v.rsplit_once('.').unwrap().0;
    write_file(
        root,
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [\"crates/cuotai-core\", \"crates/cuotai-tui\"]\nresolver = \"2\"\n\n# keep this comment\n[workspace.package]\nversion = \"{v}\"\nedition = \"2021\"\n"
        ),
    );
    write_file(
        root,
        "Cargo.lock",
        &format!(
            "version = 4\n\n[[package]]\nname = \"cuotai-core\"\nversion = \"{v}\"\n\n[[package]]\nname = \"cuotai-tui\"\nversion = \"{v}\"\ndependencies = [\n \"cuotai-core\",\n]\n"
        ),
    );
    write_file(
        root,
        "crates/cuotai-core/Cargo.toml",
        "[package]\nname = \"cuotai-core\"\nversion.workspace = true\nedition.workspace = true\n",
    );
    write_file(root, "crates/cuotai-core/src/lib.rs", "// v1\n");
    write_file(
        root,
        "crates/cuotai-tui/Cargo.toml",
        &format!(
            "[package]\nname = \"cuotai-tui\"\nversion.workspace = true\nedition.workspace = true\n\n[dependencies]\ncuotai-core = {{ path = \"../cuotai-core\", version = \"{req}\" }}\n"
        ),
    );
    write_file(
        root,
        "apps/desktop/src-tauri/Cargo.toml",
        &format!(
            "[package]\nname = \"cuotai-desktop\"\nversion = \"{v}\"\nedition = \"2021\"\n\n[workspace]\n"
        ),
    );
    write_file(
        root,
        "apps/desktop/src-tauri/Cargo.lock",
        &format!(
            "version = 4\n\n[[package]]\nname = \"cuotai-core\"\nversion = \"{v}\"\n\n[[package]]\nname = \"cuotai-desktop\"\nversion = \"{v}\"\n"
        ),
    );
    write_file(
        root,
        "apps/desktop/src-tauri/tauri.conf.json",
        &format!("{{\n  \"productName\": \"Cuotai\",\n  \"version\": \"{v}\"\n}}\n"),
    );
    write_file(root, "CHANGELOG.md", CHANGELOG);
    write_file(root, "README.md", "# Cuotai\n\nUse `vNEXT` in plans.\n");
    write_file(root, "docs/plans/0001_core.md", PLAN);
    write_file(root, "docs/plans/0000_control.md", CONTROL);
}

/// The fixture repository on `main` with one root commit at 0.79.0.
pub fn repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_path_buf();
    write_tree(&repo, VERSION);
    git(&repo, &["init", "-b", "main", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    git(&repo, &["config", "tag.gpgsign", "false"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "chore: fixture"]);
    (dir, repo)
}

/// Writes `file` and commits it with `message`.
pub fn commit(repo: &Path, message: &str, file: &str, content: &str) -> String {
    write_file(repo, file, content);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
    head(repo)
}

pub fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

pub fn short(repo: &Path, rev: &str) -> String {
    git(repo, &["rev-parse", "--short=7", rev])
        .trim()
        .to_string()
}

/// A feature delivering PRD 0001 RF2 (with `vNEXT` in the plan) and a fix.
pub fn feature_and_fix(repo: &Path) -> (String, String) {
    write_file(
        repo,
        "docs/plans/0001_core.md",
        &PLAN.replace("| RF2 | — | Proposed. |", "| RF2 | vNEXT | Sorting. |"),
    );
    let feat = commit(
        repo,
        "feat(core): sort accounts\n\nBody.\n\nChangelog: You can sort accounts by name.\nRefs: PRD 0001 RF2",
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let fix = commit(
        repo,
        "fix(core): keep the order\n\nChangelog: The order survives a restart.",
        "crates/cuotai-core/src/lib.rs",
        "// v3\n",
    );
    (feat, fix)
}

pub fn options(dry_run: bool, branch: bool) -> xtask::release::prepare::Options {
    xtask::release::prepare::Options {
        version: None,
        date: DATE.to_string(),
        dry_run,
        branch,
        refresh: false,
        verify_lockfiles: false,
    }
}

/// Replaces `file` in the last commit, keeping its message.
pub fn amend(repo: &Path, file: &str, content: &str) {
    write_file(repo, file, content);
    git(repo, &["add", "-A"]);
    git(
        repo,
        &["commit", "-q", "--amend", "--no-edit", "--cleanup=verbatim"],
    );
}
