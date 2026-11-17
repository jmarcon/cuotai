//! The commit gate (PRD 0039 RF2; replaces the PRD 0019 RF7.4 gate under
//! the same `changelog-gate` command). Every non-merge commit of a pull
//! request (`base..head`) or of a push to `main` (`before..after`) is
//! checked against the commit contract of [`crate::commits`], and every
//! problem is reported together, once, under the label of its rule:
//!
//! 1. the header is conventional;
//! 2. a user-visible commit touching product paths says what changed
//!    (`Changelog: <sentence>`) or why not (`Changelog: skip <reason>`);
//!    2b. the category trailer is valid;
//! 3. a breaking commit carries its `BREAKING CHANGE:` migration;
//! 4. no commit but a release commit changes a version location, a
//!    workspace lockfile entry or `CHANGELOG.md` (a `docs(changelog)` commit
//!    with `Changelog-Edit:` may correct released prose);
//! 5. a release commit changes only the release file set and passes
//!    `release prepare --check`;
//! 6. the message carries no skip-CI marker anywhere (GitHub honours it in
//!    the subject and the body too);
//! 7. each sentence passes a light lint;
//! 8. every `Refs:` reference is well formed and its plan requirement
//!    exists;
//! 9. author, committer and every `Co-Authored-By:` use a noreply address
//!    (AGENTS.md, "Commit identity"); a failure masks the address.
//!
//! A check added after the cutover would fail commits written before it,
//! which nobody can fix: history is never rewritten. [`RULES_SINCE`] names,
//! for each such check, the last commit of `main` it does not apply to.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::Path;

use crate::commits::{self, Commit, Reference};
use crate::release::{self, Tree};
use crate::{git, Res};

/// The first commit under the PRD 0039 rules is `72fa1d44`: the history
/// audit (`release-preflight` in CI, `preflight --full`) checks every
/// commit from it, with `--base` set to this parent.
pub const HISTORY_BASE: &str = "72fa1d44917dfeb05315769f303210c60f1e60a2^";

/// Scoped checks: `(check id, full sha on main)`. A commit that is that
/// commit or one of its ancestors is exempt from the check. When the commit
/// is not in the clone (a shallow checkout), the check applies to every
/// commit: the gate fails closed. A commit that does not exist yet (the
/// `commit-msg` hook) is never exempt.
pub const RULES_SINCE: &[(&str, &str)] = &[
    // Rule 7, UI paths written with '>' or '→': added by dda445fc
    // (2026-11-15); this is its parent.
    ("ui-path", "57cf7a68c6079a2d578220353449377275fe1eaf"),
];

/// A rule of RF2.1, as the gate labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    Header,
    Entry,
    Category,
    Breaking,
    Versions,
    Release,
    SkipMarker,
    Sentence,
    Refs,
    Identity,
}

impl Rule {
    /// `1`, `2`, `2b`… as printed after "rule".
    pub fn label(self) -> &'static str {
        match self {
            Rule::Header => "1",
            Rule::Entry => "2",
            Rule::Category => "2b",
            Rule::Breaking => "3",
            Rule::Versions => "4",
            Rule::Release => "5",
            Rule::SkipMarker => "6",
            Rule::Sentence => "7",
            Rule::Refs => "8",
            Rule::Identity => "9",
        }
    }

    /// The rule a trailer problem belongs to, from its trailer key.
    fn of_trailer(key: &str) -> Rule {
        match key {
            "Changelog-Category" => Rule::Category,
            "Refs" => Rule::Refs,
            "BREAKING CHANGE" => Rule::Breaking,
            "Changelog-Edit" | "Version-Override" => Rule::Versions,
            _ => Rule::Entry,
        }
    }
}

/// One problem of one commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub rule: Rule,
    pub text: String,
}

impl Problem {
    fn new(rule: Rule, text: impl Into<String>) -> Problem {
        Problem {
            rule,
            text: text.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rule {}: {}", self.rule.label(), self.text)
    }
}

/// Which scoped checks apply to one commit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    exempt: BTreeSet<&'static str>,
}

impl Scope {
    /// Every check applies (a commit that does not exist yet).
    pub fn all() -> Scope {
        Scope::default()
    }

