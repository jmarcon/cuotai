//! Pure rendering and text edits of `release prepare` (PRD 0039 RF4.2,
//! RF4.3, RF4.4, RF6.2): changelog entries and sections, footer links,
//! the `vNEXT` token, the plan control and a small unified diff for
//! `--dry-run`. Nothing here touches the disk or Git, so every function is
//! golden-tested.

use std::collections::BTreeMap;

use crate::changelog::Category;
use crate::commits::{Commit, Reference};

/// The canonical entry line (RF4.3):
/// `- [**Breaking**: ]<sentence>[. Migration: <migration>] (<refs>; <hash>).`
/// The sentence and the migration lose their final period, which moves to
/// the end of the line after the references.
pub fn entry(commit: &Commit, sentence: &str, short: &str) -> String {
    let sentence = sentence.trim();
    let mut text = sentence.strip_suffix('.').unwrap_or(sentence).to_string();
    if commit.is_breaking() {
        text = format!("**Breaking**: {text}");
        if let Some(migration) = &commit.trailers.breaking {
            let migration = migration.trim();
            text.push_str(". Migration: ");
            text.push_str(migration.strip_suffix('.').unwrap_or(migration));
        }
    }
    let mut parts = references(&commit.trailers.refs);
    parts.push(short.to_string());
    format!("- {text} ({}).", parts.join("; "))
}

/// References grouped by plan in first-appearance order, then issues:
/// `["PRD 0018 RF1.2, RF1.3", "PRD 0021 RF2", "issue #2"]`.
pub fn references(refs: &[Reference]) -> Vec<String> {
    let mut plans: Vec<(String, Vec<String>)> = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    for reference in refs {
        match reference {
            Reference::Plan { prd, rf } => match plans.iter_mut().find(|(p, _)| p == prd) {
                Some((_, rfs)) => {
                    if !rfs.contains(rf) {
                        rfs.push(rf.clone());
                    }
                }
                None => plans.push((prd.clone(), vec![rf.clone()])),
            },
            Reference::Issue(number) => {
                let text = format!("issue #{number}");
                if !issues.contains(&text) {
                    issues.push(text);
                }
            }
        }
    }
    plans
        .into_iter()
        .map(|(prd, rfs)| format!("PRD {prd} {}", rfs.join(", ")))
        .chain(issues)
        .collect()
}

/// Entries per category, in the parser's category order.
pub type Entries = BTreeMap<Category, Vec<String>>;

/// `## [X.Y.Z] - YYYY-MM-DD` with its categories (RF4.2), or the parser's
/// plain paragraph when there is nothing to list (RF3.5). Ends with one
/// newline.
pub fn section(version: &str, date: &str, entries: &Entries) -> String {
    let mut out = format!("## [{version}] - {date}\n");
    let mut any = false;
    for category in Category::ALL {
        let Some(lines) = entries.get(&category).filter(|lines| !lines.is_empty()) else {
            continue;
        };
        any = true;
        out.push_str(&format!("\n### {}\n\n", category.title()));
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !any {
        out.push_str("\nNo user-visible changes.\n");
    }
    out
}

/// The entry lines of a section body (`### X` blocks and `- ` lines),
/// verbatim, for `--refresh` and the carried `[Unreleased]` entries.
pub fn entries_of(body: &str) -> Entries {
    let mut out = Entries::new();
    let mut current: Option<Category> = None;
    for line in body.lines() {
        let line = line.trim_end();
        if let Some(title) = line.strip_prefix("### ") {
            current = Category::ALL
                .into_iter()
                .find(|category| category.title() == title.trim());
            continue;
        }
        if line.starts_with("- ") {
            if let Some(category) = current {
                out.entry(category).or_default().push(line.to_string());
            }
        }
    }
    out
}

/// The line range `[start, end)` of the section whose heading satisfies
/// `is_heading`, ending at the next `## ` heading or the footer links.
fn section_range(lines: &[&str], is_heading: impl Fn(&str) -> bool) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| is_heading(line))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.starts_with("## ") || is_link_definition(line))
        .map_or(lines.len(), |(index, _)| index);
    Some((start, end))
}

