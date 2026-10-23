//! Release-check (PRD 0021 RF1.2): one command that proves the workspace
//! version, both member manifests, both lockfiles, the desktop manifest,
//! `tauri.conf.json`, the top released CHANGELOG section and the supplied
//! tag/ref all agree before any build starts.

use std::path::Path;

use crate::{err, parse_tag, Channel, Error, Res, Version};

/// What the release needs to know once the check passed.
#[derive(Clone, Debug)]
pub struct CheckReport {
    pub version: Version,
    pub channel: Channel,
    /// The validated tag, when one was supplied.
    pub tag: Option<String>,
    /// Date of the `## [X.Y.Z] - <date>` changelog heading; empty for an
    /// internal version covered by the pre-release history.
    pub changelog_date: String,
    /// Body of the version's changelog section (release-notes source).
    pub changelog_body: String,
}

/// Machine-readable form written next to the workflow outputs.
#[derive(serde::Serialize)]
pub struct CheckJson {
    pub version: String,
    pub channel: Channel,
    pub prerelease: bool,
    pub tag: String,
    pub changelog_date: String,
}

impl CheckReport {
    pub fn to_json(&self, fallback_tag: &str) -> CheckJson {
        CheckJson {
            version: self.version.to_string(),
            channel: self.channel,
            prerelease: self.channel == Channel::Prerelease,
            tag: self.tag.clone().unwrap_or_else(|| fallback_tag.to_string()),
            changelog_date: self.changelog_date.clone(),
        }
    }
}

/// Runs every file-based check plus, when `require_main` is set, the git
/// checks (annotated tag, commit contained in the main branch). All file
/// problems are collected and reported together; the git check runs last.
pub fn run(root: &Path, tag: Option<&str>, require_main: Option<&str>) -> Res<CheckReport> {
    if require_main.is_some() && tag.is_none() {
        return err("release check failed (1 problem(s)):\n - --require-main requires --tag");
    }
    let report = run_files(&Files::Dir(root), tag)?;
    if let (Some(tag), Some(main_ref)) = (tag, require_main) {
        crate::git::check_tag_on_main(root, tag, main_ref)?;
    }
    Ok(report)
}

/// Where the checked files come from: a directory (the working tree) or a
/// commit of a repository (`release tag` checks the selected release
/// commit, not the caller's working tree; PRD 0039 RF5.2).
pub enum Files<'a> {
    Dir(&'a Path),
    Rev(&'a Path, &'a str),
}

impl Files<'_> {
    fn read(&self, rel: &str) -> Res<String> {
        match self {
            Files::Dir(root) => std::fs::read_to_string(root.join(rel))
                .map_err(|e| Error::Msg(format!("cannot read {rel}: {e}"))),
            Files::Rev(root, rev) => crate::git::run(root, &["show", &format!("{rev}:{rel}")])
                .map_err(|_| Error::Msg(format!("cannot read {rel} at {rev}"))),
        }
    }
}

