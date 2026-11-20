//! `cargo xtask release prepare` (PRD 0039 RF4): computes the next version
//! from the commits since the release base, writes the release file set
//! (version locations, both lockfiles, the `CHANGELOG.md` section, the
//! plans' `vNEXT` tokens and the plan control), checks the result and, with
//! `--branch`, commits it as `chore(release): X.Y.Z` on `release/vX.Y.Z`.
//! It never pushes, tags or opens a pull request (NFR5).

use std::path::Path;
use std::process::Command;

use super::render::{self, Entries};
use super::{
    bump_of, pending_release, release_base, release_commits, workspace_version, Base, Tree,
};
use crate::commits::{self, Bump, EntryEdit};
use crate::{err, git, Error, Res, Version};

/// The next step after a release PR merged (F-06): a stable tag unless the
/// owner asked for a release candidate (owner decision of 2026-11-05).
pub const TAG_HINT: &str =
    "`cargo xtask release tag` (add `--rc` only when the owner asked for an rc)";

/// Command-line options of `release prepare`.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// `--version X.Y.Z` (RF3.4).
    pub version: Option<Version>,
    /// `--date YYYY-MM-DD`; today in UTC by default.
    pub date: String,
    pub dry_run: bool,
    pub branch: bool,
    pub refresh: bool,
    /// Run `cargo metadata --locked --offline` on both workspaces after
    /// writing (off in the temp-repository tests).
    pub verify_lockfiles: bool,
    /// The recorded wording edits (`Changelog-Entry:`). `None` keeps those
    /// of the prepared release being refreshed (none for a new release).
    pub edits: Option<Vec<EntryEdit>>,
    /// Extra `Key: value` trailers of the release commit (a session's
    /// `Co-Authored-By:`), after the generated ones.
    pub trailers: Vec<String>,
}

/// One file the release writes: path, current and new content.
#[derive(Clone, Debug)]
pub struct FileChange {
    pub path: String,
    pub old: String,
    pub new: String,
}

/// What `prepare` computed (and, unless `--dry-run`, wrote).
#[derive(Debug)]
pub struct Outcome {
    pub base: Base,
    pub current: Version,
    pub version: Version,
    pub bump: Bump,
    pub rationale: Vec<String>,
    pub section: String,
    pub changes: Vec<FileChange>,
    pub warnings: Vec<String>,
    /// The commit message (`--branch`, or what it would be).
    pub message: String,
    /// The release branch, when one was created.
    pub branch: Option<String>,
    /// The wording edits applied to the section.
    pub edits: Vec<EntryEdit>,
    /// `<short> <subject>: <reason>` for every `Changelog: skip` commit.
    pub skipped: Vec<String>,
    /// The release PR body written next to the release commit (`--branch`).
    pub pr_body: Option<std::path::PathBuf>,
}

/// A release commit re-rendered from its parent (the line-level check and
/// `release edit`): its version and whether it records a
/// `Version-Override:`, instead of the computed version.
#[derive(Clone, Debug)]
pub(crate) struct Replay {
    pub version: Version,
    pub overridden: bool,
}

