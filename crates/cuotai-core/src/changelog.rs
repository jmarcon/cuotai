//! PRD 0019 RF7.5/RF7.7: the changelog as data. The root `CHANGELOG.md`
//! (Keep a Changelog, English, semantic versions) is embedded with
//! `include_str!` and parsed once, so the interfaces never parse Markdown
//! and the What's new screen works without any network.
//!
//! The parser is intentionally strict: it is also the RF7.4 CI format
//! check (shared with `xtask` through a `#[path]` include), so it must stay
//! free of dependencies on the rest of the core. Every rule it enforces is
//! one the file already follows: a `## [Unreleased]` section on top, one
//! `## [X.Y.Z] - YYYY-MM-DD` section per version in strictly descending
//! order, only the six fixed categories, no empty category, and entries as
//! `- ` bullet lines (a released section may instead carry a plain
//! paragraph, e.g. "No user-visible changes").
//!
//! The internal builds that were never published (0.1.0 to 0.79.0, PRD
//! 0039 D11) are summarized by one optional, last section, the
//! [`PreRelease`] history: `## Pre-release history (0.1.0–0.79.0, …)`.
//! It is not a version: it has no date and no footer link, and every
//! released section above it must be newer than its range.

use std::fmt;
use std::sync::LazyLock;

/// The embedded changelog, parsed once. Panics if `CHANGELOG.md` does not
/// obey the format; the `embedded_changelog_parses` test makes that
/// impossible to merge.
pub fn entries() -> &'static Changelog {
    static CHANGELOG: LazyLock<Changelog> = LazyLock::new(|| {
        parse(include_str!("../../../CHANGELOG.md"))
            .expect("CHANGELOG.md must satisfy cuotai_core::changelog::parse; fix the format")
    });
    &CHANGELOG
}

/// One parsed changelog: the `[Unreleased]` section, every released
/// version in the file's (descending) order and, last, the optional
/// pre-release history of the internal builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changelog {
    pub unreleased: Section,
    pub releases: Vec<Release>,
    pub pre_release: Option<PreRelease>,
}

impl Changelog {
    /// The released section of `version` ("0.68.2"), if present.
    pub fn find(&self, version: &str) -> Option<&Release> {
        self.releases.iter().find(|r| r.version == version)
    }

    /// The pre-release history when it covers `version` (an internal
    /// build summarized there instead of having its own section).
    pub fn pre_release_of(&self, version: &str) -> Option<&PreRelease> {
        self.pre_release
            .as_ref()
            .filter(|history| history.covers(version))
    }

    /// Whether `version` is recorded at all: its own section, or the
    /// pre-release history's range.
    pub fn knows(&self, version: &str) -> bool {
        self.find(version).is_some() || self.pre_release_of(version).is_some()
    }

    /// Every `PRD NNNN RFx` reference cited by any entry, in file order.
    pub fn references(&self) -> Vec<&PlanRef> {
        let mut refs = Vec::new();
        for section in std::iter::once(&self.unreleased)
            .chain(self.releases.iter().map(|release| &release.section))
            .chain(self.pre_release.iter().map(|history| &history.section))
        {
            for block in &section.categories {
                for entry in &block.entries {
                    refs.extend(entry.refs.iter());
                }
            }
        }
        refs
    }
}

/// One released version's section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `X.Y.Z`, without the brackets.
    pub version: String,
    /// `YYYY-MM-DD` (UTC).
    pub date: String,
    /// The section's raw Markdown body (between the heading and the next
    /// section or the footer links), trailing blanks and link definitions
    /// removed. This is the release-notes source of PRD 0021 RF4.2.
    pub body: String,
    pub section: Section,
}

/// The pre-release history (owner decision of 2026-10-23, PRD 0039 D11):
/// one section that summarizes the internal builds `first`..=`last`, which
/// were never tagged or published, instead of one section per version.
/// Heading: `## Pre-release history (0.1.0–0.79.0, <free note>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreRelease {
    /// The heading text without `## ` (shown as the section title).
    pub title: String,
    /// The oldest internal version of the range ("0.1.0").
    pub first: String,
    /// The newest internal version of the range ("0.79.0").
    pub last: String,
    /// The raw Markdown body, like [`Release::body`].
    pub body: String,
    pub section: Section,
}

