//! The line-level release-commit check and recorded wording edits (PRD
//! 0039 re-validation F-02): a release commit above 0.86.7 must be exactly
//! what `release prepare` writes on its parent, `release edit` records a
//! wording fix as a `Changelog-Entry:` trailer, and `--refresh` keeps it.

mod support;

use std::path::{Path, PathBuf};

use support::*;
use xtask::commits::EntryEdit;
use xtask::release::{self, edit, prepare};

/// The newest release before the line check: the base of these fixtures.
const BASE: &str = "0.86.7";

fn prepared() -> (tempfile::TempDir, PathBuf, String, String) {
    let (dir, repo) = repo_at(BASE);
    let (feat, fix) = feature_and_fix(&repo);
    let outcome = prepare::run(&repo, &options(false, true))
        .unwrap()
        .expect("something to release");
    assert_eq!(outcome.version.to_string(), "0.87.0");
    (dir, repo, feat, fix)
}

fn check_error(repo: &Path) -> String {
    release::check_release(repo, "HEAD")
        .expect_err("the tampered release commit must fail")
        .to_string()
}

fn edit_of(value: &str) -> EntryEdit {
    EntryEdit::parse(value).unwrap()
}

#[test]
fn a_prepared_release_passes_line_by_line() {
    let (_d, repo, _, _) = prepared();
    let outcome = release::check_release(&repo, "HEAD").unwrap();
    assert!(outcome.line_checked);
    assert!(release::line_problems(&repo, "HEAD").unwrap().is_empty());
    xtask::changelog_gate::run(&repo, "main", "HEAD").unwrap();
    // The release before the rule keeps the file-level check.
    assert!(!release::line_checked(
        &xtask::Version::parse(BASE).unwrap()
    ));
    assert!(release::line_checked(
        &xtask::Version::parse("0.86.8").unwrap()
    ));
}