    pub fn applies(&self, check: &str) -> bool {
        !self.exempt.contains(check)
    }
}

/// The ancestors of each `RULES_SINCE` commit, read once per run.
struct Markers<'a> {
    table: &'a [(&'static str, &'a str)],
    /// `None` when the marker is not in the clone.
    ancestors: HashMap<&'a str, Option<HashSet<String>>>,
}

impl<'a> Markers<'a> {
    fn read(repo: &Path, table: &'a [(&'static str, &'a str)]) -> Markers<'a> {
        let mut ancestors = HashMap::new();
        for (_, marker) in table {
            ancestors.entry(*marker).or_insert_with(|| {
                let exists =
                    git::run(repo, &["cat-file", "-e", &format!("{marker}^{{commit}}")]).is_ok();
                if !exists {
                    return None;
                }
                // The same answer as `git merge-base --is-ancestor <commit>
                // <marker>` for every commit, with one process per marker.
                git::run(repo, &["rev-list", marker])
                    .ok()
                    .map(|list| list.lines().map(str::to_string).collect())
            });
        }
        Markers { table, ancestors }
    }

    fn scope(&self, sha: &str) -> Scope {
        let exempt = self
            .table
            .iter()
            .filter(|(_, marker)| {
                self.ancestors
                    .get(marker)
                    .and_then(Option::as_ref)
                    .is_some_and(|set| set.contains(sha))
            })
            .map(|(check, _)| *check)
            .collect();
        Scope { exempt }
    }
}

/// Paths whose change is user-visible (shipped code, licenses, notices).
fn is_product(path: &str) -> bool {
    path.starts_with("crates/")
        || path.starts_with("apps/")
        || path.starts_with("LICENSE")
        || path == "THIRD_PARTY_NOTICES"
}

/// RF2.2: the base of a push to `main` is its previous tip. When that commit
/// is not in the checkout (a force push), it is fetched from `origin`; when
/// it still cannot be recovered, or the push created the branch, the gate
/// fails closed instead of checking an empty range.
pub fn push_base(repo: &Path, before: &str) -> Res<String> {
    if before.is_empty() || before.bytes().all(|b| b == b'0') {
        return crate::err(
            "the push has no previous tip (it created the branch): the commit gate cannot \
             tell which commits are new; run it with an explicit --base",
        );
    }
    if !before.bytes().all(|b| b.is_ascii_hexdigit()) {
        return crate::err(format!("--before '{before}' is not a commit hash"));
    }
    let exists =
        |repo: &Path| git::run(repo, &["cat-file", "-e", &format!("{before}^{{commit}}")]).is_ok();
    if !exists(repo) {
        let _ = git::run(repo, &["fetch", "--no-tags", "--quiet", "origin", before]);
    }
    if !exists(repo) {
        return crate::err(format!(
            "the previous tip {before} of this push is not available (force push?) and could \
             not be fetched: the commit gate fails closed. Check the pushed commits by hand \
             with `cargo xtask changelog-gate --base <last good commit> --head <new tip>`"
        ));
    }
    Ok(before.to_string())
}

/// A problem of the range, with its commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub short: String,
    pub subject: String,
    pub problem: Problem,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.short, self.subject, self.problem)
    }
}

/// Checks `base..head`; returns how many commits were checked.
pub fn run(repo: &Path, base: &str, head: &str) -> Res<usize> {
    run_with(repo, base, head, RULES_SINCE)
}

