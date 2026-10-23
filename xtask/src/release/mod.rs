//! Release preparation (PRD 0039 RF3–RF6): the release base, the next
//! version, `cargo xtask release prepare` (with `--dry-run`, `--check`,
//! `--refresh` and `--branch`) and `cargo xtask release tag`.
//!
//! Versions and the changelog are produced here, once per release, from the
//! commits since the last release; feature and fix commits never touch them
//! (RF1.3, enforced by [`crate::changelog_gate`]).

pub mod prepare;
pub mod render;
pub mod tag;

use std::path::Path;

use crate::commits::{self, Bump, Commit};
use crate::{err, git, parse_tag, Error, Res, Version};

/// The last version released under the old per-commit rules. Versions up to
/// it are **internal versions** (owner decision D11): never tagged, their
/// footer links compare commits (D10).
pub const LAST_INTERNAL_VERSION: Version = Version {
    major: 0,
    minor: 79,
    patch: 0,
};

/// The files every release commit writes (RF4.4); plan files
/// (`docs/**/*.md`, `README.md`) are added when they carry `vNEXT`.
pub const RELEASE_FILES: [&str; 7] = [
    "Cargo.toml",
    "apps/desktop/src-tauri/Cargo.toml",
    "apps/desktop/src-tauri/tauri.conf.json",
    "crates/cuotai-tui/Cargo.toml",
    "Cargo.lock",
    "apps/desktop/src-tauri/Cargo.lock",
    "CHANGELOG.md",
];

/// Whether a path may change in a release commit.
pub fn in_release_set(path: &str) -> bool {
    RELEASE_FILES.contains(&path) || is_plan_file(path)
}

/// The Markdown files `vNEXT` is replaced in (RF6.2).
pub fn is_plan_file(path: &str) -> bool {
    path == "README.md" || (path.starts_with("docs/") && path.ends_with(".md"))
}

/// Where files are read from: the working tree or a commit.
#[derive(Clone, Debug)]
pub enum Tree<'a> {
    Work(&'a Path),
    Rev(&'a Path, String),
}

impl Tree<'_> {
    pub fn read(&self, rel: &str) -> Res<String> {
        match self {
            Tree::Work(root) => std::fs::read_to_string(root.join(rel))
                .map_err(|e| Error::Msg(format!("cannot read {rel}: {e}"))),
            Tree::Rev(root, rev) => git::run(root, &["show", &format!("{rev}:{rel}")])
                .map_err(|_| Error::Msg(format!("{rel} does not exist at {rev}"))),
        }
    }

    /// `README.md` and every `docs/**/*.md`, sorted.
    pub fn plan_files(&self) -> Res<Vec<String>> {
        let mut out = Vec::new();
        match self {
            Tree::Work(root) => {
                if root.join("README.md").is_file() {
                    out.push("README.md".to_string());
                }
                let mut stack = vec![root.join("docs")];
                while let Some(dir) = stack.pop() {
                    let Ok(entries) = std::fs::read_dir(&dir) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            stack.push(path);
                        } else if let Ok(rel) = path.strip_prefix(root) {
                            let rel = rel.to_string_lossy().replace('\\', "/");
                            if is_plan_file(&rel) {
                                out.push(rel);
                            }
                        }
                    }
                }
            }
            Tree::Rev(root, rev) => {
                let list = git::run(
                    root,
                    &[
                        "ls-tree",
                        "-r",
                        "--name-only",
                        rev,
                        "--",
                        "README.md",
                        "docs",
                    ],
                )?;
                out.extend(list.lines().filter(|p| is_plan_file(p)).map(str::to_string));
            }
        }
        out.sort();
        Ok(out)
    }
}