fn is_link_definition(line: &str) -> bool {
    line.starts_with('[') && line.contains("]: ")
}

/// The body of `## [Unreleased]` (between its heading and the next one).
pub fn unreleased_body(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    match section_range(&lines, |line| line.trim_end() == "## [Unreleased]") {
        Some((start, end)) => lines[start + 1..end].join("\n"),
        None => String::new(),
    }
}

/// The body of the released section `version`.
pub fn section_body(text: &str, version: &str) -> Option<String> {
    let heading = format!("## [{version}] - ");
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = section_range(&lines, |line| line.starts_with(&heading))?;
    Some(lines[start + 1..end].join("\n"))
}

/// Replaces `[Unreleased]`'s content with an empty section followed by
/// `section` (new release), or replaces the released section `replace`
/// (refresh).
pub fn insert_section(text: &str, section: &str, replace: Option<&str>) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = match replace {
        None => {
            let (start, end) = section_range(&lines, |line| line.trim_end() == "## [Unreleased]")
                .ok_or("CHANGELOG.md has no '## [Unreleased]' section")?;
            (start + 1, end)
        }
        Some(version) => {
            let heading = format!("## [{version}] - ");
            section_range(&lines, |line| line.starts_with(&heading)).ok_or(format!(
                "CHANGELOG.md has no [{version}] section to refresh"
            ))?
        }
    };
    let mut out: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
    if replace.is_none() {
        out.push(String::new());
    }
    out.extend(section.trim_end().lines().map(str::to_string));
    out.push(String::new());
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    Ok(out.join("\n") + "\n")
}

/// The repository URL the footer links use (`https://github.com/o/r`).
pub fn repository_url(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|line| line.starts_with("[Unreleased]: "))?;
    let url = line.trim_start_matches("[Unreleased]: ").trim();
    url.split_once("/compare/")
        .map(|(base, _)| base.to_string())
}

/// RF4.4 item 6: `[Unreleased]` compares the new tag with `HEAD`, and the
/// new version gets a link right below it (kept when it already exists).
pub fn update_footer(text: &str, version: &str, from: &str) -> Result<String, String> {
    let repo = repository_url(text)
        .ok_or("CHANGELOG.md has no '[Unreleased]: <repo>/compare/…' footer link to update")?;
    let own = format!("[{version}]: ");
    let has_own = text.lines().any(|line| line.starts_with(&own));
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("[Unreleased]: ") {
            out.push(format!("[Unreleased]: {repo}/compare/v{version}...HEAD"));
            if !has_own {
                out.push(format!("{own}{repo}/compare/{from}...v{version}"));
            }
            continue;
        }
        out.push(line.to_string());
    }
    Ok(out.join("\n") + "\n")
}

/// Calls `edit` on every Markdown prose segment of `text`: outside fenced
/// code blocks and outside inline code spans.
fn map_prose(text: &str, mut edit: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced {
            out.push_str(line);
            continue;
        }
        for (index, segment) in line.split('`').enumerate() {
            if index > 0 {
                out.push('`');
            }
            if index % 2 == 0 {
                out.push_str(&edit(segment));
            } else {
                out.push_str(segment);
            }
        }
    }
    out
}

/// The plan token of RF6.1.
pub const NEXT_TOKEN: &str = "vNEXT";

