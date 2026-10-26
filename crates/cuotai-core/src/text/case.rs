//! Locale-aware casing for TUI headings (PRD 0035 RF6.4).
//!
//! `str::to_uppercase` applies the language-neutral Unicode mapping, which
//! is wrong for Turkish and Azerbaijani: there the dotted `i` capitalizes to
//! `İ` (U+0130) and the dotless `ı` to `I`. Scripts without case (CJK,
//! Devanagari, Hangul) pass through unchanged, so the same call is safe for
//! every shipped locale.

/// `text` in capitals as `locale` writes them.
pub fn uppercase(locale: &str, text: &str) -> String {
    let language = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(language.as_str(), "tr" | "az") {
        let mut out = String::with_capacity(text.len());
        for character in text.chars() {
            match character {
                'i' => out.push('İ'),
                'ı' => out.push('I'),
                other => out.extend(other.to_uppercase()),
            }
        }
        out
    } else {
        text.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::uppercase;

    #[test]
    fn turkish_keeps_the_dot_on_i() {
        assert_eq!(uppercase("tr", "fiyatlar ılık"), "FİYATLAR ILIK");
        assert_eq!(uppercase("tr-TR", "i"), "İ");
        assert_eq!(uppercase("az", "i"), "İ");
    }

    #[test]
    fn other_locales_use_the_neutral_mapping() {
        assert_eq!(uppercase("en-US", "pricing"), "PRICING");
        assert_eq!(uppercase("de", "straße"), "STRASSE");
        assert_eq!(uppercase("it", "i"), "I");
    }

    #[test]
    fn caseless_scripts_pass_through() {
        assert_eq!(uppercase("ja", "変更履歴"), "変更履歴");
        assert_eq!(uppercase("hi-IN", "बदलाव"), "बदलाव");
        assert_eq!(uppercase("ko", "변경 사항"), "변경 사항");
    }
}