/// Runs `git <args> -F <file> <tail>` with `message` in a file inside the
/// repository's own Git directory (never a shared temporary name).
pub(crate) fn git_with_message(
    repo: &Path,
    name: &str,
    message: &str,
    args: &[&str],
    tail: &[&str],
) -> Res<()> {
    let dir = git::run(repo, &["rev-parse", "--absolute-git-dir"])?;
    let file = Path::new(dir.trim()).join(name);
    std::fs::write(&file, message)?;
    let path = file.to_string_lossy().to_string();
    let mut full: Vec<&str> = args.to_vec();
    full.extend(["-F", path.as_str()]);
    full.extend(tail.iter().copied());
    let result = git::run(repo, &full);
    let _ = std::fs::remove_file(&file);
    result.map(|_| ())
}

/// `[workspace.package] version` of a root `Cargo.toml`.
pub fn workspace_version(text: &str) -> Res<Version> {
    let doc: toml::Value = text.parse().map_err(|e| Error::Toml {
        context: "Cargo.toml".into(),
        source: e,
    })?;
    let version = doc
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Msg("Cargo.toml: [workspace.package] version is missing".into()))?;
    Version::parse(version)
}

/// The release base (RF3.1).
#[derive(Clone, Debug)]
pub struct Base {
    pub sha: String,
    pub version: Version,
    /// The stable tag the base came from, if any.
    pub tag: Option<String>,
}

impl Base {
    /// What the new version's footer link compares from (RF4.4 item 6).
    pub fn link_from(&self) -> String {
        match &self.tag {
            Some(tag) => tag.clone(),
            None => self.sha[..7.min(self.sha.len())].to_string(),
        }
    }

    pub fn describe(&self) -> String {
        match &self.tag {
            Some(tag) => format!("{} (tag {tag})", self.version),
            None => format!("{} at {}", self.version, &self.sha[..7.min(self.sha.len())]),
        }
    }
}

/// RF3.1: the release base is always a **published** point: the newest
/// stable `vX.Y.Z` tag reachable from `rev` (release-candidate tags never
/// count), or, while no stable tag qualifies, the internal baseline, i.e.
/// the newest commit that set an internal version (up to
/// [`LAST_INTERNAL_VERSION`]). A release that was prepared but never got a
/// stable tag is therefore never a base: its commits stay in the next
/// release (see [`pending_release`]). With `below`, only versions lower
/// than it qualify (checking a release commit of that version).
pub fn release_base(repo: &Path, rev: &str, below: Option<&Version>) -> Res<Base> {
    let qualifies = |version: &Version| below.is_none_or(|limit| version < limit);
    let mut best: Option<Base> = None;
    if let Ok(tags) = git::run(repo, &["tag", "--merged", rev, "--list", "v*"]) {
        for name in tags.lines().map(str::trim) {
            let Ok(tag) = parse_tag(name) else { continue };
            if tag.rc.is_some() || !qualifies(&tag.version) {
                continue;
            }
            if best.as_ref().is_some_and(|b| b.version >= tag.version) {
                continue;
            }
            let sha = git::run(repo, &["rev-list", "-n", "1", name])?
                .trim()
                .to_string();
            best = Some(Base {
                sha,
                version: tag.version,
                tag: Some(name.to_string()),
            });
        }
    }
    if let Some(base) = best {
        return Ok(base);
    }
    let changes = git::run(
        repo,
        &[
            "log",
            "--format=%H",
            "-G",
            "^version = ",
            rev,
            "--",
            "Cargo.toml",
        ],
    )?;
    for sha in changes.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let Ok(text) = git::run(repo, &["show", &format!("{sha}:Cargo.toml")]) else {
            continue;
        };
        let Ok(version) = workspace_version(&text) else {
            continue;
        };
        if version <= LAST_INTERNAL_VERSION && qualifies(&version) {
            return Ok(Base {
                sha: sha.to_string(),
                version,
                tag: None,
            });
        }
    }
    let root = git::run(repo, &["rev-list", "--max-parents=0", rev])?;
    let root = root.lines().next().unwrap_or_default().trim();
    if root.is_empty() {
        return err("no commits found: is the repository initialized?");
    }
    Ok(Base {
        sha: root.to_string(),
        version: Version {
            major: 0,
            minor: 0,
            patch: 0,
        },
        tag: None,
    })
}

