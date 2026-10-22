//! Behavior-level tests for `release-check` (PRD 0021 RF1 acceptance):
//! mismatched tag/version, non-main ref, lightweight tag, missing or empty
//! changelog section and drifted manifests/lockfiles all fail; stable vs rc
//! classification works.

use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "0.68.0";

fn write_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A changelog whose first released section is `version`, followed by a
/// strictly descending history (the shared parser's format, PRD 0019
/// RF7.4). `body` is the first section's content.
fn changelog(version: &str, body: &str) -> String {
    format!(
        "# Changelog\n\n## [Unreleased]\n\n## [{version}] - 2026-10-21\n\n{body}\n\n## [0.60.0] - 2026-10-20\n\n### Fixed\n\n- Earlier thing.\n"
    )
}

/// A consistent repository fixture at `version`.
fn write_repo(root: &Path, version: &str) {
    write_file(
        root,
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [\"crates/cuotai-core\", \"crates/cuotai-tui\"]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"{version}\"\nedition = \"2021\"\n"
        ),
    );
    write_file(
        root,
        "Cargo.lock",
        &format!(
            "version = 4\n\n[[package]]\nname = \"cuotai-core\"\nversion = \"{version}\"\n\n[[package]]\nname = \"cuotai-tui\"\nversion = \"{version}\"\ndependencies = [\"cuotai-core\"]\n"
        ),
    );
    for member in ["cuotai-core", "cuotai-tui"] {
        write_file(
            root,
            &format!("crates/{member}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{member}\"\nversion.workspace = true\nedition.workspace = true\n"
            ),
        );
    }
    write_file(
        root,
        "apps/desktop/src-tauri/Cargo.toml",
        &format!(
            "[package]\nname = \"cuotai-desktop\"\nversion = \"{version}\"\nedition = \"2021\"\n\n[workspace]\n"
        ),
    );
    write_file(
        root,
        "apps/desktop/src-tauri/Cargo.lock",
        &format!(
            "version = 4\n\n[[package]]\nname = \"cuotai-core\"\nversion = \"{version}\"\n\n[[package]]\nname = \"cuotai-desktop\"\nversion = \"{version}\"\n"
        ),
    );
    write_file(
        root,
        "apps/desktop/src-tauri/tauri.conf.json",
        &format!("{{\"productName\": \"Cuotai\", \"version\": \"{version}\"}}\n"),
    );
    write_file(
        root,
        "CHANGELOG.md",
        &changelog(version, "### Added\n\n- Something new (PRD 0001 RF1)."),
    );
}

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    write_repo(dir.path(), VERSION);
    let path = dir.path().to_path_buf();
    (dir, path)
}

fn git(repo: &Path, args: &[&str]) {
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
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Turns the fixture into a git repo with one commit on `main`.
fn git_init(repo: &Path) {
    git(repo, &["init", "-b", "main", "-q"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "fixture"]);
}

#[test]
fn consistent_repo_passes_without_tag() {
    let (_d, repo) = fixture();
    let report = xtask::check::run(&repo, None, None).unwrap();
    assert_eq!(report.version.to_string(), VERSION);
    assert_eq!(report.channel, xtask::Channel::Stable);
    assert_eq!(report.changelog_date, "2026-10-21");
    assert!(report.changelog_body.contains("Something new"));
}

#[test]
fn require_main_without_tag_fails() {
    let (_d, repo) = fixture();
    let err = xtask::check::run(&repo, None, Some("refs/heads/main")).unwrap_err();
    assert!(
        err.to_string().contains("--require-main requires --tag"),
        "{err}"
    );
}

#[test]
fn annotated_tag_on_main_passes() {
    let (_d, repo) = fixture();
    git_init(&repo);
    git(&repo, &["tag", "-a", "v0.68.0", "-m", "release"]);
    let report = xtask::check::run(&repo, Some("v0.68.0"), Some("refs/heads/main")).unwrap();
    assert_eq!(report.tag.as_deref(), Some("v0.68.0"));
    assert_eq!(report.channel, xtask::Channel::Stable);
}

#[test]
fn rc_tag_is_prerelease() {
    let (_d, repo) = fixture();
    git_init(&repo);
    git(&repo, &["tag", "-a", "v0.68.0-rc.1", "-m", "rc"]);
    let report = xtask::check::run(&repo, Some("v0.68.0-rc.1"), Some("refs/heads/main")).unwrap();
    assert_eq!(report.channel, xtask::Channel::Prerelease);
    assert!(report.to_json("v0.68.0").prerelease);
}

#[test]
fn mismatched_tag_version_fails() {
    let (_d, repo) = fixture();
    let err = xtask::check::run(&repo, Some("v0.69.0"), None).unwrap_err();
    assert!(
        err.to_string()
            .contains("differs from the workspace version"),
        "unexpected error: {err}"
    );
}

#[test]
fn bad_tag_format_fails() {
    let (_d, repo) = fixture();
    for bad in ["0.68.0", "v0.68.0-beta", "v0.68", "release-0.68.0"] {
        assert!(
            xtask::check::run(&repo, Some(bad), None).is_err(),
            "accepted bad tag {bad}"
        );
    }
}

#[test]
fn tag_outside_main_fails() {
    let (_d, repo) = fixture();
    git_init(&repo);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("side.txt"), "side").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "side commit"]);
    git(&repo, &["tag", "-a", "v0.68.0", "-m", "release"]);
    git(&repo, &["checkout", "-q", "main"]);
    let err = xtask::check::run(&repo, Some("v0.68.0"), Some("refs/heads/main")).unwrap_err();
    assert!(
        err.to_string().contains("not contained in"),
        "unexpected error: {err}"
    );
}

