//! PRD 0035 RF2.2, RF3.2 and RF5.3: the catalog parity rules, shared by the
//! core's tests (core and desktop UI catalogs) and the TUI's (its chrome
//! catalogs), so every layer checks N languages the same way.

use super::{Catalog, LocaleRow, Message};
use std::collections::BTreeSet;

/// What a catalog may differ in from its reference.
#[derive(Debug, Default, Clone, Copy)]
pub struct Allowances<'a> {
    /// Ids the reference has that this catalog may lack.
    pub missing: &'a [&'a str],
    /// Ids this catalog may have beyond the reference.
    pub extra: &'a [&'a str],
    /// Ids whose braces are literal text, not parameters.
    pub literal_braces: &'a [&'a str],
}

/// The `{name}` parameters of a template.
pub fn placeholders(template: &str) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else { break };
        let name = &rest[..end];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.insert(name);
        }
        rest = &rest[end + 1..];
    }
    out
}

fn message_placeholders(message: &Message) -> BTreeSet<&str> {
    message
        .templates()
        .into_iter()
        .flat_map(placeholders)
        .collect()
}

/// PRD 0034 re-validation V-10: the unit words a figure is written with in
/// the catalogs (abbreviations, and the unit words of the languages that
/// have none). A plain space between a `{placeholder}` and one of them lets
/// the line wrap between the number and its unit ("in 3" / "T 17 Std.").
const UNIT_WORDS: &[&str] = &[
    "s",
    "ms",
    "min",
    "h",
    "d",
    "j",
    "g",
    "T",
    "Sek.",
    "Min.",
    "Std.",
    "T.",
    "sa",
    "dk",
    "sn",
    "dtk",
    "mnt",
    "hr",
    "KiB",
    "MiB",
    "GiB",
    "KB",
    "MB",
    "GB",
    "pt",
    "pts",
    "Pkt.",
    "tok/min",
    "Tok./Min.",
    "%",
    "с",
    "мс",
    "мин",
    "ч",
    "д",
    "дн.",
    "giây",
    "phút",
    "giờ",
    "ngày",
    "सेकंड",
    "मिनट",
    "घंटे",
    "घं",
    "मि",
    "दिन",
];

/// The CJK units, which run on into the next word ("秒前", "日間"); they
/// count only after a placeholder in [`FIGURE_PLACEHOLDERS`], since the same
/// characters start ordinary words too ("{source} 日志").
const CJK_UNITS: &[&str] = &["秒", "分", "時間", "日", "小时", "天"];

/// The placeholder names that hold a number.
const FIGURE_PLACEHOLDERS: &[&str] = &[
    "count",
    "n",
    "m",
    "h",
    "d",
    "s",
    "ms",
    "seconds",
    "minutes",
    "hours",
    "days",
    "day",
    "max",
    "min",
    "pct",
    "percent",
    "p",
    "used",
    "threshold",
    "value",
    "size",
    "mib",
    "configured",
    "effective",
    "default",
    "timeout",
    "rate",
    "total",
    "heavy_percent",
    "light_percent",
    "start",
    "have",
];

/// PRD 0034 re-validation V-10: the placeholders of `text` that a plain
/// space separates from their unit (`{m} min` rather than `{m}\u{a0}min`),
/// plus a `{duration}` a plain space separates from the word after it
/// (`{duration} 후`), so a figure never wraps away from its unit. A unit
/// followed by another placeholder (`{metric} %{percent}`) is not one.
pub fn figure_breaks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("} ") {
        let Some(open) = text[..at].rfind('{') else {
            continue;
        };
        let name = &text[open + 1..at];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let token = text[at + 2..].split_whitespace().next().unwrap_or("");
        if token.is_empty() || token.contains('{') {
            continue;
        }
        let bare = token.trim_end_matches([',', ';', ':', ')', '»', '"', '”', '!', '?', '…']);
        let unit = [bare, bare.trim_end_matches('.')]
            .iter()
            .any(|word| UNIT_WORDS.contains(word))
            || (FIGURE_PLACEHOLDERS.contains(&name)
                && CJK_UNITS.iter().any(|cjk| bare.starts_with(cjk)));
        let worded = name == "duration" && token.chars().next().is_some_and(char::is_alphabetic);
        if unit || worded {
            out.push(name);
        }
    }
    out
}