/// Replaces every exact `vNEXT` token in prose (not `vNEXTx`, not inside
/// code) with `version`; returns the new text and the count.
pub fn replace_next(text: &str, version: &str) -> (String, usize) {
    let mut count = 0;
    let out = map_prose(text, |segment| {
        let mut result = String::new();
        let mut rest = segment;
        while let Some(index) = rest.find(NEXT_TOKEN) {
            let before = rest[..index].chars().next_back();
            let after = rest[index + NEXT_TOKEN.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            result.push_str(&rest[..index]);
            if word(before) || word(after) {
                result.push_str(NEXT_TOKEN);
            } else {
                result.push_str(version);
                count += 1;
            }
            rest = &rest[index + NEXT_TOKEN.len()..];
        }
        result.push_str(rest);
        result
    });
    (out, count)
}

/// How many `vNEXT` tokens a file still has (RF4.7, RF5.4).
pub fn count_next(text: &str) -> usize {
    replace_next(text, "X").1
}

/// RF6.2: the `0000_control.md` header and the generated §3 rows.
pub fn update_control(text: &str, version: &str, date: &str, rows: &[String]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut in_delivered = false;
    let mut last_row: Option<usize> = None;
    let row_prefix = format!("| {version} |");
    for line in lines {
        if line.starts_with("## ") {
            in_delivered = line.contains("Delivered by version");
        }
        if line.starts_with("| Last updated |") {
            out.push(format!("| Last updated | {date} |"));
            continue;
        }
        if line.starts_with("| Cuotai version at that date |") {
            out.push(format!("| Cuotai version at that date | {version} |"));
            continue;
        }
        if in_delivered && line.starts_with(&row_prefix) {
            continue; // regenerated below (refresh)
        }
        out.push(line.to_string());
        if in_delivered && line.starts_with('|') {
            last_row = Some(out.len() - 1);
        }
    }
    if let Some(index) = last_row {
        for (offset, row) in rows.iter().enumerate() {
            out.insert(index + 1 + offset, row.clone());
        }
    }
    out.join("\n") + "\n"
}

/// RF6.2: one `| X.Y.Z | NNNN | RF0, RF1 (abc1234, def5678). |` row per
/// plan cited by a `Refs:` trailer of the released commits, by plan number.
pub fn control_rows(version: &str, commits: &[(&Commit, String)]) -> Vec<String> {
    let mut plans: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for (commit, short) in commits {
        for reference in &commit.trailers.refs {
            let Reference::Plan { prd, rf } = reference else {
                continue;
            };
            let (rfs, hashes) = plans.entry(prd.clone()).or_default();
            if !rfs.contains(rf) {
                rfs.push(rf.clone());
            }
            if !hashes.contains(short) {
                hashes.push(short.clone());
            }
        }
    }
    plans
        .into_iter()
        .map(|(prd, (rfs, hashes))| {
            format!(
                "| {version} | {prd} | {} ({}). |",
                rfs.join(", "),
                hashes.join(", ")
            )
        })
        .collect()
}

/// A minimal unified diff (two lines of context) for `--dry-run`.
pub fn unified_diff(path: &str, old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    // Edit script over the differing middle: LCS when small enough.
    let mut ops: Vec<(char, &str)> = Vec::new();
    if am.len() * bm.len() <= 4_000_000 {
        let (n, m) = (am.len(), bm.len());
        let mut lcs = vec![vec![0u32; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i][j] = if am[i] == bm[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && am[i] == bm[j] {
                ops.push((' ', am[i]));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
                ops.push(('-', am[i]));
                i += 1;
            } else {
                ops.push(('+', bm[j]));
                j += 1;
            }
        }
    } else {
        ops.extend(am.iter().map(|line| ('-', *line)));
        ops.extend(bm.iter().map(|line| ('+', *line)));
    }
    if ops.iter().all(|(op, _)| *op == ' ') {
        return String::new();
    }
    let mut full: Vec<(char, &str)> = a[..prefix].iter().map(|line| (' ', *line)).collect();
    full.extend(ops);
    full.extend(a[a.len() - suffix..].iter().map(|line| (' ', *line)));

    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let context = 2usize;
    let changed: Vec<usize> = full
        .iter()
        .enumerate()
        .filter(|(_, (op, _))| *op != ' ')
        .map(|(index, _)| index)
        .collect();
    let mut index = 0;
    while index < changed.len() {
        let start = changed[index].saturating_sub(context);
        let mut end = changed[index];
        while index < changed.len() && changed[index] <= end + 2 * context + 1 {
            end = changed[index];
            index += 1;
        }
        let end = (end + context + 1).min(full.len());
        let old_start = full[..start].iter().filter(|(op, _)| *op != '+').count() + 1;
        let new_start = full[..start].iter().filter(|(op, _)| *op != '-').count() + 1;
        let old_len = full[start..end].iter().filter(|(op, _)| *op != '+').count();
        let new_len = full[start..end].iter().filter(|(op, _)| *op != '-').count();
        out.push_str(&format!(
            "@@ -{old_start},{old_len} +{new_start},{new_len} @@\n"
        ));
        for (op, line) in &full[start..end] {
            out.push(*op);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str, body: &str) -> Commit {
        Commit::parse(
            "0123456789abcdef0123456789abcdef01234567",
            subject,
            &format!("{subject}\n\n{body}"),
        )
    }

    #[test]
    fn golden_entries() {
        let plain = commit("feat: a", "Changelog: You can sort accounts.");
        assert_eq!(
            entry(&plain, "You can sort accounts.", "0123456"),
            "- You can sort accounts (0123456)."
        );
        let refs = commit(
            "feat: a",
            "Changelog: X.\nRefs: PRD 0018 RF1.2\nRefs: PRD 0021 RF2\nRefs: PRD 0018 RF1.3\nRefs: issue #2",
        );
        assert_eq!(
            entry(&refs, "The updater checks daily.", "abcdef1"),
            "- The updater checks daily (PRD 0018 RF1.2, RF1.3; PRD 0021 RF2; issue #2; abcdef1)."
        );
        let breaking = commit(
            "feat(serve)!: token",
            "Changelog: The server needs a token.\nBREAKING CHANGE: send the bearer token.\nRefs: PRD 0012 INT-04",
        );
        assert_eq!(
            entry(&breaking, "The server needs a token.", "f4e2e54"),
            "- **Breaking**: The server needs a token. Migration: send the bearer token (PRD 0012 INT-04; f4e2e54)."
        );
    }

    #[test]
    fn grouped_references_round_trip_through_the_shared_parser() {
        let c = commit(
            "feat: a",
            "Changelog: X.\nRefs: PRD 0018 RF1.2, RF1.3, RF4\nRefs: PRD 0021 RF2\nRefs: PRD 0021 RF5.1\nRefs: #7",
        );
        let line = entry(&c, "The updater checks daily.", "abcdef1");
        let doc =
            format!("# C\n\n## [Unreleased]\n\n## [0.80.0] - 2026-10-24\n\n### Added\n\n{line}\n");
        let parsed = crate::changelog::parse(&doc).unwrap();
        let cited: Vec<String> = parsed.references().iter().map(|r| r.to_string()).collect();
        let expected: Vec<String> = c
            .trailers
            .refs
            .iter()
            .filter_map(|r| match r {
                Reference::Plan { prd, rf } => Some(format!("PRD {prd} {rf}")),
                Reference::Issue(_) => None,
            })
            .collect();
        assert_eq!(cited, expected, "{line}");
        let entry = &parsed.releases[0].section.categories[0].entries[0];
        assert_eq!(entry.hashes, ["abcdef1"]);
    }

    #[test]
    fn golden_section_and_it_parses() {
        let mut entries = Entries::new();
        entries
            .entry(Category::Fixed)
            .or_default()
            .push("- A fix (abc1234).".into());
        entries
            .entry(Category::Added)
            .or_default()
            .push("- A feature (PRD 0001 RF1; def5678).".into());
        entries
            .entry(Category::Removed)
            .or_default()
            .push("- A removal (1234abc).".into());
        let text = section("0.80.0", "2026-10-24", &entries);
        assert_eq!(
            text,
            "## [0.80.0] - 2026-10-24\n\n### Added\n\n- A feature (PRD 0001 RF1; def5678).\n\n### Fixed\n\n- A fix (abc1234).\n\n### Removed\n\n- A removal (1234abc).\n"
        );
        let doc = format!("# Changelog\n\n## [Unreleased]\n\n{text}");
        let parsed = crate::changelog::parse(&doc).unwrap();
        assert_eq!(parsed.releases[0].section.categories.len(), 3);
        assert_eq!(
            section("0.80.0", "2026-10-24", &Entries::new()),
            "## [0.80.0] - 2026-10-24\n\nNo user-visible changes.\n"
        );
    }

    const DOC: &str = "# Changelog\n\nPreface.\n\n## [Unreleased]\n\n### Added\n\n- Carried (abc1234).\n\n## [0.79.0] - 2026-10-23\n\n### Fixed\n\n- Old (2bbb39a).\n\n[Unreleased]: https://github.com/o/r/compare/2bbb39a...HEAD\n[0.79.0]: https://github.com/o/r/compare/f4e2e54...2bbb39a\n";

    #[test]
    fn inserts_a_section_and_updates_the_footer() {
        let carried = entries_of(&unreleased_body(DOC));
        assert_eq!(carried[&Category::Added], ["- Carried (abc1234)."]);
        let section = section("0.80.0", "2026-10-24", &carried);
        let text = insert_section(DOC, &section, None).unwrap();
        let text = update_footer(&text, "0.80.0", "2bbb39a").unwrap();
        let parsed = crate::changelog::parse(&text).unwrap();
        assert!(parsed.unreleased.is_empty());
        assert_eq!(parsed.releases[0].version, "0.80.0");
        assert!(text.contains(
            "[Unreleased]: https://github.com/o/r/compare/v0.80.0...HEAD\n[0.80.0]: https://github.com/o/r/compare/2bbb39a...v0.80.0\n[0.79.0]"
        ));
        // Refresh replaces the same section and keeps the footer.
        let refreshed = insert_section(
            &text,
            "## [0.80.0] - 2026-10-25\n\n### Added\n\n- Carried (abc1234).\n- New (fedcba9).\n",
            Some("0.80.0"),
        )
        .unwrap();
        let again = update_footer(&refreshed, "0.80.0", "2bbb39a").unwrap();
        let parsed = crate::changelog::parse(&again).unwrap();
        assert_eq!(parsed.releases[0].date, "2026-10-25");
        assert_eq!(parsed.releases.len(), 2);
        assert_eq!(again.matches("[0.80.0]: ").count(), 1);
    }

    #[test]
    fn next_token_replacement() {
        let text = "| RF2 | vNEXT | done |\nKeep `vNEXT` and vNEXTx and avNEXT.\n```\nvNEXT\n```\nAlso (vNEXT).\n";
        let (out, count) = replace_next(text, "0.80.0");
        assert_eq!(count, 2);
        assert_eq!(
            out,
            "| RF2 | 0.80.0 | done |\nKeep `vNEXT` and vNEXTx and avNEXT.\n```\nvNEXT\n```\nAlso (0.80.0).\n"
        );
        assert_eq!(count_next(&out), 0);
    }

    #[test]
    fn control_rows_and_header() {
        let a = commit("feat: a", "Refs: PRD 0029 RF1, RF0");
        let b = commit("fix: b", "Refs: PRD 0029 RF1\nRefs: PRD 0003 RF2");
        let rows = control_rows(
            "0.80.0",
            &[(&a, "abc1234".to_string()), (&b, "def5678".to_string())],
        );
        assert_eq!(
            rows,
            [
                "| 0.80.0 | 0003 | RF2 (def5678). |",
                "| 0.80.0 | 0029 | RF1, RF0 (abc1234, def5678). |"
            ]
        );
        let control = "# C\n\n| Field | Value |\n|---|---|\n| Last updated | 2026-10-01 |\n| Cuotai version at that date | 0.79.0 |\n\n## 3. Delivered by version\n\n| Version | Plan | Requirements delivered |\n|---|---|---|\n| 0.79.0 | 0001 | RF1. |\n| 0.80.0 | 0009 | stale. |\n\n## 4. Next\n\n| a | b |\n";
        let out = update_control(control, "0.80.0", "2026-10-24", &rows);
        assert!(out.contains("| Last updated | 2026-10-24 |"));
        assert!(out.contains("| Cuotai version at that date | 0.80.0 |"));
        assert!(!out.contains("stale"));
        assert!(out.contains(
            "| 0.79.0 | 0001 | RF1. |\n| 0.80.0 | 0003 | RF2 (def5678). |\n| 0.80.0 | 0029 | RF1, RF0 (abc1234, def5678). |\n\n## 4. Next"
        ));
    }

    #[test]
    fn diff_shows_only_changes() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nB\nc\nd\ne\nf\ng\nh\ni\n";
        let diff = unified_diff("x", old, new);
        assert!(diff.contains("-b\n+B\n"), "{diff}");
        assert!(diff.contains("+i\n"), "{diff}");
        assert!(unified_diff("x", old, old).is_empty());
    }
}