#[test]
fn hand_edits_of_a_release_commit_fail_with_path_and_line() {
    type Tamper = fn(&Path);
    let cases: [(&str, Tamper, &str); 7] = [
        (
            "extra lockfile dependency",
            |repo| {
                let lock = read(repo, "Cargo.lock").replace(
                    "dependencies = [\n \"cuotai-core\",\n]\n",
                    "dependencies = [\n \"cuotai-core\",\n \"serde\",\n]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.999\"\nsource = \"git+https://example.com/serde\"\n",
                );
                amend(repo, "Cargo.lock", &lock);
            },
            "Cargo.lock:12: expected no line, found \" \\\"serde\\\",\"",
        ),
        (
            "changed updater pubkey",
            |repo| {
                let conf = read(repo, "apps/desktop/src-tauri/tauri.conf.json")
                    .replace("\"pubkey\": \"PINNED\"", "\"pubkey\": \"OTHER\"");
                amend(repo, "apps/desktop/src-tauri/tauri.conf.json", &conf);
            },
            "apps/desktop/src-tauri/tauri.conf.json:4: expected \"  \\\"plugins\\\": { \\\"updater\\\": { \\\"pubkey\\\": \\\"PINNED\\\" } }\", found \"  \\\"plugins\\\": { \\\"updater\\\": { \\\"pubkey\\\": \\\"OTHER\\\" } }\"",
        ),
        (
            "invented entry",
            |repo| {
                let changelog = read(repo, "CHANGELOG.md")
                    .replace("### Fixed\n\n", "### Fixed\n\n- Everything is faster.\n");
                amend(repo, "CHANGELOG.md", &changelog);
            },
            "CHANGELOG.md:15: expected no line, found \"- Everything is faster.\"",
        ),
        (
            "reworded older section",
            |repo| {
                let changelog = read(repo, "CHANGELOG.md").replace(
                    "- Base (PRD 0001 RF1; 1111111).",
                    "- Base, now audited (PRD 0001 RF1; 1111111).",
                );
                amend(repo, "CHANGELOG.md", &changelog);
            },
            "CHANGELOG.md:21: expected \"- Base (PRD 0001 RF1; 1111111).\", found \"- Base, now audited (PRD 0001 RF1; 1111111).\"",
        ),
        (
            "reworded new entry",
            |repo| {
                let changelog = read(repo, "CHANGELOG.md")
                    .replace("You can sort accounts by name", "Accounts can be sorted");
                amend(repo, "CHANGELOG.md", &changelog);
            },
            "CHANGELOG.md:11: expected \"- You can sort accounts by name (PRD 0001 RF2; ",
        ),
        (
            "changed line of a release file",
            |repo| {
                let cargo = read(repo, "Cargo.toml").replace("edition = \"2021\"", "edition = \"2024\"");
                amend(repo, "Cargo.toml", &cargo);
            },
            "Cargo.toml:8: expected \"edition = \\\"2021\\\"\", found \"edition = \\\"2024\\\"\"",
        ),
        (
            "changed source file",
            |repo| amend(repo, "crates/cuotai-core/src/lib.rs", "// v3\nfn evil() {}\n"),
            "crates/cuotai-core/src/lib.rs:2: expected no line, found \"fn evil() {}\"",
        ),
    ];
    for (name, tamper, expected) in cases {
        let (_d, repo) = repo_at(BASE);
        // The updater key lives in the parent; a release may not touch it.
        write_file(
            &repo,
            "apps/desktop/src-tauri/tauri.conf.json",
            &format!(
                "{{\n  \"productName\": \"Cuotai\",\n  \"version\": \"{BASE}\",\n  \"plugins\": {{ \"updater\": {{ \"pubkey\": \"PINNED\" }} }}\n}}\n"
            ),
        );
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "chore: add the updater key"]);
        feature_and_fix(&repo);
        prepare::run(&repo, &options(false, true)).unwrap().unwrap();
        release::check_release(&repo, "HEAD").unwrap();
        tamper(&repo);
        let error = check_error(&repo);
        assert!(error.contains(expected), "{name}: {error}");
        if name == "changed source file" {
            // The commit gate reports rule 5 too.
            let gate = xtask::changelog_gate::run(&repo, "main", "HEAD")
                .unwrap_err()
                .to_string();
            assert!(
                gate.contains("not crates/cuotai-core/src/lib.rs") && gate.contains("lib.rs:2"),
                "{gate}"
            );
        }
    }
}