/// [`run`] with another scoping table (tests).
pub fn run_with(
    repo: &Path,
    base: &str,
    head: &str,
    rules_since: &[(&'static str, &str)],
) -> Res<usize> {
    let commits = commits::load(repo, &format!("{base}..{head}"))?;
    let plans = crate::plan_links::Plans::read(repo).ok();
    let markers = Markers::read(repo, rules_since);
    let mut findings: Vec<Finding> = Vec::new();
    for commit in &commits {
        let files = commits::files(repo, &commit.sha)?;
        let scope = markers.scope(&commit.sha);
        for problem in check_commit(repo, commit, &files, plans.as_ref(), &scope)? {
            findings.push(Finding {
                short: commit.short().to_string(),
                subject: commit.subject.clone(),
                problem,
            });
        }
    }
    if findings.is_empty() {
        return Ok(commits.len());
    }
    if std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true") {
        print!("{}", annotations(&findings));
        if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            use std::io::Write;
            let file = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path);
            if let Ok(mut file) = file {
                let _ = file.write_all(summary_table(&findings).as_bytes());
            }
        }
    }
    let rules: BTreeSet<Rule> = findings.iter().map(|f| f.problem.rule).collect();
    let list: Vec<String> = findings.iter().map(Finding::to_string).collect();
    crate::err(format!(
        "commit gate failed ({} problem(s)):\n - {}\n\n{}",
        list.len(),
        list.join("\n - "),
        footer(&rules)
    ))
}

/// What to do next, for the rules that failed.
pub fn footer(rules: &BTreeSet<Rule>) -> String {
    let mut lines = Vec::new();
    let message_rules = [
        Rule::Header,
        Rule::Entry,
        Rule::Category,
        Rule::Breaking,
        Rule::SkipMarker,
        Rule::Sentence,
        Rule::Refs,
    ];
    if message_rules.iter().any(|rule| rules.contains(rule)) {
        lines.push(
            "Rules 1-3 and 6-8: amend the commit message on your branch (`git commit --amend` \
             for the last commit, `git rebase --exec` for older ones) and push again."
                .to_string(),
        );
    }
    if rules.contains(&Rule::Versions) {
        lines.push(
            "Rule 4: drop the version or CHANGELOG.md hunk from the commit (`git checkout \
             origin/main -- <file>`, then `git commit --amend`): versions and entries are \
             written by `cargo xtask release prepare`."
                .to_string(),
        );
    }
    if rules.contains(&Rule::Identity) {
        lines.push(
            "Rule 9: set the repository's user.email to your GitHub noreply address, then \
             `git commit --amend --reset-author --no-edit` (or `git rebase --exec 'git commit \
             --amend --reset-author --no-edit' origin/main`); fix a co-author line by amending \
             the message."
                .to_string(),
        );
    }
    if rules.contains(&Rule::Release) {
        lines.push(
            "Rule 5: prepare the release commit again with `cargo xtask release prepare \
             --branch` instead of editing it by hand."
                .to_string(),
        );
    }
    lines.push("AGENTS.md (\"How to deliver a change\") has the template.".to_string());
    lines.join("\n")
}

/// `%`, CR and LF escaped as a workflow command's data.
fn escape_data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// One `::error` workflow command per finding (GitHub Actions annotations).
pub fn annotations(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            format!(
                "::error title=commit gate (rule {})::{}\n",
                f.problem.rule.label(),
                escape_data(&format!("{} {}: {}", f.short, f.subject, f.problem.text))
            )
        })
        .collect()
}

/// A Markdown table of the findings for the job summary.
pub fn summary_table(findings: &[Finding]) -> String {
    let cell = |text: &str| {
        text.replace('|', "\\|")
            .replace("\r\n", "<br>")
            .replace('\n', "<br>")
    };
    let mut out = format!(
        "### Commit gate: {} problem(s)\n\n| Commit | Rule | Problem |\n|---|---|---|\n",
        findings.len()
    );
    for f in findings {
        out.push_str(&format!(
            "| `{}` {} | {} | {} |\n",
            f.short,
            cell(&f.subject),
            f.problem.rule.label(),
            cell(&f.problem.text)
        ));
    }
    out.push('\n');
    out
}

/// Whether `commit` may change `CHANGELOG.md`: a `docs(changelog)` commit with
/// `Changelog-Edit:` (the content of that edit is checked against its tree).
fn may_edit_changelog(commit: &Commit) -> bool {
    commit
        .header
        .as_ref()
        .is_ok_and(|h| h.kind == "docs" && h.scope.as_deref() == Some("changelog"))
        && commit.trailers.changelog_edit.is_some()
}

/// Keeps the first of identical problems, in order.
fn dedup(problems: Vec<Problem>) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    for problem in problems {
        if !out.contains(&problem) {
            out.push(problem);
        }
    }
    out
}