/// Every file-based check of [`run`] (no Git ancestry check), reading
/// through `files`.
pub fn run_files(files: &Files, tag: Option<&str>) -> Res<CheckReport> {
    let mut problems: Vec<String> = Vec::new();

    // Without the workspace version nothing else can be compared; fail fast.
    let version = workspace_version(files)?;

    // Member manifests must inherit the workspace version (or pin it equal).
    for member in ["crates/cuotai-core", "crates/cuotai-tui"] {
        if let Err(e) = check_member_manifest(files, member, &version) {
            problems.push(format!("{e}"));
        }
    }

    // Root lockfile: core and TUI package versions.
    // The TUI's published requirement on the core follows the version
    // (`X.Y` while in 0.x, `X` from 1.0; PRD 0039 RF4.4).
    if let Err(e) = check_core_requirement(files, &version) {
        problems.push(format!("{e}"));
    }

    if let Err(e) = check_lockfile(
        files,
        "Cargo.lock",
        &["cuotai-core", "cuotai-tui"],
        &version,
    ) {
        problems.push(format!("{e}"));
    }

    // Desktop manifest (separate workspace: explicit version).
    if let Err(e) = check_desktop_manifest(files, &version) {
        problems.push(format!("{e}"));
    }

    // Desktop lockfile: the desktop package itself and its core dependency.
    if let Err(e) = check_lockfile(
        files,
        "apps/desktop/src-tauri/Cargo.lock",
        &["cuotai-desktop", "cuotai-core"],
        &version,
    ) {
        problems.push(format!("{e}"));
    }

    // tauri.conf.json.
    if let Err(e) = check_tauri_conf(files, &version) {
        problems.push(format!("{e}"));
    }

    // CHANGELOG top released section for this version, non-empty.
    let changelog = match changelog_section(&files.read("CHANGELOG.md")?, &version) {
        Ok(c) => Some(c),
        Err(e) => {
            problems.push(format!("{e}"));
            None
        }
    };

    // Tag rules (RF1.1): format, version equality, channel classification.
    let mut parsed_tag = None;
    if let Some(tag) = tag {
        if matches!(&changelog, Some((date, _)) if date.is_empty()) {
            problems.push(format!(
                "tag '{tag}': {version} is an internal version summarized by the pre-release history; internal versions are never tagged or published"
            ));
        }
        match parse_tag(tag) {
            Ok(t) => {
                if t.version != version {
                    problems.push(format!(
                        "tag '{tag}' version {} differs from the workspace version {version}",
                        t.version
                    ));
                }
                parsed_tag = Some(t);
            }
            Err(e) => problems.push(format!("{e}")),
        }
    }

    if !problems.is_empty() {
        return err(format!(
            "release check failed ({} problem(s)):\n - {}",
            problems.len(),
            problems.join("\n - ")
        ));
    }

    let (date, body) = changelog.expect("changelog problems returned above");
    Ok(CheckReport {
        version,
        channel: parsed_tag.map_or(Channel::Stable, |t| t.channel()),
        tag: tag.map(str::to_string),
        changelog_date: date,
        changelog_body: body,
    })
}

fn workspace_version(files: &Files) -> Res<Version> {
    let text = files.read("Cargo.toml")?;
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
    Version::parse(version).map_err(|e| Error::Msg(format!("Cargo.toml [workspace.package]: {e}")))
}

/// `version.workspace = true` (inherits) or an explicit version equal to the
/// workspace one; anything else is a drift.
fn check_member_manifest(files: &Files, member: &str, version: &Version) -> Res<()> {
    let rel = Path::new(member).join("Cargo.toml");
    let text = files.read(&format!("{member}/Cargo.toml"))?;
    let doc: toml::Value = text.parse().map_err(|e| Error::Toml {
        context: rel.display().to_string(),
        source: e,
    })?;
    let package = doc
        .get("package")
        .ok_or_else(|| Error::Msg(format!("{}: [package] is missing", rel.display())))?;
    match package.get("version") {
        Some(toml::Value::String(s)) if matches!(Version::parse(s), Ok(v) if v == *version) => {
            Ok(())
        }
        Some(toml::Value::String(s)) => err(format!(
            "{}: version '{s}' differs from the workspace version {version}",
            rel.display()
        )),
        Some(toml::Value::Table(t))
            if t.get("workspace").and_then(|w| w.as_bool()) == Some(true) =>
        {
            Ok(())
        }
        _ => err(format!(
            "{}: version must be `workspace = true` or equal to {version}",
            rel.display()
        )),
    }
}

/// Every named package in the lockfile must carry the workspace version.
fn check_lockfile(files: &Files, rel: &str, packages: &[&str], version: &Version) -> Res<()> {
    let path = Path::new(rel);
    let text = files.read(rel)?;
    let doc: toml::Value = text.parse().map_err(|e| Error::Toml {
        context: path.display().to_string(),
        source: e,
    })?;
    let entries = doc
        .get("package")
        .and_then(|p| p.as_array())
        .ok_or_else(|| Error::Msg(format!("{}: no [[package]] entries", path.display())))?;
    for name in packages {
        let found = entries
            .iter()
            .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(*name))
            .filter_map(|p| p.get("version").and_then(|v| v.as_str()))
            .collect::<Vec<_>>();
        match found.as_slice() {
            [] => {
                return err(format!(
                    "{}: package '{name}' not found (run `cargo check` to refresh the lockfile)",
                    path.display()
                ))
            }
            [v] if matches!(Version::parse(v), Ok(parsed) if parsed == *version) => {}
            [v] => {
                return err(format!(
                    "{}: package '{name}' is at '{v}', expected {version} (run `cargo check` to refresh)",
                    path.display()
                ))
            }
            many => {
                return err(format!(
                    "{}: package '{name}' has several versions {many:?}, expected exactly {version}",
                    path.display()
                ))
            }
        }
    }
    Ok(())
}