#[test]
fn release_edit_records_a_wording_fix_that_the_check_and_refresh_keep() {
    let (_d, repo, feat, fix) = prepared();
    let (f, x) = (short(&repo, &feat), short(&repo, &fix));

    // Not on the release branch: refused, nothing changes.
    git(&repo, &["switch", "-q", "-c", "elsewhere"]);
    let error = edit::run(&repo, edit_of(&format!("{f}#1 Accounts sort by name.")))
        .unwrap_err()
        .to_string();
    assert!(error.contains("release/v0.87.0"), "{error}");
    git(&repo, &["switch", "-q", "release/v0.87.0"]);
    // A sentence the gate would refuse is refused here too.
    let error = edit::run(&repo, edit_of(&format!("{f}#1 accounts sort by name")))
        .unwrap_err()
        .to_string();
    assert!(error.contains("sentence check"), "{error}");
    // An id that names no commit of the release.
    let error = edit::run(&repo, edit_of("abcdef1#1 Accounts sort by name."))
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a commit of this release"), "{error}");

    // A trailer the release commit already carries survives the amend.
    let message = git(&repo, &["log", "-1", "--format=%B"]);
    git(
        &repo,
        &[
            "commit",
            "-q",
            "--amend",
            "--cleanup=verbatim",
            "-m",
            &format!(
                "{}Co-Authored-By: Example <noreply@example.com>\n",
                message.trim_end().to_string() + "\n"
            ),
        ],
    );
    release::check_release(&repo, "HEAD").unwrap();

    let outcome = edit::run(
        &repo,
        edit_of(&format!("{f}#1 Accounts can be sorted by name.")),
    )
    .unwrap();
    assert_eq!(outcome.edits.len(), 1);
    let changelog = read(&repo, "CHANGELOG.md");
    assert!(
        changelog.contains(&format!(
            "### Added\n\n- Accounts can be sorted by name (PRD 0001 RF2; {f}).\n"
        )),
        "{changelog}"
    );
    // Replacing the same entry, moving it, and merging the fix into it.
    let outcome = edit::run(
        &repo,
        edit_of(&format!(
            "{f}#1 {x}#1 [Changed] Accounts keep their sorted order."
        )),
    )
    .unwrap();
    assert_eq!(outcome.replaced.len(), 1);
    let body = std::fs::read_to_string(&outcome.pr_body).unwrap();
    assert!(
        body.contains(&format!(
            "### Wording edits\n\n- `Changelog-Entry: {f}#1 {x}#1 [Changed] Accounts keep their sorted order.`\n"
        )),
        "{body}"
    );
    let changelog = read(&repo, "CHANGELOG.md");
    assert!(
        changelog.contains(&format!(
            "## [0.87.0] - {DATE}\n\n### Changed\n\n- Accounts keep their sorted order (PRD 0001 RF2; {f}; {x}).\n\n## [0.86.7]"
        )) || changelog.contains(&format!(
            "## [0.87.0] - {DATE}\n\n### Changed\n\n- Accounts keep their sorted order (PRD 0001 RF2; {f}; {x}).\n\n## [0.79.0]"
        )),
        "{changelog}"
    );
    let message = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        message.contains(&format!(
            "Changelog-Entry: {f}#1 {x}#1 [Changed] Accounts keep their sorted order.\n"
        )),
        "{message}"
    );
    assert!(message.contains("Wording edits:\n- "), "{message}");
    assert!(
        message.contains("Co-Authored-By: Example <noreply@example.com>"),
        "{message}"
    );
    assert_eq!(
        git(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
        "1"
    );
    assert!(git(&repo, &["status", "--porcelain"]).trim().is_empty());
    release::check_release(&repo, "HEAD").unwrap();
    xtask::changelog_gate::run(&repo, "main", "HEAD").unwrap();

    // The recorded edit is part of what the line check re-renders: without
    // it, the commit no longer matches.
    let without = release::line_problems_with(&repo, "HEAD", Some(Vec::new())).unwrap();
    assert!(
        without.iter().any(|p| p.starts_with("CHANGELOG.md:")),
        "{without:?}"
    );

    // A late fix lands; --refresh keeps the recorded edit and adds the fix.
    let late = commit(
        &repo,
        "fix(core): late fix\n\nChangelog: A late problem is fixed.",
        "crates/cuotai-core/src/lib.rs",
        "// v4\n",
    );
    let mut opts = options(false, false);
    opts.refresh = true;
    let outcome = prepare::run(&repo, &opts).unwrap().unwrap();
    assert_eq!(outcome.edits.len(), 1);
    let changelog = read(&repo, "CHANGELOG.md");
    assert!(
        changelog.contains("- Accounts keep their sorted order (PRD 0001 RF2;"),
        "{changelog}"
    );
    assert!(changelog.contains(&format!(
        "- A late problem is fixed ({}).",
        short(&repo, &late)
    )));
    assert!(!changelog.contains("You can sort accounts"), "{changelog}");
    assert!(
        outcome
            .message
            .contains(&format!("Changelog-Entry: {f}#1 {x}#1 [Changed] ")),
        "{}",
        outcome.message
    );
    git(&repo, &["add", "-A"]);
    support::git_commit_message(&repo, &outcome.message);
    assert!(
        git(&repo, &["log", "-1", "--format=%s"]).starts_with("chore(release): 0.87.0 (refresh)")
    );
    let checked = release::check_release(&repo, "HEAD").unwrap();
    assert!(checked.line_checked);
}

