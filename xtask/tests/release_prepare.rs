//! Temp-repository tests of `release prepare`, `--dry-run`, `--refresh`,
//! `--check` and `release tag` (PRD 0039 RF3–RF6 acceptance).

mod support;

use std::path::Path;

use support::*;
use xtask::release::{self, prepare, tag};

fn prepare_branch(repo: &Path) -> prepare::Outcome {
    prepare::run(repo, &options(false, true))
        .unwrap()
        .expect("something to release")
}

#[test]
fn prepare_writes_the_release_and_a_second_run_has_nothing_to_do() {
    let (_d, repo) = repo();
    let (feat, fix) = feature_and_fix(&repo);
    commit(
        &repo,
        "docs: explain sorting",
        "docs/sorting.md",
        "Sorting.\n",
    );
    let outcome = prepare_branch(&repo);
    assert_eq!(outcome.version.to_string(), "0.80.0");
    assert_eq!(outcome.branch.as_deref(), Some("release/v0.80.0"));
    let (f, x) = (short(&repo, &feat), short(&repo, &fix));

    let changelog = read(&repo, "CHANGELOG.md");
    let expected = format!(
        "## [Unreleased]\n\n## [0.80.0] - {DATE}\n\n### Added\n\n- You can sort accounts by name (PRD 0001 RF2; {f}).\n\n### Fixed\n\n- The order survives a restart ({x}).\n\n## [0.79.0]"
    );
    assert!(changelog.contains(&expected), "{changelog}");
    assert!(changelog.contains(
        "[Unreleased]: https://github.com/o/r/compare/v0.80.0...HEAD\n[0.80.0]: https://github.com/o/r/compare/"
    ));
    xtask::changelog::parse(&changelog).unwrap();

    assert!(read(&repo, "Cargo.toml")
        .contains("# keep this comment\n[workspace.package]\nversion = \"0.80.0\"\n"));
    assert!(read(&repo, "crates/cuotai-tui/Cargo.toml").contains("version = \"0.80\" }"));
    assert!(
        read(&repo, "apps/desktop/src-tauri/tauri.conf.json").contains("\"version\": \"0.80.0\"")
    );
    assert!(read(&repo, "Cargo.lock").contains("name = \"cuotai-tui\"\nversion = \"0.80.0\""));
    assert!(read(&repo, "apps/desktop/src-tauri/Cargo.lock")
        .contains("name = \"cuotai-desktop\"\nversion = \"0.80.0\""));
    assert!(read(&repo, "docs/plans/0001_core.md").contains("| RF2 | 0.80.0 | Sorting. |"));
    assert!(
        read(&repo, "README.md").contains("`vNEXT`"),
        "code spans stay"
    );
    let control = read(&repo, "docs/plans/0000_control.md");
    assert!(control.contains(&format!("| Last updated | {DATE} |")));
    assert!(control.contains("| Cuotai version at that date | 0.80.0 |"));
    assert!(control.contains(&format!(
        "| 0.79.0 | 0001 | RF1 (base). |\n| 0.80.0 | 0001 | RF2 ({f}). |\n"
    )));

    let message = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(message.starts_with("chore(release): 0.80.0\n"), "{message}");
    assert!(message.contains("- minor: "), "{message}");
    assert!(
        message.contains("### Added"),
        "headings survive the commit: {message}"
    );
    assert!(message.contains("Changelog: skip release commit"));
    assert!(git(&repo, &["status", "--porcelain"]).trim().is_empty());

    xtask::check::run(&repo, None, None).unwrap();
    xtask::plan_links::run(&repo, &changelog).unwrap();
    release::check_release(&repo, "HEAD").unwrap();
    // The release PR passes the commit gate too.
    xtask::changelog_gate::run(&repo, "main", "HEAD").unwrap();

    assert!(prepare::run(&repo, &options(true, false))
        .unwrap()
        .is_none());
}

