//! PRD 0035 RF1 and RF4: the locale registry, the one resolution of a
//! language code, and the plural rules every layer shares.
//!
//! [`LOCALES`] is the single list of built-in languages. The core (its
//! catalogs and [`super::Localized::resolve`]), the TUI (its chrome
//! catalogs and `CUOTAI_LANG`) and the desktop (the `locale_registry`
//! command, the language list and detection) all read it, so adding a
//! language means one row here plus its catalogs. A row whose catalogs do
//! not exist yet is registered as [`LocaleStatus::InProgress`]: it resolves
//! (so a Japanese system is recognized as Japanese) but renders the English
//! source until its catalogs land, and the desktop lists it as "translation
//! in progress".

use serde::Serialize;

/// Writing direction. Every built-in language is left-to-right; a row that
/// declares `Rtl` fails a test until a PRD covers mirroring (PRD 0035 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Ltr,
    Rtl,
}

/// The CLDR plural rule of a language, hard-coded (no ICU dependency) and
/// cross-checked against a fixture recorded from `Intl.PluralRules`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluralRule {
    /// No grammatical plural (ja, zh-CN, ko, id-ID, vi): always `other`.
    Other,
    /// `one` for exactly 1 (en, de, es, it, tr).
    One,
    /// `one` for an integer part of 0 or 1 (pt-BR, fr).
    ZeroOne,
    /// `one` for an integer part of 0 or exactly 1 (hi-IN).
    Hindi,
    /// `one`, `few`, `many`, `other` (ru).
    EastSlavic,
}

impl PluralRule {
    /// The categories a catalog of this rule must give a plural object,
    /// in CLDR order. `many` of es, it, pt-BR and fr (millions) is not
    /// used: it falls to `other` (PRD 0035 RF4).
    pub const fn categories(self) -> &'static [&'static str] {
        match self {
            PluralRule::Other => &["other"],
            PluralRule::One | PluralRule::ZeroOne | PluralRule::Hindi => &["one", "other"],
            PluralRule::EastSlavic => &["one", "few", "many", "other"],
        }
    }
}

/// How far a built-in language is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocaleStatus {
    /// The source language: its text is in the code (core, TUI) and in
    /// `en-US.json` (desktop UI).
    Source,
    /// Core, TUI and desktop catalogs exist and pass the parity tests.
    Shipped,
    /// Registered, but its catalogs are not written yet: it renders the
    /// English source and the desktop marks it "translation in progress".
    InProgress,
}

/// One built-in language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LocaleRow {
    /// The canonical BCP 47 code, also the catalog file name.
    pub code: &'static str,
    /// The language's own name (DS-CMP-SEL-3), never translated.
    pub endonym: &'static str,
    /// The flag's ISO 3166 country (lower case) drawn next to the endonym.
    pub flag: &'static str,
    /// The ISO 15924 script, which selects the font stack (RF8).
    pub script: &'static str,
    pub dir: Direction,
    /// Language subtags (lower case) that resolve to this row: the primary
    /// one first, then legacy or macrolanguage codes (`in` for Indonesian,
    /// `cmn` for Mandarin).
    pub languages: &'static [&'static str],
    /// Script subtags (lower case) that must not resolve here.
    pub excluded_scripts: &'static [&'static str],
    /// Region subtags (lower case) that must not resolve here unless a
    /// script subtag says otherwise (`zh-TW` is not Simplified Chinese,
    /// `zh-Hans-TW` is).
    pub excluded_regions: &'static [&'static str],
    pub plural_rule: PluralRule,
    pub status: LocaleStatus,
    /// Whether a native reviewer signed the critical strings (RF11.3).
    pub critical_reviewed: bool,
    /// Rollout phase (PRD 0035 §9): 0 for the four languages that shipped
    /// before, then 1 or 2.
    pub phase: u8,
    /// The first day of the week in the language's territory, as an ISO
    /// weekday (1 = Monday, 7 = Sunday), from CLDR `weekData` `firstDay`
    /// (PRD 0032 RF2.3: the quiet-hours editor lists the days from it).
    pub first_weekday: u8,
}

impl LocaleRow {
    /// Whether this language has its own catalogs (or is the source).
    pub const fn is_translated(&self) -> bool {
        !matches!(self.status, LocaleStatus::InProgress)
    }