/// The `_meta` keys a built-in catalog may carry (PRD 0035 RF11.5, RF11.6).
pub const META_KEYS: &[&str] = &[
    "name",
    "status",
    "reviewed",
    "machine_reviewed",
    "source",
    "pending_review",
];

fn is_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// PRD 0035 RF11.5/RF11.6: the shape of `_meta` beyond `name`: `status`,
/// `machine_reviewed` and `source` are non-empty texts (the date one an ISO
/// date); `reviewed` is `false` or a list of `{ handle, date, scope }` with
/// `scope` `critical` or `full`; `pending_review` lists ids of the catalog
/// itself, each once. Unknown keys are refused so a typo is not silently
/// ignored.
fn meta_problems(code: &str, catalog: &Catalog) -> Vec<String> {
    use serde_json::Value;
    let mut out = Vec::new();
    for (key, value) in catalog.meta() {
        let text = value.as_str().filter(|text| !text.trim().is_empty());
        match key.as_str() {
            "name" => {}
            "status" | "source" if text.is_none() => {
                out.push(format!("{code}: _meta.{key} must be a non-empty text"));
            }
            "machine_reviewed" if !text.is_some_and(is_iso_date) => {
                out.push(format!(
                    "{code}: _meta.machine_reviewed must be a YYYY-MM-DD date"
                ));
            }
            "status" | "source" | "machine_reviewed" => {}
            "reviewed" => match value {
                Value::Bool(false) => {}
                Value::Array(entries) if !entries.is_empty() => {
                    for entry in entries {
                        let field = |name: &str| entry.get(name).and_then(Value::as_str);
                        let ok = field("handle").is_some_and(|h| !h.trim().is_empty())
                            && field("date").is_some_and(is_iso_date)
                            && matches!(field("scope"), Some("critical" | "full"));
                        if !ok {
                            out.push(format!(
                                "{code}: _meta.reviewed entries need a handle, a YYYY-MM-DD date and the scope critical or full"
                            ));
                        }
                    }
                }
                _ => out.push(format!(
                    "{code}: _meta.reviewed must be false or a list of reviews"
                )),
            },
            "pending_review" => match value {
                Value::Array(ids) => {
                    let mut seen = BTreeSet::new();
                    for id in ids {
                        match id.as_str() {
                            Some(id) if !catalog.contains_key(id) => out.push(format!(
                                "{code}: _meta.pending_review names {id}, which the catalog lacks"
                            )),
                            Some(id) if !seen.insert(id) => {
                                out.push(format!("{code}: _meta.pending_review names {id} twice"))
                            }
                            Some(_) => {}
                            None => out.push(format!("{code}: _meta.pending_review must list ids")),
                        }
                    }
                }
                _ => out.push(format!(
                    "{code}: _meta.pending_review must be a list of ids"
                )),
            },
            other => out.push(format!("{code}: _meta.{other} is not one of {META_KEYS:?}")),
        }
    }
    out
}

/// The ids `_meta.pending_review` of `catalog` lists (RF11.6).
pub fn pending_review(catalog: &Catalog) -> Vec<&str> {
    catalog
        .meta()
        .get("pending_review")
        .and_then(|value| value.as_array())
        .map(|ids| ids.iter().filter_map(|id| id.as_str()).collect())
        .unwrap_or_default()
}

