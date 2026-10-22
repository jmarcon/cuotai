//! Changelog and plan-control agreement (PRD 0019 RF7.4 and RF7.8):
//!
//! * every `PRD NNNN RFx` cited by a changelog entry points to a real
//!   requirement of an existing plan (RF7.4);
//! * every version a plan (or `0000_control.md`) marks as delivered exists
//!   as a `CHANGELOG.md` section (RF7.8).
//!
//! The changelog itself is parsed by [`crate::changelog`], the same file the
//! product compiles in, so the two never disagree about the format.

use std::collections::BTreeMap;
use std::path::Path;

use crate::changelog::Changelog;
use crate::{err, Error, Res};

/// Runs every check and reports all problems together, the way
/// [`crate::check::run`] does.
pub fn run(root: &Path, text: &str) -> Res<()> {
    let changelog = crate::changelog::parse(text).map_err(|e| Error::Msg(e.to_string()))?;
    let plans = Plans::read(root)?;
    let mut problems = Vec::new();
    check_references(&changelog, &plans, &mut problems);
    check_delivered_versions(root, &changelog, &mut problems);
    if !problems.is_empty() {
        return err(format!(
            "changelog/plan check failed ({} problem(s)):\n - {}",
            problems.len(),
            problems.join("\n - ")
        ));
    }
    Ok(())
}

/// Every plan of `docs/plans`, keyed by its number, loaded once.
struct Plans {
    by_number: BTreeMap<String, String>,
}

impl Plans {
    fn read(root: &Path) -> Res<Plans> {
        let dir = root.join("docs/plans");
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| Error::Msg(format!("cannot read {}: {e}", dir.display())))?;
        let mut by_number = BTreeMap::new();
        for entry in entries {
            let path = entry
                .map_err(|e| Error::Msg(format!("cannot read docs/plans: {e}")))?
                .path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".md") {
                continue;
            }
            let (number, _) = name.split_once('_').unwrap_or((name, name));
            if number.len() != 4 || !number.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let body = std::fs::read_to_string(&path)
                .map_err(|e| Error::Msg(format!("cannot read {}: {e}", path.display())))?;
            by_number.insert(number.to_string(), body);
        }
        Ok(Plans { by_number })
    }

    fn body(&self, number: &str) -> Option<&str> {
        self.by_number.get(number).map(String::as_str)
    }
}

/// `RF7.4` exists in `body` when it appears as a whole requirement id
/// (`RF7` does not match `RF7.1` or `RF70`).
fn has_requirement(body: &str, rf: &str) -> bool {
    body.match_indices(rf).any(|(index, _)| {
        let after = body[index + rf.len()..].chars().next();
        let before = body[..index].chars().next_back();
        !after.is_some_and(|c| c.is_ascii_digit() || c == '.')
            && !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '.')
    })
}

/// RF7.4: every cited reference points at an existing requirement.
fn check_references(changelog: &Changelog, plans: &Plans, problems: &mut Vec<String>) {
    let mut seen = Vec::new();
    for reference in changelog.references() {
        if seen
            .iter()
            .any(|other: &crate::changelog::PlanRef| other == reference)
        {
            continue;
        }
        seen.push(reference.clone());
        let Some(body) = plans.body(&reference.prd) else {
            problems.push(format!(
                "{}: there is no docs/plans/{}_*.md plan",
                reference, reference.prd
            ));
            continue;
        };
        if !has_requirement(body, &reference.rf) {
            problems.push(format!(
                "{reference}: docs/plans/{prd}_*.md has no {rf}",
                prd = reference.prd,
                rf = reference.rf
            ));
        }
    }
}

/// Every `X.Y.Z` inside a table cell (a range cites both ends).
fn cited_versions(cell: &str) -> Vec<String> {
    let bytes = cell.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let mut parts = 0usize;
            let mut cursor = index;
            loop {
                if cursor < bytes.len() && bytes[cursor] == b'.' && cursor + 1 < bytes.len() {
                    let digit = cursor + 1;
                    let mut end = digit;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                    if end > digit {
                        parts += 1;
                        cursor = end;
                        continue;
                    }
                }
                break;
            }
            if parts == 2 {
                let preceded_by_digit = start > 0 && bytes[start - 1].is_ascii_digit();
                if !preceded_by_digit {
                    out.push(cell[start..cursor].to_string());
                }
                index = cursor;
                continue;
            }
            continue;
        }
        index += 1;
    }
    out
}

/// RF7.8: the versions a plan marks as delivered exist in the changelog.
///
/// Two sources are compared: the "Delivered by version" table of
/// `0000_control.md` (`| 0.68.0 | 0021 | … |`) and the "Implementation
/// status" table of every plan (`| RF1.1 | 0.27.1 | … |`). A cell that
/// names a version the changelog does not have is a divergence.
fn check_delivered_versions(root: &Path, changelog: &Changelog, problems: &mut Vec<String>) {
    let known = |version: &str| changelog.find(version).is_some();
    match control_table(root) {
        Some((name, table)) => {
            for line in table.lines() {
                let Some(cells) = table_cells(line) else {
                    continue;
                };
                // The header and the separator carry no version.
                if cells[0].starts_with("Version") || cells[0].starts_with("--") {
                    continue;
                }
                for version in cited_versions(&cells[0]) {
                    if !known(&version) {
                        problems.push(format!(
                            "docs/plans/{name}: delivered row '{row}' cites {version}, which CHANGELOG.md does not have",
                            row = cells[0].trim()
                        ));
                    }
                }
            }
        }
        None => problems.push(
            "docs/plans/0000_control.md has no 'Delivered by version' table (PRD 0019 RF7.8)"
                .to_string(),
        ),
    }
    for (name, body) in plans_of(root) {
        for (index, line) in body.lines().enumerate() {
            let Some(cells) = table_cells(line) else {
                continue;
            };
            if !cells[0].starts_with("RF") || cells.len() < 2 {
                continue;
            }
            for version in cited_versions(&cells[1]) {
                if !known(&version) {
                    problems.push(format!(
                        "docs/plans/{name}:{}: {rf} marks {version} as delivered but CHANGELOG.md has no [{version}] section",
                        index + 1,
                        rf = cells[0].trim()
                    ));
                }
            }
        }
    }
}

/// `[a, b, c]` → `Some([a, b, c])`, keeping the non-empty cells only.
fn table_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let cells: Vec<String> = trimmed
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    if cells.len() < 2 {
        return None;
    }
    Some(cells)
}

/// Every plan file of `docs/plans` with its four-digit number.
fn plans_of(root: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(root.join("docs/plans")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let (number, file) = name.split_once('_').unwrap_or((name, name));
        if !file.ends_with(".md") || number.len() != 4 {
            continue;
        }
        if let Ok(body) = std::fs::read_to_string(&path) {
            out.push((name.to_string(), body));
        }
    }
    out
}

/// The "Delivered by version" section of `docs/plans/0000_control.md`, as
/// `(file name, section text)`.
fn control_table(root: &Path) -> Option<(String, String)> {
    let dir = root.join("docs/plans");
    let entry = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("0000_") && n.ends_with(".md"))
        })?;
    let name = entry.file_name()?.to_str()?.to_string();
    let text = std::fs::read_to_string(&entry).ok()?;
    let mut collecting = false;
    let mut table = String::new();
    for line in text.lines() {
        if line.starts_with("## ") {
            collecting = line.contains("Delivered by version");
        }
        if collecting {
            table.push_str(line);
            table.push('\n');
        }
    }
    Some((name, table))
}