    /// The categories its plural objects must carry.
    pub const fn plural_categories(&self) -> &'static [&'static str] {
        self.plural_rule.categories()
    }
}

#[allow(clippy::too_many_arguments)] // one positional table row per language
const fn row(
    code: &'static str,
    endonym: &'static str,
    flag: &'static str,
    script: &'static str,
    languages: &'static [&'static str],
    plural_rule: PluralRule,
    status: LocaleStatus,
    phase: u8,
    first_weekday: u8,
) -> LocaleRow {
    LocaleRow {
        code,
        endonym,
        flag,
        script,
        dir: Direction::Ltr,
        languages,
        excluded_scripts: &[],
        excluded_regions: &[],
        plural_rule,
        status,
        critical_reviewed: false,
        phase,
        first_weekday,
    }
}

#[allow(unused_imports)]
// every built-in language has shipped; a new one starts as `InProgress`
use LocaleStatus::{InProgress, Shipped, Source};

/// The built-in languages, in the fixed order of the language list
/// (PRD 0035 RF3.3): the four that shipped first, then phase 1, then
/// phase 2. No alphabetization across scripts. The last number of a row is
/// its first weekday from CLDR `weekData` (Sunday for BR, US, JP, KR, IN
/// and ID; Monday for the others, CN included, as ICU's
/// `Intl.Locale("zh-CN").getWeekInfo().firstDay` also answers).
pub const LOCALES: &[LocaleRow] = &[
    row(
        "pt-BR",
        "Português",
        "br",
        "Latn",
        &["pt"],
        PluralRule::ZeroOne,
        Shipped,
        0,
        7,
    ),
    row(
        "en-US",
        "English",
        "us",
        "Latn",
        &["en"],
        PluralRule::One,
        Source,
        0,
        7,
    ),
    row(
        "es",
        "Español",
        "es",
        "Latn",
        &["es"],
        PluralRule::One,
        Shipped,
        0,
        1,
    ),
    row(
        "it",
        "Italiano",
        "it",
        "Latn",
        &["it"],
        PluralRule::One,
        Shipped,
        0,
        1,
    ),
    LocaleRow {
        excluded_scripts: &["hant"],
        excluded_regions: &["tw", "hk", "mo"],
        ..row(
            "zh-CN",
            "中文（简体）",
            "cn",
            "Hans",
            &["zh", "cmn"],
            PluralRule::Other,
            Shipped,
            1,
            1,
        )
    },
    row(
        "fr",
        "Français",
        "fr",
        "Latn",
        &["fr"],
        PluralRule::ZeroOne,
        Shipped,
        1,
        1,
    ),
    row(
        "de",
        "Deutsch",
        "de",
        "Latn",
        &["de"],
        PluralRule::One,
        Shipped,
        1,
        1,
    ),
    row(
        "ja",
        "日本語",
        "jp",
        "Jpan",
        &["ja"],
        PluralRule::Other,
        Shipped,
        1,
        7,
    ),
    row(
        "ru",
        "Русский",
        "ru",
        "Cyrl",
        &["ru"],
        PluralRule::EastSlavic,
        Shipped,
        1,
        1,
    ),
    row(
        "ko",
        "한국어",
        "kr",
        "Kore",
        &["ko"],
        PluralRule::Other,
        Shipped,
        2,
        7,
    ),
    row(
        "hi-IN",
        "हिन्दी",
        "in",
        "Deva",
        &["hi"],
        PluralRule::Hindi,
        Shipped,
        2,
        7,
    ),
    row(
        "id-ID",
        "Bahasa Indonesia",
        "id",
        "Latn",
        &["id", "in"],
        PluralRule::Other,
        Shipped,
        2,
        7,
    ),
    row(
        "tr",
        "Türkçe",
        "tr",
        "Latn",
        &["tr"],
        PluralRule::One,
        Shipped,
        2,
        1,
    ),
    row(
        "vi",
        "Tiếng Việt",
        "vn",
        "Latn",
        &["vi"],
        PluralRule::Other,
        Shipped,
        2,
        1,
    ),
];

/// The source language row (en-US): the last step of every fallback chain.
pub fn source_locale() -> &'static LocaleRow {
    LOCALES
        .iter()
        .find(|row| row.status == Source)
        .expect("the registry has a source row")
}