impl PreRelease {
    /// Whether `version` (`X.Y.Z`) lies in `first..=last`.
    pub fn covers(&self, version: &str) -> bool {
        valid_version(version)
            && compare_versions(version, &self.first) != std::cmp::Ordering::Less
            && compare_versions(version, &self.last) != std::cmp::Ordering::Greater
    }

    /// The range as written in the heading ("0.1.0–0.79.0").
    pub fn range(&self) -> String {
        format!("{}{PRE_RELEASE_DASH}{}", self.first, self.last)
    }
}

/// The heading prefix of the pre-release history.
pub const PRE_RELEASE_PREFIX: &str = "Pre-release history (";
/// The range separator of the pre-release heading (an en dash).
pub const PRE_RELEASE_DASH: char = '–';

/// The parsed content of one section (released or `[Unreleased]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    /// Non-bullet paragraphs (e.g. "No user-visible changes (…)"), in
    /// order. They carry no category.
    pub paragraphs: Vec<String>,
    /// Category blocks in the file's order; never empty-headed.
    pub categories: Vec<CategoryBlock>,
}

impl Section {
    /// No entries and no paragraphs (only valid for `[Unreleased]`).
    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty() && self.categories.iter().all(|block| block.entries.is_empty())
    }
}

/// One `### <Category>` block with its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryBlock {
    pub category: Category,
    pub entries: Vec<Entry>,
}

/// The six fixed categories (PRD 0019 RF7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Added,
    Changed,
    Fixed,
    Security,
    Deprecated,
    Removed,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Added,
        Category::Changed,
        Category::Fixed,
        Category::Security,
        Category::Deprecated,
        Category::Removed,
    ];

    /// The exact `### ` heading text (English, per RF1.4).
    pub fn title(self) -> &'static str {
        match self {
            Category::Added => "Added",
            Category::Changed => "Changed",
            Category::Fixed => "Fixed",
            Category::Security => "Security",
            Category::Deprecated => "Deprecated",
            Category::Removed => "Removed",
        }
    }

    fn from_title(title: &str) -> Option<Category> {
        Category::ALL
            .into_iter()
            .find(|category| category.title() == title)
    }

    /// Stable localization id of the category label (`changelog.category.*`),
    /// present in the core `en-US`/`es` catalogs (PRD 0019 RF7.7).
    pub fn l10n_id(self) -> &'static str {
        match self {
            Category::Added => "changelog.category.added",
            Category::Changed => "changelog.category.changed",
            Category::Fixed => "changelog.category.fixed",
            Category::Security => "changelog.category.security",
            Category::Deprecated => "changelog.category.deprecated",
            Category::Removed => "changelog.category.removed",
        }
    }

    /// Portuguese label (the `Localized` fallback for `l10n_id`).
    pub fn fallback_pt(self) -> &'static str {
        match self {
            Category::Added => "Adicionado",
            Category::Changed => "Alterado",
            Category::Fixed => "Corrigido",
            Category::Security => "Segurança",
            Category::Deprecated => "Obsoleto",
            Category::Removed => "Removido",
        }
    }
}

/// Localization id (in the core `en-US`/`es` catalogs) of the note stating
/// that the entries stay in English (PRD 0019 RF7.7).
pub const ENTRIES_LANGUAGE_ID: &str = "changelog.entries_language";
/// Portuguese fallback of [`ENTRIES_LANGUAGE_ID`].
pub const ENTRIES_LANGUAGE_PT: &str = "As entradas são mantidas em inglês, o idioma do changelog.";
/// Localization id of the What's new screen title.
pub const TITLE_ID: &str = "changelog.title";
/// Portuguese fallback of [`TITLE_ID`].
pub const TITLE_PT: &str = "Novidades";