#[test]
fn dry_run_writes_nothing() {
    let (_d, repo) = repo();
    feature_and_fix(&repo);
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "CHANGELOG.md",
        "crates/cuotai-tui/Cargo.toml",
        "apps/desktop/src-tauri/Cargo.toml",
        "apps/desktop/src-tauri/Cargo.lock",
        "apps/desktop/src-tauri/tauri.conf.json",
        "docs/plans/0001_core.md",
        "docs/plans/0000_control.md",
    ];
    let before: Vec<Vec<u8>> = files
        .iter()
        .map(|f| std::fs::read(repo.join(f)).unwrap())
        .collect();
    let outcome = prepare::run(&repo, &options(true, false)).unwrap().unwrap();
    assert_eq!(outcome.version.to_string(), "0.80.0");
    assert_eq!(outcome.changes.len(), files.len());
    let after: Vec<Vec<u8>> = files
        .iter()
        .map(|f| std::fs::read(repo.join(f)).unwrap())
        .collect();
    assert_eq!(before, after);
    assert!(git(&repo, &["status", "--porcelain"]).trim().is_empty());
    let diff = xtask::release::render::unified_diff(
        &outcome.changes[0].path,
        &outcome.changes[0].old,
        &outcome.changes[0].new,
    );
    assert!(diff.contains("+version = \"0.80.0\""), "{diff}");
    // The draft view prints the same section.
    let draft = prepare::draft(&repo, DATE).unwrap();
    assert!(
        draft.starts_with("Next version: 0.80.0 (minor bump from 0.79.0 at "),
        "{draft}"
    );
}

#[test]
fn nothing_to_release_and_overrides() {
    let (_d, repo) = repo();
    commit(&repo, "docs: a note", "docs/note.md", "Note.\n");
    assert!(prepare::run(&repo, &options(true, false))
        .unwrap()
        .is_none());
    let mut opts = options(true, false);
    opts.version = Some(xtask::Version::parse("0.79.0").unwrap());
    let error = prepare::run(&repo, &opts).unwrap_err().to_string();
    assert!(
        error.contains("must be greater than the current"),
        "{error}"
    );
    opts.version = Some(xtask::Version::parse("0.79.1").unwrap());
    let outcome = prepare::run(&repo, &opts).unwrap().unwrap();
    assert!(outcome.section.contains("No user-visible changes."));

    commit(
        &repo,
        "feat: a\n\nChangelog: Something new.",
        "crates/cuotai-core/src/lib.rs",
        "// x\n",
    );
    let error = prepare::run(&repo, &opts).unwrap_err().to_string();
    assert!(error.contains("lower than the computed 0.80.0"), "{error}");
    // A recorded override passes --check.
    opts.version = Some(xtask::Version::parse("1.0.0").unwrap());
    opts.dry_run = false;
    opts.branch = true;
    prepare::run(&repo, &opts).unwrap().unwrap();
    assert!(git(&repo, &["log", "-1", "--format=%B"]).contains("Version-Override: 1.0.0"));
    assert!(read(&repo, "crates/cuotai-tui/Cargo.toml").contains("version = \"1\" }"));
    release::check_release(&repo, "HEAD").unwrap();
}

#[test]
fn refresh_keeps_edited_entries_and_adds_new_commits() {
    let (_d, repo) = repo();
    feature_and_fix(&repo);
    prepare_branch(&repo);
    // The owner rewords the feature entry in the release PR.
    let edited = read(&repo, "CHANGELOG.md").replace(
        "You can sort accounts by name",
        "Accounts can now be sorted by name",
    );
    amend(&repo, "CHANGELOG.md", &edited);
    release::check_release(&repo, "HEAD").unwrap();
    // Rebase-merge onto main, then a fix lands after an rc.
    git(&repo, &["switch", "-q", "main"]);
    git(&repo, &["merge", "-q", "--ff-only", "release/v0.80.0"]);
    let late = commit(
        &repo,
        "fix(core): late fix\n\nChangelog: A late problem is fixed.",
        "crates/cuotai-core/src/lib.rs",
        "// v4\n",
    );
    let mut opts = options(false, false);
    opts.refresh = true;
    opts.date = "2026-10-26".into();
    let outcome = prepare::run(&repo, &opts).unwrap().unwrap();
    assert_eq!(outcome.version.to_string(), "0.80.0");
    let changelog = read(&repo, "CHANGELOG.md");
    assert!(
        changelog.contains("## [0.80.0] - 2026-10-26"),
        "{changelog}"
    );
    assert!(
        changelog.contains("Accounts can now be sorted by name"),
        "{changelog}"
    );
    assert!(!changelog.contains("You can sort accounts"), "{changelog}");
    assert!(changelog.contains("- The order survives a restart ("));
    assert!(changelog.contains(&format!(
        "- A late problem is fixed ({}).",
        short(&repo, &late)
    )));
    assert_eq!(changelog.matches("[0.80.0]: ").count(), 1);
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "commit",
            "-q",
            "--cleanup=verbatim",
            "-m",
            "chore(release): 0.80.0 (refresh)",
        ],
    );
    release::check_release(&repo, "HEAD").unwrap();
}