/// The registry row of a canonical code (exact, case-insensitive).
pub fn locale_row(code: &str) -> Option<&'static LocaleRow> {
    LOCALES
        .iter()
        .find(|row| row.code.eq_ignore_ascii_case(code))
}

/// PRD 0035 RF1.3: the built-in language of a raw code from a saved choice,
/// `CUOTAI_LANG`, `sys_locale` or `navigator.languages`.
///
/// Strips an encoding (`.UTF-8`) and a modifier (`@euro`), accepts `_` for
/// `-`, ignores case, then matches on the parsed **language subtag** (never
/// a string prefix: `est` is not `es`), honouring the row's excluded
/// scripts and regions (`zh-TW` and `zh-Hant` are never Simplified
/// Chinese). `None` for empty, unparsable or unknown input; callers then
/// use en-US.
pub fn normalize(raw: &str) -> Option<&'static LocaleRow> {
    let raw = raw.trim();
    let raw = raw.split(['.', '@']).next().unwrap_or_default();
    let mut subtags = raw.split(['-', '_']);
    let language = subtags.next()?.to_ascii_lowercase();
    if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut script = None;
    let mut region = None;
    for subtag in subtags {
        let alpha = subtag.bytes().all(|b| b.is_ascii_alphabetic());
        let digit = subtag.bytes().all(|b| b.is_ascii_digit());
        match subtag.len() {
            4 if alpha && script.is_none() && region.is_none() => {
                script = Some(subtag.to_ascii_lowercase())
            }
            2 if alpha && region.is_none() => region = Some(subtag.to_ascii_lowercase()),
            3 if digit && region.is_none() => region = Some(subtag.to_owned()),
            1..=8 if subtag.bytes().all(|b| b.is_ascii_alphanumeric()) => {}
            _ => return None,
        }
    }
    let row = LOCALES
        .iter()
        .find(|row| row.languages.contains(&language.as_str()))?;
    match (&script, &region) {
        (Some(script), _) if row.excluded_scripts.contains(&script.as_str()) => None,
        (None, Some(region)) if row.excluded_regions.contains(&region.as_str()) => None,
        _ => Some(row),
    }
}