/// One `- ` entry: the full sentence, plus the traceability data extracted
/// from its trailing parentheses (RF7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The entry text as written (single line, without the leading `- `).
    pub text: String,
    /// Carries the `**Breaking**` marker (RF7.2).
    pub breaking: bool,
    /// Every `PRD NNNN RFx` reference; `RF5/RF8` lists and `RF1–RF4` ranges
    /// are expanded into one [`PlanRef`] per requirement.
    pub refs: Vec<PlanRef>,
    /// Short commit hashes cited in the trailing parentheses (7–12 hex
    /// characters). A commit cannot cite its own hash, so the newest
    /// entries may have none until the next changelog touch (AGENTS.md).
    pub hashes: Vec<String>,
}

/// One plan-requirement reference: `PRD 0019 RF7.4` → `prd` "0019",
/// `rf` "RF7.4".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRef {
    /// The zero-padded plan number ("0019").
    pub prd: String,
    /// The requirement id with its `RF` prefix ("RF7.4").
    pub rf: String,
}

impl fmt::Display for PlanRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PRD {} {}", self.prd, self.rf)
    }
}

/// A strict-format failure, with the 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CHANGELOG.md:{}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

fn fail<T>(line: usize, message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        line,
        message: message.into(),
    })
}

/// Parses the whole document strictly (RF7.4): `## [Unreleased]` first,
/// released versions in strictly descending order each with a
/// `YYYY-MM-DD` date, only the six fixed categories, no empty or duplicated
/// category, no entry outside a category and nothing after the footer link
/// definitions.
pub fn parse(text: &str) -> Result<Changelog, ParseError> {
    let mut unreleased: Option<Section> = None;
    let mut releases: Vec<Release> = Vec::new();
    let mut pre_release: Option<PreRelease> = None;
    // Current section being filled: its heading line, body lines and
    // category state.
    let mut current: Option<SectionBuilder> = None;
    let mut footer = false;

    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.trim_end();
        if line.starts_with("## ") {
            if let Some(builder) = current.take() {
                finish_section(builder, &mut unreleased, &mut releases, &mut pre_release)?;
            }
            let heading = line.trim_start_matches("## ").trim();
            if pre_release.is_some() {
                return fail(
                    line_number,
                    format!(
                        "section '## {heading}' after the pre-release history: the pre-release history is the last section"
                    ),
                );
            }
            if heading.starts_with("Pre-release history") {
                if unreleased.is_none() {
                    return fail(
                        line_number,
                        "pre-release history before '## [Unreleased]': the unreleased section comes first",
                    );
                }
                let (first, last) = parse_pre_release_heading(heading, line_number)?;
                if let Some(oldest) = releases.last() {
                    if compare_versions(&oldest.version, &last) != std::cmp::Ordering::Greater {
                        return fail(
                            line_number,
                            format!(
                                "the pre-release history ends at {last}, but [{}] above it is not newer: released sections follow the internal builds",
                                oldest.version
                            ),
                        );
                    }
                }
                current = Some(SectionBuilder::new(
                    line_number,
                    Heading::PreRelease(heading.to_string(), first, last),
                ));
                continue;
            }
            if heading == "[Unreleased]" {
                if unreleased.is_some() || !releases.is_empty() {
                    return fail(
                        line_number,
                        "'## [Unreleased]' must be the first section and appear only once",
                    );
                }
                current = Some(SectionBuilder::new(line_number, Heading::Unreleased));
            } else {
                if unreleased.is_none() {
                    return fail(
                        line_number,
                        "released section before '## [Unreleased]': the unreleased section comes first",
                    );
                }
                let (version, date) = parse_release_heading(heading, line_number)?;
                if releases.iter().any(|r| r.version == version) {
                    return fail(line_number, format!("duplicated section for [{version}]"));
                }
                if let Some(previous) = releases.last() {
                    if compare_versions(&previous.version, &version) != std::cmp::Ordering::Greater
                    {
                        return fail(
                            line_number,
                            format!(
                                "versions out of order: [{version}] follows [{}]; sections are in strictly descending order",
                                previous.version
                            ),
                        );
                    }
                }
                current = Some(SectionBuilder::new(
                    line_number,
                    Heading::Release(version, date),
                ));
            }
            continue;
        }
        if is_link_definition(line) {
            footer = true;
            continue;
        }
        if footer {
            if line.trim().is_empty() {
                continue;
            }
            return fail(
                line_number,
                "unexpected content after the footer link definitions",
            );
        }
        let Some(section) = current.as_mut() else {
            // Preamble (title, "How to read" notes) before [Unreleased].
            continue;
        };
        section.push(line_number, line)?;
    }
    if let Some(builder) = current.take() {
        finish_section(builder, &mut unreleased, &mut releases, &mut pre_release)?;
    }
    let Some(unreleased) = unreleased else {
        return fail(0, "no '## [Unreleased]' section");
    };
    Ok(Changelog {
        unreleased,
        releases,
        pre_release,
    })
}

