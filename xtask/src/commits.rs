//! The commit contract (PRD 0039 RF1): one parser for the conventional
//! header and the trailers that carry a changelog entry, shared by the
//! commit gate (RF2), the version computation (RF3) and `release prepare`
//! (RF4).
//!
//! A commit message is `type(scope)!: subject`, a body, and a last
//! paragraph of Git trailers (`Key: value`, continuation lines indented by
//! one space). The trailers this module understands are:
//!
//! * `Changelog: <sentence>` (several allowed) or `Changelog: skip <reason>`;
//! * `Changelog-Category: Added|Changed|Deprecated|Removed|Fixed|Security`;
//! * `Refs: PRD 0006 RF2, RF3; PRD 0009 RF1` and `Refs: #12` / `issue #12`;
//! * `BREAKING CHANGE: <migration>` (also `BREAKING-CHANGE:`);
//! * `Changelog-Edit: <reason>` (a `docs(changelog)` prose correction);
//! * `Version-Override: X.Y.Z` (release commits only).
//!
//! Parsing never fails: every problem is collected in [`Commit::problems`],
//! so the gate can report all of them together.

use std::path::Path;

use crate::changelog::Category;
use crate::{git, Res, Version};

/// The conventional-commit types (RF1.1).
pub const TYPES: [&str; 8] = [
    "feat", "fix", "docs", "refactor", "test", "style", "chore", "revert",
];

/// The parsed `type(scope)!: subject` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: String,
    pub scope: Option<String>,
    /// The `!` marker.
    pub bang: bool,
    pub description: String,
}

/// Parses a conventional header, or explains why it is not one.
pub fn parse_header(subject: &str) -> Result<Header, String> {
    let subject = subject.trim_end_matches(['\r', '\n']);
    let Some((prefix, description)) = subject.split_once(": ") else {
        return Err(format!(
            "header '{subject}' is not conventional: expected 'type(scope): subject'"
        ));
    };
    let (prefix, bang) = match prefix.strip_suffix('!') {
        Some(rest) => (rest, true),
        None => (prefix, false),
    };
    let (kind, scope) = match prefix.split_once('(') {
        Some((kind, rest)) => {
            let Some(scope) = rest.strip_suffix(')') else {
                return Err(format!("header '{subject}' has an unclosed scope"));
            };
            if scope.is_empty()
                || !scope
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
            {
                return Err(format!("header '{subject}' has an invalid scope '{scope}'"));
            }
            (kind, Some(scope.to_string()))
        }
        None => (prefix, None),
    };
    if !TYPES.contains(&kind) {
        return Err(format!(
            "header '{subject}' has the unknown type '{kind}' (allowed: {})",
            TYPES.join(", ")
        ));
    }
    if description.trim().is_empty() {
        return Err(format!("header '{subject}' has an empty subject"));
    }
    Ok(Header {
        kind: kind.to_string(),
        scope,
        bang,
        description: description.trim().to_string(),
    })
}

/// One reference of a `Refs:` trailer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reference {
    /// `PRD 0006 RF2` → plan "0006", requirement "RF2".
    Plan { prd: String, rf: String },
    /// `#12` or `issue #12`.
    Issue(u64),
}

/// The trailers of one message (RF1.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trailers {
    /// Every `Changelog:` sentence, in order (skip lines excluded).
    pub sentences: Vec<String>,
    /// The reason of `Changelog: skip <reason>`.
    pub skip: Option<String>,
    pub category: Option<Category>,
    pub refs: Vec<Reference>,
    /// The migration of `BREAKING CHANGE:`.
    pub breaking: Option<String>,
    pub changelog_edit: Option<String>,
    pub version_override: Option<String>,
    /// Every trailer as `(key, value)`, continuation lines joined.
    pub all: Vec<(String, String)>,
}

/// Keys this module interprets; a different capitalization is a mistake.
const KNOWN_KEYS: [&str; 7] = [
    "Changelog",
    "Changelog-Category",
    "Refs",
    "BREAKING CHANGE",
    "BREAKING-CHANGE",
    "Changelog-Edit",
    "Version-Override",
];