/// A release that was prepared after the base but has no stable tag yet.
#[derive(Clone, Debug)]
pub struct Pending {
    /// The newest release commit of that version.
    pub sha: String,
    pub version: Version,
    /// Its recorded `Version-Override:` (RF3.4), if any.
    pub version_override: Option<Version>,
}

/// The newest `chore(release): <version>` commit of `base..rev`, when the
/// workspace `version` is above the base (prepared, not yet published).
pub fn pending_release(
    repo: &Path,
    base: &Base,
    rev: &str,
    version: &Version,
) -> Res<Option<Pending>> {
    if *version <= base.version {
        return Ok(None);
    }
    let found = commits::load(repo, &format!("{}..{rev}", base.sha))?
        .into_iter()
        .rev()
        .find(|commit| commit.release_version().as_ref() == Some(version));
    let Some(commit) = found else {
        return Ok(None);
    };
    let version_override = commit
        .trailers
        .version_override
        .as_deref()
        .map(Version::parse)
        .transpose()?;
    Ok(Some(Pending {
        sha: commit.sha,
        version: version.clone(),
        version_override,
    }))
}

/// The commits of a release (`base..rev`, oldest first, release commits
/// excluded) with their short hashes.
pub fn release_commits(repo: &Path, base: &str, rev: &str) -> Res<Vec<(Commit, String)>> {
    let commits: Vec<Commit> = commits::load(repo, &format!("{base}..{rev}"))?
        .into_iter()
        .filter(|commit| commit.release_version().is_none())
        .collect();
    let shorts = short_hashes(repo, &commits)?;
    Ok(commits.into_iter().zip(shorts).collect())
}

/// `git rev-parse --short=7` for every commit (longer when ambiguous).
fn short_hashes(repo: &Path, commits: &[Commit]) -> Res<Vec<String>> {
    commits
        .iter()
        .map(|commit| {
            git::run(repo, &["rev-parse", "--short=7", &commit.sha])
                .map(|text| text.trim().to_string())
        })
        .collect()
}

/// The bump of a set of commits (the maximum, RF3.2) and the commits that
/// drove it, for the rationale.
pub fn bump_of(commits: &[(Commit, String)], current: &Version) -> (Bump, Vec<String>) {
    let bump = commits
        .iter()
        .map(|(c, _)| c.bump(current))
        .max()
        .unwrap_or(Bump::None);
    let rationale = commits
        .iter()
        .filter(|(c, _)| c.bump(current) != Bump::None)
        .map(|(c, short)| format!("{}: {short} {}", c.bump(current).as_str(), c.subject))
        .collect();
    (bump, rationale)
}

/// Whether `sha` is cited by `text` (RF4.6: "covered"): any word of at
/// least seven lowercase hexadecimal characters that prefixes it. Git may
/// abbreviate a hash to digits only (`1234567`) or letters only
/// (`abcdefa`), so this is deliberately not the sentence-lint heuristic of
/// [`commits::looks_like_hash`].
pub fn is_covered(text: &str, sha: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| is_hex_abbreviation(token) && sha.starts_with(token))
}