/// The rules that need only the message and the changed paths, not a commit
/// object: the `commit-msg` hook runs them before the commit exists.
pub fn check_message(
    commit: &Commit,
    files: &[String],
    plans: Option<&crate::plan_links::Plans>,
    scope: &Scope,
) -> Vec<Problem> {
    let mut out = Vec::new();
    if let Err(problem) = &commit.header {
        out.push(Problem::new(Rule::Header, problem.clone()));
    }
    out.extend(
        commit
            .problems
            .iter()
            .map(|p| Problem::new(Rule::of_trailer(p.key), p.text.clone())),
    );

    if commit.release_version().is_none() {
        // Rule 2, unless a trailer problem of rule 2 already explains it
        // (a misspelt key, a skip without a reason).
        if commit.is_user_visible()
            && files.iter().any(|f| is_product(f))
            && commit.trailers.sentences.is_empty()
            && commit.trailers.skip.is_none()
            && !out.iter().any(|p| p.rule == Rule::Entry)
        {
            out.push(Problem::new(
                Rule::Entry,
                "user-visible change without a changelog entry: add a \
                 'Changelog: <one sentence for the user>.' trailer, or 'Changelog: skip <reason>'",
            ));
        }
        // Rule 3 (an empty footer is already a trailer problem).
        if commit.is_breaking()
            && commit.trailers.breaking.is_none()
            && !out.iter().any(|p| p.rule == Rule::Breaking)
        {
            out.push(Problem::new(
                Rule::Breaking,
                "breaking commit without its migration: add 'BREAKING CHANGE: <what users must change>'",
            ));
        }
        // Rule 4, the parts visible without a tree.
        if files.iter().any(|f| f == "CHANGELOG.md") && !may_edit_changelog(commit) {
            out.push(Problem::new(
                Rule::Versions,
                "changes CHANGELOG.md: entries are generated at release time from the \
                 'Changelog:' trailer; drop the CHANGELOG.md hunk (a prose fix of a released \
                 section is a 'docs(changelog)' commit with 'Changelog-Edit: <reason>')",
            ));
        }
        if commit.trailers.version_override.is_some() {
            out.push(Problem::new(
                Rule::Versions,
                "'Version-Override:' belongs only in a release commit",
            ));
        }
    }
    // Rule 6, anywhere in the message.
    if let Some(problem) = crate::preflight::message::skip_marker_problem(commit) {
        out.push(Problem::new(Rule::SkipMarker, problem));
    }
    // Rule 9, the co-authors (author and committer need the commit object).
    out.extend(
        crate::preflight::message::co_author_problems(commit)
            .into_iter()
            .map(|p| Problem::new(Rule::Identity, p)),
    );
    // Rule 7.
    let applies = |check: &str| scope.applies(check);
    for sentence in &commit.trailers.sentences {
        out.extend(
            commits::lint_sentence_with(sentence, &applies)
                .into_iter()
                .map(|p| Problem::new(Rule::Sentence, p)),
        );
    }
    // Rule 8.
    if let Some(plans) = plans {
        for reference in &commit.trailers.refs {
            if let Reference::Plan { prd, rf } = reference {
                if let Some(problem) = plans.missing(prd, rf) {
                    out.push(Problem::new(
                        Rule::Refs,
                        format!("Refs: PRD {prd} {rf}: {problem}"),
                    ));
                }
            }
        }
    }
    dedup(out)
}