/// `Key: value` where the key is a Git trailer token (or the conventional
/// `BREAKING CHANGE`).
fn trailer_key(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let valid = key == "BREAKING CHANGE"
        || (!key.is_empty()
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && key.as_bytes()[0].is_ascii_alphabetic());
    if !valid {
        return None;
    }
    if !(value.is_empty() || value.starts_with(' ')) {
        return None;
    }
    Some((key, value.trim()))
}

/// The trailers of `body` (the full message or the body without the
/// subject) plus the problems found while reading them.
pub fn parse_trailers(body: &str) -> (Trailers, Vec<String>) {
    let text = body.replace("\r\n", "\n").replace('\r', "\n");
    let paragraphs: Vec<Vec<&str>> = text
        .split("\n\n")
        .map(|paragraph| {
            paragraph
                .lines()
                .map(|line| line.trim_end())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|lines| !lines.is_empty())
        .collect();
    // Trailers are the trailing paragraphs made only of `Key: value` lines
    // and their continuations (conventional footers may be split from the
    // Git trailers by a blank line).
    let mut lines: Vec<&str> = Vec::new();
    for paragraph in paragraphs.iter().rev() {
        let is_trailer_block = trailer_key(paragraph[0]).is_some()
            && paragraph.iter().all(|line| {
                trailer_key(line).is_some() || line.starts_with(' ') || line.starts_with('\t')
            });
        if !is_trailer_block {
            break;
        }
        let mut block = paragraph.clone();
        block.extend(lines);
        lines = block;
    }

    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = pairs.last_mut() {
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((key, value)) = trailer_key(line) {
            pairs.push((key.to_string(), value.to_string()));
        }
    }

    let mut trailers = Trailers::default();
    let mut problems = Vec::new();
    for (key, value) in &pairs {
        match key.as_str() {
            "Changelog" => {
                if let Some(rest) = value.strip_prefix("skip") {
                    if rest.is_empty() || rest.starts_with([' ', ':', '-', '(']) {
                        let reason = rest
                            .trim_start_matches([':', '-', ' '])
                            .trim_matches(['(', ')'])
                            .trim();
                        if reason.is_empty() {
                            problems.push(
                                "'Changelog: skip' needs a reason ('Changelog: skip <reason>')"
                                    .to_string(),
                            );
                        } else {
                            trailers.skip = Some(reason.to_string());
                        }
                        continue;
                    }
                }
                if value.is_empty() {
                    problems.push("'Changelog:' trailer without a sentence".to_string());
                } else {
                    trailers.sentences.push(value.clone());
                }
            }
            "Changelog-Category" => match Category::ALL
                .into_iter()
                .find(|category| category.title() == value)
            {
                Some(category) => trailers.category = Some(category),
                None => problems.push(format!(
                    "'Changelog-Category: {value}' is not one of Added, Changed, Deprecated, Removed, Fixed, Security"
                )),
            },
            "Refs" => parse_refs(value, &mut trailers.refs, &mut problems),
            "BREAKING CHANGE" | "BREAKING-CHANGE" => {
                if value.is_empty() {
                    problems.push(
                        "'BREAKING CHANGE:' needs the migration text ('BREAKING CHANGE: <migration>')"
                            .to_string(),
                    );
                } else {
                    trailers.breaking = Some(value.clone());
                }
            }
            "Changelog-Edit" => {
                if value.is_empty() {
                    problems.push("'Changelog-Edit:' needs a reason".to_string());
                } else {
                    trailers.changelog_edit = Some(value.clone());
                }
            }
            "Version-Override" => trailers.version_override = Some(value.clone()),
            other => {
                if let Some(known) = KNOWN_KEYS
                    .iter()
                    .find(|known| known.eq_ignore_ascii_case(other))
                {
                    problems.push(format!(
                        "trailer key '{other}' must be spelled '{known}' (keys are case-sensitive)"
                    ));
                }
            }
        }
    }
    if trailers.skip.is_some() && !trailers.sentences.is_empty() {
        problems.push("'Changelog: skip' and a 'Changelog:' sentence in the same commit".into());
    }
    trailers.all = pairs;
    (trailers, problems)
}

/// `PRD 0006 RF2, RF3; PRD 0009 RF1; #12; issue #13`. A bare requirement
/// continues the previous plan.
fn parse_refs(value: &str, out: &mut Vec<Reference>, problems: &mut Vec<String>) {
    let mut plan: Option<String> = None;
    for token in value.split([',', ';']) {
        let token = token.trim().trim_start_matches('-').trim();
        if token.is_empty() {
            continue;
        }
        let issue = token
            .strip_prefix("issue ")
            .unwrap_or(token)
            .trim()
            .strip_prefix('#');
        if let Some(number) = issue {
            match number.parse::<u64>() {
                Ok(n) if n > 0 => out.push(Reference::Issue(n)),
                _ => problems.push(format!("'Refs: {token}' is not an issue number")),
            }
            continue;
        }
        let (prd, rf) = match token.strip_prefix("PRD ") {
            Some(rest) => match rest.split_once(' ') {
                Some((number, rf)) => (number.trim().to_string(), rf.trim()),
                None => {
                    problems.push(format!(
                        "'Refs: {token}' names a plan without a requirement (expected 'PRD NNNN RFx')"
                    ));
                    continue;
                }
            },
            None => match &plan {
                Some(number) => (number.clone(), token),
                None => {
                    problems.push(format!(
                        "'Refs: {token}' is not a reference (expected 'PRD NNNN RFx' or '#12')"
                    ));
                    continue;
                }
            },
        };
        if prd.len() != 4 || !prd.bytes().all(|b| b.is_ascii_digit()) {
            problems.push(format!(
                "'Refs: {token}': plan number must have four digits"
            ));
            continue;
        }
        if !valid_requirement(rf) {
            problems.push(format!(
                "'Refs: {token}': '{rf}' is not a requirement id (RF2, RF7.4, FIX-01)"
            ));
            continue;
        }
        plan = Some(prd.clone());
        let reference = Reference::Plan {
            prd,
            rf: rf.to_string(),
        };
        if !out.contains(&reference) {
            out.push(reference);
        }
    }
}

/// `RF2`, `RF7.4`, `INT-04`, `FIX-01`: letters, an optional dash, then
/// dot-separated numbers.
fn valid_requirement(rf: &str) -> bool {
    let letters = rf.bytes().take_while(|b| b.is_ascii_uppercase()).count();
    if letters == 0 {
        return false;
    }
    let rest = rf[letters..].strip_prefix('-').unwrap_or(&rf[letters..]);
    !rest.is_empty()
        && rest
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// The bump a commit asks for (RF3.2); ordered so the release takes the
/// maximum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

impl Bump {
    pub fn as_str(self) -> &'static str {
        match self {
            Bump::None => "none",
            Bump::Patch => "patch",
            Bump::Minor => "minor",
            Bump::Major => "major",
        }
    }
}

/// RF3.3: the pure next-version function. While `major == 0` a major bump
/// is a minor one.
pub fn next_version(current: &Version, bump: Bump) -> Version {
    let Version {
        major,
        minor,
        patch,
    } = current.clone();
    match bump {
        Bump::None => current.clone(),
        Bump::Patch => Version {
            major,
            minor,
            patch: patch + 1,
        },
        Bump::Minor => Version {
            major,
            minor: minor + 1,
            patch: 0,
        },
        Bump::Major if major == 0 => Version {
            major,
            minor: minor + 1,
            patch: 0,
        },
        Bump::Major => Version {
            major: major + 1,
            minor: 0,
            patch: 0,
        },
    }
}

/// One commit of a range, parsed.
#[derive(Clone, Debug)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    /// The full message (`%B`: subject, body and trailers).
    pub message: String,
    pub header: Result<Header, String>,
    pub trailers: Trailers,
    /// Trailer problems (lowercase keys, missing reasons…).
    pub problems: Vec<String>,
}