/// `cuotai-core = { …, version = "X.Y" }` in `crates/cuotai-tui/Cargo.toml`
/// when the dependency carries a version requirement.
fn check_core_requirement(files: &Files, version: &Version) -> Res<()> {
    let rel = "crates/cuotai-tui/Cargo.toml";
    let text = files.read(rel)?;
    let doc: toml::Value = text.parse().map_err(|e| Error::Toml {
        context: rel.into(),
        source: e,
    })?;
    let Some(requirement) = doc
        .get("dependencies")
        .and_then(|d| d.get("cuotai-core"))
        .and_then(|c| c.get("version"))
        .and_then(|v| v.as_str())
    else {
        return Ok(());
    };
    let expected = if version.major == 0 {
        format!("0.{}", version.minor)
    } else {
        version.major.to_string()
    };
    if requirement != expected {
        return err(format!(
            "{rel}: the cuotai-core requirement is '{requirement}', expected '{expected}' for {version}"
        ));
    }
    Ok(())
}

fn check_desktop_manifest(files: &Files, version: &Version) -> Res<()> {
    let rel = Path::new("apps/desktop/src-tauri/Cargo.toml");
    let text = files.read("apps/desktop/src-tauri/Cargo.toml")?;
    let doc: toml::Value = text.parse().map_err(|e| Error::Toml {
        context: rel.display().to_string(),
        source: e,
    })?;
    let v = doc
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Msg(format!("{}: [package] version is missing", rel.display())))?;
    if !matches!(Version::parse(v), Ok(parsed) if parsed == *version) {
        return err(format!(
            "{}: version '{v}' differs from the workspace version {version}",
            rel.display()
        ));
    }
    Ok(())
}

fn check_tauri_conf(files: &Files, version: &Version) -> Res<()> {
    let rel = Path::new("apps/desktop/src-tauri/tauri.conf.json");
    let text = files.read("apps/desktop/src-tauri/tauri.conf.json")?;
    let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::Json {
        context: rel.display().to_string(),
        source: e,
    })?;
    let v = doc
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Msg(format!("{}: version is missing", rel.display())))?;
    if !matches!(Version::parse(v), Ok(parsed) if parsed == *version) {
        return err(format!(
            "{}: version '{v}' differs from the workspace version {version}",
            rel.display()
        ));
    }
    Ok(())
}

/// Extracts the first released `## [X.Y.Z] - YYYY-MM-DD` section immediately
/// after `[Unreleased]` from the shared parser (PRD 0019 RF7.4/RF7.5).
/// Returns `(date, body)` and rejects a workspace version that appears
/// only in older history; a format violation fails with the parser's exact
/// line, so the release gate and the repository check never disagree.
///
/// Before the first release the workspace still carries an internal
/// version (up to [`crate::release::LAST_INTERNAL_VERSION`]) that has no
/// section of its own: when no released section exists and the pre-release
/// history covers it, the history stands in, with an empty date (internal
/// builds were never published, so no release notes are made from it).
pub fn changelog_section(text: &str, version: &Version) -> Res<(String, String)> {
    let changelog = crate::changelog::parse(text).map_err(|e| Error::Msg(e.to_string()))?;
    let internal = *version <= crate::release::LAST_INTERNAL_VERSION;
    if let (true, None, Some(history)) = (
        internal,
        changelog.releases.first(),
        changelog.pre_release_of(&version.to_string()),
    ) {
        return Ok((String::new(), history.body.clone()));
    }
    let Some(first) = changelog.releases.first() else {
        return err(format!(
            "CHANGELOG.md: no released section after [Unreleased] for workspace version {version}"
        ));
    };
    if first.version != version.to_string() {
        return err(format!(
            "CHANGELOG.md: first released section after [Unreleased] is '## [{}] - {}', expected '## [{version}] - YYYY-MM-DD'",
            first.version, first.date
        ));
    }
    if first.section.is_empty() {
        return err(format!(
            "CHANGELOG.md: the section for {version} is empty; the release notes come from it"
        ));
    }
    Ok((first.date.clone(), first.body.clone()))
}