#[test]
fn the_gate_checks_changelog_entry_trailers() {
    let (_d, repo, feat, _) = prepared();
    let f = short(&repo, &feat);
    let message = git(&repo, &["log", "-1", "--format=%B"]);
    let with = |trailer: &str| {
        git(
            &repo,
            &[
                "commit",
                "-q",
                "--amend",
                "--cleanup=verbatim",
                "-m",
                &format!("{}\n{trailer}\n", message.trim_end()),
            ],
        );
        xtask::changelog_gate::run(&repo, "main", "HEAD")
            .unwrap_err()
            .to_string()
    };
    let bad_id = with("Changelog-Entry: zz#1 A sentence.");
    assert!(
        bad_id.contains("rule 5") && bad_id.contains("not a commit hash"),
        "{bad_id}"
    );
    let bad_category = with(&format!("Changelog-Entry: {f}#1 [Improved] A sentence."));
    assert!(bad_category.contains("unknown category"), "{bad_category}");
    let bad_sentence = with(&format!("Changelog-Entry: {f}#1 see https://example.com"));
    assert!(bad_sentence.contains("rule 7"), "{bad_sentence}");

    // Outside a release commit, the trailer is refused.
    git(&repo, &["switch", "-q", "main"]);
    commit(
        &repo,
        &format!(
            "fix(core): x\n\nChangelog: Something works.\nChangelog-Entry: {f}#1 Other words."
        ),
        "crates/cuotai-core/src/lib.rs",
        "// v9\n",
    );
    let error = xtask::changelog_gate::run(&repo, "HEAD~1", "HEAD")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("belongs only in a release commit"),
        "{error}"
    );
}

/// The repository's own release commits v0.85.5 to v0.86.7 (read-only).
/// Each passes the file-level check it was released under, and each is
/// reproduced byte for byte by `release prepare` from its parent with its
/// own version and date; 0.86.4, whose section was edited by hand (two
/// entries moved from Security to Fixed), needs two recorded edits. A
/// shallow clone without the tags skips the table.
#[test]
fn the_real_releases_are_reproduced_line_by_line() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let versions = [
        "0.85.5", "0.85.6", "0.85.7", "0.85.8", "0.85.9", "0.85.10", "0.85.11", "0.85.12",
        "0.86.0", "0.86.1", "0.86.2", "0.86.3", "0.86.4", "0.86.5", "0.86.6", "0.86.7",
    ];
    let has_tags = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--verify", "--quiet", "v0.85.5^{commit}"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_tags {
        eprintln!("skipped: the release tags are not in this clone");
        return;
    }
    for version in versions {
        let rev = format!("v{version}^{{commit}}");
        release::check_release(&root, &rev)
            .unwrap_or_else(|e| panic!("{version}: the file-level check fails: {e}"));
        let edits = if version == "0.86.4" {
            // The hand edit: before the rule, every sentence of 67e7f8e
            // took its one `Changelog-Category: Security`; the owner moved
            // the second and third to Fixed. Recorded, it reproduces.
            let raw = release::line_problems(&root, &rev).unwrap();
            assert_eq!(raw.len(), 4, "{raw:?}");
            assert!(
                raw.iter().all(|p| p.starts_with("CHANGELOG.md:")),
                "{raw:?}"
            );
            Some(vec![
                edit_of("67e7f8e#2 [Fixed] Invalid saved plan prices or second currency are ignored with a note instead of showing infinite or negative amounts."),
                edit_of("67e7f8e#3 [Fixed] A model repeated in pricing.json is now an error, and --doctor checks pricing.json."),
            ])
        } else {
            None
        };
        let problems = release::line_problems_with(&root, &rev, edits).unwrap();
        assert!(problems.is_empty(), "{version}: {problems:?}");
    }
}