impl Commit {
    pub fn parse(sha: &str, subject: &str, message: &str) -> Commit {
        let (trailers, problems) = parse_trailers(message);
        Commit {
            sha: sha.to_string(),
            subject: subject.to_string(),
            message: message.to_string(),
            header: parse_header(subject),
            trailers,
            problems,
        }
    }

    pub fn short(&self) -> &str {
        &self.sha[..7.min(self.sha.len())]
    }

    fn kind(&self) -> Option<&str> {
        self.header.as_ref().ok().map(|h| h.kind.as_str())
    }

    fn scope(&self) -> Option<&str> {
        self.header.as_ref().ok().and_then(|h| h.scope.as_deref())
    }

    /// `!` or a `BREAKING CHANGE:` footer (an empty footer still counts:
    /// the gate then asks for the migration).
    pub fn is_breaking(&self) -> bool {
        self.header.as_ref().is_ok_and(|h| h.bang)
            || self.trailers.breaking.is_some()
            || self
                .trailers
                .all
                .iter()
                .any(|(key, _)| key == "BREAKING CHANGE" || key == "BREAKING-CHANGE")
    }

    /// `fix(deps)`, `fix(security)`, `chore(security)`.
    fn is_security(&self) -> bool {
        matches!(
            (self.kind(), self.scope()),
            (Some("fix"), Some("deps" | "security")) | (Some("chore"), Some("security"))
        )
    }