/// Every problem of one commit, without the commit prefix.
fn check_commit(
    repo: &Path,
    commit: &Commit,
    files: &[String],
    plans: Option<&crate::plan_links::Plans>,
    scope: &Scope,
) -> Res<Vec<Problem>> {
    let mut out = check_message(commit, files, plans, scope);
    // Rule 9.
    out.extend(
        crate::preflight::message::identity_problems(&commit.author_email, &commit.committer_email)
            .into_iter()
            .map(|p| Problem::new(Rule::Identity, p)),
    );
    if commit.release_version().is_none() {
        // Rule 4.
        for change in version_changes(repo, &commit.sha, files)? {
            out.push(Problem::new(
                Rule::Versions,
                format!(
                    "{change}; versions move only in the release commit of \
                     `cargo xtask release prepare`: drop this hunk"
                ),
            ));
        }
        if files.iter().any(|f| f == "CHANGELOG.md") && may_edit_changelog(commit) {
            if let Some(problem) = changelog_edit_problem(repo, &commit.sha)? {
                out.push(Problem::new(Rule::Versions, problem));
            }
        }
    } else {
        // Rule 5.
        for file in files.iter().filter(|f| !release::in_release_set(f)) {
            out.push(Problem::new(
                Rule::Release,
                format!("a release commit changes only the release file set, not {file}"),
            ));
        }
        if let Err(problem) = release::check_release(repo, &commit.sha) {
            out.push(Problem::new(Rule::Release, problem.to_string()));
        }
    }
    Ok(dedup(out))
}

/// The version values a commit changes (RF1.3), described for the message.
fn version_changes(repo: &Path, sha: &str, files: &[String]) -> Res<Vec<String>> {
    type Extract = fn(&str) -> Vec<String>;
    let watched: [(&str, Extract); 6] = [
        ("Cargo.toml", |t| {
            toml_path(t, &["workspace", "package", "version"])
        }),
        ("apps/desktop/src-tauri/Cargo.toml", |t| {
            toml_path(t, &["package", "version"])
        }),
        ("apps/desktop/src-tauri/tauri.conf.json", |t| {
            serde_json::from_str::<serde_json::Value>(t)
                .ok()
                .and_then(|v| v.get("version")?.as_str().map(str::to_string))
                .into_iter()
                .collect()
        }),
        ("crates/cuotai-tui/Cargo.toml", |t| {
            toml_path(t, &["dependencies", "cuotai-core", "version"])
        }),
        ("Cargo.lock", |t| {
            lock_versions(t, &["cuotai-core", "cuotai-tui"])
        }),
        ("apps/desktop/src-tauri/Cargo.lock", |t| {
            lock_versions(t, &["cuotai-core", "cuotai-desktop"])
        }),
    ];
    let parent = format!("{sha}^");
    if git::run(repo, &["rev-parse", "--verify", "--quiet", &parent]).is_err() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (path, extract) in watched {
        if !files.iter().any(|f| f == path) {
            continue;
        }
        let before = Tree::Rev(repo, parent.clone())
            .read(path)
            .map(|t| extract(&t))
            .unwrap_or_default();
        let after = Tree::Rev(repo, sha.to_string())
            .read(path)
            .map(|t| extract(&t))
            .unwrap_or_default();
        if before != after {
            out.push(format!(
                "changes the version in {path} ({} -> {})",
                before.join(", "),
                after.join(", ")
            ));
        }
    }
    Ok(out)
}

fn toml_path(text: &str, keys: &[&str]) -> Vec<String> {
    let Ok(doc) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let mut value = &doc;
    for key in keys {
        let Some(next) = value.get(key) else {
            return Vec::new();
        };
        value = next;
    }
    value.as_str().map(str::to_string).into_iter().collect()
}

fn lock_versions(text: &str, names: &[&str]) -> Vec<String> {
    let Ok(doc) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let Some(packages) = doc.get("package").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    names
        .iter()
        .flat_map(|name| {
            packages
                .iter()
                .filter(move |p| p.get("name").and_then(|n| n.as_str()) == Some(*name))
                .filter_map(move |p| {
                    p.get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("{name} {v}"))
                })
        })
        .collect()
}