/// Computes the release; writes it unless `--dry-run`. `Ok(None)` means
/// "nothing to release" (RF3.5).
pub fn run(repo: &Path, opts: &Options) -> Res<Option<Outcome>> {
    // Before anything is computed: the release file set must match HEAD. A
    // hand-edited version location would otherwise become the "current"
    // version and silently drop out of the files the release writes and
    // commits. The dry run checks the version locations and the changelog
    // too (it computes from them); a write also needs every plan file
    // tracked and clean.
    let mut status_args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    status_args.extend(super::RELEASE_FILES);
    if !opts.dry_run {
        status_args.extend(["README.md", "docs"]);
    }
    let dirty: Vec<String> = git::run(repo, &status_args)?
        .lines()
        .filter(|line| {
            let path = line.get(3..).unwrap_or_default().trim();
            super::in_release_set(path)
        })
        .map(str::to_string)
        .collect();
    if !dirty.is_empty() {
        return err(format!(
            "these release files differ from HEAD (modified, staged or untracked); versions and the changelog are written only by release prepare, so commit, discard or move them first:\n{}",
            dirty.join("\n")
        ));
    }
    for trailer in &opts.trailers {
        check_trailer(trailer)?;
    }
    let Some(mut outcome) = compute(repo, &Tree::Work(repo), "HEAD", opts, None)? else {
        return Ok(None);
    };
    if opts.dry_run {
        return Ok(Some(outcome));
    }

    let paths: Vec<&str> = outcome.changes.iter().map(|c| c.path.as_str()).collect();
    // Every path the release writes must be tracked and unmodified: an
    // untracked file (another worktree user's draft with `vNEXT`) is
    // refused, never adopted.
    let mut status_args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    status_args.extend(paths.iter().copied());
    let dirty = git::run(repo, &status_args)?;
    if !dirty.trim().is_empty() {
        return err(format!(
            "these release files are modified or untracked; commit, discard or move them first:\n{}",
            dirty.trim_end()
        ));
    }
    if opts.branch {
        let staged = git::run(repo, &["diff", "--cached", "--name-only"])?;
        if !staged.trim().is_empty() {
            return err(format!(
                "the index already holds staged changes, which the release commit would include; unstage them first:\n{}",
                staged.trim_end()
            ));
        }
    }
    let branch = format!("release/v{}", outcome.version);
    if opts.branch && git::run(repo, &["rev-parse", "--verify", "--quiet", &branch]).is_ok() {
        return err(format!(
            "branch {branch} already exists; delete it or check it out and use --refresh"
        ));
    }
    write_changes(repo, &outcome.changes)?;
    if opts.verify_lockfiles {
        verify_lockfiles(repo)?;
    }
    crate::check::run(repo, None, None)?;
    crate::plan_links::run(repo, &Tree::Work(repo).read("CHANGELOG.md")?)?;
    if opts.branch {
        git::run(repo, &["switch", "-c", &branch])?;
        let mut add = vec!["add", "--"];
        add.extend(paths.iter().copied());
        git::run(repo, &add)?;
        let mut only = vec!["--"];
        only.extend(paths.iter().copied());
        super::git_with_message(
            repo,
            "CUOTAI_RELEASE_MSG",
            &outcome.message,
            &["commit", "--cleanup=verbatim"],
            &only,
        )?;
        outcome.branch = Some(branch);
        outcome.pr_body = Some(write_pr_body(repo, &outcome)?);
    }
    Ok(Some(outcome))
}

/// Trailer keys `release prepare` writes itself, or that belong only to
/// other commits: `--trailer` cannot set them.
const RESERVED_TRAILERS: [&str; 7] = [
    "Changelog",
    "Changelog-Category",
    "Changelog-Entry",
    "Changelog-Edit",
    "Version-Override",
    "Refs",
    "BREAKING-CHANGE",
];

/// A `--trailer "Key: value"` of the release commit (F-13): one line, a
/// token key that `release prepare` does not write itself, and every
/// e-mail address in the value a noreply one (AGENTS.md, "Commit
/// identity"). Returns the trailer as it is written.
pub fn check_trailer(text: &str) -> Res<String> {
    let text = text.trim();
    if text.contains(['\n', '\r']) {
        return err("--trailer takes one line, 'Key: value'");
    }
    let Some((key, value)) = text.split_once(": ") else {
        return err(format!(
            "--trailer '{text}' is not 'Key: value' (e.g. 'Co-Authored-By: Name <noreply@example.com>')"
        ));
    };
    let valid_key = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !valid_key || value.trim().is_empty() {
        return err(format!(
            "--trailer '{text}' is not 'Key: value' (a key of letters, digits and '-', then a value)"
        ));
    }
    if let Some(reserved) = RESERVED_TRAILERS
        .iter()
        .find(|r| r.eq_ignore_ascii_case(key))
    {
        return err(format!(
            "--trailer cannot set '{reserved}:': release prepare writes the release commit's own trailers"
        ));
    }
    for word in value.split(|c: char| c.is_whitespace() || "<>(),;\"'".contains(c)) {
        if word.contains('@') && !crate::preflight::message::is_noreply(word) {
            return err(format!(
                "--trailer '{key}' uses {}: commit identities use a noreply address (AGENTS.md, \"Commit identity\")",
                crate::preflight::message::mask(word)
            ));
        }
    }
    Ok(text.to_string())
}