#[test]
fn check_rejects_incomplete_or_inconsistent_releases() {
    type Edit = fn(&str, &str) -> String;
    let cases: [(&str, Edit, &str); 3] = [
        (
            "missing commit",
            |text, _| {
                text.lines()
                    .map(|line| {
                        if line.starts_with("- The order survives a restart (") {
                            "- The order survives a restart."
                        } else {
                            line
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            },
            "does not cite it",
        ),
        (
            "foreign hash",
            |text, _| {
                text.replace(
                    "### Fixed\n\n",
                    "### Fixed\n\n- Something else (deadbee1).\n",
                )
            },
            "cites deadbee1",
        ),
        (
            "leftover token",
            |text, _| text.to_string(),
            "still has 1 'vNEXT'",
        ),
    ];
    for (name, edit, expected) in cases {
        let (_d, repo) = repo();
        feature_and_fix(&repo);
        prepare_branch(&repo);
        let changelog = read(&repo, "CHANGELOG.md");
        amend(&repo, "CHANGELOG.md", &edit(&changelog, name));
        if name == "leftover token" {
            amend(&repo, "docs/later.md", "| RF3 | vNEXT | Filtering. |\n");
        }
        let error = release::check_release(&repo, "HEAD")
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn check_rejects_a_wrong_version() {
    let (_d, repo) = repo();
    feature_and_fix(&repo);
    let mut opts = options(false, true);
    opts.version = Some(xtask::Version::parse("0.81.0").unwrap());
    prepare::run(&repo, &opts).unwrap().unwrap();
    // Dropping the recorded override makes 0.81.0 a wrong version.
    git(
        &repo,
        &["commit", "-q", "--amend", "-m", "chore(release): 0.81.0"],
    );
    let error = release::check_release(&repo, "HEAD")
        .unwrap_err()
        .to_string();
    assert!(error.contains("make the next version 0.80.0"), "{error}");
    let error = release::check_release(&repo, "HEAD~1")
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not a release commit"), "{error}");
}

#[test]
fn release_base_ignores_rc_tags_and_prefers_stable_tags() {
    let (_d, repo) = repo();
    git(&repo, &["tag", "-a", "v0.79.0", "-m", "v0.79.0"]);
    feature_and_fix(&repo);
    prepare_branch(&repo);
    git(&repo, &["tag", "-a", "v0.80.0-rc.1", "-m", "rc"]);
    let base = release::release_base(
        &repo,
        "HEAD",
        Some(&xtask::Version::parse("0.80.0").unwrap()),
    )
    .unwrap();
    assert_eq!(base.tag.as_deref(), Some("v0.79.0"));
    assert_eq!(base.link_from(), "v0.79.0");
    let base = release::release_base(&repo, "HEAD", None).unwrap();
    assert_eq!(
        base.version.to_string(),
        "0.80.0",
        "the rc'd release commit, not the rc tag"
    );
    assert!(base.tag.is_none());
}

#[test]
fn tag_creates_annotated_rc_and_stable_tags_once() {
    let (_d, repo) = repo();
    feature_and_fix(&repo);
    prepare_branch(&repo);
    let release_sha = head(&repo);
    git(&repo, &["switch", "-q", "main"]);
    // Not merged yet: origin/main is the old tip.
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(&repo, &["switch", "-q", "release/v0.80.0"]);
    let error = tag::run(&repo, true, "origin/main")
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not on origin/main"), "{error}");

    git(&repo, &["switch", "-q", "main"]);
    git(&repo, &["merge", "-q", "--ff-only", "release/v0.80.0"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    let rc1 = tag::run(&repo, true, "origin/main").unwrap();
    assert_eq!(rc1.tag, "v0.80.0-rc.1");
    assert_eq!(rc1.sha, release_sha);
    assert_eq!(
        git(&repo, &["cat-file", "-t", "v0.80.0-rc.1"]).trim(),
        "tag"
    );
    let message = git(
        &repo,
        &["tag", "-l", "--format=%(contents)", "v0.80.0-rc.1"],
    );
    assert!(message.contains("## [0.80.0] - 2026-10-24"), "{message}");
    assert!(message.contains("### Added"), "{message}");
    assert_eq!(
        tag::run(&repo, true, "origin/main").unwrap().tag,
        "v0.80.0-rc.2"
    );
    // Later commits on main do not move the tag target.
    commit(&repo, "docs: later", "docs/later.md", "Later.\n");
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    let stable = tag::run(&repo, false, "origin/main").unwrap();
    assert_eq!(stable.tag, "v0.80.0");
    assert_eq!(stable.sha, release_sha);
    let error = tag::run(&repo, false, "origin/main")
        .unwrap_err()
        .to_string();
    assert!(error.contains("already tagged"), "{error}");
}
