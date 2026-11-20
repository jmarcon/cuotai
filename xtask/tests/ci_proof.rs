//! `release ci-proof` (PRD 0039 RF5.4, re-validation F-01/S-01), evaluated
//! offline against GitHub API responses captured on 2026-11-17 with
//! read-only `gh api` GETs and sanitized (only the fields the evaluator
//! reads; no URLs, avatars or tokens). `commit.json` holds the git facts of
//! the tagged or merged commit `T`, in the `git/commits` shape.
//!
//! - `green-v0.86.7`: `cdfdb5f0` (tag v0.86.7), its `main` CI run green;
//!   PR #264's head `16de5ada` has the same tree and parent.
//! - `pr11-admin-bypass`: `295234b8`, the admin-bypassed merge of PR #11 on
//!   2026-10-23. Its `main` CI run concluded success after the merge.
//! - `red-main-push`: `5bf840d6` (PR #254), whose `main` CI run failed.

mod support;

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use support::*;
use xtask::release::ci_proof::{
    self, evaluate, files, identity_problems, CommitFacts, Evidence, Proof,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ci-proof")
        .join(name)
}

fn facts(name: &str) -> CommitFacts {
    let text = std::fs::read_to_string(fixture(name).join("commit.json")).unwrap();
    CommitFacts::from_api_json(&text).unwrap()
}

fn evidence(name: &str) -> Evidence {
    Evidence::load(&fixture(name)).unwrap()
}

/// Copies a fixture into a temporary directory, letting `edit` change each
/// file's JSON (or delete it by returning `None`).
fn variant(
    name: &str,
    edit: impl Fn(&str, Value) -> Option<Value>,
) -> (tempfile::TempDir, Evidence) {
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(fixture(name)).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_str().unwrap().to_string();
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if let Some(value) = edit(&file, value) {
            std::fs::write(dir.path().join(&file), value.to_string()).unwrap();
        }
    }
    let evidence = Evidence::load(dir.path()).unwrap();
    (dir, evidence)
}

const GREEN_T: &str = "cdfdb5f084428f20b4d66d2e3e7ae370a8ad89e6";
const GREEN_H: &str = "16de5ada250816f51b5e45ee232ab63e59552efd";

#[test]
fn the_green_release_is_proved_by_its_main_run() {
    let commit = facts("green-v0.86.7");
    assert_eq!(commit.sha, GREEN_T);
    for switch in [false, true] {
        let proof = evaluate(&commit, &evidence("green-v0.86.7"), switch).unwrap();
        assert_eq!(
            proof,
            Proof::Strict {
                sha: GREEN_T.into(),
                run_id: 44043685873,
                event: "push".into(),
            }
        );
        assert_eq!(
            proof.describe(),
            format!("CI result success on {GREEN_T} (run 44043685873, ci.yml on push)")
        );
    }
    assert!(identity_problems(&commit).is_empty());
}

/// The bypassed merge of PR #11 merged while `CI result` was still expected,
/// but its `main` run concluded success nine minutes later. The strict form
/// accepts it today: the control stops releases built on red merges, not
/// merges that bypassed a check still running (only the ruleset setting,
/// B-1, closes that). The identical-tree form refuses it: PR #11 was
/// rebased, so its head has another tree and parent.
#[test]
fn the_pr11_bypass_is_accepted_by_its_later_main_run_only() {
    let commit = facts("pr11-admin-bypass");
    let proof = evaluate(&commit, &evidence("pr11-admin-bypass"), false).unwrap();
    assert_eq!(
        proof.describe(),
        "CI result success on 295234b87b16b5abcaa7b7ac0cda6d52e2003a64 (run 39990370365, ci.yml on push)"
    );
    let (_d, without_main_run) = variant("pr11-admin-bypass", |file, v| {
        (file != files::CHECK_RUNS).then_some(v)
    });
    let error = evaluate(&commit, &without_main_run, true).unwrap_err();
    assert!(error.contains("check-runs.json is missing"), "{error}");
    assert!(error.contains("differs from"), "{error}");
    assert!(error.contains("tree af562eec"), "{error}");
    assert!(error.contains("parents 0a215f8e"), "{error}");
}

