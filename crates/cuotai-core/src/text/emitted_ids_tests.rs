//! PRD 0034 RF4.1 (re-validation R-01): every catalog id the core emits
//! exists in every core catalog.
//!
//! Parity compares the catalogs with each other, so an id written in code
//! and missing from every catalog would pass it. This test reads the
//! core's non-test sources and collects the ids written as string literals
//! where a text is built:
//!
//! - `Localized::{fixed, with_param, with_params, plural}` (and a local
//!   alias such as `Localized as L10n`);
//! - `Notes::push` and `Notes::push_params` (`.push("id", ...)`);
//! - a file's own helpers that take `id: &'static str` and return a
//!   `Localized` (`fn note(id: &'static str, ...) -> Localized`).
//!
//! An id built at run time (`format!("access.summary.{slug}")`) cannot be
//! read here: its literal prefix must still name at least one id of every
//! catalog. Ids chosen by a `match` and passed in a variable are covered by
//! the per-module tests. The English source is the fallback written at the
//! call site; a fixed text with a literal fallback must have the same
//! placeholders in every catalog. [`NOT_IN_CATALOGS`] lists the ids that
//! are deliberately absent.

use super::{catalog_locales, core_catalog, Message};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Ids the core emits on purpose without a catalog entry: their text is
/// the fallback in every language.
const NOT_IN_CATALOGS: [&str; 1] = [
    // A raw adapter label shown as written (PRD 0034 RF2).
    "metric.verbatim",
];

/// A source file with comments removed, and `#[cfg(test)]` items removed
/// from it, plus the paths of the modules its `#[cfg(test)] mod name;`
/// declarations load (test-only files).
struct Stripped {
    code: String,
    test_modules: Vec<PathBuf>,
}

/// Length in bytes of the string or char literal that starts at `at`
/// (`"…"`, `r"…"`, `r#"…"#`, `b"…"`, `'x'`, `'\n'`), or `None` if none
/// starts there (a lifetime such as `'a` is not a literal).
fn literal_len(src: &[u8], at: usize) -> Option<usize> {
    let rest = &src[at..];
    let mut start = 0;
    if rest.first() == Some(&b'b') {
        start = 1;
    }
    if rest.get(start) == Some(&b'r') {
        let hashes = rest[start + 1..].iter().take_while(|c| **c == b'#').count();
        if rest.get(start + 1 + hashes) == Some(&b'"') {
            let body = start + 2 + hashes;
            let close: Vec<u8> = std::iter::once(b'"')
                .chain(std::iter::repeat_n(b'#', hashes))
                .collect();
            let end = rest[body..]
                .windows(close.len())
                .position(|w| w == close.as_slice())?;
            return Some(body + end + close.len());
        }
        return None;
    }
    match rest.get(start) {
        Some(b'"') => {
            let mut i = start + 1;
            while i < rest.len() {
                match rest[i] {
                    b'\\' => i += 2,
                    b'"' => return Some(i + 1),
                    _ => i += 1,
                }
            }
            None
        }
        Some(b'\'') => {
            // 'x', '\n', '\'', '\u{1F600}', '{', but not the lifetime 'a.
            if rest.get(start + 1) == Some(&b'\\') {
                let end = rest.get(start + 3..)?.iter().position(|c| *c == b'\'')?;
                return Some(start + 3 + end + 1);
            }
            let tail = rest.get(start + 1..)?;
            let n = (1..=tail.len().min(4)).find(|n| std::str::from_utf8(&tail[..*n]).is_ok())?;
            let after = start + 1 + n;
            (rest.get(after) == Some(&b'\'')).then_some(after + 1)
        }
        _ => None,
    }
}

/// Whether `at` starts a literal: not in the middle of an identifier
/// (`br"` and `r#"` are literals, `for"` cannot occur).
fn starts_literal(src: &[u8], at: usize) -> bool {
    let prev = at.checked_sub(1).map(|i| src[i]);
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    match src[at] {
        b'"' | b'\'' => true,
        b'r' | b'b' => !prev.is_some_and(ident),
        _ => false,
    }
}