/// PRD 0035 RF4.1: the plural category of `count` under `rule`. `count` is
/// the parameter as text ("3", "1.5"); a value that is not a plain
/// non-negative decimal selects `other`.
pub fn plural_category(rule: PluralRule, count: &str) -> &'static str {
    let count = count.trim();
    let (int, frac) = count.split_once('.').unwrap_or((count, ""));
    let valid = !int.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.bytes().all(|b| b.is_ascii_digit())
        && !(count.contains('.') && frac.is_empty());
    if !valid {
        return "other";
    }
    // Only the last digits matter for the modular rules; avoid overflow.
    let tail = &int[int.len().saturating_sub(4)..];
    let i: u64 = tail.parse().unwrap_or(0);
    let short = int.len() <= 4;
    let v = frac.len();
    let frac_zero = frac.bytes().all(|b| b == b'0');
    let n_is_one = short && i == 1 && frac_zero;
    match rule {
        PluralRule::Other => "other",
        PluralRule::One => {
            if n_is_one && v == 0 {
                "one"
            } else {
                "other"
            }
        }
        PluralRule::ZeroOne => {
            if short && i <= 1 {
                "one"
            } else {
                "other"
            }
        }
        PluralRule::Hindi => {
            if (short && i == 0) || n_is_one {
                "one"
            } else {
                "other"
            }
        }
        PluralRule::EastSlavic => {
            if v != 0 {
                return "other";
            }
            let (m10, m100) = (i % 10, i % 100);
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// PRD 0035 RF1.3/RF1.4: every alias row, the codes the visual review
    /// tested, and the inputs where the desktop twin used to differ
    /// (`"\u{212a}o"` with a KELVIN SIGN, a leading U+FEFF). The same file
    /// runs against `normalizeLocale` in `js/locale-registry.js` and the
    /// `js/first-paint.js` copy (`scripts/test-i18n-infra-ui.mjs`). This
    /// function is the reference: add a row here, then make the JS agree.
    #[test]
    fn normalize_follows_the_alias_table() {
        let cases: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(include_str!("vectors/normalize.json")).unwrap();
        assert!(cases.len() > 100, "the vector file lost its rows");
        for (raw, expected) in &cases {
            assert_eq!(
                normalize(raw).map(|row| row.code),
                expected.as_str(),
                "normalize({raw:?})"
            );
        }
        // Every canonical code resolves to itself.
        for row in LOCALES {
            assert_eq!(normalize(row.code).map(|r| r.code), Some(row.code));
            assert_eq!(locale_row(row.code), Some(row));
        }
    }

    #[test]
    fn the_registry_is_consistent() {
        let codes: BTreeSet<_> = LOCALES.iter().map(|row| row.code).collect();
        assert_eq!(codes.len(), LOCALES.len(), "duplicate code");
        assert_eq!(LOCALES.len(), 14, "four shipped plus ten (PRD 0035 RF1.2)");
        let mut languages = BTreeSet::new();
        for row in LOCALES {
            for language in row.languages {
                assert!(languages.insert(*language), "{language} in two rows");
            }
            assert_eq!(
                row.dir,
                Direction::Ltr,
                "{}: right-to-left needs its own PRD (PRD 0035 §4)",
                row.code
            );
            assert!(!row.endonym.is_empty() && !row.flag.is_empty());
            assert!(
                ["Latn", "Cyrl", "Hans", "Jpan", "Kore", "Deva"].contains(&row.script),
                "{}: unknown script {}",
                row.code,
                row.script
            );
            assert!(
                row.code.to_ascii_lowercase().starts_with(row.languages[0]),
                "{}: the primary language subtag comes first",
                row.code
            );
        }
        assert_eq!(source_locale().code, "en-US");
        // Fixed order: shipped, then phase 1, then phase 2.
        let phases: Vec<u8> = LOCALES.iter().map(|row| row.phase).collect();
        assert!(phases.windows(2).all(|w| w[0] <= w[1]), "{phases:?}");
    }

    /// PRD 0032 RF2.3 (re-validation C-05): every row names its week's
    /// first day as an ISO weekday, as CLDR `weekData` gives it.
    #[test]
    fn every_row_has_a_first_weekday() {
        for row in LOCALES {
            assert!(
                (1..=7).contains(&row.first_weekday),
                "{}: first weekday {}",
                row.code,
                row.first_weekday
            );
        }
        let first = |code| locale_row(code).unwrap().first_weekday;
        for code in ["en-US", "pt-BR", "ja", "ko", "hi-IN", "id-ID"] {
            assert_eq!(first(code), 7, "{code} starts on Sunday");
        }
        for code in ["de", "es", "fr", "it", "ru", "tr", "vi", "zh-CN"] {
            assert_eq!(first(code), 1, "{code} starts on Monday");
        }
    }

    /// PRD 0035 RF4.4: the hard-coded rules agree with what the WebView's
    /// `Intl.PluralRules` answered when the fixture was recorded.
    #[test]
    fn plural_rules_match_the_intl_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("plural_fixture.json")).unwrap();
        for row in LOCALES {
            let expected = fixture[row.code]
                .as_object()
                .unwrap_or_else(|| panic!("fixture lacks {}", row.code));
            for (n, category) in expected {
                let category = category.as_str().unwrap();
                // `many` for millions (es, it, pt-BR, fr) is not used.
                let category = if category == "many" && row.plural_rule != PluralRule::EastSlavic {
                    "other"
                } else {
                    category
                };
                assert_eq!(
                    plural_category(row.plural_rule, n),
                    category,
                    "{} {n}",
                    row.code
                );
                assert!(row.plural_categories().contains(&category));
            }
        }
    }

    #[test]
    fn plural_category_handles_odd_input() {
        assert_eq!(plural_category(PluralRule::One, "1"), "one");
        assert_eq!(plural_category(PluralRule::One, " 1 "), "one");
        assert_eq!(plural_category(PluralRule::One, "1.0"), "other");
        assert_eq!(plural_category(PluralRule::One, "-1"), "other");
        assert_eq!(plural_category(PluralRule::One, "abc"), "other");
        assert_eq!(plural_category(PluralRule::One, ""), "other");
        assert_eq!(plural_category(PluralRule::One, "1."), "other");
        assert_eq!(plural_category(PluralRule::EastSlavic, "1001"), "one");
        assert_eq!(
            plural_category(PluralRule::EastSlavic, "100000000021"),
            "one"
        );
        assert_eq!(plural_category(PluralRule::EastSlavic, "1011"), "many");
        assert_eq!(plural_category(PluralRule::ZeroOne, "10001"), "other");
    }
}