/// 7–40 lowercase hexadecimal characters.
pub fn is_hex_abbreviation(token: &str) -> bool {
    (7..=40).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What `--check` proved (RF4.7).
#[derive(Debug)]
pub struct CheckOutcome {
    pub version: Version,
    pub base: Base,
    pub commits: usize,
}

/// RF4.7: recomputes the release that `rev` (a release commit) claims and
/// fails unless its version is the computed one (or a recorded valid
/// override), every user-visible commit of `base..rev^` is covered by the
/// section, no cited hash is foreign and no `vNEXT` remains.
pub fn check_release(repo: &Path, rev: &str) -> Res<CheckOutcome> {
    let sha = git::run(
        repo,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )
    .map_err(|_| Error::Msg(format!("'{rev}' is not a commit")))?
    .trim()
    .to_string();
    let message = git::run(repo, &["log", "-1", "--format=%B", &sha])?;
    let subject = message.lines().next().unwrap_or_default().to_string();
    let release = Commit::parse(&sha, &subject, &message);
    let Some(version) = release.release_version() else {
        return err(format!(
            "{} '{subject}' is not a release commit ('chore(release): X.Y.Z'); \
             run `cargo xtask release prepare --branch` to produce one",
            &sha[..7]
        ));
    };
    let tree = Tree::Rev(repo, sha.clone());
    let mut problems = Vec::new();
    let declared = workspace_version(&tree.read("Cargo.toml")?)?;
    if declared != version {
        problems.push(format!(
            "the release commit says {version} but Cargo.toml declares {declared}"
        ));
    }
    let parent = format!("{sha}^");
    let base = release_base(repo, &parent, Some(&version))?;
    let commits = release_commits(repo, &base.sha, &parent)?;
    let (bump, _) = bump_of(&commits, &base.version);
    let computed = commits::next_version(&base.version, bump);
    let override_ = release
        .trailers
        .version_override
        .as_deref()
        .map(Version::parse)
        .transpose()?;
    match override_ {
        Some(over) if over == version => {
            if version <= base.version || version < computed {
                problems.push(format!(
                    "Version-Override {version} must be greater than {} and not lower than the computed {computed}",
                    base.version
                ));
            }
        }
        Some(over) => problems.push(format!(
            "Version-Override {over} differs from the released version {version}"
        )),
        None if bump == Bump::None => problems.push(format!(
            "no user-visible commit since {}: nothing to release without a Version-Override",
            base.describe()
        )),
        None if computed != version => problems.push(format!(
            "the commits since {} make the next version {computed} ({} bump), not {version}",
            base.describe(),
            bump.as_str()
        )),
        None => {}
    }
    let changelog = tree.read("CHANGELOG.md")?;
    let parsed = crate::changelog::parse(&changelog).map_err(|e| Error::Msg(e.to_string()))?;
    match parsed.releases.first() {
        Some(first) if first.version == version.to_string() => {
            for (commit, short) in &commits {
                if !commit.trailers.sentences.is_empty() && !is_covered(&first.body, &commit.sha) {
                    problems.push(format!(
                        "{short} {} has a Changelog: sentence but the [{version}] section does not cite it",
                        commit.subject
                    ));
                }
            }
            for block in &first.section.categories {
                for entry in &block.entries {
                    for hash in &entry.hashes {
                        if !commits
                            .iter()
                            .any(|(c, _)| c.sha.starts_with(hash.as_str()))
                        {
                            problems.push(format!(
                                "the [{version}] section cites {hash}, which is not a commit of this release"
                            ));
                        }
                    }
                }
            }
        }
        _ => problems.push(format!(
            "CHANGELOG.md: the first released section is not [{version}]"
        )),
    }
    for path in tree.plan_files()? {
        let count = render::count_next(&tree.read(&path)?);
        if count > 0 {
            problems.push(format!(
                "{path} still has {count} '{}' token(s)",
                render::NEXT_TOKEN
            ));
        }
    }
    if !problems.is_empty() {
        return err(format!(
            "release check of {} ({version}) failed ({} problem(s)):\n - {}",
            &sha[..7],
            problems.len(),
            problems.join("\n - ")
        ));
    }
    Ok(CheckOutcome {
        version,
        base,
        commits: commits.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_accepts_every_hex_abbreviation() {
        let digits = "1234567890abcdef1234567890abcdef12345678";
        let letters = "abcdefabcdef1234567890abcdef1234567890ab";
        assert!(is_covered("- Something (PRD 0001 RF1; 1234567).", digits));
        assert!(is_covered("- Something (abcdefa).", letters));
        assert!(is_covered("- Longer (1234567890a).", digits));
        assert!(
            !is_covered("- Something (123456).", digits),
            "six characters"
        );
        assert!(
            !is_covered("- Something (abcdefb).", letters),
            "not a prefix"
        );
        // The sentence lint stays a heuristic: plain hex words are prose.
        assert!(!commits::looks_like_hash("abcdefa"));
        assert!(is_hex_abbreviation("abcdefa") && is_hex_abbreviation("1234567"));
    }
}