#[test]
fn the_red_main_run_fails_in_both_forms() {
    let commit = facts("red-main-push");
    let error = evaluate(&commit, &evidence("red-main-push"), false).unwrap_err();
    assert!(error.contains("concluded failure"), "{error}");
    assert!(error.contains("run 43866033873"), "{error}");
    assert!(
        error.contains("identical PR head form is turned off"),
        "{error}"
    );
    // PR #254's head was green, but it is not identical to the merge.
    let error = evaluate(&commit, &evidence("red-main-push"), true).unwrap_err();
    assert!(error.contains("concluded failure"), "{error}");
    assert!(error.contains("identical PR head form"), "{error}");
    assert!(error.contains("parents"), "{error}");
}

type Edit = dyn Fn(&str, Value) -> Option<Value>;

fn set_check_run(file: &str, mut v: Value, key: &str, value: Value) -> Option<Value> {
    if file == files::CHECK_RUNS {
        v["check_runs"][0][key] = value;
    }
    Some(v)
}

#[test]
fn pending_wrong_app_other_workflow_and_missing_runs_fail() {
    let commit = facts("green-v0.86.7");
    let cases: Vec<(Box<Edit>, &str)> = vec![
        (
            Box::new(|f, v| {
                let v = set_check_run(f, v, "status", json!("in_progress"))?;
                set_check_run(f, v, "conclusion", Value::Null)
            }),
            "is still in_progress",
        ),
        (
            Box::new(|f, v| set_check_run(f, v, "conclusion", json!("cancelled"))),
            "concluded cancelled",
        ),
        (
            Box::new(|f, v| set_check_run(f, v, "app", json!({"id": 9426, "slug": "other"}))),
            "comes from app 9426, not GitHub Actions",
        ),
        (
            Box::new(|f, mut v| {
                if f == files::RUNS {
                    for run in v["workflow_runs"].as_array_mut().unwrap() {
                        if run["path"] == ".github/workflows/ci.yml" {
                            run["path"] = json!(".github/workflows/other.yml");
                        }
                    }
                }
                Some(v)
            }),
            "not .github/workflows/ci.yml",
        ),
        (
            Box::new(|f, mut v| {
                if f == files::RUNS {
                    for run in v["workflow_runs"].as_array_mut().unwrap() {
                        run["head_sha"] = json!(GREEN_H);
                    }
                }
                Some(v)
            }),
            "which ran on 16de5ada",
        ),
        (
            Box::new(|f, mut v| {
                if f == files::RUNS {
                    v["workflow_runs"] = json!([]);
                }
                Some(v)
            }),
            "which is no workflow run of",
        ),
        (
            Box::new(|f, mut v| {
                if f == files::CHECK_RUNS {
                    v["check_runs"] = json!([]);
                }
                Some(v)
            }),
            "CI has not reported on this commit (yet)",
        ),
        (
            Box::new(|f, v| (f != files::RUNS).then_some(v)),
            "runs.json is missing",
        ),
    ];
    for (edit, expected) in cases {
        let (_d, evidence) = variant("green-v0.86.7", edit);
        let error = evaluate(&commit, &evidence, false).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

/// Without the main run's check, the green release can be proved only by
/// PR #264's head, which is identical: accepted with the switch on only.
#[test]
fn identical_pr_head_is_accepted_only_with_the_switch_on() {
    const {
        assert!(
            ci_proof::ACCEPT_IDENTICAL_PR_HEAD,
            "owner decision B-6 (2026-11-20)"
        )
    };
    let commit = facts("green-v0.86.7");
    let (_d, evidence) = variant("green-v0.86.7", |f, v| {
        set_check_run(f, v, "conclusion", Value::Null)
            .and_then(|v| set_check_run(f, v, "status", json!("queued")))
    });
    let error = evaluate(&commit, &evidence, false).unwrap_err();
    assert!(error.contains("is still queued"), "{error}");
    assert!(error.contains("turned off"), "{error}");
    let proof = evaluate(&commit, &evidence, true).unwrap();
    assert_eq!(
        proof,
        Proof::IdenticalPrHead {
            sha: GREEN_T.into(),
            pr: 264,
            head: GREEN_H.into(),
            run_id: proof_run_id(&proof),
        }
    );
    assert!(
        proof.describe().contains("the head of PR #264"),
        "{proof:?}"
    );
}

fn proof_run_id(proof: &Proof) -> u64 {
    match proof {
        Proof::IdenticalPrHead { run_id, .. } | Proof::Strict { run_id, .. } => *run_id,
    }
}

#[test]
fn identical_pr_head_rejects_every_difference() {
    let without_main = |f: &str, v: Value| (f != files::CHECK_RUNS).then_some(v);
    let base = facts("green-v0.86.7");
    let mut other_parent = base.clone();
    other_parent.parents = vec!["0".repeat(40)];
    let mut other_tree = base.clone();
    other_tree.tree = "1".repeat(40);
    let mut other_message = base.clone();
    other_message.message.push_str("Extra.\n");
    let mut other_author = base.clone();
    other_author.author.email = "1+someone@users.noreply.github.com".into();
    let (_d, evidence) = variant("green-v0.86.7", without_main);
    for (commit, expected) in [
        (&other_parent, "parents"),
        (&other_tree, "tree"),
        (&other_message, "message"),
        (&other_author, "author"),
    ] {
        let error = evaluate(commit, &evidence, true).unwrap_err();
        assert!(error.contains("differs from"), "{error}");
        assert!(error.contains(expected), "{expected}: {error}");
    }

    // The head's CI result must come from a pull_request run of ci.yml.
    let (_d, push_run) = variant("green-v0.86.7", |f, mut v| {
        if f == files::CHECK_RUNS {
            return None;
        }
        if f == files::HEAD_RUNS {
            for run in v["workflow_runs"].as_array_mut().unwrap() {
                run["event"] = json!("push");
            }
        }
        Some(v)
    });
    let error = evaluate(&base, &push_run, true).unwrap_err();
    assert!(error.contains("not a `pull_request` run"), "{error}");

    // Not merged, or merged elsewhere: no proof.
    let (_d, unmerged) = variant("green-v0.86.7", |f, mut v| {
        if f == files::CHECK_RUNS {
            return None;
        }
        if f == files::PULLS {
            v[0]["merged_at"] = Value::Null;
        }
        Some(v)
    });
    let error = evaluate(&base, &unmerged, true).unwrap_err();
    assert!(
        error.contains("came from no PR merged into main"),
        "{error}"
    );

    // A head file for another commit is refused.
    let (_d, wrong_head) = variant("green-v0.86.7", |f, mut v| {
        if f == files::CHECK_RUNS {
            return None;
        }
        if f == files::PULLS {
            v[0]["head"]["sha"] = json!("2".repeat(40));
        }
        Some(v)
    });
    let error = evaluate(&base, &wrong_head, true).unwrap_err();
    assert!(error.contains("not the head"), "{error}");

    // Missing PR files are named.
    for missing in [
        files::PULLS,
        files::HEAD_COMMIT,
        files::HEAD_CHECK_RUNS,
        files::HEAD_RUNS,
    ] {
        let (_d, evidence) = variant("green-v0.86.7", |f, v| {
            (f != files::CHECK_RUNS && f != missing).then_some(v)
        });
        let error = evaluate(&base, &evidence, true).unwrap_err();
        assert!(error.contains(&format!("{missing} is missing")), "{error}");
    }
}

#[test]
fn malformed_evidence_and_a_missing_directory_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(files::CHECK_RUNS), "{\"nope\": 1}").unwrap();
    let error = Evidence::load(dir.path()).unwrap_err().to_string();
    assert!(error.contains("check-runs.json"), "{error}");
    let error = Evidence::load(&dir.path().join("absent"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not exist"), "{error}");
}

#[test]
fn identities_must_be_github_noreply() {
    let mut commit = facts("green-v0.86.7");
    assert!(identity_problems(&commit).is_empty());
    commit.committer.name = "GitHub".into();
    commit.committer.email = "noreply@github.com".into();
    assert!(identity_problems(&commit).is_empty());
    commit.author.email = "noreply@github.com".into();
    commit.committer.email = "someone@example.com".into();
    let problems = identity_problems(&commit);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(ci_proof::is_github_noreply(
        "1767705+jmarcon@users.noreply.github.com"
    ));
    assert!(ci_proof::is_github_noreply(
        "octocat@users.noreply.github.com"
    ));
    assert!(!ci_proof::is_github_noreply("@users.noreply.github.com"));
    assert!(!ci_proof::is_github_noreply("a@b.users.noreply.github.com"));
    assert!(!ci_proof::is_github_noreply("someone@example.com"));
}

const NOREPLY: &str = "1+tester@users.noreply.github.com";

/// A temp repository with the 0.80.0 release commit merged into main, the
/// identity set to a GitHub noreply address, and the evidence directory of
/// the green release rewritten for that commit.
fn released_repo() -> (tempfile::TempDir, PathBuf, String, tempfile::TempDir) {
    let (dir, repo) = repo();
    git(&repo, &["config", "user.email", NOREPLY]);
    git(
        &repo,
        &["commit", "-q", "--amend", "--no-edit", "--reset-author"],
    );
    feature_and_fix(&repo);
    xtask::release::prepare::run(&repo, &options(false, true))
        .unwrap()
        .unwrap();
    git(&repo, &["switch", "-q", "main"]);
    git(&repo, &["merge", "-q", "--ff-only", "release/v0.80.0"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    let sha = head(&repo);
    let evidence = tempfile::tempdir().unwrap();
    for file in [files::CHECK_RUNS, files::RUNS] {
        let text = std::fs::read_to_string(fixture("green-v0.86.7").join(file)).unwrap();
        std::fs::write(evidence.path().join(file), text.replace(GREEN_T, &sha)).unwrap();
    }
    (dir, repo, sha, evidence)
}

#[test]
fn run_checks_the_proof_identities_and_the_tag() {
    let (_d, repo, sha, evidence) = released_repo();
    let stable = xtask::release::tag::run(&repo, false, "origin/main").unwrap();
    assert_eq!(stable.tag, "v0.80.0");
    let report = ci_proof::run(&repo, &sha, evidence.path(), Some("v0.80.0"), false);
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.passed.len(), 3, "{:?}", report.passed);
    assert!(report.passed[0].starts_with(&format!("CI result success on {sha}")));

    // An abbreviated --commit is refused.
    let report = ci_proof::run(&repo, &sha[..12], evidence.path(), None, false);
    assert!(
        report.problems[0].contains("full commit hash"),
        "{report:?}"
    );

    // No evidence at all: the proof fails, the rest still runs.
    let report = ci_proof::run(&repo, &sha, &repo.join("absent"), None, false);
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert_eq!(report.passed.len(), 1, "{report:?}");
}

#[test]
fn a_hand_made_tag_an_old_version_and_a_foreign_tagger_are_refused() {
    let (_d, repo, sha, evidence) = released_repo();
    // A hand-made annotated tag with another message.
    git(&repo, &["tag", "-a", "-m", "v0.80.0", "v0.80.0", &sha]);
    let report = ci_proof::run(&repo, &sha, evidence.path(), Some("v0.80.0"), false);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("not the one `cargo xtask release tag` writes")),
        "{report:?}"
    );
    git(&repo, &["tag", "-d", "v0.80.0"]);

    // A newer stable tag already exists: 0.80.0 cannot be published.
    let stable = xtask::release::tag::run(&repo, false, "origin/main").unwrap();
    let root = git(&repo, &["rev-list", "--max-parents=0", "HEAD"]);
    git(
        &repo,
        &["tag", "-a", "-m", "v0.81.0", "v0.81.0", root.trim()],
    );
    let report = ci_proof::run(&repo, &sha, evidence.path(), Some(&stable.tag), false);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("is not above the newest stable tag v0.81.0")),
        "{report:?}"
    );
    git(&repo, &["tag", "-d", "v0.81.0", "v0.80.0"]);

    // A tagger without a noreply address; a lightweight tag.
    git(&repo, &["config", "user.email", "someone@example.com"]);
    let rc = xtask::release::tag::run(&repo, true, "origin/main").unwrap();
    let report = ci_proof::run(&repo, &sha, evidence.path(), Some(&rc.tag), false);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("tagger") && p.contains("someone@example.com")),
        "{report:?}"
    );
    git(&repo, &["tag", "v0.80.0", &sha]);
    let report = ci_proof::run(&repo, &sha, evidence.path(), Some("v0.80.0"), false);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("not an annotated tag")),
        "{report:?}"
    );
}
