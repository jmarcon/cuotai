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
//!   a sentence may start with its own category, `Changelog: [Security] …`;
//! * `Changelog-Category: Added|Changed|Deprecated|Removed|Fixed|Security`
//!   (for a commit with one sentence; the gate refuses it with several);
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

/// One `Changelog:` sentence and the category of its optional
/// `[Category]` prefix (F-04), which is stripped from `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub category: Option<Category>,
}

impl Entry {
    pub fn new(text: impl Into<String>, category: Option<Category>) -> Entry {
        Entry {
            text: text.into(),
            category,
        }
    }
}

impl PartialEq<&str> for Entry {
    fn eq(&self, other: &&str) -> bool {
        self.category.is_none() && self.text == *other
    }
}

/// The six category names, for messages.
pub fn category_names() -> String {
    Category::ALL
        .iter()
        .map(|c| c.title())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The trailers of one message (RF1.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trailers {
    /// Every `Changelog:` sentence, in order (skip lines excluded), with its
    /// per-entry category.
    pub sentences: Vec<Entry>,
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

/// A problem found while reading the trailers, with the trailer key it is
/// about (the commit gate files it under that key's rule).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrailerProblem {
    /// `Changelog`, `Changelog-Category`, `Refs`, `BREAKING CHANGE`,
    /// `Changelog-Edit` or `Version-Override`.
    pub key: &'static str,
    pub text: String,
}

impl TrailerProblem {
    fn new(key: &'static str, text: impl Into<String>) -> TrailerProblem {
        TrailerProblem {
            key,
            text: text.into(),
        }
    }
}

impl std::fmt::Display for TrailerProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
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
pub fn parse_trailers(body: &str) -> (Trailers, Vec<TrailerProblem>) {
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
    let mut problems: Vec<TrailerProblem> = Vec::new();
    let mut refs_problems: Vec<String> = Vec::new();
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
                            problems.push(TrailerProblem::new(
                                "Changelog",
                                "'Changelog: skip' needs a reason ('Changelog: skip <reason>')",
                            ));
                        } else {
                            trailers.skip = Some(reason.to_string());
                        }
                        continue;
                    }
                }
                if value.is_empty() {
                    problems.push(TrailerProblem::new(
                        "Changelog",
                        "'Changelog:' trailer without a sentence",
                    ));
                    continue;
                }
                let prefix = value
                    .strip_prefix('[')
                    .and_then(|rest| rest.split_once(']'));
                match prefix {
                    Some((name, rest)) => {
                        let name = name.trim();
                        let text = rest.trim();
                        match Category::ALL.into_iter().find(|c| c.title() == name) {
                            Some(category) if !text.is_empty() => {
                                trailers.sentences.push(Entry::new(text, Some(category)))
                            }
                            Some(_) => problems.push(TrailerProblem::new(
                                "Changelog",
                                format!("'Changelog: [{name}]' has no sentence after the category"),
                            )),
                            None => problems.push(TrailerProblem::new(
                                "Changelog-Category",
                                format!(
                                    "'Changelog: [{name}] …' names an unknown category: use one of {}",
                                    category_names()
                                ),
                            )),
                        }
                    }
                    None => trailers.sentences.push(Entry::new(value.clone(), None)),
                }
            }
            "Changelog-Category" => match Category::ALL
                .into_iter()
                .find(|category| category.title() == value)
            {
                Some(category) => trailers.category = Some(category),
                None => problems.push(TrailerProblem::new(
                    "Changelog-Category",
                    format!(
                        "'Changelog-Category: {value}' is not one of {}",
                        category_names()
                    ),
                )),
            },
            "Refs" => parse_refs(value, &mut trailers.refs, &mut refs_problems),
            "BREAKING CHANGE" | "BREAKING-CHANGE" => {
                if value.is_empty() {
                    problems.push(TrailerProblem::new(
                        "BREAKING CHANGE",
                        "'BREAKING CHANGE:' needs the migration text ('BREAKING CHANGE: <migration>')",
                    ));
                } else {
                    trailers.breaking = Some(value.clone());
                }
            }
            "Changelog-Edit" => {
                if value.is_empty() {
                    problems.push(TrailerProblem::new(
                        "Changelog-Edit",
                        "'Changelog-Edit:' needs a reason",
                    ));
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
                    let key = match *known {
                        "BREAKING-CHANGE" => "BREAKING CHANGE",
                        known => known,
                    };
                    problems.push(TrailerProblem::new(
                        key,
                        format!(
                            "trailer key '{other}' must be spelled '{known}' (keys are case-sensitive)"
                        ),
                    ));
                }
            }
        }
    }
    if trailers.skip.is_some() && !trailers.sentences.is_empty() {
        problems.push(TrailerProblem::new(
            "Changelog",
            "'Changelog: skip' and a 'Changelog:' sentence in the same commit",
        ));
    }
    problems.extend(
        refs_problems
            .into_iter()
            .map(|text| TrailerProblem::new("Refs", text)),
    );
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
    pub problems: Vec<TrailerProblem>,
    /// The author and committer addresses ([`load`] fills them; empty for
    /// a message parsed on its own).
    pub author_email: String,
    pub committer_email: String,
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
            author_email: String::new(),
            committer_email: String::new(),
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

    /// The category of one of this commit's entries: its `[Category]`
    /// prefix, or the commit's category.
    pub fn entry_category(&self, entry: &Entry) -> Category {
        entry.category.unwrap_or_else(|| self.category())
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

/// RF2.1 rule 7: the light lint of one `Changelog:` sentence, every check
/// included.
pub fn lint_sentence(sentence: &str) -> Vec<String> {
    lint_sentence_with(sentence, &|_| true)
}

/// [`lint_sentence`] where `applies(id)` says whether a check that was added
/// after the cutover applies to this commit (the commit gate scopes them
/// with `RULES_SINCE`). The ids are `ui-path` and `code-format`.
pub fn lint_sentence_with(sentence: &str, applies: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut problems = Vec::new();
    // Every entry is shown on GitHub (Markdown), in the TUI and in the
    // desktop dialog: anything that renders differently in one of them is
    // refused. Code spans render literally everywhere, so the markup checks
    // read the text outside them.
    let invisible: Vec<String> = sentence
        .chars()
        .filter(|c| is_invisible(*c))
        .map(|c| format!("U+{:04X}", c as u32))
        .collect();
    if !invisible.is_empty() {
        problems.push(format!(
            "sentence '{}' contains invisible, control or direction characters ({}): \
             write plain text",
            sentence.escape_debug(),
            invisible.join(", ")
        ));
    }
    let prose = outside_code_spans(sentence);
    if has_markup_tag(&prose) {
        problems.push(format!(
            "sentence '{sentence}' contains an HTML tag or comment: entries are plain text \
             (a lone '<' is fine; wrap code in backticks)"
        ));
    }
    if prose.contains("](") || prose.contains("![") {
        problems.push(format!(
            "sentence '{sentence}' contains a Markdown link or image: entries are plain text"
        ));
    }
    let lower = sentence.to_ascii_lowercase();
    if ["http://", "https://", "www."]
        .iter()
        .any(|link| lower.contains(link))
    {
        problems.push(format!(
            "sentence '{sentence}' contains a link: name the place in words (the docs, \
             Settings › About) instead"
        ));
    }
    if has_second_sentence(&prose) {
        problems.push(format!(
            "sentence '{sentence}' holds two sentences: write one 'Changelog:' line per change"
        ));
    }
    if applies("code-format") {
        let bare = bare_code_tokens(&prose);
        if !bare.is_empty() {
            problems.push(format!(
                "sentence '{sentence}' names {} outside backticks: write flags, paths and field \
                 names as code (`--watch`, `cuotai.toml`, `remaining_ratio`)",
                bare.iter()
                    .map(|t| format!("'{t}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
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
    // A skip marker is rule 6 of the gate, which reads the whole message.
    if applies("ui-path") && has_ascii_ui_path(sentence) {
        problems.push(format!(
            "sentence '{sentence}' writes a UI path with '>' or '→': use › (Settings › About)"
        ));
    }
    problems
}

/// Unicode `Cc` and `Cf` characters: C0/C1 controls, zero-width characters,
/// bidirectional embeddings, overrides and isolates, and the other format
/// characters (soft hyphen, word joiner, BOM, tag characters…).
fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c as u32,
            0x00AD
                | 0x0600..=0x0605
                | 0x061C
                | 0x06DD
                | 0x070F
                | 0x0890..=0x0891
                | 0x08E2
                | 0x180E
                | 0x200B..=0x200F
                | 0x202A..=0x202E
                | 0x2060..=0x2064
                | 0x2066..=0x206F
                | 0xFEFF
                | 0xFFF9..=0xFFFB
                | 0x110BD
                | 0x110CD
                | 0x13430..=0x1343F
                | 0x1BCA0..=0x1BCA3
                | 0x1D173..=0x1D17A
                | 0xE0001
                | 0xE0020..=0xE007F
        )
}

/// `sentence` with every backtick code span replaced by a space.
fn outside_code_spans(sentence: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in sentence.chars() {
        if c == '`' {
            inside = !inside;
            out.push(' ');
        } else if !inside {
            out.push(c);
        }
    }
    out
}

/// `<b>`, `</p>`, `<!-- … -->`: a `<` followed by a letter, `/` or `!`.
fn has_markup_tag(text: &str) -> bool {
    text.match_indices('<').any(|(at, _)| {
        text[at + 1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!')
    })
}

/// `[.!?]`, spaces, then a capital letter before the end of the sentence.
fn has_second_sentence(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|i| {
        if !matches!(chars[i], '.' | '!' | '?') {
            return false;
        }
        let spaces = chars[i + 1..].iter().take_while(|c| **c == ' ').count();
        spaces > 0 && chars.get(i + 1 + spaces).is_some_and(|c| c.is_uppercase())
    })
}

/// Tokens of plain prose that are code by their shape: a flag (`--watch`),
/// a path (`/v1/limits`, `~/.config`, `./cuotai.toml`), a file with a
/// configuration or source extension (`cuotai.toml`), or a snake_case name
/// (`remaining_ratio`). Kept narrow so ordinary words never match.
fn bare_code_tokens(prose: &str) -> Vec<String> {
    const EXTENSIONS: [&str; 10] = [
        ".toml", ".json", ".yml", ".yaml", ".rs", ".sh", ".md", ".sqlite3", ".env", ".plist",
    ];
    let mut out = Vec::new();
    for raw in prose.split_whitespace() {
        let token = raw
            .trim_start_matches(['(', '"', '\'', '[', '{'])
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'', ']', '}']);
        let letter_after = |prefix: &str| {
            token
                .strip_prefix(prefix)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_alphabetic())
        };
        let flag = letter_after("--");
        let path = letter_after("/") || letter_after("~/") || letter_after("./");
        let lower = token.to_ascii_lowercase();
        let file = EXTENSIONS
            .iter()
            .any(|ext| lower.len() > ext.len() && lower.ends_with(ext));
        let snake = token.contains('_')
            && token.split('_').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
        if (flag || path || file || snake) && !out.iter().any(|t: &String| t == token) {
            out.push(token.to_string());
        }
    }
    out
}

/// `Settings > About` or `Settings → About`: a word, the separator, then a
/// capitalised word. UI paths use "›" (PRD 0021 re-validation V-06c); a
/// comparison such as "5 > 3" is not a path.
fn has_ascii_ui_path(sentence: &str) -> bool {
    [" > ", " → "].iter().any(|separator| {
        sentence.match_indices(separator).any(|(at, _)| {
            let before = sentence[..at].chars().next_back();
            let after = sentence[at + separator.len()..].chars().next();
            before.is_some_and(char::is_alphabetic) && after.is_some_and(char::is_uppercase)
        })
    })
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
            "--format=%H%x1f%ae%x1f%ce%x1f%s%x1f%B%x1e",
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
        let author = fields.next().unwrap_or_default();
        let committer = fields.next().unwrap_or_default();
        let subject = fields.next().unwrap_or_default();
        let message = fields.next().unwrap_or_default();
        if sha.is_empty() {
            continue;
        }
        let mut commit = Commit::parse(sha, subject, message);
        commit.author_email = author.to_string();
        commit.committer_email = committer.to_string();
        out.push(commit);
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
        assert!(
            c.problems[0].text.contains("case-sensitive"),
            "{:?}",
            c.problems
        );
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
        assert!(bare.problems[0].text.contains("needs a reason"));
        // "Skipped" is a sentence, not the skip keyword.
        let sentence = commit("feat: a", "Changelog: skipping frames is gone.");
        assert_eq!(sentence.trailers.sentences.len(), 1);
    }

    #[test]
    fn breaking_without_text_is_rejected_and_bang_counts() {
        let empty = commit("feat: a", "BREAKING CHANGE:");
        assert!(empty.is_breaking());
        assert!(empty.problems[0].text.contains("migration"));
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
    fn per_entry_categories() {
        let c = commit(
            "fix: a",
            "Changelog: [Security] A token no longer leaks.\nChangelog: The button works.",
        );
        assert_eq!(
            c.trailers.sentences,
            [
                Entry::new("A token no longer leaks.", Some(Category::Security)),
                Entry::new("The button works.", None),
            ]
        );
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(
            c.entry_category(&c.trailers.sentences[0]),
            Category::Security
        );
        assert_eq!(c.entry_category(&c.trailers.sentences[1]), Category::Fixed);
        let unknown = commit("fix: a", "Changelog: [Features] A thing.");
        assert!(unknown.trailers.sentences.is_empty());
        assert_eq!(unknown.problems[0].key, "Changelog-Category");
        assert!(
            unknown.problems[0]
                .text
                .contains("Added, Changed, Fixed, Security, Deprecated, Removed"),
            "{:?}",
            unknown.problems
        );
        let empty = commit("fix: a", "Changelog: [Added]");
        assert!(empty.problems[0].text.contains("no sentence"));
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
        assert!(bad.problems[0].text.contains("Changelog-Category"));
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
        assert_eq!(lint_sentence("Open Settings > About to see it.").len(), 1);
        assert_eq!(lint_sentence("Open Settings → Integration now.").len(), 1);
        assert!(lint_sentence("Open Settings › About to see it.").is_empty());
        assert!(lint_sentence("Alerts fire when used > 90 percent.").is_empty());
    }

    fn one_problem(sentence: &str, needle: &str) {
        let found = lint_sentence(sentence);
        assert_eq!(found.len(), 1, "{sentence}: {found:?}");
        assert!(found[0].contains(needle), "{sentence}: {found:?}");
    }

    #[test]
    fn invisible_and_direction_characters_are_refused() {
        for c in [
            '\u{200B}',
            '\u{202E}',
            '\u{2066}',
            '\u{FEFF}',
            '\u{1B}',
            '\u{00AD}',
            '\u{E0041}',
        ] {
            one_problem(&format!("A{c}b works."), "invisible");
        }
        let found = lint_sentence("A\u{202E}b works.");
        assert!(found[0].contains("U+202E"), "{found:?}");
        assert!(
            found[0].contains("\\u{202e}"),
            "escaped, not raw: {found:?}"
        );
        assert!(lint_sentence("Ünïcode and emoji 🎉 stay fine.").is_empty());
    }

    #[test]
    fn markup_links_and_two_sentences_are_refused() {
        one_problem("A <img src=x> shows.", "HTML tag");
        one_problem("A</b> shows.", "HTML tag");
        one_problem("A<!-- hidden --> shows.", "HTML tag");
        one_problem("See [the docs](docs) now.", "Markdown link");
        one_problem("A ![logo] shows.", "Markdown link or image");
        one_problem("See https://example.com now.", "link");
        one_problem("See www.example.com now.", "link");
        one_problem("Two sentences. In one trailer.", "two sentences");
        one_problem("Really? Yes it works.", "two sentences");
        // A lone '<', a comparison or code spans are fine.
        assert!(lint_sentence("Channel labels with \"&\" or \"<\" show as typed.").is_empty());
        assert!(lint_sentence("Alerts fire below < 5 percent.").is_empty());
        assert!(lint_sentence("The `--account <name>` option works.").is_empty());
        assert!(lint_sentence("Version 1.2 is faster, e.g. on start.").is_empty());
    }

    #[test]
    fn code_must_be_in_backticks_where_the_check_applies() {
        one_problem("The --watch option refreshes.", "'--watch'");
        one_problem("The /v1/limits endpoint reports more.", "'/v1/limits'");
        one_problem("Edit cuotai.toml by hand.", "'cuotai.toml'");
        one_problem("The remaining_ratio field is new.", "'remaining_ratio'");
        assert!(lint_sentence("The `--watch` option and `remaining_ratio` work.").is_empty());
        // Ordinary prose is not code.
        for fine in [
            "Read-only mode and sign-in work -- even offline.",
            "Usage and/or cost show in 1/2 the time.",
            "Node.js users and the README page are unaffected.",
        ] {
            assert!(
                lint_sentence(fine).is_empty(),
                "{fine}: {:?}",
                lint_sentence(fine)
            );
        }
        // Older commits are exempt.
        assert!(
            lint_sentence_with("The --watch option refreshes.", &|id| id != "code-format")
                .is_empty()
        );
    }
}