/// Whether `c` has no business in a catalog text (PRD 0034 re-validation
/// S-04): a control character other than a line break, a bidi override or
/// isolate ([`super::is_disguising`]), a bidi mark (U+200E, U+200F, U+061C)
/// or a zero-width or invisible character (U+200B–U+200D, U+2060–U+2064,
/// U+FEFF). Each can reorder or hide what a terminal, a notice or a page
/// shows. `char::is_control` alone sees only category Cc, not these Cf ones.
pub fn is_hidden_or_directional(c: char) -> bool {
    c != '\n'
        && (super::is_disguising(c)
            || matches!(
                c,
                '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{061C}'
            ))
}

/// HTML element names a catalog text never carries. A bare `<word>` that is
/// not one of these stays allowed: catalogs write example values that way
/// (`<token>`, `<user_id>`, `<区域>`).
const HTML_ELEMENTS: &[&str] = &[
    "a", "abbr", "audio", "b", "body", "br", "button", "div", "em", "embed", "font", "form", "hr",
    "html", "i", "iframe", "img", "input", "li", "link", "math", "meta", "object", "ol", "p",
    "pre", "s", "script", "small", "span", "strong", "style", "sub", "sup", "svg", "table", "td",
    "textarea", "tr", "u", "ul", "video",
];

