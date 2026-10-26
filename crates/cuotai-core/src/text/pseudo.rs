//! PRD 0035 RF9.4 (DS-MSG-16): the `en-XA` pseudo-locale.
//!
//! The Rust twin of `pseudoText` in `apps/desktop/ui/js/i18n.js`: each
//! English text grows (+100 % up to 10 characters, +50 % up to 20, +40 %
//! beyond), its letters take accents and it is wrapped in `⟦ ⟧`, so
//! hard-coded text, concatenation and clipping show. `{name}` placeholders
//! are kept. Only development builds register it (`cfg(debug_assertions)`):
//! in a release build `CUOTAI_LANG=en-XA` is plain English and the registry
//! has exactly the fourteen rows of [`super::LOCALES`].

use super::registry::{Direction, LocaleRow, LocaleStatus, PluralRule, LOCALES};

/// The pseudo-locale's code.
pub const PSEUDO_LOCALE: &str = "en-XA";

/// Its registry row, outside [`LOCALES`] so catalog and parity tests never
/// expect files for it.
static PSEUDO_ROW: LocaleRow = LocaleRow {
    code: PSEUDO_LOCALE,
    endonym: "Pseudo",
    flag: "",
    script: "Latn",
    dir: Direction::Ltr,
    // No language subtag: `normalize("en-XA")` stays en-US, so a release
    // build reads English.
    languages: &[],
    excluded_scripts: &[],
    excluded_regions: &[],
    plural_rule: PluralRule::One,
    status: LocaleStatus::InProgress,
    critical_reviewed: false,
    phase: 0,
};

/// Whether `locale` selects the pseudo-locale; always `false` in release
/// builds.
pub fn is_pseudo(locale: &str) -> bool {
    cfg!(debug_assertions) && locale.trim().eq_ignore_ascii_case(PSEUDO_LOCALE)
}

/// Every registered language: the fourteen built-in rows, plus `en-XA` in
/// development builds only.
pub fn registered_locales() -> Vec<&'static LocaleRow> {
    let mut rows: Vec<&'static LocaleRow> = LOCALES.iter().collect();
    if cfg!(debug_assertions) {
        rows.push(&PSEUDO_ROW);
    }
    rows
}

fn accented(c: char) -> char {
    match c {
        'a' => 'á',
        'e' => 'ë',
        'i' => 'ï',
        'o' => 'ô',
        'u' => 'ü',
        'c' => 'ç',
        'n' => 'ñ',
        'y' => 'ý',
        'A' => 'Å',
        'E' => 'É',
        'I' => 'Î',
        'O' => 'Ö',
        'U' => 'Ü',
        'C' => 'Ç',
        'N' => 'Ñ',
        other => other,
    }
}

/// Length of a `{name}` placeholder (`\{\w+\}`) at the start of `text`.
fn placeholder_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('{')?;
    let name = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (name > 0 && rest[name..].starts_with('}')).then_some(name + 2)
}

/// `text` pseudo-localized, as the desktop's `pseudoText` does it.
pub fn pseudo_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    out.push('⟦');
    let mut letters = 0usize;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(len) = placeholder_len(rest) {
            out.push_str(&rest[..len]);
            rest = &rest[len..];
            continue;
        }
        out.push(accented(c));
        letters += 1;
        rest = &rest[c.len_utf8()..];
    }
    let grow = match letters {
        0..=10 => letters,
        11..=20 => letters.div_ceil(2),
        _ => (letters * 2).div_ceil(5),
    };
    out.extend(std::iter::repeat_n('·', grow));
    out.push('⟧');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same outputs as `pseudoText` in `i18n.js` (`Math.ceil` of the growth).
    #[test]
    fn pseudo_text_matches_the_desktop_transform() {
        assert_eq!(pseudo_text("Service"), "⟦Sërvïçë·······⟧");
        assert_eq!(
            pseudo_text("Service unavailable ({n})"),
            "⟦Sërvïçë üñáváïláblë ({n})·········⟧"
        );
        assert_eq!(pseudo_text(""), "⟦⟧");
        assert_eq!(pseudo_text("{a}{b}"), "⟦{a}{b}⟧");
        assert_eq!(pseudo_text("{not closed"), "⟦{ñôt çlôsëd······⟧");
    }

    #[test]
    fn registry_has_the_pseudo_row_only_in_debug() {
        let rows = registered_locales();
        #[cfg(debug_assertions)]
        {
            assert_eq!(rows.len(), 15);
            assert!(is_pseudo("en-XA") && is_pseudo("EN-xa"));
        }
        #[cfg(not(debug_assertions))]
        {
            assert_eq!(rows.len(), 14, "release builds never register en-XA");
            assert!(!is_pseudo("en-XA"));
        }
        assert_eq!(LOCALES.len(), 14);
        assert_eq!(
            super::super::normalize("en-XA").map(|row| row.code),
            Some("en-US"),
            "en-XA is not a built-in language"
        );
    }
}
