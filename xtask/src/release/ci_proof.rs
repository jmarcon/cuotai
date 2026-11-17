//! `cargo xtask release ci-proof` (PRD 0039 RF5.4, re-validation F-01/S-01):
//! before `release.yml` builds anything, prove that the tagged commit passed
//! `CI result`, and check the identities, the tag message and the version
//! order of the release.
//!
//! The evaluator is pure and offline (NFR2): `release.yml` fetches the
//! GitHub API responses with read-only `gh api` calls into one directory
//! (the file names are in [`files`]), and this module decides from those
//! files plus `git` facts about the tagged commit. Tests feed it captured,
//! sanitized responses.
//!
//! Two forms of proof share one evaluator:
//!
//! - **strict** (always on): a check run named `CI result`, concluded
//!   `success`, created by GitHub Actions (app id [`GITHUB_ACTIONS_APP_ID`],
//!   so a commit status or another app cannot stand in for it), whose check
//!   suite belongs to a run of [`CI_WORKFLOW`] whose `head_sha` is the
//!   tagged commit;
//! - **identical PR head** (written and tested, not enabled, see
//!   [`ACCEPT_IDENTICAL_PR_HEAD`]): the same proof on the head `H` of the
//!   pull request merged into `main` as the tagged commit `T`, from a
//!   `pull_request` run, when `H` and `T` have the same tree, the same
//!   parents, the same message and the same author.

use std::path::Path;

use serde::Deserialize;

use crate::{err, git, parse_tag, Error, Res};

/// Whether the identical-tree form may prove a release. Available, not
/// enabled: turning it on is owner decision B-6 of the PRD 0039
/// re-validation (it lets a release be tagged without waiting for the
/// `main` CI run of the release commit). The CLI always passes this
/// constant; tests pass both values.
pub const ACCEPT_IDENTICAL_PR_HEAD: bool = false;

/// The GitHub Actions app: only its check runs prove a workflow result.
pub const GITHUB_ACTIONS_APP_ID: u64 = 15368;

/// The workflow whose aggregate check proves a commit.
pub const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

/// The aggregate check of [`CI_WORKFLOW`] (the required check on `main`).
pub const CHECK_NAME: &str = "CI result";

/// The evidence files `release.yml` writes, one GitHub API response each.
pub mod files {
    /// `GET repos/{repo}/commits/{T}/check-runs?check_name=CI%20result&filter=latest&per_page=100`
    pub const CHECK_RUNS: &str = "check-runs.json";
    /// `GET repos/{repo}/actions/runs?head_sha={T}&per_page=100`
    pub const RUNS: &str = "runs.json";
    /// `GET repos/{repo}/commits/{T}/pulls`
    pub const PULLS: &str = "pulls.json";
    /// `GET repos/{repo}/git/commits/{H}` for the merged PR's head `H`.
    pub const HEAD_COMMIT: &str = "head-commit.json";
    /// `GET repos/{repo}/commits/{H}/check-runs?check_name=CI%20result&filter=latest&per_page=100`
    pub const HEAD_CHECK_RUNS: &str = "head-check-runs.json";
    /// `GET repos/{repo}/actions/runs?head_sha={H}&per_page=100`
    pub const HEAD_RUNS: &str = "head-runs.json";
}