/// Removes comments, then each item under a `cfg` attribute that names
/// `test` (but not `not(test)`): up to its `;` or its matching `}`.
fn strip(src: &str, file: &Path) -> Stripped {
    let bytes = src.as_bytes();
    // 1. Comments out, literals kept verbatim.
    let mut code = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < bytes.len() {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            code.push(b' ');
        } else if starts_literal(bytes, i) {
            match literal_len(bytes, i) {
                Some(len) => {
                    code.extend_from_slice(&bytes[i..i + len]);
                    i += len;
                }
                None => {
                    code.push(bytes[i]);
                    i += 1;
                }
            }
        } else {
            code.push(bytes[i]);
            i += 1;
        }
    }
    // 2. `#[cfg(...test...)]` items out.
    let attr = Regex::new(r"#\[cfg\(([^\]]*)\)\]").unwrap();
    let test_word = Regex::new(r"\btest\b").unwrap();
    let mod_decl = Regex::new(
        r#"^\s*(?:#\[path\s*=\s*"([^"]+)"\]\s*)?(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;"#,
    )
    .unwrap();
    let item_start = Regex::new(
        r"^(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:fn|async|unsafe|const|static|impl|mod|use|struct|enum|trait|type|extern|macro_rules|thread_local)\b",
    )
    .unwrap();
    let text = String::from_utf8(code).expect("UTF-8 source");
    let bytes = text.as_bytes();
    let dir = file.parent().unwrap();
    let stem = file.file_stem().unwrap().to_str().unwrap();
    let child_dir = if matches!(stem, "mod" | "lib" | "main") {
        dir.to_path_buf()
    } else {
        dir.join(stem)
    };
    let mut out = String::with_capacity(text.len());
    let mut test_modules = Vec::new();
    let mut copied = 0;
    for found in attr.captures_iter(&text) {
        let whole = found.get(0).unwrap();
        if whole.start() < copied {
            continue;
        }
        let cfg = &found[1];
        if !test_word.is_match(cfg) || cfg.contains("not(test") {
            continue;
        }
        out.push_str(&text[copied..whole.start()]);
        let after = whole.end();
        if let Some(decl) = mod_decl.captures(&text[after..]) {
            test_modules.push(match decl.get(1) {
                Some(path) => dir.join(path.as_str()),
                None => child_dir.join(format!("{}.rs", &decl[2])),
            });
        }
        // Skip to the item's `;`, past its matching `}`, or (a field, an
        // argument or a match arm) to its `,`; stop before a `}` that
        // closes the enclosing block.
        let head = text[after..].trim_start();
        let is_item = item_start.is_match(head);
        let mut i = after;
        let mut braces = 0usize;
        let mut parens = 0usize;
        while i < bytes.len() {
            if starts_literal(bytes, i) {
                if let Some(len) = literal_len(bytes, i) {
                    i += len;
                    continue;
                }
            }
            match bytes[i] {
                b'{' => braces += 1,
                b'}' if braces == 0 => break,
                b'}' => {
                    braces -= 1;
                    if braces == 0 && parens == 0 {
                        i += 1;
                        break;
                    }
                }
                b'(' | b'[' => parens += 1,
                b')' | b']' => parens = parens.saturating_sub(1),
                b';' if braces == 0 && parens == 0 => {
                    i += 1;
                    break;
                }
                b',' if braces == 0 && parens == 0 && !is_item => {
                    i += 1;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        copied = i;
    }
    out.push_str(&text[copied..]);
    Stripped {
        code: out,
        test_modules,
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// One id literal found in the sources, with where it was written.
#[derive(Debug)]
struct Use {
    at: String,
    /// The literal fallback of a fixed text, when the call writes one.
    fixed_fallback: Option<String>,
    plural: bool,
}

/// `(ids, prefixes)`: every literal id with its uses, and every literal
/// prefix of a `format!` id.
fn scan() -> (BTreeMap<String, Vec<Use>>, BTreeMap<String, String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();
    let mut stripped: BTreeMap<PathBuf, Stripped> = files
        .iter()
        .map(|file| {
            let src = std::fs::read_to_string(file).unwrap();
            (file.clone(), strip(&src, file))
        })
        .collect();
    // Test-only files: declared under `#[cfg(test)]`, named as tests, or
    // inside a test-only module's directory.
    let mut test_only: BTreeSet<PathBuf> = stripped
        .values()
        .flat_map(|s| s.test_modules.iter().cloned())
        .collect();
    for file in &files {
        let name = file.file_name().unwrap().to_str().unwrap();
        if name == "tests.rs" || name.ends_with("_tests.rs") {
            test_only.insert(file.clone());
        }
    }
    let dirs: Vec<PathBuf> = test_only.iter().map(|f| f.with_extension("")).collect();
    stripped
        .retain(|file, _| !test_only.contains(file) && !dirs.iter().any(|d| file.starts_with(d)));

    let alias = Regex::new(r"\bLocalized\s+as\s+(\w+)").unwrap();
    let helper = Regex::new(r"\bfn\s+(\w+)\s*\(\s*id\s*:\s*&'static\s+str\b[^)]*\)\s*->\s*(?:crate::text::)?Localized\b").unwrap();
    let prefix_re = Regex::new(
        r#"\b(?:Localized)::(?:fixed|with_param|with_params|plural)\(\s*format!\(\s*"([^"{]*)\{"#,
    )
    .unwrap();
    let notes = Regex::new(r#"\.(push|push_params)\(\s*"([^"]*)"\s*,"#).unwrap();
    let fixed_fallback = Regex::new(r#"^\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)"#).unwrap();
    let mut ids: BTreeMap<String, Vec<Use>> = BTreeMap::new();
    let mut prefixes = BTreeMap::new();
    for (file, s) in &stripped {
        let rel = file.strip_prefix(&root).unwrap().display().to_string();
        let mut ctors = vec!["Localized".to_owned()];
        ctors.extend(alias.captures_iter(&s.code).map(|c| c[1].to_owned()));
        let ctors = ctors.join("|");
        let mut patterns = vec![
            Regex::new(&format!(
                r#"\b(?:{ctors})::(fixed|with_param|with_params|plural)\(\s*"([^"]*)"\s*,"#
            ))
            .unwrap(),
            notes.clone(),
        ];
        for found in helper.captures_iter(&s.code) {
            let name = regex::escape(&found[1]);
            patterns
                .push(Regex::new(&format!(r#"(?:^|[^\w.:])({name})\(\s*"([^"]*)"\s*,"#)).unwrap());
        }
        for pattern in &patterns {
            for found in pattern.captures_iter(&s.code) {
                let call = found.get(1).unwrap();
                let line = s.code[..call.start()].matches('\n').count() + 1;
                let kind = call.as_str();
                let rest = &s.code[found.get(0).unwrap().end()..];
                let fallback = matches!(kind, "fixed" | "push")
                    .then(|| fixed_fallback.captures(rest).map(|c| c[1].to_owned()))
                    .flatten();
                ids.entry(found[2].to_owned()).or_default().push(Use {
                    at: format!("{rel}:{line}"),
                    fixed_fallback: fallback,
                    plural: kind == "plural",
                });
            }
        }
        for found in prefix_re.captures_iter(&s.code) {
            let line = s.code[..found.get(0).unwrap().start()]
                .matches('\n')
                .count()
                + 1;
            prefixes.insert(found[1].to_owned(), format!("{rel}:{line}"));
        }
    }
    (ids, prefixes)
}

fn placeholders(text: &str) -> BTreeSet<&str> {
    static NAME: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"\{([A-Za-z0-9_]+)\}").unwrap());
    NAME.captures_iter(text)
        .map(|c| c.get(1).unwrap().as_str())
        .collect()
}

#[test]
fn every_id_the_core_writes_is_in_every_core_catalog() {
    let (ids, prefixes) = scan();
    // A scan that found nothing would pass vacuously.
    assert!(ids.len() > 500, "only {} literal ids found", ids.len());
    assert!(
        prefixes.len() > 10,
        "only {} id prefixes found",
        prefixes.len()
    );
    // Each way of writing an id is seen: a constructor, a note, a helper
    // (`doctor::fixed`, `tray::wrap`, `channels::url_error`) and a
    // `format!` prefix; ids written only in test code are not.
    for seen in [
        "remove.kept.declined",
        "doctor.config.missing",
        "tray.item.stale",
        "channels.error.url_empty",
    ] {
        assert!(ids.contains_key(seen), "{seen} not found by the scan");
    }
    assert!(prefixes.contains_key("access.summary."));
    for test_only in [
        "note.no.such.id",
        "test.seeded",
        "oauth_error.test.keychain_unavailable",
    ] {
        assert!(
            !ids.contains_key(test_only),
            "{test_only} read from test code"
        );
    }
    let shape = Regex::new(r"^[a-z][a-z0-9_]*(\.[A-Za-z0-9_\-]+)+$").unwrap();
    let mut problems = Vec::new();
    for (id, uses) in &ids {
        if !shape.is_match(id) {
            problems.push(format!("{}: {id:?} is not an id", uses[0].at));
        }
    }
    for allowed in NOT_IN_CATALOGS {
        assert!(ids.contains_key(allowed), "{allowed} is no longer written");
    }
    let locales = catalog_locales();
    assert!(!locales.is_empty());
    for locale in locales {
        let catalog = core_catalog(locale).unwrap();
        for (id, uses) in &ids {
            if NOT_IN_CATALOGS.contains(&id.as_str()) {
                if catalog.get(id.as_str()).is_some() {
                    problems.push(format!("{locale}: {id} is listed as absent"));
                }
                continue;
            }
            let Some(message) = catalog.get(id.as_str()) else {
                problems.push(format!(
                    "{locale}: {id} missing (written at {})",
                    uses[0].at
                ));
                continue;
            };
            for used in uses {
                if used.plural && !matches!(message, Message::Plural(_)) {
                    problems.push(format!(
                        "{locale}: {id} is a plural at {} but not in the catalog",
                        used.at
                    ));
                }
                if let Some(english) = &used.fixed_fallback {
                    let want = placeholders(english);
                    for template in message.templates() {
                        if placeholders(template) != want {
                            problems.push(format!(
                                "{locale}: {id} placeholders differ from the English at {}",
                                used.at
                            ));
                        }
                    }
                }
            }
        }
        for (prefix, at) in &prefixes {
            if !catalog.keys().any(|key| key.starts_with(prefix.as_str())) {
                problems.push(format!("{locale}: no id starts with {prefix:?} ({at})"));
            }
        }
    }
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The scanner itself: comments and test items are left out, literals and
/// lifetimes do not confuse the brace matching.
#[test]
fn the_id_scanner_skips_comments_and_test_code() {
    let src = r##"
// Localized::fixed("comment.line", "x")
/* Localized::fixed("comment.block", "x") */
fn kept<'a>(s: &'a str) -> char { let _ = "{"; let _ = r#"}"#; '{' }
#[cfg(test)]
mod tests { fn f() { Localized::fixed("test.only", "}"); } }
#[cfg(all(test, unix))]
fn unix_test() { Localized::fixed("test.unix", "x"); }
#[cfg(not(test))]
fn real() { Localized::fixed("real.id", "x"); }
#[cfg(test)]
#[path = "x/y_check.rs"]
mod y;
#[cfg(test)]
mod z;
fn after() { Localized::fixed("after.id", "x"); }
"##;
    let stripped = strip(src, Path::new("/src/a.rs"));
    let code = &stripped.code;
    for gone in ["comment.line", "comment.block", "test.only", "test.unix"] {
        assert!(!code.contains(gone), "{gone} kept:\n{code}");
    }
    for kept in ["real.id", "after.id", "fn kept<'a>", "'{'", "r#\"}\"#"] {
        assert!(code.contains(kept), "{kept} lost:\n{code}");
    }
    assert_eq!(
        stripped.test_modules,
        [
            PathBuf::from("/src/x/y_check.rs"),
            PathBuf::from("/src/a/z.rs")
        ]
    );
}
