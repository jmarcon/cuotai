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
    check_footer_links(text, &changelog, &mut problems);
    if let Some(control) = control_text(root) {
        problems.extend(plan_graph_problems(&control));
    }
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
pub struct Plans {
    by_number: BTreeMap<String, String>,
}

impl Plans {
    pub fn read(root: &Path) -> Res<Plans> {
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

    /// Why `PRD prd rf` does not resolve, or `None` when it does (the
    /// commit gate checks `Refs:` trailers with it, PRD 0039 RF2).
    pub fn missing(&self, prd: &str, rf: &str) -> Option<String> {
        match self.body(prd) {
            None => Some(format!("there is no docs/plans/{prd}_*.md plan")),
            Some(body) if !has_requirement(body, rf) => {
                Some(format!("docs/plans/{prd}_*.md has no {rf}"))
            }
            Some(_) => None,
        }
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

/// PRD 0039 RF9.5: every released section has a footer link; an internal
/// version (up to [`crate::release::LAST_INTERNAL_VERSION`], never tagged)
/// links commits, never a `v*` tag or a moving branch; a published version
/// compares tags; `[Unreleased]` compares the newest version with `HEAD`.
fn check_footer_links(text: &str, changelog: &Changelog, problems: &mut Vec<String>) {
    let links: BTreeMap<&str, &str> = text
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix('[')?;
            let (label, url) = rest.split_once("]: ")?;
            Some((label, url.trim()))
        })
        .collect();
    let internal = |version: &str| {
        crate::Version::parse(version).is_ok_and(|v| v <= crate::release::LAST_INTERNAL_VERSION)
    };
    let moving = |url: &str| url.ends_with("...main") || url.ends_with("/main");
    match (links.get("Unreleased"), changelog.releases.first()) {
        (None, _) => problems.push("CHANGELOG.md: no [Unreleased] footer link".to_string()),
        (Some(url), newest) => {
            if !url.ends_with("...HEAD") {
                problems.push(format!(
                    "CHANGELOG.md: the [Unreleased] link must compare with HEAD: {url}"
                ));
            }
            if let Some(newest) = newest {
                let tagged = format!("/compare/v{}...HEAD", newest.version);
                if internal(&newest.version) {
                    if url.contains("/compare/v") {
                        problems.push(format!(
                            "CHANGELOG.md: [Unreleased] compares a tag, but {} is an internal version without one: {url}",
                            newest.version
                        ));
                    }
                } else if !url.ends_with(&tagged) {
                    problems.push(format!(
                        "CHANGELOG.md: [Unreleased] must compare v{}...HEAD: {url}",
                        newest.version
                    ));
                }
            }
        }
    }
    for release in &changelog.releases {
        let Some(url) = links.get(release.version.as_str()) else {
            problems.push(format!(
                "CHANGELOG.md: [{}] has no footer link",
                release.version
            ));
            continue;
        };
        if internal(&release.version) {
            if url.contains("/compare/v") || url.contains("...v") || moving(url) {
                problems.push(format!(
                    "CHANGELOG.md: [{}] is an internal version (no tag): its link must compare commits, not tags or main: {url}",
                    release.version
                ));
            }
        } else if !url.ends_with(&format!("...v{}", release.version)) {
            problems.push(format!(
                "CHANGELOG.md: [{0}] must compare up to the tag v{0}: {url}",
                release.version
            ));
        }
    }
    for label in links.keys() {
        if *label != "Unreleased" && changelog.find(label).is_none() {
            problems.push(format!(
                "CHANGELOG.md: footer link [{label}] has no section"
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
/// The whole `docs/plans/0000_*.md` text.
fn control_text(root: &Path) -> Option<String> {
    let entry = std::fs::read_dir(root.join("docs/plans"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("0000_") && n.ends_with(".md"))
        })?;
    std::fs::read_to_string(entry).ok()
}

/// Text outside parentheses (qualifiers such as `(RF6)` or
/// `(which supersede 0019 RF3)` name requirements, not dependencies).
fn outside_parentheses(text: &str) -> String {
    let mut depth = 0usize;
    text.chars()
        .filter(|c| match c {
            '(' => {
                depth += 1;
                false
            }
            ')' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

/// Every four-digit plan number of `text`, outside parentheses.
fn plan_numbers(text: &str) -> std::collections::BTreeSet<String> {
    outside_parentheses(text)
        .split(|c: char| !c.is_ascii_digit())
        .filter(|token| token.len() == 4)
        .map(str::to_string)
        .collect()
}

/// PRD 0039 RF11: the strong dependencies of §2, the solid edges of the §4
/// graph (`P24 --> P29`: 0024 depends on 0029; sub-nodes such as `P29R0`
/// are not plans) and the "What each plan unblocks" table agree.
pub fn plan_graph_problems(control: &str) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut section = "";
    let mut strong: BTreeSet<(String, String)> = BTreeSet::new();
    let mut graph: BTreeSet<(String, String)> = BTreeSet::new();
    let mut unblocks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut in_unblocks = false;
    for line in control.lines() {
        if line.starts_with("## ") {
            section = line;
        }
        if line.starts_with("### ") || line.starts_with("## ") {
            in_unblocks = line.starts_with("### What each plan unblocks");
        }
        if section.starts_with("## 2.") && line.starts_with("| [") {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            let plan = cells[0].trim().get(1..5).unwrap_or_default().to_string();
            if let Some(deps) = cells.get(4) {
                for dep in plan_numbers(deps) {
                    strong.insert((plan.clone(), dep));
                }
            }
        }
        if let Some((from, to)) = line.trim().split_once(" --> ") {
            let node = |n: &str| -> Option<String> {
                let digits = n.trim().strip_prefix('P')?;
                (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| format!("{:04}", digits.parse::<u32>().unwrap_or(0)))
            };
            if let (Some(a), Some(b)) = (node(from), node(to)) {
                graph.insert((a, b));
            }
        }
        if in_unblocks {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            let plan = cells[0].trim();
            if plan.len() == 4 && plan.bytes().all(|b| b.is_ascii_digit()) && cells.len() > 1 {
                unblocks.insert(plan.to_string(), plan_numbers(cells[1]));
            }
        }
    }
    let mut problems = Vec::new();
    for (a, b) in strong.difference(&graph) {
        problems.push(format!(
            "0000_control.md: §2 says {a} strongly depends on {b}, but the §4 graph has no 'P{} --> P{}' edge",
            a.trim_start_matches('0'),
            b.trim_start_matches('0')
        ));
    }
    for (a, b) in graph.difference(&strong) {
        problems.push(format!(
            "0000_control.md: the §4 graph has a strong edge {a} --> {b} that §2 does not list"
        ));
    }
    let mut expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (a, b) in &strong {
        expected.entry(b.clone()).or_default().insert(a.clone());
    }
    let plans: BTreeSet<&String> = expected.keys().chain(unblocks.keys()).collect();
    for plan in plans {
        let want = expected.get(plan).cloned().unwrap_or_default();
        let have = unblocks.get(plan).cloned().unwrap_or_default();
        if want != have {
            problems.push(format!(
                "0000_control.md: \"What each plan unblocks\" for {plan} lists {:?}, but §2's strong dependencies give {:?}",
                have, want
            ));
        }
    }
    problems
}

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

#[cfg(test)]
mod tests {
    use super::plan_graph_problems;

    const CONTROL: &str = "\
## 2. Current situation

| ID | Plan | Status | Delivered | Depends on (strong) | Depends on (weak) | Notes |
|---|---|---|---|---|---|---|
| [0018](0018_x.md) | U | In progress | — | 0019 (RF2), 0021 (RF1–RF4, which supersede 0019 RF3) | 0019 (RF7) | — |
| [0024](0024_x.md) | P | Proposed | — | 0008; 0029 RF1 (for RF7) | 0010 | — |
| [0029](0029_x.md) | I | Proposed | — | — | — | — |

## 4. Dependency graph

```mermaid
graph LR
  P18 --> P19
  P18 --> P21
  P24 --> P8
  P24 --> P29
  P29 --> P29R0
  P24 -.-> P10
```

### What each plan unblocks

| Plan | Unblocks (strong) | Benefits (weak) |
|---|---|---|
| 0008 | 0024 | — |
| 0019 | 0018 | 0018 |
| 0021 | 0018 | — |
| 0029 | 0024 (RF7) | — |

## 5. Next
";

    #[test]
    fn agreeing_views_pass() {
        let problems = plan_graph_problems(CONTROL);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_missing_edge_an_extra_edge_and_a_stale_cell_fail() {
        let missing = CONTROL.replace("  P24 --> P29\n", "");
        assert!(plan_graph_problems(&missing)[0].contains("0024 strongly depends on 0029"));
        let extra = CONTROL.replace("  P24 -.-> P10\n", "  P24 --> P10\n");
        assert!(plan_graph_problems(&extra)[0].contains("strong edge 0024 --> 0010"));
        let stale = CONTROL.replace("| 0021 | 0018 | — |", "| 0021 | — | — |");
        assert!(plan_graph_problems(&stale)[0].contains("unblocks\" for 0021"));
    }
}
