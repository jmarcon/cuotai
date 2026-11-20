//! Changelog and plan-control agreement (PRD 0019 RF7.4 and RF7.8):
//!
//! * every `PRD NNNN RFx` cited by a changelog entry points to a real
//!   requirement of an existing plan (RF7.4);
//! * every version a plan (or `0000_control.md`) marks as delivered exists
//!   as a `CHANGELOG.md` section (RF7.8), or, for an internal build
//!   (0.1.0–0.79.0), falls in the pre-release history's range.
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
    check_pre_release(&changelog, &mut problems);
    if let Some(control) = control_text(root) {
        problems.extend(plan_graph_problems(&control));
        problems.extend(plan_status_problems(&control, &plans));
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

    /// Why `PRD prd rf` is not *defined* by its plan (PRD 0053 RF2.4), or
    /// `None` when it is: a mention in a range or in prose is not enough.
    pub fn undefined(&self, prd: &str, rf: &str) -> Option<String> {
        if let Some(problem) = self.missing(prd, rf) {
            return Some(problem);
        }
        let body = self.body(prd)?;
        (!defines_requirement(body, rf)).then(|| {
            format!(
                "docs/plans/{prd}_*.md mentions {rf} but does not define it (a `### {rf} —` \
                 heading, a `**{rf}**` item, a `- {rf}:` list item or a `| {rf} |` table row)"
            )
        })
    }
}

