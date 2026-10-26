//! PRD 0035 RF10.5: the search folding shared by the desktop (`fold()` in
//! `js/util.js`) and the TUI filter, so `dong` finds `Đồng`, `istanbul`
//! finds `İstanbul` and `strasse` finds `Straße`.
//!
//! The desktop decomposes (NFD) and strips combining marks; the tree has no
//! Unicode normalization crate, so this twin maps the precomposed Latin
//! letters the fourteen languages use (Latin-1, Latin Extended-A, the
//! Vietnamese block and `ơ`/`ư`) to their base letter by table, then strips
//! any combining mark (U+0300..U+036F) and lower-cases. CJK, Hangul and
//! Devanagari text is compared as typed.

/// Base letters of U+00C0..U+00FF; `.` keeps the character.
const LATIN_1: &[u8; 64] = b"aaaaaa.ceeeeiiiidnooooo.ouuuuy..aaaaaa.ceeeeiiiidnooooo.ouuuuy.y";
/// Base letters of U+0100..U+017F; `.` keeps the character.
const LATIN_EXT_A: &[u8; 128] = b"aaaaaaccccccccddddeeeeeeeeeegggggggghhhhiiiiiiiiii..jjkk.llllllllllnnnnnnn..oooooo..rrrrrrssssssssttttttuuuuuuuuuuuuwwyyyzzzzzzs";
/// Base letters of the Vietnamese block U+1EA0..U+1EF9.
const VIETNAMESE: &[u8; 90] =
    b"aaaaaaaaaaaaaaaaaaaaaaaaeeeeeeeeeeeeeeeeiiiioooooooooooooooooooooooouuuuuuuuuuuuuuyyyyyyyy";

fn base(c: char) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    let pick = |table: &[u8], index: u32| {
        let byte = table[index as usize];
        (byte != b'.').then(|| LETTERS[usize::from(byte - b'a')])
    };
    let code = c as u32;
    match c {
        'ß' | 'ẞ' => Some("ss"),
        'Ơ' | 'ơ' => Some("o"),
        'Ư' | 'ư' => Some("u"),
        'Ș' | 'ș' | 'Ş' | 'ş' => Some("s"),
        'Ț' | 'ț' => Some("t"),
        '\u{c0}'..='\u{ff}' => pick(LATIN_1, code - 0xc0),
        '\u{100}'..='\u{17f}' => pick(LATIN_EXT_A, code - 0x100),
        '\u{1ea0}'..='\u{1ef9}' => pick(VIETNAMESE, code - 0x1ea0),
        _ => None,
    }
}

/// Fold `text` for a case- and accent-insensitive search.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        match base(c) {
            Some(letters) => out.push_str(letters),
            None => out.extend(
                c.to_lowercase()
                    .filter(|c| !('\u{300}'..='\u{36f}').contains(c)),
            ),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn fold_matches_the_desktop_examples() {
        assert_eq!(fold("Đồng"), "dong");
        assert_eq!(fold("İstanbul"), "istanbul");
        assert_eq!(fold("ıstanbul"), "istanbul");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("Łódź"), "lodz");
        assert_eq!(fold("Tiếng Việt"), "tieng viet");
        assert_eq!(fold("Ưu đãi"), "uu dai");
        assert_eq!(fold("Çalışma Ğ Ş"), "calisma g s");
        assert_eq!(fold("Configuração São"), "configuracao sao");
        assert_eq!(fold("Vie\u{302}\u{323}t"), "viet", "decomposed input");
        assert_eq!(fold("日本語のアカウント"), "日本語のアカウント");
        assert_eq!(fold("한국어"), "한국어");
        assert_eq!(fold("हिन्दी"), "हिन्दी");
        assert_eq!(fold("Русский"), "русский");
        assert_eq!(fold("Æble × Œuvre"), "æble × œuvre");
    }

    #[test]
    fn the_tables_line_up_with_their_blocks() {
        // Spot checks at the edges of each block.
        assert_eq!(fold("À"), "a");
        assert_eq!(fold("ÿ"), "y");
        assert_eq!(fold("Ā"), "a");
        assert_eq!(fold("ſ"), "s");
        assert_eq!(fold("Ŋ"), "ŋ");
        assert_eq!(fold("Ĳ"), "ĳ");
        assert_eq!(fold("Ạ"), "a");
        assert_eq!(fold("ỹ"), "y");
        assert_eq!(fold("Ệ"), "e");
        assert_eq!(fold("ỉ"), "i");
        assert_eq!(fold("Ợ"), "o");
        assert_eq!(fold("ự"), "u");
    }
}