    /// RF2.1 rule 2: the commits that must say what changed for the user.
    pub fn is_user_visible(&self) -> bool {
        matches!(self.kind(), Some("feat" | "fix" | "revert"))
            || self.is_security()
            || self.is_breaking()
            || !self.trailers.sentences.is_empty()
    }

    /// `chore(release): X.Y.Z` or `chore(release): X.Y.Z (refresh)`.
    pub fn release_version(&self) -> Option<Version> {
        release_version(&self.subject)
    }

    /// RF3.2: the bump of one commit.
    pub fn bump(&self, current: &Version) -> Bump {
        if self.release_version().is_some() {
            return Bump::None;
        }
        if self.is_breaking() {
            return if current.major == 0 {
                Bump::Minor
            } else {
                Bump::Major
            };
        }
        if self.trailers.skip.is_some() {
            return Bump::None;
        }
        match self.kind() {
            Some("feat") => Bump::Minor,
            Some("fix" | "revert") => Bump::Patch,
            _ if self.is_security() => Bump::Patch,
            _ if !self.trailers.sentences.is_empty() => Bump::Patch,
            _ => Bump::None,
        }
    }

    /// RF3.2: the default category, overridden by `Changelog-Category:`.
    pub fn category(&self) -> Category {
        if let Some(category) = self.trailers.category {
            return category;
        }
        if self.is_security() {
            return Category::Security;
        }
        match self.kind() {
            Some("feat") => Category::Added,
            Some("fix" | "revert") => Category::Fixed,
            _ => Category::Changed,
        }
    }
}

/// The version of a release-commit subject.
pub fn release_version(subject: &str) -> Option<Version> {
    let rest = subject.trim().strip_prefix("chore(release): ")?;
    let rest = rest.strip_suffix(" (refresh)").unwrap_or(rest);
    Version::parse(rest.trim()).ok()
}

/// The GitHub markers that skip workflow runs when they appear in a commit
/// message (lowercase; matched case-insensitively).
const SKIP_MARKERS: [&str; 5] = [
    "[skip ci]",
    "[ci skip]",
    "[no ci]",
    "[skip actions]",
    "[actions skip]",
];

/// Whether `text` carries a skip-CI marker (or the `skip-checks` trailer).
pub fn has_skip_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    SKIP_MARKERS.iter().any(|marker| lower.contains(marker))
        || lower
            .lines()
            .any(|line| line.trim_start().starts_with("skip-checks:"))
}

