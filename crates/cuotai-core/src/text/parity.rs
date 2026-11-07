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