#[derive(Clone, Debug, Deserialize)]
pub struct CheckRuns {
    pub check_runs: Vec<CheckRun>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CheckRun {
    pub id: u64,
    pub name: String,
    pub head_sha: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub app: Option<IdOnly>,
    pub check_suite: Option<IdOnly>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IdOnly {
    pub id: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkflowRuns {
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub path: String,
    pub event: String,
    pub head_sha: String,
    pub check_suite_id: u64,
    pub run_number: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pull {
    pub number: u64,
    pub merged_at: Option<String>,
    pub base: PullRef,
    pub head: PullHead,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PullRef {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PullHead {
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Person {
    pub name: String,
    pub email: String,
}

/// What the evaluator needs to know about a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitFacts {
    pub sha: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub message: String,
    pub author: Person,
    pub committer: Person,
}

/// The `git/commits` API shape, also used by the test fixtures for `T`.
#[derive(Deserialize)]
struct ApiCommit {
    sha: String,
    tree: Sha,
    parents: Vec<Sha>,
    message: String,
    author: Person,
    committer: Person,
}

#[derive(Deserialize)]
struct Sha {
    sha: String,
}

impl CommitFacts {
    /// Parses a `GET repos/{repo}/git/commits/{sha}` response.
    pub fn from_api_json(text: &str) -> Res<Self> {
        let api: ApiCommit = serde_json::from_str(text)
            .map_err(|e| Error::Msg(format!("not a git commit response: {e}")))?;
        Ok(Self {
            sha: api.sha,
            tree: api.tree.sha,
            parents: api.parents.into_iter().map(|p| p.sha).collect(),
            message: api.message,
            author: api.author,
            committer: api.committer,
        })
    }

    /// Reads the facts of `rev` from the local repository.
    pub fn from_git(repo: &Path, rev: &str) -> Res<Self> {
        let out = git::run(
            repo,
            &[
                "log",
                "-1",
                "--format=%H%x00%T%x00%P%x00%an%x00%ae%x00%cn%x00%ce%x00%B",
                &format!("{rev}^{{commit}}"),
            ],
        )
        .map_err(|_| Error::Msg(format!("commit '{rev}' is not in this checkout")))?;
        let fields: Vec<&str> = out.splitn(8, '\0').collect();
        let [sha, tree, parents, an, ae, cn, ce, message] = fields[..] else {
            return err(format!("cannot read the facts of commit '{rev}'"));
        };
        Ok(Self {
            sha: sha.to_string(),
            tree: tree.to_string(),
            parents: parents.split_whitespace().map(str::to_string).collect(),
            message: message.to_string(),
            author: Person {
                name: an.to_string(),
                email: ae.to_string(),
            },
            committer: Person {
                name: cn.to_string(),
                email: ce.to_string(),
            },
        })
    }
}

/// The API responses `release.yml` fetched. The merged PR's files are
/// optional: they exist only when the commit came from a merged PR.
#[derive(Clone, Debug, Default)]
pub struct Evidence {
    pub check_runs: Option<Vec<CheckRun>>,
    pub runs: Option<Vec<WorkflowRun>>,
    pub pulls: Option<Vec<Pull>>,
    pub head_commit: Option<CommitFacts>,
    pub head_check_runs: Option<Vec<CheckRun>>,
    pub head_runs: Option<Vec<WorkflowRun>>,
}

fn read_optional(dir: &Path, name: &str) -> Res<Option<String>> {
    let path = dir.join(name);
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| Error::Msg(format!("cannot read evidence file {name}: {e}")))
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, text: &str) -> Res<T> {
    serde_json::from_str(text).map_err(|e| {
        Error::Msg(format!(
            "evidence file {name} is not the expected GitHub API response: {e}"
        ))
    })
}

impl Evidence {
    /// Loads every evidence file present in `dir`. A missing file stays
    /// `None` (the evaluator explains what it lacks); a malformed one is an
    /// error.
    pub fn load(dir: &Path) -> Res<Self> {
        if !dir.is_dir() {
            return err(format!(
                "the evidence directory {} does not exist",
                dir.display()
            ));
        }
        let check_runs = |name: &str| -> Res<Option<Vec<CheckRun>>> {
            read_optional(dir, name)?
                .map(|t| parse::<CheckRuns>(name, &t).map(|c| c.check_runs))
                .transpose()
        };
        let runs = |name: &str| -> Res<Option<Vec<WorkflowRun>>> {
            read_optional(dir, name)?
                .map(|t| parse::<WorkflowRuns>(name, &t).map(|r| r.workflow_runs))
                .transpose()
        };
        Ok(Self {
            check_runs: check_runs(files::CHECK_RUNS)?,
            runs: runs(files::RUNS)?,
            pulls: read_optional(dir, files::PULLS)?
                .map(|t| parse::<Vec<Pull>>(files::PULLS, &t))
                .transpose()?,
            head_commit: read_optional(dir, files::HEAD_COMMIT)?
                .map(|t| {
                    CommitFacts::from_api_json(&t)
                        .map_err(|e| Error::Msg(format!("{}: {e}", files::HEAD_COMMIT)))
                })
                .transpose()?,
            head_check_runs: check_runs(files::HEAD_CHECK_RUNS)?,
            head_runs: runs(files::HEAD_RUNS)?,
        })
    }
}

/// The accepted proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proof {
    /// `CI result` succeeded on the tagged commit itself.
    Strict {
        sha: String,
        run_id: u64,
        event: String,
    },
    /// `CI result` succeeded on the merged PR's head, a commit identical
    /// to the tagged one except for its committer.
    IdenticalPrHead {
        sha: String,
        pr: u64,
        head: String,
        run_id: u64,
    },
}

impl Proof {
    /// The line the `check` job log prints.
    pub fn describe(&self) -> String {
        match self {
            Proof::Strict { sha, run_id, event } => {
                format!("CI result success on {sha} (run {run_id}, ci.yml on {event})")
            }
            Proof::IdenticalPrHead {
                sha,
                pr,
                head,
                run_id,
            } => format!(
                "CI result success on {head} (run {run_id}, ci.yml on pull_request), the head of PR #{pr}, identical to {sha} (same tree, parents, message and author)"
            ),
        }
    }
}

/// Looks for a successful `CI result` of `sha` among `check_runs`, each
/// matched to its workflow run in `runs`. On failure, the error says why
/// each candidate did not count.
fn ci_result(
    sha: &str,
    check_runs: &[CheckRun],
    runs: &[WorkflowRun],
    event: Option<&str>,
) -> Result<(u64, String), String> {
    let candidates: Vec<&CheckRun> = check_runs
        .iter()
        .filter(|c| c.name == CHECK_NAME && c.head_sha == sha)
        .collect();
    if candidates.is_empty() {
        return Err(format!(
            "no `{CHECK_NAME}` check run on {sha}: CI has not reported on this commit (yet)"
        ));
    }
    let mut reasons = Vec::new();
    for check in candidates {
        let id = check.id;
        let app = check.app.as_ref().map(|a| a.id);
        if app != Some(GITHUB_ACTIONS_APP_ID) {
            reasons.push(format!(
                "check run {id} comes from app {}, not GitHub Actions ({GITHUB_ACTIONS_APP_ID})",
                app.map_or_else(|| "none".to_string(), |a| a.to_string())
            ));
            continue;
        }
        let Some(suite) = check.check_suite.as_ref().map(|s| s.id) else {
            reasons.push(format!("check run {id} has no check suite"));
            continue;
        };
        let Some(run) = runs.iter().find(|r| r.check_suite_id == suite) else {
            reasons.push(format!(
                "check run {id} belongs to check suite {suite}, which is no workflow run of {sha}"
            ));
            continue;
        };
        if run.path != CI_WORKFLOW {
            reasons.push(format!(
                "check run {id} comes from run {} of {}, not {CI_WORKFLOW}",
                run.id, run.path
            ));
            continue;
        }
        if run.head_sha != sha {
            reasons.push(format!(
                "check run {id} comes from run {}, which ran on {}, not {sha}",
                run.id, run.head_sha
            ));
            continue;
        }
        if let Some(event) = event {
            if run.event != event {
                reasons.push(format!(
                    "check run {id} comes from a `{}` run ({}), not a `{event}` run",
                    run.event, run.id
                ));
                continue;
            }
        }
        if check.status != "completed" {
            reasons.push(format!(
                "check run {id} (run {}) is still {}: wait for CI to finish",
                run.id, check.status
            ));
            continue;
        }
        match check.conclusion.as_deref() {
            Some("success") => return Ok((run.id, run.event.clone())),
            other => reasons.push(format!(
                "check run {id} (run {}) concluded {}",
                run.id,
                other.unwrap_or("nothing")
            )),
        }
    }
    Err(format!(
        "`{CHECK_NAME}` did not succeed on {sha}: {}",
        reasons.join("; ")
    ))
}

fn missing(name: &str) -> String {
    format!("evidence file {name} is missing (the GitHub API call failed or was not made)")
}

fn strict(commit: &CommitFacts, evidence: &Evidence) -> Result<Proof, String> {
    let check_runs = evidence
        .check_runs
        .as_ref()
        .ok_or_else(|| missing(files::CHECK_RUNS))?;
    let runs = evidence.runs.as_ref().ok_or_else(|| missing(files::RUNS))?;
    let (run_id, event) = ci_result(&commit.sha, check_runs, runs, None)?;
    Ok(Proof::Strict {
        sha: commit.sha.clone(),
        run_id,
        event,
    })
}

fn identical_pr_head(commit: &CommitFacts, evidence: &Evidence) -> Result<Proof, String> {
    let pulls = evidence
        .pulls
        .as_ref()
        .ok_or_else(|| missing(files::PULLS))?;
    let merged: Vec<&Pull> = pulls
        .iter()
        .filter(|p| p.merged_at.is_some() && p.base.name == "main")
        .collect();
    let pull = match merged[..] {
        [pull] => pull,
        [] => return Err(format!("{} came from no PR merged into main", commit.sha)),
        _ => {
            return Err(format!(
                "{} is associated with {} merged PRs; the proof needs exactly one",
                commit.sha,
                merged.len()
            ))
        }
    };
    let head = evidence
        .head_commit
        .as_ref()
        .ok_or_else(|| missing(files::HEAD_COMMIT))?;
    let pr = pull.number;
    if head.sha != pull.head.sha {
        return Err(format!(
            "{} is commit {}, not the head {} of PR #{pr}",
            files::HEAD_COMMIT,
            head.sha,
            pull.head.sha
        ));
    }
    let mut differences = Vec::new();
    if head.tree != commit.tree {
        differences.push(format!("tree {} vs {}", head.tree, commit.tree));
    }
    if head.parents != commit.parents {
        differences.push(format!(
            "parents {} vs {}",
            head.parents.join(" "),
            commit.parents.join(" ")
        ));
    }
    if head.message.trim_end() != commit.message.trim_end() {
        differences.push("message".to_string());
    }
    if head.author != commit.author {
        differences.push(format!(
            "author {} <{}> vs {} <{}>",
            head.author.name, head.author.email, commit.author.name, commit.author.email
        ));
    }
    if !differences.is_empty() {
        return Err(format!(
            "the head {} of PR #{pr} differs from {}: {}",
            head.sha,
            commit.sha,
            differences.join(", ")
        ));
    }
    let check_runs = evidence
        .head_check_runs
        .as_ref()
        .ok_or_else(|| missing(files::HEAD_CHECK_RUNS))?;
    let runs = evidence
        .head_runs
        .as_ref()
        .ok_or_else(|| missing(files::HEAD_RUNS))?;
    let (run_id, _) = ci_result(&head.sha, check_runs, runs, Some("pull_request"))?;
    Ok(Proof::IdenticalPrHead {
        sha: commit.sha.clone(),
        pr,
        head: head.sha.clone(),
        run_id,
    })
}

/// Decides whether `evidence` proves that `commit` passed `CI result`.
/// `accept_identical_pr_head` enables the second form; the CLI passes
/// [`ACCEPT_IDENTICAL_PR_HEAD`].
pub fn evaluate(
    commit: &CommitFacts,
    evidence: &Evidence,
    accept_identical_pr_head: bool,
) -> Result<Proof, String> {
    let strict_error = match strict(commit, evidence) {
        Ok(proof) => return Ok(proof),
        Err(e) => e,
    };
    if !accept_identical_pr_head {
        return Err(format!(
            "{strict_error} (the identical PR head form is available but not enabled: owner decision B-6)"
        ));
    }
    identical_pr_head(commit, evidence)
        .map_err(|e| format!("{strict_error}; identical PR head form: {e}"))
}

/// A GitHub noreply address: `login@` or `id+login@users.noreply.github.com`.
pub fn is_github_noreply(email: &str) -> bool {
    email
        .strip_suffix("@users.noreply.github.com")
        .is_some_and(|local| {
            !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '+')
        })
}

/// GitHub's own committer identity (web merges): accepted as committer only.
fn is_github_committer(person: &Person) -> bool {
    person.name == "GitHub" && person.email == "noreply@github.com"
}

/// The author and committer of the tagged commit are GitHub noreply
/// identities (the committer of a rebase-merged commit is set by GitHub at
/// merge, so only this side can check it).
pub fn identity_problems(commit: &CommitFacts) -> Vec<String> {
    let mut problems = Vec::new();
    if !is_github_noreply(&commit.author.email) {
        problems.push(format!(
            "the author of {} is <{}>, not a GitHub noreply address",
            commit.sha, commit.author.email
        ));
    }
    if !is_github_noreply(&commit.committer.email) && !is_github_committer(&commit.committer) {
        problems.push(format!(
            "the committer of {} is <{}>, not a GitHub noreply address",
            commit.sha, commit.committer.email
        ));
    }
    problems
}

/// The checks of the pushed tag: it is annotated and points at `sha`, its
/// tagger is a GitHub noreply identity, its message is exactly what
/// `release tag` writes, and its version is above the newest other stable
/// tag.
pub fn tag_problems(repo: &Path, tag: &str, sha: &str) -> Res<Vec<String>> {
    let mut problems = Vec::new();
    let parsed = parse_tag(tag)?;
    let tag_ref = format!("refs/tags/{tag}");
    let kind = git::run(repo, &["cat-file", "-t", &tag_ref])
        .map_err(|_| Error::Msg(format!("tag '{tag}' is not available in this checkout")))?;
    if kind.trim() != "tag" {
        problems.push(format!("tag '{tag}' is not an annotated tag"));
        return Ok(problems);
    }
    let target = git::run(repo, &["rev-parse", &format!("{tag_ref}^{{commit}}")])?;
    if target.trim() != sha {
        problems.push(format!(
            "tag '{tag}' points at {}, not at the checked commit {sha}",
            target.trim()
        ));
    }
    let raw = git::run(repo, &["cat-file", "tag", &tag_ref])?;
    let (header, message) = raw.split_once("\n\n").unwrap_or((raw.as_str(), ""));
    let tagger = header
        .lines()
        .find_map(|l| l.strip_prefix("tagger "))
        .unwrap_or("");
    let email = tagger
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map_or("", |(email, _)| email);
    if !is_github_noreply(email) {
        problems.push(format!(
            "the tagger of '{tag}' is <{email}>, not a GitHub noreply address"
        ));
    }
    match crate::check::run_files(&crate::check::Files::Rev(repo, sha), Some(tag)) {
        Ok(report) => {
            let expected = super::tag::message(
                tag,
                &report.version,
                &report.changelog_date,
                &report.changelog_body,
            );
            if message != expected {
                problems.push(format!(
                    "the message of '{tag}' is not the one `cargo xtask release tag` writes (the tag name, then the changelog section); recreate tags only with `release tag`"
                ));
            }
        }
        Err(e) => problems.push(format!("release check of {sha} for '{tag}': {e}")),
    }
    let tags = git::run(repo, &["tag", "--list", "v*"])?;
    let newest_stable = tags
        .lines()
        .map(str::trim)
        .filter(|name| *name != tag)
        .filter_map(|name| parse_tag(name).ok())
        .filter(|t| t.rc.is_none())
        .map(|t| t.version)
        .max();
    if let Some(newest) = newest_stable {
        if parsed.version <= newest {
            problems.push(format!(
                "'{tag}' is not above the newest stable tag v{newest}: an old release commit cannot be published"
            ));
        }
    }
    Ok(problems)
}

/// What the step found: the checks that passed and every problem.
#[derive(Debug, Default)]
pub struct Report {
    pub passed: Vec<String>,
    pub problems: Vec<String>,
}

/// The whole step: the CI proof, the commit identities and, when `tag` is
/// given, the tag checks.
pub fn run(
    repo: &Path,
    commit: &str,
    evidence_dir: &Path,
    tag: Option<&str>,
    accept_identical_pr_head: bool,
) -> Report {
    let mut passed = Vec::new();
    let mut problems = Vec::new();
    let facts = match CommitFacts::from_git(repo, commit) {
        Ok(facts) => facts,
        Err(e) => {
            problems.push(e.to_string());
            return Report { passed, problems };
        }
    };
    if !commit.eq_ignore_ascii_case(&facts.sha) {
        problems.push(format!(
            "--commit must be the full commit hash ({}), not '{commit}'",
            facts.sha
        ));
        return Report { passed, problems };
    }
    match Evidence::load(evidence_dir) {
        Ok(evidence) => match evaluate(&facts, &evidence, accept_identical_pr_head) {
            Ok(proof) => passed.push(proof.describe()),
            Err(e) => problems.push(e),
        },
        Err(e) => problems.push(e.to_string()),
    }
    let identity = identity_problems(&facts);
    if identity.is_empty() {
        passed.push(format!(
            "author and committer of {} are GitHub noreply identities",
            facts.sha
        ));
    }
    problems.extend(identity);
    if let Some(tag) = tag {
        match tag_problems(repo, tag, &facts.sha) {
            Ok(found) if found.is_empty() => passed.push(format!(
                "tag {tag}: annotated, noreply tagger, message written by `release tag`, above the newest stable tag"
            )),
            Ok(found) => problems.extend(found),
            Err(e) => problems.push(e.to_string()),
        }
    }
    Report { passed, problems }
}