#[test]
fn lightweight_tag_fails() {
    let (_d, repo) = fixture();
    git_init(&repo);
    git(&repo, &["tag", "v0.68.0"]);
    let err = xtask::check::run(&repo, Some("v0.68.0"), Some("refs/heads/main")).unwrap_err();
    assert!(
        err.to_string().contains("annotated tag"),
        "unexpected error: {err}"
    );
}

#[test]
fn missing_changelog_section_fails() {
    let (_d, repo) = fixture();
    write_file(
        &repo,
        "CHANGELOG.md",
        &changelog("0.65.0", "### Added\n\n- Only old."),
    );
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(
        err.to_string().contains("first released section"),
        "unexpected error: {err}"
    );
}

#[test]
fn newer_first_changelog_section_fails_deterministically() {
    let (_d, repo) = fixture();
    write_file(
        &repo,
        "CHANGELOG.md",
        &format!(
            "# Changelog\n\n## [Unreleased]\n\n## [9.9.9] - 2026-10-22\n\n### Added\n\n- Newer.\n\n## [{VERSION}] - 2026-10-21\n\n### Added\n\n- Stale workspace section.\n"
        ),
    );
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(
        err.to_string().contains("first released section")
            && err.to_string().contains("9.9.9")
            && err.to_string().contains(VERSION),
        "unexpected error: {err}"
    );
}

#[test]
fn empty_changelog_section_fails() {
    let (_d, repo) = fixture();
    write_file(&repo, "CHANGELOG.md", &changelog(VERSION, ""));
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(err.to_string().contains("empty"), "unexpected error: {err}");
}

#[test]
fn header_only_changelog_section_fails() {
    let (_d, repo) = fixture();
    write_file(&repo, "CHANGELOG.md", &changelog(VERSION, "### Added"));
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(err.to_string().contains("empty"), "unexpected error: {err}");
}

#[test]
fn drifted_lockfile_fails() {
    let (_d, repo) = fixture();
    write_file(
        &repo,
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"cuotai-core\"\nversion = \"0.67.0\"\n\n[[package]]\nname = \"cuotai-tui\"\nversion = \"0.68.0\"\n",
    );
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(
        err.to_string().contains("cuotai-core") && err.to_string().contains("Cargo.lock"),
        "unexpected error: {err}"
    );
}

#[test]
fn drifted_desktop_manifest_fails() {
    let (_d, repo) = fixture();
    write_file(
        &repo,
        "apps/desktop/src-tauri/Cargo.toml",
        "[package]\nname = \"cuotai-desktop\"\nversion = \"0.67.0\"\n\n[workspace]\n",
    );
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(
        err.to_string().contains("src-tauri/Cargo.toml"),
        "unexpected error: {err}"
    );
}

#[test]
fn drifted_tauri_conf_fails() {
    let (_d, repo) = fixture();
    write_file(
        &repo,
        "apps/desktop/src-tauri/tauri.conf.json",
        "{\"productName\": \"Cuotai\", \"version\": \"0.67.0\"}\n",
    );
    let err = xtask::check::run(&repo, None, None).unwrap_err();
    assert!(
        err.to_string().contains("tauri.conf.json"),
        "unexpected error: {err}"
    );
}

#[test]
fn several_problems_are_reported_together() {
    let (_d, repo) = fixture();
    write_file(
        &repo,
        "apps/desktop/src-tauri/tauri.conf.json",
        "{\"productName\": \"Cuotai\", \"version\": \"0.67.0\"}\n",
    );
    write_file(
        &repo,
        "CHANGELOG.md",
        &changelog("0.65.0", "### Added\n\n- Old."),
    );
    let err = xtask::check::run(&repo, Some("v0.69.0"), None).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("tauri.conf.json"),
        "missing tauri problem: {text}"
    );
    assert!(
        text.contains("first released section"),
        "missing changelog problem: {text}"
    );
    assert!(
        text.contains("differs from the workspace version"),
        "missing tag problem: {text}"
    );
}

#[test]
fn binary_end_to_end() {
    let (_d, repo) = fixture();
    let bin = env!("CARGO_BIN_EXE_xtask");
    let ok = Command::new(bin)
        .args(["release-check", "--repo"])
        .arg(&repo)
        .args(["--tag", "v0.68.0"])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let bad = Command::new(bin)
        .args(["release-check", "--repo"])
        .arg(&repo)
        .args(["--tag", "v9.9.9"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("differs from the workspace version"));
}