/// `Pre-release history (X.Y.Z–X.Y.Z)` or `Pre-release history
/// (X.Y.Z–X.Y.Z, <note>)`, strictly; returns `(first, last)`.
fn parse_pre_release_heading(heading: &str, line: usize) -> Result<(String, String), ParseError> {
    let expected = "expected '## Pre-release history (X.Y.Z–X.Y.Z, <note>)' with an en dash";
    let Some(inner) = heading
        .strip_prefix(PRE_RELEASE_PREFIX)
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return fail(
            line,
            format!("malformed pre-release heading '## {heading}': {expected}"),
        );
    };
    let range = inner.split_once(", ").map_or(inner, |(range, _)| range);
    let Some((first, last)) = range.split_once(PRE_RELEASE_DASH) else {
        return fail(
            line,
            format!("malformed pre-release heading '## {heading}': {expected}"),
        );
    };
    if !valid_version(first) || !valid_version(last) {
        return fail(
            line,
            format!(
                "malformed pre-release heading '## {heading}': '{range}' is not an X.Y.Z–X.Y.Z range"
            ),
        );
    }
    if compare_versions(first, last) == std::cmp::Ordering::Greater {
        return fail(
            line,
            format!("pre-release range '{range}' is reversed: the oldest version comes first"),
        );
    }
    Ok((first.to_string(), last.to_string()))
}

/// `## [X.Y.Z] - YYYY-MM-DD`, strictly; reports the exact broken part.
fn parse_release_heading(heading: &str, line: usize) -> Result<(String, String), ParseError> {
    let Some(inner) = heading.strip_prefix('[') else {
        return fail(
            line,
            format!("malformed section heading '## {heading}': expected '## [X.Y.Z] - YYYY-MM-DD'"),
        );
    };
    let Some((version, rest)) = inner.split_once(']') else {
        return fail(
            line,
            format!("malformed section heading '## {heading}': missing ']'"),
        );
    };
    if !valid_version(version) {
        return fail(
            line,
            format!(
                "malformed section heading '## {heading}': '{version}' is not an X.Y.Z version"
            ),
        );
    }
    let Some(date) = rest.strip_prefix(" - ") else {
        return fail(
            line,
            format!("section [{version}] has no date: expected '## [{version}] - YYYY-MM-DD'"),
        );
    };
    let date = date.trim();
    if !valid_date(date) {
        return fail(
            line,
            format!("'{date}' is not a valid YYYY-MM-DD date (section [{version}])"),
        );
    }
    Ok((version.to_string(), date.to_string()))
}

fn valid_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn valid_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let numeric = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return false;
    }
    if !numeric(year) || !numeric(month) || !numeric(day) {
        return false;
    }
    let month: u32 = month.parse().unwrap_or(0);
    let day: u32 = day.parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// Numeric semver-ish comparison of two `X.Y.Z` strings.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> [u64; 3] {
        let mut out = [0u64; 3];
        for (slot, part) in out.iter_mut().zip(v.split('.')) {
            *slot = part.parse().unwrap_or(0);
        }
        out
    };
    parts(a).cmp(&parts(b))
}

/// A footer link definition: `[Unreleased]: …` or `[0.68.2]: …`.
fn is_link_definition(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    let Some((label, after)) = rest.split_once(']') else {
        return false;
    };
    (label == "Unreleased" || valid_version(label)) && after.starts_with(": ")
}

