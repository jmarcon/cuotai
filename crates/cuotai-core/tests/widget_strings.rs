//! PRD 0034: the macOS widget speaks every language the desktop app does.
//!
//! The widget extension keeps its own strings in
//! `apps/desktop/widgets/macos/Localizable.xcstrings` and declares its
//! languages in `Info.plist` (`CFBundleLocalizations`, Apple codes). This
//! test ties both to the language registry ([`cuotai_core::text::LOCALES`]):
//! every built-in language is declared and translated, nothing else is, every
//! translation keeps the source's format arguments (count, position and
//! type) and plural categories, and no value carries direction, zero-width or
//! markup characters.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cuotai_core::text::LOCALES;
use serde_json::Value;

fn widget_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/widgets/macos")
}

/// The Apple localization a registry code is shipped under in the widget.
fn apple_code(code: &str) -> &str {
    match code {
        "en-US" => "en",
        "zh-CN" => "zh-Hans",
        "hi-IN" => "hi",
        "id-ID" => "id",
        other => other,
    }
}

/// The registry row of an Apple localization.
fn registry_row(apple: &str) -> Option<&'static cuotai_core::text::LocaleRow> {
    LOCALES.iter().find(|row| apple_code(row.code) == apple)
}

fn catalog() -> Value {
    let path = widget_dir().join("Localizable.xcstrings");
    let raw = std::fs::read_to_string(&path).expect("the widget string catalog");
    serde_json::from_str(&raw).expect("the widget string catalog is JSON")
}

/// `CFBundleLocalizations` of the extension's `Info.plist`, in order.
fn plist_languages() -> Vec<String> {
    let raw = std::fs::read_to_string(widget_dir().join("Info.plist")).expect("Info.plist");
    let after = raw
        .split("<key>CFBundleLocalizations</key>")
        .nth(1)
        .expect("Info.plist declares CFBundleLocalizations");
    let array = after
        .split_once("</array>")
        .expect("CFBundleLocalizations is an array")
        .0;
    array
        .split("<string>")
        .skip(1)
        .map(|item| {
            item.split("</string>")
                .next()
                .unwrap_or("")
                .trim()
                .to_owned()
        })
        .collect()
}

#[test]
fn the_widget_declares_exactly_the_desktop_languages() {
    let declared = plist_languages();
    let unique: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), declared.len(), "a language is declared twice");
    let expected: BTreeSet<&str> = LOCALES
        .iter()
        .filter(|row| row.is_translated())
        .map(|row| apple_code(row.code))
        .collect();
    assert_eq!(
        unique, expected,
        "Info.plist CFBundleLocalizations must list every built-in language (Apple codes)"
    );
    assert_eq!(catalog()["sourceLanguage"], "en");
}

/// One format argument: its position and its conversion (`lld`, `@`), or
/// the substitution it stands for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Spec {
    Conversion(String),
    Substitution(String),
}

/// The format arguments of `text` (`%%` is a literal), positional ones at
/// their `n$` and the others in order, as Foundation numbers them.
fn specs(text: &str) -> Result<Vec<(Option<usize>, Spec)>, String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            continue;
        }
        let mut digits = String::new();
        while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
            digits.push(*d);
            chars.next();
        }
        let position = if chars.peek() == Some(&'$') {
            chars.next();
            Some(
                digits
                    .parse::<usize>()
                    .map_err(|_| format!("bad position in {text:?}"))?,
            )
        } else if digits.is_empty() {
            None
        } else {
            return Err(format!(
                "width digits are not used in widget strings: {text:?}"
            ));
        };
        if chars.peek() == Some(&'#') {
            chars.next();
            if chars.next() != Some('@') {
                return Err(format!("bad substitution in {text:?}"));
            }
            let name: String = chars.by_ref().take_while(|c| *c != '@').collect();
            out.push((position, Spec::Substitution(name)));
            continue;
        }
        let mut conversion = String::new();
        while let Some(l) = chars.peek().filter(|l| **l == 'l') {
            conversion.push(*l);
            chars.next();
        }
        match chars.next() {
            Some(end @ ('@' | 'd' | 'u' | 'f' | 's')) => conversion.push(end),
            other => return Err(format!("unexpected conversion {other:?} in {text:?}")),
        }
        out.push((position, Spec::Conversion(conversion)));
    }
    Ok(out)
}

/// The arguments a localization consumes, by position (1-based), with each
/// substitution resolved to its own argument and conversion.
fn arguments(text: &str, substitutions: Option<&Value>) -> Result<BTreeMap<usize, String>, String> {
    let mut map = BTreeMap::new();
    let mut next = 1;
    for (position, spec) in specs(text)? {
        let (index, conversion) = match spec {
            Spec::Conversion(conversion) => {
                let index = position.unwrap_or(next);
                (index, conversion)
            }
            Spec::Substitution(name) => {
                let sub = substitutions
                    .and_then(|subs| subs.get(&name))
                    .ok_or_else(|| format!("{text:?} names the missing substitution {name}"))?;
                let index = sub["argNum"]
                    .as_u64()
                    .ok_or("a substitution without argNum")? as usize;
                let conversion = sub["formatSpecifier"]
                    .as_str()
                    .ok_or("a substitution without formatSpecifier")?
                    .to_owned();
                (index, conversion)
            }
        };
        next = index + 1;
        if let Some(previous) = map.insert(index, conversion.clone()) {
            if previous != conversion {
                return Err(format!("argument {index} has two types in {text:?}"));
            }
        }
    }
    Ok(map)
}

/// Characters a widget string never carries: controls, direction marks and
/// isolates, zero-width characters, and markup.
fn disguising(text: &str) -> Option<char> {
    text.chars().find(|c| {
        c.is_control()
            || matches!(*c as u32,
                0x061C | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFEFF)
            || matches!(c, '<' | '>' | '&')
    })
}

