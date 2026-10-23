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
    bump_of, is_covered, pending_release, release_base, release_commits, workspace_version, Base,
    Tree,
};
use crate::commits::{self, Bump};
use crate::{err, git, Error, Res, Version};

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
}

/// Computes the release; writes it unless `--dry-run`. `Ok(None)` means
/// "nothing to release" (RF3.5).
pub fn run(repo: &Path, opts: &Options) -> Res<Option<Outcome>> {
    let work = Tree::Work(repo);
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
    let current = workspace_version(&work.read("Cargo.toml")?)?;
    let mut warnings = Vec::new();
    if opts.refresh && opts.version.is_some() {
        return err("--refresh keeps the prepared version; it cannot be combined with --version");
    }
    // The base is always published (a stable tag or the internal baseline);
    // a release prepared after it but not tagged yet is `pending`, and its
    // section is refreshed instead of getting a second one.
    let base = release_base(repo, "HEAD", None)?;
    let pending = pending_release(repo, &base, "HEAD", &current)?;
    if current > base.version && pending.is_none() {
        return err(format!(
            "HEAD declares {current}, above the release base {}, but no 'chore(release): {current}' commit prepared it: \
             a version was raised by hand; revert that change and let release prepare compute the version",
            base.describe()
        ));
    }
    let commits = release_commits(repo, &base.sha, "HEAD")?;
    let (bump, rationale) = bump_of(&commits, &base.version);
    let computed = commits::next_version(&base.version, bump);
    let recorded = pending.as_ref().and_then(|p| p.version_override.clone());
    let (version, overridden) = if opts.refresh {
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

    // The section (RF4.2/RF4.6).
    let changelog = work.read("CHANGELOG.md")?;
    let v = version.to_string();
    let mut entries: Entries = if refreshing {
        let body = render::section_body(&changelog, &v).ok_or_else(|| {
            Error::Msg(format!(
                "the prepared release {v} has no [{v}] section in CHANGELOG.md"
            ))
        })?;
        render::entries_of(&body)
    } else {
        render::entries_of(&render::unreleased_body(&changelog))
    };
    let existing: String = entries
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let mut added = 0usize;
    for (commit, short) in &commits {
        if refreshing && is_covered(&existing, &commit.sha) {
            continue;
        }
        for sentence in &commit.trailers.sentences {
            added += 1;
            entries
                .entry(commit.category())
                .or_default()
                .push(render::entry(commit, sentence, short));
        }
    }
    if refreshing && !opts.refresh && added == 0 {
        // Prepared and complete: the next step is the tag, not a new commit.
        return Ok(None);
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
        let text = work.read("Cargo.toml")?;
        push(
            "Cargo.toml",
            text.clone(),
            set_section_version(&text, "[workspace.package]", &c, &n, "Cargo.toml")?,
        );
        let path = "apps/desktop/src-tauri/Cargo.toml";
        let text = work.read(path)?;
        push(
            path,
            text.clone(),
            set_section_version(&text, "[package]", &c, &n, path)?,
        );
        let path = "apps/desktop/src-tauri/tauri.conf.json";
        let text = work.read(path)?;
        push(path, text.clone(), set_tauri_version(&text, &c, &n)?);
        let path = "crates/cuotai-tui/Cargo.toml";
        let text = work.read(path)?;
        push(path, text.clone(), set_core_requirement(&text, &version)?);
        let text = work.read("Cargo.lock")?;
        let new = set_lock_version(&text, "cuotai-core", &c, &n, "Cargo.lock")?;
        let new = set_lock_version(&new, "cuotai-tui", &c, &n, "Cargo.lock")?;
        push("Cargo.lock", text, new);
        let path = "apps/desktop/src-tauri/Cargo.lock";
        let text = work.read(path)?;
        let new = set_lock_version(&text, "cuotai-core", &c, &n, path)?;
        let new = set_lock_version(&new, "cuotai-desktop", &c, &n, path)?;
        push(path, text, new);
    }
    push("CHANGELOG.md", changelog, new_changelog);

    // Plans (RF6.2).
    let all: Vec<(&commits::Commit, String)> =
        commits.iter().map(|(c, s)| (c, s.clone())).collect();
    let rows = render::control_rows(&v, &all);
    for path in work.plan_files()? {
        let text = work.read(&path)?;
        let (mut new, _) = render::replace_next(&text, &v);
        if path.starts_with("docs/plans/0000_") {
            new = render::update_control(&new, &v, &opts.date, &rows);
        }
        push(&path, text, new);
    }

    let message = message(
        &base, &version, bump, &rationale, &section, refreshing, overridden,
    );
    let mut outcome = Outcome {
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
    for change in &outcome.changes {
        write_atomic(&repo.join(&change.path), &change.new)?;
    }
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
    }
    Ok(Some(outcome))
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
            "Nothing new to release: {} is prepared at {} and waits for its tag (cargo xtask release tag --rc).",
            pending.version,
            &pending.sha[..7]
        ),
        None => format!(
            "Nothing to release: no user-visible commit since {}.",
            base.describe()
        ),
    })
}

/// The release commit message (RF4.5), with every skip-CI marker
/// neutralized.
fn message(
    base: &Base,
    version: &Version,
    bump: Bump,
    rationale: &[String],
    section: &str,
    refresh: bool,
    overridden: bool,
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
    out.push_str(section.trim_end());
    out.push_str(&format!(
        "\n\nVersion {version}.\n\nChangelog: skip release commit\n"
    ));
    if overridden {
        out.push_str(&format!("Version-Override: {version}\n"));
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