/// Which section a [`SectionBuilder`] fills.
enum Heading {
    Unreleased,
    /// `(version, date)`.
    Release(String, String),
    /// `(title, first, last)`.
    PreRelease(String, String, String),
}

struct SectionBuilder {
    line: usize,
    heading: Heading,
    body: Vec<String>,
    section: Section,
    current_category: Option<usize>,
}

impl SectionBuilder {
    fn new(line: usize, heading: Heading) -> Self {
        Self {
            line,
            heading,
            body: Vec::new(),
            section: Section::default(),
            current_category: None,
        }
    }

    fn push(&mut self, line_number: usize, line: &str) -> Result<(), ParseError> {
        self.body.push(line.to_string());
        if line.trim().is_empty() {
            return Ok(());
        }
        if let Some(title) = line.strip_prefix("### ") {
            let title = title.trim();
            let Some(category) = Category::from_title(title) else {
                return fail(
                    line_number,
                    format!(
                        "invalid category '{title}': expected one of Added, Changed, Fixed, Security, Deprecated, Removed"
                    ),
                );
            };
            if self
                .section
                .categories
                .iter()
                .any(|block| block.category == category)
            {
                return fail(
                    line_number,
                    format!("duplicated category '{title}' in the same section"),
                );
            }
            self.section.categories.push(CategoryBlock {
                category,
                entries: Vec::new(),
            });
            self.current_category = Some(self.section.categories.len() - 1);
            return Ok(());
        }
        if line.starts_with('#') {
            return fail(
                line_number,
                format!("malformed heading '{line}': categories use '### '"),
            );
        }
        if let Some(entry) = line.strip_prefix("- ") {
            let Some(index) = self.current_category else {
                return fail(
                    line_number,
                    "entry outside a category: entries live under a '### <Category>' heading",
                );
            };
            self.section.categories[index]
                .entries
                .push(parse_entry(entry));
            return Ok(());
        }
        // A plain paragraph (e.g. "No user-visible changes (…)").
        self.section.paragraphs.push(line.to_string());
        Ok(())
    }
}

/// Moves the finished section into the changelog, rejecting empty
/// categories (RF7.1: an empty category does not appear).
fn finish_section(
    builder: SectionBuilder,
    unreleased: &mut Option<Section>,
    releases: &mut Vec<Release>,
    pre_release: &mut Option<PreRelease>,
) -> Result<(), ParseError> {
    for block in &builder.section.categories {
        if block.entries.is_empty() {
            let where_ = match &builder.heading {
                Heading::Unreleased => " of [Unreleased]".to_string(),
                Heading::Release(version, _) => format!(" of [{version}]"),
                Heading::PreRelease(..) => " of the pre-release history".to_string(),
            };
            return Err(ParseError {
                line: builder.line,
                message: format!(
                    "category '{}'{where_} is empty: an empty category does not appear",
                    block.category.title()
                ),
            });
        }
    }
    let mut body = builder.body.clone();
    while let Some(last) = body.last() {
        let trimmed = last.trim();
        if trimmed.is_empty() || is_link_definition(trimmed) {
            body.pop();
        } else {
            break;
        }
    }
    match builder.heading {
        Heading::Unreleased => *unreleased = Some(builder.section),
        Heading::Release(version, date) => releases.push(Release {
            version,
            date,
            body: body.join("\n"),
            section: builder.section,
        }),
        Heading::PreRelease(title, first, last) => {
            if builder.section.is_empty() {
                return fail(
                    builder.line,
                    "the pre-release history is empty: it summarizes the internal builds",
                );
            }
            *pre_release = Some(PreRelease {
                title,
                first,
                last,
                body: body.join("\n"),
                section: builder.section,
            });
        }
    }
    Ok(())
}