/// PRD 0053 RF2.4: `rf` is defined by a line that starts with it, after
/// heading marks, a list marker, a table pipe and bold marks.
pub fn defines_requirement(body: &str, rf: &str) -> bool {
    body.lines().any(|line| {
        let mut rest = line.trim_start();
        let heading = rest.starts_with('#');
        rest = rest.trim_start_matches('#').trim_start();
        let listed = rest.starts_with("- ") || rest.starts_with("* ");
        if listed {
            rest = rest[2..].trim_start();
        }
        let cell = rest.starts_with('|');
        rest = rest.trim_start_matches('|').trim_start();
        let bold = rest.starts_with("**");
        rest = rest.trim_start_matches('*');
        let Some(after) = rest.strip_prefix(rf) else {
            return false;
        };
        let next = after.chars().next();
        if next.is_some_and(|c| c.is_ascii_digit() || c == '.') {
            return false;
        }
        // A bare list item needs a separator (`- RF2.2: …`, `- RF3.3 (…)`).
        heading
            || bold
            || cell
            || (listed && next.is_some_and(|c| matches!(c, ':' | '—' | '–' | '-' | '(' | ' ')))
    })
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

/// The pre-release history summarizes internal builds only: its range ends
/// at or before [`crate::release::LAST_INTERNAL_VERSION`], so a published
/// version can never hide in it (owner decision of 2026-10-23, PRD 0039
/// D11).
fn check_pre_release(changelog: &Changelog, problems: &mut Vec<String>) {
    let Some(history) = &changelog.pre_release else {
        return;
    };
    let last = crate::Version::parse(&history.last);
    if !last.is_ok_and(|v| v <= crate::release::LAST_INTERNAL_VERSION) {
        problems.push(format!(
            "CHANGELOG.md: the pre-release history ends at {}, after the last internal version {}: published versions get their own section",
            history.last,
            crate::release::LAST_INTERNAL_VERSION
        ));
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
    // A version is known by its own section or, for an internal build, by
    // the pre-release history that summarizes it (PRD 0039 D11): the §3
    // rows of 0.1.0–0.79.0 stay the per-version detail.
    let known = |version: &str| changelog.knows(version);
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

/// The statuses of `0000_control.md` §1, with the §4 node class of each.
const STATUSES: [(&str, &[&str]); 6] = [
    ("Awaiting owner verification", &["await"]),
    ("Implemented", &["done"]),
    ("In progress", &["wip", "deferred"]),
    ("Proposed", &["todo", "deferred"]),
    ("Superseded", &["done", "deferred"]),
    ("Cancelled", &["deferred"]),
];

/// The §1 status a cell starts with (`Implemented, with owner steps …`).
fn status_of(cell: &str) -> Option<&'static str> {
    let cell = cell.trim();
    STATUSES
        .iter()
        .map(|(status, _)| *status)
        .find(|status| cell.starts_with(status))
}

/// PRD 0053 RF2.2/RF2.3 (first part): the header's "Plans" counts, each
/// plan's own `| Status |` and its §4 node class agree with its §2 row.
pub fn plan_status_problems(control: &str, plans: &Plans) -> Vec<String> {
    let mut problems = Vec::new();
    let mut section = "";
    let mut rows: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut classes: BTreeMap<String, String> = BTreeMap::new();
    let mut header: Option<&str> = None;
    for line in control.lines() {
        if line.starts_with("## ") {
            section = line;
        }
        if let Some(rest) = line.strip_prefix("| Plans |") {
            header = Some(rest.trim().trim_end_matches('|').trim());
        }
        if section.starts_with("## 2.") && line.starts_with("| [") {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            let plan = cells[0].trim().get(1..5).unwrap_or_default().to_string();
            match cells.get(2).and_then(|c| status_of(c)) {
                Some(status) => {
                    rows.insert(plan, status);
                }
                None => problems.push(format!(
                    "0000_control.md: §2 row {plan} has no known status ({:?})",
                    cells.get(2).map(|c| c.trim()).unwrap_or_default()
                )),
            }
        }
        if section.starts_with("## 4.") {
            let trimmed = line.trim();
            if let (Some(rest), Some((_, class))) =
                (trimmed.strip_prefix('P'), trimmed.split_once("]:::"))
            {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if !digits.is_empty() && rest[digits.len()..].starts_with('[') {
                    let number = format!("{:04}", digits.parse::<u32>().unwrap_or(0));
                    classes.insert(number, class.trim().to_string());
                }
            }
        }
    }
    // Every numbered plan has a §2 row, and every row a plan.
    for number in plans.by_number.keys().filter(|n| n.as_str() != "0000") {
        if !rows.contains_key(number)
            && !problems
                .iter()
                .any(|p| p.contains(&format!("row {number} ")))
        {
            problems.push(format!(
                "0000_control.md: §2 has no row for docs/plans/{number}_*.md"
            ));
        }
    }
    for (number, status) in &rows {
        match plans.body(number) {
            None => problems.push(format!(
                "0000_control.md: §2 row {number} has no docs/plans/{number}_*.md plan"
            )),
            Some(body) => {
                let own = body
                    .lines()
                    .find_map(|l| l.strip_prefix("| Status |"))
                    .map(|cell| cell.trim().trim_end_matches('|').trim());
                if own.and_then(status_of) != Some(status) {
                    problems.push(format!(
                        "docs/plans/{number}_*.md: its Status ({:?}) does not start with §2's \"{status}\"",
                        own.unwrap_or("missing")
                    ));
                }
            }
        }
        let allowed = STATUSES
            .iter()
            .find(|(s, _)| s == status)
            .map(|(_, c)| *c)
            .unwrap_or(&[]);
        match classes.get(number) {
            Some(class) if allowed.contains(&class.as_str()) => {}
            Some(class) => {
                problems.push(format!(
                "0000_control.md: §4 gives P{} the class :::{class}, but §2 says \"{status}\" ({})",
                number.trim_start_matches('0'),
                allowed.iter().map(|c| format!(":::{c}")).collect::<Vec<_>>().join(" or ")
            ))
            }
            None => problems.push(format!(
                "0000_control.md: §4 has no classed node P{} for plan {number}",
                number.trim_start_matches('0')
            )),
        }
    }
    // The header: `56 (21 implemented, 34 awaiting owner verification, …)`.
    match header {
        None => problems.push("0000_control.md: the header has no \"Plans\" row".to_string()),
        Some(cell) => {
            let (total, rest) = cell.split_once(' ').unwrap_or((cell, ""));
            if total.parse::<usize>().ok() != Some(rows.len()) {
                problems.push(format!(
                    "0000_control.md: the header counts {total} plans, but §2 has {} rows",
                    rows.len()
                ));
            }
            let mut stated: BTreeMap<&'static str, usize> = BTreeMap::new();
            let inner = rest.trim().trim_start_matches('(').trim_end_matches(')');
            for part in inner.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                let (count, label) = part.split_once(' ').unwrap_or((part, ""));
                let status = STATUSES
                    .iter()
                    .map(|(s, _)| *s)
                    .find(|s| s.eq_ignore_ascii_case(label.trim()));
                match (count.parse::<usize>(), status) {
                    (Ok(n), Some(status)) => {
                        stated.insert(status, n);
                    }
                    (Ok(0), None) => {} // a zero count of a label §1 does not list
                    _ => problems.push(format!(
                        "0000_control.md: the header's \"{part}\" is not a count of a §1 status"
                    )),
                }
            }
            for (status, _) in STATUSES {
                let actual = rows.values().filter(|s| **s == status).count();
                let said = stated.get(status).copied().unwrap_or(0);
                if said != actual {
                    problems.push(format!(
                        "0000_control.md: the header says {said} {}, but §2 has {actual}",
                        status.to_lowercase()
                    ));
                }
            }
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
    use super::{defines_requirement, plan_graph_problems, plan_status_problems, Plans};
    use std::collections::BTreeMap;

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

    #[test]
    fn each_definition_shape_defines_and_a_mention_does_not() {
        for (body, rf) in [
            ("### RF2 — Sorting", "RF2"),
            ("- **RF2.4** Resolve ids.", "RF2.4"),
            ("- **RF1 — Base**", "RF1"),
            ("  - RF3.3 (ordering) sorts ids.", "RF3.3"),
            ("- RF7: notes.", "RF7"),
            ("| RF2 | 0.80.0 | Done. |", "RF2"),
            ("| G-0048-2 | vNEXT | Fixed. |", "G-0048-2"),
            ("| FIX-01 | 0.86.0 | Fixed. |", "FIX-01"),
        ] {
            assert!(defines_requirement(body, rf), "{rf} in {body:?}");
        }
        for (body, rf) in [
            ("| RF4–RF8 | later |", "RF8"),
            ("See RF4–RF8 for the rest.", "RF8"),
            ("RF2 is mentioned in prose.", "RF2"),
            ("### RF1.1 — Part", "RF1"),
            ("### RF10 — Tenth", "RF1"),
            ("- RF2", "RF2"),
        ] {
            assert!(!defines_requirement(body, rf), "{rf} in {body:?}");
        }
    }

    fn plans(entries: &[(&str, &str)]) -> Plans {
        Plans {
            by_number: entries
                .iter()
                .map(|(n, b)| ((*n).to_string(), (*b).to_string()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    const STATUS_CONTROL: &str = "\
| Plans | 2 (1 implemented, 0 planned for the next version, 1 proposed) |

## 2. Current situation

| ID | Plan | Status |
|---|---|---|
| [0001](0001_a.md) | A | Implemented |
| [0002](0002_b.md) | B | Proposed |

## 4. Dependency graph

  P1[\"0001 A\"]:::done
  P2[\"0002 B\"]:::todo

## 5. Next
";

    fn status_plans() -> Plans {
        plans(&[
            ("0001", "| Status | Implemented |"),
            ("0002", "| Status | Proposed |"),
        ])
    }

    #[test]
    fn agreeing_statuses_pass() {
        let problems = plan_status_problems(STATUS_CONTROL, &status_plans());
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn drifted_counts_statuses_and_classes_fail() {
        let count = STATUS_CONTROL.replace("2 (1 implemented", "3 (1 implemented");
        let problems = plan_status_problems(&count, &status_plans());
        assert!(problems[0].contains("counts 3 plans"), "{problems:?}");

        let header = plans(&[
            ("0001", "| Status | Implemented |"),
            ("0002", "| Status | In progress |"),
        ]);
        let problems = plan_status_problems(STATUS_CONTROL, &header);
        assert!(
            problems[0].contains("0002_*.md: its Status"),
            "{problems:?}"
        );

        let class = STATUS_CONTROL.replace(":::todo", ":::done");
        let problems = plan_status_problems(&class, &status_plans());
        assert!(problems[0].contains("P2 the class :::done"), "{problems:?}");

        let node = STATUS_CONTROL.replace("  P2[\"0002 B\"]:::todo\n", "");
        let problems = plan_status_problems(&node, &status_plans());
        assert!(problems[0].contains("no classed node P2"), "{problems:?}");

        let unknown = STATUS_CONTROL.replace("| B | Proposed |", "| B | Maybe |");
        let problems = plan_status_problems(&unknown, &status_plans());
        assert!(
            problems[0].contains("row 0002 has no known status"),
            "{problems:?}"
        );

        let label = STATUS_CONTROL.replace("0 planned for", "1 planned for");
        let problems = plan_status_problems(&label, &status_plans());
        assert!(
            problems
                .iter()
                .any(|p| p.contains("\"1 planned for the next version\"")),
            "{problems:?}"
        );

        let extra = plans(&[
            ("0001", "| Status | Implemented |"),
            ("0002", "| Status | Proposed |"),
            ("0003", "| Status | Proposed |"),
        ]);
        let problems = plan_status_problems(STATUS_CONTROL, &extra);
        assert!(
            problems[0].contains("no row for docs/plans/0003"),
            "{problems:?}"
        );
    }

    #[test]
    fn the_real_control_file_agrees_with_the_plans() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let plans = Plans::read(&root).unwrap();
        let control = std::fs::read_to_string(root.join("docs/plans/0000_control.md")).unwrap();
        let problems = plan_status_problems(&control, &plans);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