/// Where the release PR body goes: `$(git rev-parse --git-path
/// cuotai-release-pr-body.md)`, inside the repository's own Git directory
/// (never a shared temporary name, and per worktree).
pub fn pr_body_path(repo: &Path) -> Res<std::path::PathBuf> {
    let path = git::run(
        repo,
        &["rev-parse", "--git-path", "cuotai-release-pr-body.md"],
    )?;
    let path = std::path::PathBuf::from(path.trim());
    Ok(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}

/// Writes [`pr_body`] to [`pr_body_path`].
pub fn write_pr_body(repo: &Path, outcome: &Outcome) -> Res<std::path::PathBuf> {
    let path = pr_body_path(repo)?;
    std::fs::write(&path, pr_body(repo, outcome))?;
    Ok(path)
}

/// The heading of the per-release checklist in `docs/releasing.md`, the one
/// source of the list the release PR body repeats (F-07).
pub const CHECKLIST_HEADING: &str = "## Manual desktop checklist per release";

/// The release PR body (F-07, RF4.5): the bump rationale, the wording
/// edits, the commits skipped with a reason, the section, then the manual
/// checklist of `docs/releasing.md` under a heading that says it is no
/// merge condition.
pub fn pr_body(repo: &Path, outcome: &Outcome) -> String {
    let mut out = format!(
        "## Release {}\n\n{} bump from {}.\n\n",
        outcome.version,
        outcome.bump.as_str(),
        outcome.base.describe()
    );
    out.push_str("### Bump rationale\n\n");
    if outcome.rationale.is_empty() {
        out.push_str("No commit raises the version (an explicit `--version`).\n");
    }
    for line in &outcome.rationale {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str("\n### Wording edits\n\n");
    if outcome.edits.is_empty() {
        out.push_str(
            "None. Fix wording with `cargo xtask release edit <hash>#<n> --text \"<sentence>\"`.\n",
        );
    }
    for edit in &outcome.edits {
        out.push_str(&format!("- `Changelog-Entry: {edit}`\n"));
    }
    out.push_str("\n### Skipped with a reason (no entry)\n\n");
    if outcome.skipped.is_empty() {
        out.push_str("None.\n");
    }
    for line in &outcome.skipped {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str("\n### Changelog section\n\n");
    for line in outcome.section.trim_end().lines() {
        // One level down, under this body's own headings.
        if line.starts_with('#') {
            out.push('#');
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\n## After publication, by a person (not a merge condition)\n\n");
    if desktop_or_packaging_changed(repo, &outcome.base) == Some(false) {
        out.push_str(
            "None for this release: no commit since the base changed `apps/desktop/` or `packaging/` (PRD 0039 RF4.5, owner decision B-9).\n",
        );
        return commits::neutralize_skip_markers(&out)
            .trim_end()
            .to_string()
            + "\n";
    }
    match Tree::Work(repo)
        .read("docs/releasing.md")
        .ok()
        .and_then(|doc| checklist(&doc))
    {
        Some(list) => out.push_str(&list),
        None => out.push_str(&format!(
            "See `docs/releasing.md` (no \"{}\" section was found).\n",
            CHECKLIST_HEADING
                .trim_start_matches("# ")
                .trim_start_matches('#')
                .trim()
        )),
    }
    // The neutralizer drops the final line break; a file ends with one.
    commits::neutralize_skip_markers(&out)
        .trim_end()
        .to_string()
        + "\n"
}

/// Owner decision B-9 (2026-11-20, PRD 0039 RF4.5): the manual desktop
/// checklist is due only after a release that changed `apps/desktop/` or
/// packaging (`packaging/`). Whether a commit since `base` changed such a
/// path, the release files themselves left out (the release commit always
/// writes the desktop version files). `None` when Git cannot tell, and the
/// checklist is then kept.
pub fn desktop_or_packaging_changed(repo: &Path, base: &super::Base) -> Option<bool> {
    let range = format!("{}..HEAD", base.sha);
    let names = git::run(
        repo,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            &range,
            "--",
            "apps/desktop",
            "packaging",
        ],
    )
    .ok()?;
    Some(
        names
            .lines()
            .map(str::trim)
            .any(|path| !path.is_empty() && !super::in_release_set(path)),
    )
}

/// The `- [ ]` items of the checklist section of `docs/releasing.md`, with
/// their continuation lines; links are made relative to the repository
/// root, as a pull request body resolves them.
pub fn checklist(doc: &str) -> Option<String> {
    let start = doc
        .lines()
        .position(|l| l.trim_end() == CHECKLIST_HEADING)?;
    let mut out = String::new();
    let mut in_item = false;
    for line in doc.lines().skip(start + 1) {
        if line.starts_with("## ") {
            break;
        }
        if line.starts_with("- [ ]") {
            in_item = true;
        } else if line.trim().is_empty() || !line.starts_with(' ') {
            in_item = false;
        }
        if in_item {
            let line = line.replace("](#", "](docs/releasing.md#");
            out.push_str(&relative_doc_links(&line));
            out.push('\n');
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `](name.md…)` → `](docs/name.md…)` for links between documents of
/// `docs/`.
fn relative_doc_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(index) = rest.find("](") {
        out.push_str(&rest[..index + 2]);
        rest = &rest[index + 2..];
        let target_end = rest.find(')').unwrap_or(rest.len());
        let target = &rest[..target_end];
        let local = !target.contains("://")
            && !target.starts_with('#')
            && !target.starts_with('/')
            && !target.starts_with("docs/")
            && !target.starts_with("../");
        if local {
            out.push_str("docs/");
        }
    }
    out.push_str(rest);
    out
}

/// Writes every change atomically.
pub(crate) fn write_changes(repo: &Path, changes: &[FileChange]) -> Res<()> {
    for change in changes {
        write_atomic(&repo.join(&change.path), &change.new)?;
    }
    Ok(())
}

/// The release computed from `tree` (the working tree, or the parent of a
/// release commit being re-rendered) and the history of `rev`, written
/// nowhere. Every file is produced by deterministic text edits, so the same
/// inputs give byte-identical files: the line-level release check relies
/// on it.
pub(crate) fn compute(
    repo: &Path,
    tree: &Tree,
    rev: &str,
    opts: &Options,
    replay: Option<&Replay>,
) -> Res<Option<Outcome>> {
    let current = workspace_version(&tree.read("Cargo.toml")?)?;
    let mut warnings = Vec::new();
    if opts.refresh && opts.version.is_some() {
        return err("--refresh keeps the prepared version; it cannot be combined with --version");
    }
    // The base is always published (a stable tag or the internal baseline);
    // a release prepared after it but not tagged yet is `pending`, and its
    // section is refreshed instead of getting a second one.
    let base = release_base(repo, rev, replay.map(|r| &r.version))?;
    let pending = pending_release(repo, &base, rev, &current)?;
    if current > base.version && pending.is_none() {
        return err(format!(
            "{rev} declares {current}, above the release base {}, but no 'chore(release): {current}' commit prepared it: \
             a version was raised by hand; revert that change and let release prepare compute the version",
            base.describe()
        ));
    }
    let commits = release_commits(repo, &base.sha, rev)?;
    let (bump, rationale) = bump_of(&commits, &base.version);
    let computed = commits::next_version(&base.version, bump);
    let recorded = pending.as_ref().and_then(|p| p.version_override.clone());
    let (version, overridden) = if let Some(replay) = replay {
        (replay.version.clone(), replay.overridden)
    } else if opts.refresh {
        if pending.is_none() {
            return err(format!(
                "--refresh needs a prepared release without a stable tag, but {current} is the published base {}",
                base.describe()
            ));
        }
        match &recorded {
            Some(over) if computed > *over => {
                return err(format!(
                    "the commits since {} make the next version {computed}, above the recorded Version-Override {over}: prepare a new version instead of --refresh",
                    base.describe()
                ))
            }
            Some(_) => {}
            None if bump != Bump::None && computed != current => {
                return err(format!(
                    "the commits since {} make the next version {computed}, not {current}: prepare a new version instead of --refresh",
                    base.describe()
                ))
            }
            None => {}
        }
        (current.clone(), recorded.is_some())
    } else if let Some(over) = &opts.version {
        if pending.is_none() && *over <= current {
            return err(format!(
                "--version {over} must be greater than the current version {current}"
            ));
        }
        if *over < computed {
            return err(format!(
                "--version {over} is lower than the computed {computed} ({} bump)",
                bump.as_str()
            ));
        }
        (over.clone(), true)
    } else if pending.is_some() {
        match &recorded {
            Some(over) if *over >= computed => (over.clone(), true),
            _ => (computed.clone(), false),
        }
    } else if bump == Bump::None {
        return Ok(None);
    } else {
        (computed.clone(), false)
    };
    if let Some(pending) = &pending {
        if version != pending.version {
            return err(format!(
                "{} was prepared at {} but has no stable tag, and the commits since {} now make {version}: \
                 tag it first (`cargo xtask release tag`, then prepare again), or keep {} with --refresh",
                pending.version,
                &pending.sha[..7],
                base.describe(),
                pending.version
            ));
        }
    }
    let refreshing = pending.is_some();
    for (commit, short) in &commits {
        if commit.bump(&base.version) != Bump::None
            && commit.trailers.sentences.is_empty()
            && commit.trailers.skip.is_none()
        {
            warnings.push(format!(
                "{short} {} raises the version but has no Changelog: sentence, so it has no entry",
                commit.subject
            ));
        }
    }

    warnings.extend(near_duplicates(&commits));

    // The recorded wording edits: given, or kept from the release being
    // refreshed.
    let edits: Vec<EntryEdit> = match &opts.edits {
        Some(edits) => edits.clone(),
        None => pending
            .as_ref()
            .map(|p| p.entry_edits.clone())
            .unwrap_or_default(),
    };
    let items = render::items(&commits, &edits).map_err(Error::Msg)?;

    // The section (RF4.2/RF4.6). A new release carries the `[Unreleased]`
    // entries; a refresh re-renders every entry of the release's commits
    // from their trailers and the recorded edits, and keeps only the lines
    // that cite none of them.
    let changelog = tree.read("CHANGELOG.md")?;
    let v = version.to_string();
    let mut entries: Entries = if refreshing {
        let body = render::section_body(&changelog, &v).ok_or_else(|| {
            Error::Msg(format!(
                "the prepared release {v} has no [{v}] section in CHANGELOG.md"
            ))
        })?;
        let existing = render::entries_of(&body);
        let mut kept = Entries::new();
        for (category, lines) in existing {
            for line in lines {
                let cites = commits
                    .iter()
                    .any(|(c, _)| super::is_covered(&line, &c.sha));
                if !cites {
                    kept.entry(category).or_default().push(line);
                } else if !items.iter().any(|item| item.line == line) {
                    warnings.push(format!(
                        "the [{v}] section had an entry the commits do not render, which is replaced: \"{line}\"; record a wording change with `cargo xtask release edit`"
                    ));
                }
            }
        }
        if !opts.refresh
            && replay.is_none()
            && items
                .iter()
                .all(|item| body.lines().any(|l| l.trim_end() == item.line))
        {
            // Prepared and complete: the next step is the tag, not a new commit.
            return Ok(None);
        }
        kept
    } else {
        render::entries_of(&render::unreleased_body(&changelog))
    };
    for item in &items {
        entries
            .entry(item.category)
            .or_default()
            .push(item.line.clone());
    }
    let section = render::section(&v, &opts.date, &entries);
    let replace = refreshing.then_some(v.as_str());
    let new_changelog =
        render::insert_section(&changelog, &section, replace).map_err(Error::Msg)?;
    let new_changelog =
        render::update_footer(&new_changelog, &v, &base.link_from()).map_err(Error::Msg)?;
    crate::changelog::parse(&new_changelog)
        .map_err(|e| Error::Msg(format!("the rendered CHANGELOG.md does not parse: {e}")))?;

    let mut changes = Vec::new();
    let mut push = |path: &str, old: String, new: String| {
        if old != new {
            changes.push(FileChange {
                path: path.to_string(),
                old,
                new,
            });
        }
    };
    if version != current {
        let (c, n) = (current.to_string(), v.clone());
        let text = tree.read("Cargo.toml")?;
        push(
            "Cargo.toml",
            text.clone(),
            set_section_version(&text, "[workspace.package]", &c, &n, "Cargo.toml")?,
        );
        let path = "apps/desktop/src-tauri/Cargo.toml";
        let text = tree.read(path)?;
        push(
            path,
            text.clone(),
            set_section_version(&text, "[package]", &c, &n, path)?,
        );
        let path = "apps/desktop/src-tauri/tauri.conf.json";
        let text = tree.read(path)?;
        push(path, text.clone(), set_tauri_version(&text, &c, &n)?);
        let path = "crates/cuotai-tui/Cargo.toml";
        let text = tree.read(path)?;
        push(path, text.clone(), set_core_requirement(&text, &version)?);
        let text = tree.read("Cargo.lock")?;
        let new = set_lock_version(&text, "cuotai-core", &c, &n, "Cargo.lock")?;
        let new = set_lock_version(&new, "cuotai-tui", &c, &n, "Cargo.lock")?;
        push("Cargo.lock", text, new);
        let path = "apps/desktop/src-tauri/Cargo.lock";
        let text = tree.read(path)?;
        let new = set_lock_version(&text, "cuotai-core", &c, &n, path)?;
        let new = set_lock_version(&new, "cuotai-desktop", &c, &n, path)?;
        push(path, text, new);
    }
    push("CHANGELOG.md", changelog, new_changelog);

    // Plans (RF6.2).
    let all: Vec<(&commits::Commit, String)> =
        commits.iter().map(|(c, s)| (c, s.clone())).collect();
    let plan_paths = tree.plan_files()?;
    for (path, text) in plan_paths.iter().zip(tree.read_many(&plan_paths)?) {
        let text = text.ok_or_else(|| Error::Msg(format!("cannot read {path}")))?;
        let (mut new, _) = render::replace_next(&text, &v);
        if path.starts_with("docs/plans/0000_") {
            let rows = render::control_rows(&v, &all, render::numeric_rows(&text));
            new = render::update_control(&new, &v, &opts.date, &rows);
        }
        push(path, text, new);
    }

    let skipped: Vec<String> = commits
        .iter()
        .filter_map(|(commit, short)| {
            let reason = commit.trailers.skip.as_ref()?;
            Some(format!("{short} {}: {reason}", commit.subject))
        })
        .collect();
    let message = message(
        &base,
        &version,
        bump,
        &rationale,
        &skipped,
        &edits,
        &section,
        refreshing,
        overridden,
        &opts.trailers,
    );
    Ok(Some(Outcome {
        base,
        current,
        version,
        bump,
        rationale,
        section,
        changes,
        warnings,
        message,
        branch: None,
        edits,
        skipped,
        pr_body: None,
    }))
}

/// The would-be section of the next release, for `changelog-draft` and the
/// job summary on `main` (RF4.8, RF8.4).
pub fn draft(repo: &Path, date: &str) -> Res<String> {
    let opts = Options {
        date: date.to_string(),
        dry_run: true,
        ..Options::default()
    };
    match run(repo, &opts)? {
        None => Ok(format!("{}\n", idle_status(repo)?)),
        Some(outcome) => Ok(format!(
            "Next version: {} ({} bump from {}).\n\n{}",
            outcome.version,
            outcome.bump.as_str(),
            outcome.base.describe(),
            outcome.section
        )),
    }
}

/// Why `run` had nothing to do: no user-visible commit since the base, or
/// a prepared release that only waits for its tag.
pub fn idle_status(repo: &Path) -> Res<String> {
    let current = workspace_version(&Tree::Work(repo).read("Cargo.toml")?)?;
    let base = release_base(repo, "HEAD", None)?;
    Ok(match pending_release(repo, &base, "HEAD", &current)? {
        Some(pending) => format!(
            "Nothing new to release: {} is prepared at {} and waits for its tag ({TAG_HINT}).",
            pending.version,
            &pending.sha[..7]
        ),
        None => format!(
            "Nothing to release: no user-visible commit since {}.",
            base.describe()
        ),
    })
}

/// The first five words of a sentence, lowercased, when it has five.
fn opening(sentence: &str) -> Option<String> {
    let words: Vec<String> = sentence
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(5)
        .map(str::to_lowercase)
        .collect();
    (words.len() == 5).then(|| words.join(" "))
}

/// A warning for every pair of sentences of the range that open with the
/// same five words (F-16): probably the same change told twice, worth a
/// look in the release PR. Never an error.
fn near_duplicates(commits: &[(commits::Commit, String)]) -> Vec<String> {
    let mut seen: Vec<(String, &str, &str)> = Vec::new();
    let mut out = Vec::new();
    for (commit, short) in commits {
        for entry in &commit.trailers.sentences {
            let Some(key) = opening(&entry.text) else {
                continue;
            };
            if let Some((_, other_short, other)) = seen.iter().find(|(k, _, _)| *k == key) {
                out.push(format!(
                    "{other_short} \"{other}\" and {short} \"{}\" start with the same five \
                     words: check they are not the same change told twice",
                    entry.text
                ));
            } else {
                seen.push((key, short, &entry.text));
            }
        }
    }
    out
}

/// The release commit message (RF4.5), with every skip-CI marker
/// neutralized.
#[allow(clippy::too_many_arguments)]
fn message(
    base: &Base,
    version: &Version,
    bump: Bump,
    rationale: &[String],
    skipped: &[String],
    edits: &[EntryEdit],
    section: &str,
    refresh: bool,
    overridden: bool,
    trailers: &[String],
) -> String {
    let refresh = if refresh { " (refresh)" } else { "" };
    let mut out = format!("chore(release): {version}{refresh}\n\n");
    out.push_str(&format!(
        "Release {version}: {} bump from {}.\n\n",
        bump.as_str(),
        base.describe()
    ));
    if !rationale.is_empty() {
        out.push_str("Bump rationale:\n");
        for line in rationale {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }
    if !skipped.is_empty() {
        // F-12: what ships without an entry, and why, is visible in the
        // release PR.
        out.push_str("Skipped with a reason (no entry):\n");
        for line in skipped {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }
    if !edits.is_empty() {
        // F-02: the wording edits of this release, recorded as trailers so
        // the line-level check and a refresh re-apply them.
        out.push_str("Wording edits:\n");
        for edit in edits {
            out.push_str(&format!("- {edit}\n"));
        }
        out.push('\n');
    }
    out.push_str(section.trim_end());
    out.push_str(&format!(
        "\n\nVersion {version}.\n\nChangelog: skip release commit\n"
    ));
    if overridden {
        out.push_str(&format!("Version-Override: {version}\n"));
    }
    for edit in edits {
        out.push_str(&format!("Changelog-Entry: {edit}\n"));
    }
    for trailer in trailers {
        out.push_str(trailer.trim());
        out.push('\n');
    }
    commits::neutralize_skip_markers(&out) + "\n"
}

/// Replaces `version = "old"` inside `[section]` (exactly once).
fn set_section_version(text: &str, section: &str, old: &str, new: &str, path: &str) -> Res<String> {
    let mut current = String::new();
    let mut hits = 0;
    let lines: Vec<String> = text
        .split_inclusive('\n')
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                current = trimmed.to_string();
            }
            if current == section && trimmed == format!("version = \"{old}\"") {
                hits += 1;
                return line.replacen(&format!("\"{old}\""), &format!("\"{new}\""), 1);
            }
            line.to_string()
        })
        .collect();
    if hits != 1 {
        return err(format!(
            "{path}: expected one 'version = \"{old}\"' line in {section}, found {hits}"
        ));
    }
    Ok(lines.concat())
}

fn set_tauri_version(text: &str, old: &str, new: &str) -> Res<String> {
    let path = "apps/desktop/src-tauri/tauri.conf.json";
    let needle = format!("\"version\": \"{old}\"");
    let Some(index) = text.find(&needle) else {
        return err(format!("{path}: no '{needle}'"));
    };
    let mut out = text.to_string();
    out.replace_range(
        index..index + needle.len(),
        &format!("\"version\": \"{new}\""),
    );
    let doc: serde_json::Value = serde_json::from_str(&out).map_err(|e| Error::Json {
        context: path.into(),
        source: e,
    })?;
    if doc.get("version").and_then(|v| v.as_str()) != Some(new) {
        return err(format!("{path}: the edited top-level version is not {new}"));
    }
    Ok(out)
}

/// RF4.4 item 4: `"X.Y"` while `X == 0`, `"X"` from 1.0.
fn set_core_requirement(text: &str, version: &Version) -> Res<String> {
    let requirement = if version.major == 0 {
        format!("{}.{}", version.major, version.minor)
    } else {
        version.major.to_string()
    };
    let mut hits = 0;
    let out: String = text
        .split_inclusive('\n')
        .map(|line| {
            if !line.trim_start().starts_with("cuotai-core = {") {
                return line.to_string();
            }
            let Some(start) = line.find("version = \"") else {
                return line.to_string();
            };
            let value_start = start + "version = \"".len();
            let Some(len) = line[value_start..].find('"') else {
                return line.to_string();
            };
            hits += 1;
            let mut line = line.to_string();
            line.replace_range(value_start..value_start + len, &requirement);
            line
        })
        .collect();
    if hits != 1 {
        return err(format!(
            "crates/cuotai-tui/Cargo.toml: expected one 'cuotai-core = {{ … version = \"…\" }}' line, found {hits}"
        ));
    }
    Ok(out)
}

/// The `version` line right after `name = "<package>"` in a lockfile.
fn set_lock_version(text: &str, package: &str, old: &str, new: &str, path: &str) -> Res<String> {
    let name = format!("name = \"{package}\"");
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    let mut hits = 0;
    for index in 0..lines.len().saturating_sub(1) {
        if lines[index].trim() == name && lines[index + 1].trim() == format!("version = \"{old}\"")
        {
            lines[index + 1] =
                lines[index + 1].replacen(&format!("\"{old}\""), &format!("\"{new}\""), 1);
            hits += 1;
        }
    }
    if hits != 1 {
        return err(format!(
            "{path}: expected one '{package}' entry at {old}, found {hits}"
        ));
    }
    let out = lines.concat();
    out.parse::<toml::Value>().map_err(|e| Error::Toml {
        context: path.into(),
        source: e,
    })?;
    Ok(out)
}

fn write_atomic(path: &Path, text: &str) -> Res<()> {
    let tmp = path.with_extension("xtask-release-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Both lockfiles must already agree with the manifests: `cargo metadata
/// --locked --offline` fails otherwise, without moving any dependency.
fn verify_lockfiles(repo: &Path) -> Res<()> {
    for manifest in ["Cargo.toml", "apps/desktop/src-tauri/Cargo.toml"] {
        let output = Command::new("cargo")
            .current_dir(repo)
            .args([
                "metadata",
                "--locked",
                "--offline",
                "--format-version",
                "1",
                "--manifest-path",
                manifest,
            ])
            .output()
            .map_err(|e| Error::Msg(format!("cannot run cargo metadata: {e}")))?;
        if !output.status.success() {
            return err(format!(
                "the lockfile of {manifest} does not match after the version edit: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    Ok(())
}

/// Today's date in UTC (`YYYY-MM-DD`), from the system clock: the only
/// wall-clock read of the release tooling (NFR1).
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The changelog entries a pull request adds (PRD 0039 F-19): every
/// `Changelog:` sentence of the non-merge commits in `base..HEAD`, by
/// category, with the commits that skip an entry and their reasons. The CI
/// job summary shows it on each pull request, so the wording is reviewed
/// before the release renders it.
pub fn pull_request_preview(repo: &Path, base: &str) -> Res<String> {
    let commits = crate::commits::load(repo, &format!("{base}..HEAD"))?;
    let mut by_category: std::collections::BTreeMap<crate::changelog::Category, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut skipped = Vec::new();
    for commit in &commits {
        for entry in &commit.trailers.sentences {
            by_category
                .entry(commit.entry_category(entry))
                .or_default()
                .push(format!("- {} ({})", entry.text, commit.short()));
        }
        if let Some(reason) = &commit.trailers.skip {
            skipped.push(format!("- {} {}: {reason}", commit.short(), commit.subject));
        }
    }
    let mut out = format!(
        "{} commit(s) in this pull request, {} changelog entr{}.\n",
        commits.len(),
        by_category.values().map(Vec::len).sum::<usize>(),
        if by_category.values().map(Vec::len).sum::<usize>() == 1 {
            "y"
        } else {
            "ies"
        }
    );
    for (category, lines) in &by_category {
        out.push_str(&format!(
            "\n#### {}\n\n{}\n",
            category.title(),
            lines.join("\n")
        ));
    }
    if !skipped.is_empty() {
        out.push_str(&format!(
            "\n#### No entry (Changelog: skip)\n\n{}\n",
            skipped.join("\n")
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_targeted_edits_keep_formatting() {
        let cargo = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.79.0\" \nedition = \"2021\"\n\n[dependencies]\nx = { version = \"0.79.0\" }\n";
        let cargo = cargo.replace("\" \n", "\"\n");
        let out =
            set_section_version(&cargo, "[workspace.package]", "0.79.0", "0.80.0", "C").unwrap();
        assert!(out.contains("[workspace.package]\nversion = \"0.80.0\"\n"));
        assert!(out.contains("x = { version = \"0.79.0\" }"));
        assert!(set_section_version(&cargo, "[package]", "0.79.0", "0.80.0", "C").is_err());

        let tui = "[dependencies]\n# comment\ncuotai-core = { path = \"../cuotai-core\", version = \"0.79\", features = [\"serve\"] }\n";
        let out = set_core_requirement(tui, &Version::parse("0.80.0").unwrap()).unwrap();
        assert!(out.contains("version = \"0.80\", features"));
        let out = set_core_requirement(tui, &Version::parse("1.0.0").unwrap()).unwrap();
        assert!(out.contains("version = \"1\", features"));

        let lock = "version = 4\n\n[[package]]\nname = \"cuotai-core\"\nversion = \"0.79.0\"\n\n[[package]]\nname = \"other\"\nversion = \"0.79.0\"\n";
        let out = set_lock_version(lock, "cuotai-core", "0.79.0", "0.80.0", "L").unwrap();
        assert!(out.contains("name = \"cuotai-core\"\nversion = \"0.80.0\""));
        assert!(out.contains("name = \"other\"\nversion = \"0.79.0\""));

        let tauri =
            "{\n  \"productName\": \"Cuotai\",\n  \"version\": \"0.79.0\",\n  \"x\": 1\n}\n";
        let out = set_tauri_version(tauri, "0.79.0", "0.80.0").unwrap();
        assert_eq!(out, tauri.replace("0.79.0", "0.80.0"));
    }

    #[test]
    fn today_is_a_date() {
        let today = today_utc();
        assert_eq!(today.len(), 10);
        assert!(today.starts_with("20"));
    }
}