/// Extracts the traceability data of one entry from its trailing
/// parentheses: `… (PRD 0006 RF2, abc1234)`, `… (PRD 0001 RF5/RF8)`,
/// `… (PRD 0021 RF1–RF4; PRD 0019 RF3)`, `… (e0a6a60, 6813ecb)`, and the
/// grouped form `release prepare` writes (PRD 0039 RF4.3):
/// `… (PRD 0018 RF1.2, RF1.3; PRD 0021 RF2; abc1234)`, where a bare
/// requirement after a comma continues the plan before it (a `;` ends it).
fn parse_entry(text: &str) -> Entry {
    let mut refs = Vec::new();
    let mut hashes = Vec::new();
    for group in parenthesized_groups(text) {
        for clause in group.split(';') {
            let mut plan: Option<&str> = None;
            for part in clause.split(',') {
                let part = part.trim().trim_matches('`');
                if let Some(rest) = part.strip_prefix("PRD ") {
                    plan = rest.split_once(' ').map(|(number, _)| number.trim());
                    parse_prd_ref(rest, &mut refs);
                } else if is_hash(part) {
                    hashes.push(part.to_string());
                } else if let (Some(prd), true) = (plan, part.starts_with("RF") || valid_rf(part)) {
                    parse_prd_ref(&format!("{prd} {part}"), &mut refs);
                }
            }
        }
    }
    Entry {
        text: text.to_string(),
        breaking: text.contains("**Breaking**"),
        refs,
        hashes,
    }
}

/// The top-level `(...)` groups of a line (entries never nest parentheses
/// in the traceability tail; nested groups are joined into their parent).
fn parenthesized_groups(text: &str) -> Vec<&str> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = index + 1;
                }
                depth += 1;
            }
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    groups.push(&text[start..index]);
                }
            }
            _ => {}
        }
    }
    groups
}

/// `0001 RF5/RF8` → one [`PlanRef`] per requirement; `RF1–RF4` expands the
/// inclusive range. Anything that does not start with `RF` is ignored (the
/// entry may cite a plan without a requirement).
fn parse_prd_ref(rest: &str, refs: &mut Vec<PlanRef>) {
    let Some((prd, rf_list)) = rest.split_once(' ') else {
        return;
    };
    let prd = prd.trim();
    if prd.len() != 4 || !prd.bytes().all(|b| b.is_ascii_digit()) {
        return;
    }
    for token in rf_list.split('/') {
        let token = token.trim().trim_end_matches(['.', ',']);
        if let Some((from, to)) = token.split_once(['–', '-']) {
            if let (Some(from), Some(to)) = (plain_rf(from), plain_rf(to)) {
                if from <= to && to - from <= 32 {
                    for number in from..=to {
                        refs.push(PlanRef {
                            prd: prd.to_string(),
                            rf: format!("RF{number}"),
                        });
                    }
                    continue;
                }
            }
        }
        if valid_rf(token) {
            refs.push(PlanRef {
                prd: prd.to_string(),
                rf: token.to_string(),
            });
        }
    }
}

/// `RF12` → 12, only for a plain (undotted) id.
fn plain_rf(token: &str) -> Option<u32> {
    token
        .strip_prefix("RF")
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse().ok())
}

/// A requirement id: `RF2`, `RF7.4`, `RF1.10` (`RF` and dot-separated
/// numbers), or a review id such as `FIX-01`, `INT-04`, `GEM-01` or
/// `SES-01` (two or more capital letters, a dash, dot-separated numbers),
/// the shapes the commit gate accepts in `Refs:` (PRD 0039 RF1.2).
fn valid_rf(token: &str) -> bool {
    let numbers = |rest: &str| {
        !rest.is_empty()
            && rest
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    };
    if let Some(rest) = token.strip_prefix("RF") {
        return numbers(rest);
    }
    let letters = token.bytes().take_while(|b| b.is_ascii_uppercase()).count();
    letters >= 2 && token[letters..].strip_prefix('-').is_some_and(numbers)
}

/// 7–12 lowercase hex characters with at least one digit (so English words
/// made of hex letters are not mistaken for hashes).
fn is_hash(token: &str) -> bool {
    (7..=12).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && token.bytes().any(|b| b.is_ascii_digit())
}

#[cfg(test)]
#[path = "changelog_tests.rs"]
mod tests;