/// Every value of one localization, with what it must keep: the plain or
/// plural values (with their arguments) and, for substitutions, each form.
struct Checked {
    problems: Vec<String>,
}

impl Checked {
    fn value(&mut self, at: &str, text: &str) {
        if text.trim().is_empty() {
            self.problems.push(format!("{at}: empty"));
        }
        if let Some(c) = disguising(text) {
            self.problems
                .push(format!("{at}: carries U+{:04X}", c as u32));
        }
    }

    /// The plural forms of `variations` cover the language's categories.
    fn plural(&mut self, at: &str, variations: &Value, language: &str) -> Vec<(String, String)> {
        let Some(plural) = variations.get("plural").and_then(Value::as_object) else {
            self.problems
                .push(format!("{at}: a variation that is not a plural"));
            return Vec::new();
        };
        let forms: BTreeSet<&str> = plural.keys().map(String::as_str).collect();
        if let Some(row) = registry_row(language) {
            for category in row.plural_categories() {
                if !forms.contains(category) {
                    self.problems
                        .push(format!("{at}: no `{category}` form for {language}"));
                }
            }
        }
        plural
            .iter()
            .map(|(form, unit)| {
                let text = unit["stringUnit"]["value"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                (form.clone(), text)
            })
            .collect()
    }

    /// The arguments of one localization of `key`, or nothing when it is
    /// malformed (and then a problem says why).
    fn localization(
        &mut self,
        key: &str,
        language: &str,
        loc: &Value,
    ) -> Option<BTreeMap<usize, String>> {
        let at = format!("{key:?} [{language}]");
        let substitutions = loc.get("substitutions");
        if let Some(subs) = substitutions.and_then(Value::as_object) {
            for (name, sub) in subs {
                let forms = self.plural(&format!("{at} %#@{name}@"), &sub["variations"], language);
                for (form, text) in forms {
                    let where_ = format!("{at} %#@{name}@ {form}");
                    self.value(&where_, &text);
                    if !text.contains("%arg") {
                        self.problems.push(format!("{where_}: no %arg"));
                    }
                }
            }
        }
        let texts: Vec<(String, String)> = if let Some(variations) = loc.get("variations") {
            self.plural(&at, variations, language)
        } else if let Some(text) = loc["stringUnit"]["value"].as_str() {
            vec![(String::new(), text.to_owned())]
        } else {
            self.problems.push(format!("{at}: no value"));
            return None;
        };
        let mut found: Option<BTreeMap<usize, String>> = None;
        for (form, text) in texts {
            let where_ = format!("{at} {form}");
            self.value(&where_, &text);
            match arguments(&text, substitutions) {
                Ok(args) => {
                    if let Some(previous) = &found {
                        if previous != &args {
                            self.problems
                                .push(format!("{where_}: arguments differ between plural forms"));
                        }
                    }
                    found = Some(args);
                }
                Err(problem) => self.problems.push(format!("{where_}: {problem}")),
            }
        }
        found
    }
}

#[test]
fn every_widget_string_is_in_every_language_with_the_same_arguments() {
    let catalog = catalog();
    let declared = plist_languages();
    let strings = catalog["strings"].as_object().expect("strings");
    assert!(!strings.is_empty());
    let mut checked = Checked {
        problems: Vec::new(),
    };
    for (key, entry) in strings {
        if entry.get("shouldTranslate") == Some(&Value::Bool(false)) {
            continue;
        }
        let localizations = entry
            .get("localizations")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for language in localizations.keys() {
            if !declared.iter().any(|d| d == language) {
                checked
                    .problems
                    .push(format!("{key:?}: {language} is not a declared language"));
            }
        }
        // The source: its English localization (a plural one), else the key.
        let source = match localizations.get("en") {
            Some(en) => checked.localization(key, "en", en),
            None => {
                checked.value(&format!("{key:?} [en]"), key);
                match arguments(key, None) {
                    Ok(args) => Some(args),
                    Err(problem) => {
                        checked.problems.push(format!("{key:?}: {problem}"));
                        None
                    }
                }
            }
        };
        for language in declared.iter().filter(|l| *l != "en") {
            let Some(loc) = localizations.get(language) else {
                checked
                    .problems
                    .push(format!("{key:?}: no {language} translation"));
                continue;
            };
            let args = checked.localization(key, language, loc);
            if let (Some(source), Some(args)) = (&source, &args) {
                if source != args {
                    checked.problems.push(format!(
                        "{key:?} [{language}]: arguments {args:?}, the source has {source:?}"
                    ));
                }
            }
        }
    }
    assert!(
        checked.problems.is_empty(),
        "widget strings:\n{}",
        checked.problems.join("\n")
    );
}

#[test]
fn the_argument_reader_sees_positions_types_and_substitutions() {
    let subs: Value = serde_json::json!({
        "samples": { "argNum": 1, "formatSpecifier": "lld" }
    });
    let args = arguments("%#@samples@, now %2$lld percent", Some(&subs)).unwrap();
    assert_eq!(args, BTreeMap::from([(1, "lld".into()), (2, "lld".into())]));
    let args = arguments("%@. %lld percent used, 100%% max", None).unwrap();
    assert_eq!(args, BTreeMap::from([(1, "@".into()), (2, "lld".into())]));
    assert_eq!(
        arguments("%2$@ · %1$@", None).unwrap(),
        BTreeMap::from([(1, "@".into()), (2, "@".into())])
    );
    assert!(arguments("%#@missing@", None).is_err());
    assert_eq!(disguising("a\u{202e}b"), Some('\u{202e}'));
    assert_eq!(disguising("<b>"), Some('<'));
    assert_eq!(disguising("compte affiché\u{a0};"), None);
}
