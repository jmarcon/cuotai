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

fn has_markup(message: &Message) -> bool {
    message
        .templates()
        .iter()
        .any(|text| text.contains('<') || text.contains('>'))
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
        if has_markup(message) && !has_markup(source) {
            out.push(format!("{code}: {id} carries markup"));
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