/// Rewrites every skip-CI marker so it no longer matches (`[skip ci]` →
/// `(skip ci)`), for generated messages (NFR5).
pub fn neutralize_skip_markers(text: &str) -> String {
    let mut out = text.to_string();
    for marker in SKIP_MARKERS {
        loop {
            let lower = out.to_ascii_lowercase();
            let Some(index) = lower.find(marker) else {
                break;
            };
            let inner = out[index + 1..index + marker.len() - 1].to_string();
            out.replace_range(index..index + marker.len(), &format!("({inner})"));
        }
    }
    out.lines()
        .map(|line| {
            if line
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("skip-checks:")
            {
                line.replacen(':', " =", 1)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// RF2.1 rule 7: the light lint of one `Changelog:` sentence.
pub fn lint_sentence(sentence: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if !sentence.chars().next().is_some_and(|c| c.is_uppercase()) {
        problems.push(format!(
            "sentence '{sentence}' must start with a capital letter"
        ));
    }
    if !sentence.ends_with('.') {
        problems.push(format!("sentence '{sentence}' must end with a period"));
    }
    if sentence.contains("PRD ") {
        problems.push(format!(
            "sentence '{sentence}' cites a PRD: references go in the 'Refs:' trailer"
        ));
    }
    if sentence
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(looks_like_hash)
    {
        problems.push(format!(
            "sentence '{sentence}' cites a commit hash: the release adds the hash"
        ));
    }
    if sentence.chars().count() > 600 {
        problems.push(format!(
            "sentence starting '{}…' is longer than 600 characters",
            sentence.chars().take(40).collect::<String>()
        ));
    }
    if has_skip_marker(sentence) {
        problems.push("sentence carries a skip-CI marker".to_string());
    }
    problems
}

/// 7–40 lowercase hex characters with at least one digit and one letter.
pub fn looks_like_hash(token: &str) -> bool {
    (7..=40).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && token.bytes().any(|b| b.is_ascii_digit())
        && token.bytes().any(|b| b.is_ascii_alphabetic())
}

/// The non-merge commits of `range` (`a..b`), oldest first.
pub fn load(repo: &Path, range: &str) -> Res<Vec<Commit>> {
    let log = git::run(
        repo,
        &[
            "log",
            "--no-merges",
            "--reverse",
            "--format=%H%x1f%s%x1f%B%x1e",
            range,
        ],
    )?;
    let mut out = Vec::new();
    for record in log.split('\u{1e}') {
        let record = record.trim_start_matches(['\n', '\r']);
        if record.trim().is_empty() {
            continue;
        }
        let mut fields = record.split('\u{1f}');
        let sha = fields.next().unwrap_or_default().trim();
        let subject = fields.next().unwrap_or_default();
        let message = fields.next().unwrap_or_default();
        if sha.is_empty() {
            continue;
        }
        out.push(Commit::parse(sha, subject, message));
    }
    Ok(out)
}

/// The files a commit changes (against its first parent; every file for a
/// root commit).
pub fn files(repo: &Path, sha: &str) -> Res<Vec<String>> {
    let out = git::run(
        repo,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--root",
            "--no-renames",
            sha,
        ],
    )?;
    Ok(out.lines().map(str::to_string).collect())
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
    fn headers() {
        let h = parse_header("feat(cli)!: add x").unwrap();
        assert_eq!(h.kind, "feat");
        assert_eq!(h.scope.as_deref(), Some("cli"));
        assert!(h.bang);
        assert!(parse_header("revert: undo").is_ok());
        assert!(parse_header("chore(release-flow): a").is_ok());
        for bad in [
            "feature: x",
            "feat x",
            "feat(: x",
            "feat(): x",
            "Feat: x",
            "feat:  ",
            "Revert \"feat: x\"",
            "ci: x",
        ] {
            assert!(parse_header(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn single_and_several_sentences() {
        let c = commit(
            "feat: a",
            "Body.\n\nChangelog: You can do A.\nChangelog: You can do B.\nRefs: PRD 0006 RF2",
        );
        assert_eq!(c.trailers.sentences, ["You can do A.", "You can do B."]);
        assert!(c.problems.is_empty(), "{:?}", c.problems);
    }

    #[test]
    fn continuation_lines_crlf_and_trailing_whitespace() {
        let c = commit(
            "fix: a",
            "Body.\r\n\r\nChangelog: A long sentence that\r\n continues here.   \r\nRefs: PRD 0001 RF1  \r\n",
        );
        assert_eq!(
            c.trailers.sentences,
            ["A long sentence that continues here."]
        );
        assert_eq!(
            c.trailers.refs,
            [Reference::Plan {
                prd: "0001".into(),
                rf: "RF1".into()
            }]
        );
    }

    #[test]
    fn footers_split_from_git_trailers_are_read() {
        let c = commit(
            "feat!: a",
            "Body.\n\nBREAKING CHANGE: rename x to y.\n\nChangelog: Something.\nCo-Authored-By: A <a@example.com>",
        );
        assert_eq!(c.trailers.breaking.as_deref(), Some("rename x to y."));
        assert_eq!(c.trailers.sentences, ["Something."]);
    }

    #[test]
    fn body_paragraphs_are_not_trailers() {
        let c = commit(
            "feat: a",
            "Changelog: not a trailer\nbecause this line is prose.",
        );
        assert!(c.trailers.sentences.is_empty());
    }

    #[test]
    fn lowercase_key_is_rejected() {
        let c = commit("feat: a", "changelog: Something.");
        assert!(c.trailers.sentences.is_empty());
        assert!(c.problems[0].contains("case-sensitive"), "{:?}", c.problems);
    }

    #[test]
    fn skip_needs_a_reason() {
        assert_eq!(
            commit("feat: a", "Changelog: skip internal only")
                .trailers
                .skip
                .as_deref(),
            Some("internal only")
        );
        assert_eq!(
            commit("chore: a", "Changelog: skip (release commit)")
                .trailers
                .skip
                .as_deref(),
            Some("release commit")
        );
        let bare = commit("feat: a", "Changelog: skip");
        assert!(bare.trailers.skip.is_none());
        assert!(bare.problems[0].contains("needs a reason"));
        // "Skipped" is a sentence, not the skip keyword.
        let sentence = commit("feat: a", "Changelog: skipping frames is gone.");
        assert_eq!(sentence.trailers.sentences.len(), 1);
    }

    #[test]
    fn breaking_without_text_is_rejected_and_bang_counts() {
        let empty = commit("feat: a", "BREAKING CHANGE:");
        assert!(empty.is_breaking());
        assert!(empty.problems[0].contains("migration"));
        let bang = commit("feat!: a", "Changelog: Something.");
        assert!(bang.is_breaking());
        assert!(bang.trailers.breaking.is_none());
    }

    #[test]
    fn mixed_refs() {
        let c = commit(
            "feat: a",
            "Refs: PRD 0018 RF1.2, RF1.3; PRD 0021 RF2\nRefs: #12, issue #13\nRefs: PRD 0013 FIX-01",
        );
        assert_eq!(
            c.trailers.refs,
            [
                Reference::Plan {
                    prd: "0018".into(),
                    rf: "RF1.2".into()
                },
                Reference::Plan {
                    prd: "0018".into(),
                    rf: "RF1.3".into()
                },
                Reference::Plan {
                    prd: "0021".into(),
                    rf: "RF2".into()
                },
                Reference::Issue(12),
                Reference::Issue(13),
                Reference::Plan {
                    prd: "0013".into(),
                    rf: "FIX-01".into()
                },
            ]
        );
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        let bad = commit("feat: a", "Refs: something");
        assert!(!bad.problems.is_empty());
    }

    #[test]
    fn category_override_and_defaults() {
        assert_eq!(
            commit("feat: a", "Changelog: X.").category(),
            Category::Added
        );
        assert_eq!(
            commit("feat: a", "Changelog: X.\nChangelog-Category: Changed").category(),
            Category::Changed
        );
        assert_eq!(
            commit("fix(deps): a", "Changelog: X.").category(),
            Category::Security
        );
        assert_eq!(
            commit("chore(security): a", "").category(),
            Category::Security
        );
        assert_eq!(
            commit("revert: a", "Changelog: X.").category(),
            Category::Fixed
        );
        assert_eq!(
            commit("docs: a", "Changelog: X.").category(),
            Category::Changed
        );
        let bad = commit("feat: a", "Changelog-Category: Features");
        assert!(bad.problems[0].contains("Changelog-Category"));
    }

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn bump_table() {
        let zero = v("0.77.0");
        let one = v("1.2.3");
        let rows: [(&str, &str, Bump, Bump); 13] = [
            ("feat!: a", "", Bump::Minor, Bump::Major),
            ("fix: a", "BREAKING CHANGE: x.", Bump::Minor, Bump::Major),
            ("feat: a", "", Bump::Minor, Bump::Minor),
            ("fix: a", "", Bump::Patch, Bump::Patch),
            ("revert: a", "", Bump::Patch, Bump::Patch),
            ("fix(deps): a", "", Bump::Patch, Bump::Patch),
            ("fix(security): a", "", Bump::Patch, Bump::Patch),
            ("chore(security): a", "", Bump::Patch, Bump::Patch),
            (
                "chore(license): a",
                "Changelog: X.",
                Bump::Patch,
                Bump::Patch,
            ),
            ("docs: a", "", Bump::None, Bump::None),
            (
                "feat: a",
                "Changelog: skip docs only",
                Bump::None,
                Bump::None,
            ),
            ("chore(release): 0.78.0", "", Bump::None, Bump::None),
            (
                "feat: a",
                "Changelog: X.\nChangelog-Category: Removed",
                Bump::Minor,
                Bump::Minor,
            ),
        ];
        for (subject, body, at_zero, at_one) in rows {
            let c = commit(subject, body);
            assert_eq!(c.bump(&zero), at_zero, "{subject} at 0.x");
            assert_eq!(c.bump(&one), at_one, "{subject} at 1.x");
        }
        let max = [
            commit("fix: a", ""),
            commit("feat: b", ""),
            commit("docs: c", ""),
        ]
        .iter()
        .map(|c| c.bump(&zero))
        .max();
        assert_eq!(max, Some(Bump::Minor));
    }

    #[test]
    fn next_versions() {
        assert_eq!(next_version(&v("0.9.3"), Bump::Minor), v("0.10.0"));
        assert_eq!(next_version(&v("0.77.0"), Bump::Patch), v("0.77.1"));
        assert_eq!(next_version(&v("0.77.4"), Bump::Major), v("0.78.0"));
        assert_eq!(next_version(&v("1.4.2"), Bump::Major), v("2.0.0"));
        assert_eq!(next_version(&v("1.4.2"), Bump::Minor), v("1.5.0"));
        assert_eq!(next_version(&v("1.4.2"), Bump::None), v("1.4.2"));
    }

    #[test]
    fn release_subjects() {
        assert_eq!(release_version("chore(release): 0.80.0"), Some(v("0.80.0")));
        assert_eq!(
            release_version("chore(release): 0.80.0 (refresh)"),
            Some(v("0.80.0"))
        );
        assert_eq!(release_version("chore(release): next"), None);
        assert_eq!(release_version("chore(release-flow): 0.80.0"), None);
    }

    #[test]
    fn user_visibility() {
        assert!(commit("feat: a", "").is_user_visible());
        assert!(commit("chore(security): a", "").is_user_visible());
        assert!(commit("docs: a", "Changelog: X.").is_user_visible());
        assert!(commit("refactor!: a", "").is_user_visible());
        assert!(!commit("docs: a", "").is_user_visible());
        assert!(!commit("chore(deps): a", "").is_user_visible());
    }

    #[test]
    fn skip_markers_are_found_and_neutralized() {
        let marker = format!("[{} {}]", "SKIP", "ci");
        assert!(has_skip_marker(&format!("text {marker} more")));
        assert!(has_skip_marker("a\nskip-checks: true"));
        assert!(!has_skip_marker("the skip marker"));
        let fixed = neutralize_skip_markers(&format!("a {marker} b\nskip-checks: true"));
        assert!(!has_skip_marker(&fixed), "{fixed}");
        assert!(fixed.contains("(SKIP ci)"));
    }

    #[test]
    fn sentence_lint() {
        assert!(lint_sentence("The window opens faster.").is_empty());
        assert_eq!(lint_sentence("lowercase start.").len(), 1);
        assert_eq!(lint_sentence("No period").len(), 1);
        assert_eq!(lint_sentence("Cites PRD 0001 RF1.").len(), 1);
        assert_eq!(lint_sentence("Cites abc1234 here.").len(), 1);
        assert_eq!(lint_sentence(&format!("A{}.", "a".repeat(600))).len(), 1);
        // Plain words made of hex letters are not hashes.
        assert!(lint_sentence("Faded decaf beef added.").is_empty());
    }
}