/// Whether `text` holds markup in absolute terms, whatever its source holds:
/// a closing tag, a comment or declaration (`</`, `<!`, `<?`), an HTML
/// element (`<b>`, `<img …>`), any tag with an attribute (`<x y=…>`), or a
/// character reference (`&amp;`, `&#…`).
pub fn has_markup(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        match c {
            '<' => {
                if matches!(next, Some('/' | '!' | '?')) {
                    return true;
                }
                let name: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
                    continue;
                }
                let after = &chars[i + 1 + name.len()..];
                let ends_tag = matches!(after.first(), None | Some('>' | '/'))
                    || after.first().is_some_and(|c| c.is_whitespace());
                if ends_tag && HTML_ELEMENTS.contains(&name.to_ascii_lowercase().as_str()) {
                    return true;
                }
                // `<x attr=` (an attribute), whatever the element.
                if after.first().is_some_and(|c| c.is_whitespace()) {
                    let rest: String = after.iter().skip_while(|c| c.is_whitespace()).collect();
                    let attr: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .collect();
                    if !attr.is_empty() && rest[attr.len()..].trim_start().starts_with('=') {
                        return true;
                    }
                }
            }
            '&' => {
                if next == Some('#') {
                    return true;
                }
                let name: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if !name.is_empty() && chars.get(i + 1 + name.len()) == Some(&';') {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// How many of the markup characters `<`, `>` and `&` `text` holds, each
/// counted on its own.
fn markup_counts(text: &str) -> [usize; 3] {
    let mut counts = [0; 3];
    for c in text.chars() {
        match c {
            '<' => counts[0] += 1,
            '>' => counts[1] += 1,
            '&' => counts[2] += 1,
            _ => {}
        }
    }
    counts
}

/// Every problem of `catalog` (the language of `row`) against `reference`
/// (the source catalog, or the first translated one where the source lives
/// in code). Empty when the catalog is in parity.
pub fn problems(
    row: &LocaleRow,
    catalog: &Catalog,
    reference: &Catalog,
    allow: Allowances<'_>,
) -> Vec<String> {
    let code = row.code;
    let mut out = Vec::new();
    match catalog.meta().get("name").and_then(|name| name.as_str()) {
        Some(name) if name == row.endonym => {}
        other => out.push(format!(
            "{code}: _meta.name is {other:?}, the registry endonym is {:?}",
            row.endonym
        )),
    }
    out.extend(meta_problems(code, catalog));
    for id in reference.keys() {
        if !catalog.contains_key(id) && !allow.missing.contains(&id.as_str()) {
            out.push(format!("{code}: lacks {id}"));
        }
    }
    for (id, message) in catalog.iter() {
        let Some(source) = reference.get(id) else {
            if !allow.extra.contains(&id.as_str()) {
                out.push(format!("{code}: has {id}, which the reference lacks"));
            }
            continue;
        };
        if !allow.literal_braces.contains(&id.as_str())
            && message_placeholders(message) != message_placeholders(source)
        {
            out.push(format!("{code}: placeholders of {id} differ"));
        }
        // S-04: a translation adds no `<`, `>` or `&` beyond the most any
        // template of its source holds, counted per character, so an id
        // whose source shows `<token>` cannot smuggle `<img src=x>`.
        let source_counts = source
            .templates()
            .iter()
            .map(|text| markup_counts(text))
            .fold([0; 3], |a, b| {
                [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]
            });
        if message.templates().iter().any(|text| {
            let counts = markup_counts(text);
            (0..3).any(|i| counts[i] > source_counts[i])
        }) {
            out.push(format!("{code}: {id} carries markup its source lacks"));
        }
        match (message, source) {
            (Message::Plural(forms), Message::Plural(_)) => {
                let have: BTreeSet<&str> = forms.keys().map(String::as_str).collect();
                let want: BTreeSet<&str> = row.plural_categories().iter().copied().collect();
                if have != want {
                    out.push(format!(
                        "{code}: plural {id} has {have:?}, its rule needs {want:?}"
                    ));
                }
            }
            (Message::Text(_), Message::Text(_)) => {}
            _ => out.push(format!(
                "{code}: {id} is a plural in one catalog and a text in the other"
            )),
        }
        for text in message.templates() {
            if text.trim().is_empty() && !source.templates().iter().all(|t| t.trim().is_empty()) {
                out.push(format!("{code}: {id} is empty"));
            }
            if text.chars().any(|c| c.is_control() && c != '\n') {
                out.push(format!("{code}: {id} has a control character"));
            }
            for name in figure_breaks(text) {
                out.push(format!(
                    "{code}: {id} breaks {{{name}}} from its unit with a plain space; use U+00A0"
                ));
            }
            if let Some(c) = text
                .chars()
                .find(|c| !c.is_control() && is_hidden_or_directional(*c))
            {
                out.push(format!(
                    "{code}: {id} has the direction or zero-width character U+{:04X}",
                    c as u32
                ));
            }
            if has_markup(text) {
                out.push(format!("{code}: {id} carries markup"));
            }
            // A cheap NFC proxy (no normalization crate in the tree): a
            // Latin or Cyrillic text stores precomposed letters, so a
            // combining diacritic (U+0300..U+036F) means a decomposed form.
            if matches!(row.script, "Latn" | "Cyrl")
                && text.chars().any(|c| ('\u{300}'..='\u{36f}').contains(&c))
            {
                out.push(format!("{code}: {id} is not NFC"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::locale_row;

    /// PRD 0034 re-validation S-04: direction, zero-width and markup
    /// characters are refused in every catalog text, the reference too.
    #[test]
    fn hidden_characters_and_markup_are_refused_in_absolute_terms() {
        let row = locale_row("de").unwrap();
        let reference = Catalog::parse(
            r#"{"_meta":{"name":"English"},"a":"Use <token>","b":"Data & privacy","c":"x"}"#,
        )
        .unwrap();
        let check = |a: &str, b: &str, c: &str| {
            let raw = serde_json::json!({"_meta": {"name": "Deutsch"}, "a": a, "b": b, "c": c});
            let catalog = Catalog::parse(&raw.to_string()).unwrap();
            problems(row, &catalog, &reference, Default::default())
        };
        let fine = check("Nutze <Token>", "Daten & Datenschutz", "x\ny");
        assert!(fine.is_empty(), "{fine:?}");
        for (c, why) in [
            ('\u{202E}', "U+202E"),
            ('\u{2066}', "U+2066"),
            ('\u{200B}', "U+200B"),
            ('\u{200F}', "U+200F"),
            ('\u{2060}', "U+2060"),
            ('\u{FEFF}', "U+FEFF"),
            ('\u{061C}', "U+061C"),
        ] {
            let found = check("Nutze <Token>", "Daten & Datenschutz", &format!("x{c}y"));
            assert!(found.iter().any(|p| p.contains(why)), "{why}: {found:?}");
        }
        // The source shows `<token>`, yet a tag is refused, and so is any
        // `<`, `>` or `&` beyond the source's own.
        for (a, b, c) in [
            ("<img src=x> <Token>", "Daten & Datenschutz", "x"),
            ("Nutze <b>", "Daten & Datenschutz", "x"),
            ("Nutze <Token>", "Daten &amp; Datenschutz", "x"),
            ("Nutze <Token>", "Daten & Datenschutz", "<x>"),
            ("Nutze <Token> <x>", "Daten & Datenschutz", "x"),
        ] {
            let found = check(a, b, c);
            assert!(
                found.iter().any(|p| p.contains("markup")),
                "{a} / {b} / {c}: {found:?}"
            );
        }
        // The reference checked against itself is held to the same rules.
        let bad = Catalog::parse(r#"{"_meta":{"name":"Deutsch"},"a":"<a href=x>y</a>"}"#).unwrap();
        let found = problems(row, &bad, &bad, Default::default());
        assert!(found.iter().any(|p| p.contains("markup")), "{found:?}");
    }

    /// PRD 0034 re-validation V-10: a figure is held to its unit by a
    /// no-break space; a word or another placeholder after it is not a unit.
    #[test]
    fn a_plain_space_between_a_figure_and_its_unit_is_refused() {
        for (text, want) in [
            ("{m} min", vec!["m"]),
            ("{h} Std. {m} Min.", vec!["h", "m"]),
            ("vor {n} T.", vec!["n"]),
            ("{n} дн. назад", vec!["n"]),
            ("{seconds} с\"", vec!["seconds"]),
            ("({heavy_percent} %)", vec!["heavy_percent"]),
            ("{n} 秒前", vec!["n"]),
            ("{days} 日間の履歴", vec!["days"]),
            ("{duration} 후", vec!["duration"]),
            ("{max} MiB:", vec!["max"]),
        ] {
            assert_eq!(figure_breaks(text), want, "{text}");
        }
        for text in [
            "{m}\u{a0}min",
            "{h}h {m}m",
            "in {duration}",
            "{metric} %{percent}",
            "{source} 日志",
            "{name} 分成了多个部分",
            "{provider} hat {count} Nutzer",
            "{count} days at the limit",
            "{n} d'abord",
        ] {
            assert!(figure_breaks(text).is_empty(), "{text}");
        }
        let row = locale_row("de").unwrap();
        let reference = Catalog::parse(r#"{"_meta":{"name":"English"},"a":"{m} min"}"#).unwrap();
        let bad = Catalog::parse(r#"{"_meta":{"name":"Deutsch"},"a":"{m} Min."}"#).unwrap();
        let found = problems(row, &bad, &reference, Default::default());
        assert!(found.iter().any(|p| p.contains("U+00A0")), "{found:?}");
        let good =
            Catalog::parse("{\"_meta\":{\"name\":\"Deutsch\"},\"a\":\"{m}\u{a0}Min.\"}").unwrap();
        assert!(problems(row, &good, &reference, Default::default()).is_empty());
    }

    #[test]
    fn has_markup_tells_tags_from_example_values() {
        for text in [
            "<token>",
            "<user_id>",
            "<区域>",
            "< {amount}",
            "a > b",
            "R&D",
            "x & y;",
        ] {
            assert!(!has_markup(text), "{text}");
        }
        for text in [
            "<b>x",
            "</x>",
            "<!-- x -->",
            "<?x?>",
            "<IMG SRC=x>",
            "<x onclick=y>",
            "<br/>",
            "&lt;",
            "&#60;",
        ] {
            assert!(has_markup(text), "{text}");
        }
    }
}