/// A `Changelog-Edit` commit may reword released sections and the
/// preface/footer, but never touch `[Unreleased]`, add or remove a section
/// or change how many entries a section has. The one structural edit it may
/// make is folding internal versions into the pre-release history.
fn changelog_edit_problem(repo: &Path, sha: &str) -> Res<Option<String>> {
    let parse = |rev: String| -> Option<crate::changelog::Changelog> {
        let text = Tree::Rev(repo, rev).read("CHANGELOG.md").ok()?;
        crate::changelog::parse(&text).ok()
    };
    let (Some(before), Some(after)) = (parse(format!("{sha}^")), parse(sha.to_string())) else {
        return Ok(Some(
            "the CHANGELOG.md edit leaves a file the shared parser rejects".to_string(),
        ));
    };
    if before.unreleased != after.unreleased {
        return Ok(Some(
            "a Changelog-Edit commit must not touch [Unreleased]".to_string(),
        ));
    }
    let shape = |c: &crate::changelog::Changelog| -> Vec<(String, usize)> {
        c.releases
            .iter()
            .map(|r| {
                (
                    r.version.clone(),
                    r.section.categories.iter().map(|b| b.entries.len()).sum(),
                )
            })
            .collect()
    };
    // The pre-release history (PRD 0039 D11) may be introduced, folding the
    // internal versions' sections into it, and reworded freely (it is a
    // summary), but never removed, re-ranged or widened over a published
    // version.
    match (&before.pre_release, &after.pre_release) {
        (Some(_), None) => {
            return Ok(Some(
                "a Changelog-Edit commit must not remove the pre-release history".to_string(),
            ))
        }
        (Some(old), Some(new)) if (&old.first, &old.last) != (&new.first, &new.last) => {
            return Ok(Some(format!(
                "a Changelog-Edit commit must not change the pre-release range ({} -> {})",
                old.range(),
                new.range()
            )))
        }
        _ => {}
    }
    let folded = |version: &str| after.pre_release_of(version).is_some();
    for release in before.releases.iter().filter(|r| folded(&r.version)) {
        if !crate::Version::parse(&release.version)
            .is_ok_and(|v| v <= release::LAST_INTERNAL_VERSION)
        {
            return Ok(Some(format!(
                "[{}] is a published version: only internal versions (up to {}) fold into the pre-release history",
                release.version,
                release::LAST_INTERNAL_VERSION
            )));
        }
    }
    let mut kept = shape(&before);
    kept.retain(|(version, _)| !folded(version));
    if kept != shape(&after) {
        return Ok(Some(
            "a Changelog-Edit commit may reword released entries but not add or remove sections or entries"
                .to_string(),
        ));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_paths() {
        assert!(is_product("crates/cuotai-core/src/lib.rs"));
        assert!(is_product("apps/desktop/ui/index.html"));
        assert!(is_product("LICENSE-MIT"));
        assert!(is_product("THIRD_PARTY_NOTICES"));
        assert!(!is_product("docs/plans/0000_control.md"));
        assert!(!is_product("xtask/src/main.rs"));
    }

    fn message(subject: &str, body: &str) -> Commit {
        Commit::parse(
            "0123456789abcdef0123456789abcdef01234567",
            subject,
            &format!("{subject}\n\n{body}"),
        )
    }

    fn problems(subject: &str, body: &str) -> Vec<String> {
        let files = ["crates/cuotai-core/src/lib.rs".to_string()];
        check_message(&message(subject, body), &files, None, &Scope::all())
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn labels_follow_rf2_1() {
        let labels: Vec<&str> = [
            Rule::Header,
            Rule::Entry,
            Rule::Category,
            Rule::Breaking,
            Rule::Versions,
            Rule::Release,
            Rule::SkipMarker,
            Rule::Sentence,
            Rule::Refs,
            Rule::Identity,
        ]
        .iter()
        .map(|rule| rule.label())
        .collect();
        assert_eq!(labels, ["1", "2", "2b", "3", "4", "5", "6", "7", "8", "9"]);
        let refs = problems("fix(core): a", "Changelog: A fix.\nRefs: something");
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert!(refs[0].starts_with("rule 8: "), "{refs:?}");
        let category = problems(
            "fix(core): a",
            "Changelog: A fix.\nChangelog-Category: Features",
        );
        assert!(category[0].starts_with("rule 2b: "), "{category:?}");
        let breaking = problems("fix(core)!: a", "Changelog: A fix.\nBREAKING CHANGE:");
        assert_eq!(breaking.len(), 1, "{breaking:?}");
        assert!(breaking[0].starts_with("rule 3: "), "{breaking:?}");
    }

    #[test]
    fn each_problem_is_said_once() {
        // A misspelt key explains the missing entry.
        let lower = problems("fix(core): a", "changelog: A fix.");
        assert_eq!(lower.len(), 1, "{lower:?}");
        assert!(lower[0].contains("case-sensitive"), "{lower:?}");
        // So does a skip without a reason.
        let bare = problems("fix(core): a", "Changelog: skip");
        assert_eq!(bare.len(), 1, "{bare:?}");
        assert!(bare[0].starts_with("rule 2: ") && bare[0].contains("needs a reason"));
        // A marker in a sentence is rule 6 only.
        let marker = format!("[{} {}]", "skip", "ci");
        let found = problems("fix(core): a", &format!("Changelog: A fix {marker}."));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("rule 6: "), "{found:?}");
        // The same sentence twice is one problem.
        let twice = problems("fix(core): a", "Changelog: no period\nChangelog: no period");
        assert_eq!(twice.len(), 2, "{twice:?}");
    }

    #[test]
    fn scoped_checks_can_be_exempted() {
        let commit = message("fix(core): a", "Changelog: Open Settings > About.");
        let files = ["crates/cuotai-core/src/lib.rs".to_string()];
        assert_eq!(check_message(&commit, &files, None, &Scope::all()).len(), 1);
        let old = Scope {
            exempt: BTreeSet::from(["ui-path"]),
        };
        assert!(check_message(&commit, &files, None, &old).is_empty());
    }

    #[test]
    fn every_scoped_check_names_a_full_sha() {
        for (check, sha) in RULES_SINCE {
            assert!(!check.is_empty());
            assert_eq!(sha.len(), 40, "{check}");
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()), "{check}");
        }
    }

    #[test]
    fn ci_audits_the_same_history() {
        let ci = include_str!("../../.github/workflows/ci.yml");
        let cutover = HISTORY_BASE.trim_end_matches('^');
        assert!(
            ci.contains(&format!("CUTOVER: {cutover}")),
            "ci.yml must audit the history from {HISTORY_BASE}"
        );
    }

    #[test]
    fn the_footer_fits_the_rules_hit() {
        let only = |rules: &[Rule]| footer(&rules.iter().copied().collect());
        let message_only = only(&[Rule::Sentence]);
        assert!(
            message_only.contains("git commit --amend"),
            "{message_only}"
        );
        assert!(!message_only.contains("Rule 4"), "{message_only}");
        let versions = only(&[Rule::Versions]);
        assert!(versions.contains("Rule 4: drop the version"), "{versions}");
        assert!(!versions.contains("Rules 1-3"), "{versions}");
        let release = only(&[Rule::Release]);
        assert!(release.contains("release prepare"), "{release}");
        assert!(release.ends_with("has the template."), "{release}");
    }

    fn finding(rule: Rule, text: &str) -> Finding {
        Finding {
            short: "abc1234".to_string(),
            subject: "fix(core): a | b".to_string(),
            problem: Problem::new(rule, text),
        }
    }

    #[test]
    fn annotations_and_the_summary() {
        let findings = [
            finding(Rule::Category, "line one\nline two 100%"),
            finding(Rule::Header, "bad"),
        ];
        assert_eq!(
            annotations(&findings),
            "::error title=commit gate (rule 2b)::abc1234 fix(core): a | b: line one%0Aline two 100%25\n\
             ::error title=commit gate (rule 1)::abc1234 fix(core): a | b: bad\n"
        );
        let table = summary_table(&findings);
        assert!(table.contains("| Commit | Rule | Problem |"), "{table}");
        assert!(
            table.contains("| `abc1234` fix(core): a \\| b | 2b | line one<br>line two 100% |"),
            "{table}"
        );
    }

    #[test]
    fn lock_and_toml_extraction() {
        let lock = "[[package]]\nname = \"cuotai-core\"\nversion = \"0.79.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n";
        assert_eq!(
            lock_versions(lock, &["cuotai-core", "cuotai-tui"]),
            ["cuotai-core 0.79.0"]
        );
        let tui = "[dependencies]\ncuotai-core = { path = \"x\", version = \"0.79\" }\n";
        assert_eq!(
            toml_path(tui, &["dependencies", "cuotai-core", "version"]),
            ["0.79"]
        );
    }
}
