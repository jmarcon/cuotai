//! PRD 0051 RF1/RF2: the two readers of `cuotai.toml` and their redacted
//! diagnostics.
//!
//! - [`ConfigParseMode::Tolerant`] is the runtime reader ([`super::ConfigFile`]):
//!   an `[[accounts]]` table with a provider slug or a key this release does
//!   not know is set aside as a [`ConfigAccountIssue`] (the other accounts
//!   keep working), and an unknown top-level key or table becomes a
//!   [`ConfigWarning`]. Syntax errors, duplicate keys and wrong types still
//!   fail the whole document.
//! - [`ConfigParseMode::Strict`] rejects every unknown provider and key; it is
//!   what `cuotai --check-config` and the editors' round trip use.
//!
//! Every error is a [`crate::ConfigError::Parse`] with a one-based line and
//! column and, when the position falls on a key or a value, its
//! [`ConfigPath`] (`accounts[2].provider`). Neither ever carries the value,
//! so a diagnostic is safe to paste into an issue.

use std::fmt;
use std::ops::Range;

use serde::Serialize;

use crate::text::{params, Localized, ParamValue};
use crate::{Config, ConfigError};

/// How strictly [`parse`] treats syntax this release does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigParseMode {
    /// Runtime reading: unknown accounts and keys are isolated.
    Tolerant,
    /// Authoring and `--check-config`: anything unknown is an error.
    Strict,
}

/// The top-level keys and tables this release understands.
const KNOWN_TOP_LEVEL: [&str; 7] = [
    "refresh_seconds",
    "concurrency",
    "timeout_seconds",
    "live_sessions",
    "accounts",
    "groups",
    "updates",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Key(String),
    Index(usize),
}

/// The location of a key in the document, such as `accounts[2].provider`.
/// It names keys and array positions only, never a value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigPath(Vec<Segment>);

impl ConfigPath {
    fn key(mut self, key: &str) -> Self {
        self.0.push(Segment::Key(key.to_owned()));
        self
    }

    fn index(mut self, index: usize) -> Self {
        self.0.push(Segment::Index(index));
        self
    }

    /// Whether the path is an account's `id` (`accounts[N].id`).
    fn is_account_id(&self) -> bool {
        matches!(
            self.0.as_slice(),
            [Segment::Key(accounts), Segment::Index(_), Segment::Key(id)]
                if accounts == "accounts" && id == "id"
        )
    }

    /// Build a path from its keys and indices, for tests and callers that
    /// already know the location.
    pub fn of(parts: &[&str]) -> Self {
        let mut path = Self::default();
        for part in parts {
            path = match part.parse::<usize>() {
                Ok(index) => path.index(index),
                Err(_) => path.key(part),
            };
        }
        path
    }
}

fn bare_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl fmt::Display for ConfigPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, segment) in self.0.iter().enumerate() {
            match segment {
                Segment::Index(index) => write!(f, "[{index}]")?,
                Segment::Key(key) => {
                    if position > 0 {
                        f.write_str(".")?;
                    }
                    if bare_key(key) {
                        f.write_str(key)?;
                    } else {
                        // A quoted key, escaped as TOML would; still a name.
                        write!(f, "{}", serde_json::Value::from(key.as_str()))?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Serialize for ConfigPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// One `[[accounts]]` table the tolerant reader set aside because it names
/// a provider or a key this release does not know. It keeps no value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigAccountIssue {
    /// Zero-based position of the table among `[[accounts]]`.
    pub index: usize,
    /// The provider slug when it is one this release knows; an unknown slug
    /// is not repeated (it may be a typo of something private).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The unknown key or the `provider` key.
    pub path: ConfigPath,
}

impl ConfigAccountIssue {
    pub fn l10n(&self) -> Localized {
        Localized::with_params(
            "config.account.requires_newer",
            params([
                ("number", ParamValue::from(self.index + 1)),
                ("path", ParamValue::from(self.path.to_string())),
            ]),
            "Account {number} uses {path}, which this version of Cuotai does not know; it is skipped and the other accounts still work. Update Cuotai, or run cuotai --check-config to find a typo.",
        )
    }
}

/// An unknown top-level key or table, kept in the file and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigWarning {
    pub path: ConfigPath,
}

impl ConfigWarning {
    pub fn l10n(&self) -> Localized {
        Localized::with_param(
            "config.warning.unknown_key",
            "path",
            self.path.to_string(),
            "{path} is not known to this version of Cuotai; it is ignored and kept when the file is saved.",
        )
    }
}

/// What [`parse`] read: the usable configuration (not yet validated) and,
/// in tolerant mode, what it set aside.
#[derive(Debug, Clone)]
pub struct ParsedConfig {
    pub config: Config,
    pub issues: Vec<ConfigAccountIssue>,
    pub warnings: Vec<ConfigWarning>,
}

/// PRD 0051 RF1.1: one `[[accounts]]` table in document order, usable or
/// set aside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigEntry<'a> {
    Ready(&'a crate::Account),
    NeedsConfiguration(&'a ConfigAccountIssue),
}

/// Interleave `accounts` (in order, without the set-aside ones) and
/// `issues` (each with its document index) back into document order.
pub fn entries<'a>(
    accounts: &'a [crate::Account],
    issues: &'a [ConfigAccountIssue],
) -> Vec<ConfigEntry<'a>> {
    let mut out = Vec::with_capacity(accounts.len() + issues.len());
    let mut accounts = accounts.iter();
    let mut issues = issues.iter().peekable();
    loop {
        if let Some(issue) = issues.next_if(|issue| issue.index <= out.len()) {
            out.push(ConfigEntry::NeedsConfiguration(issue));
        } else if let Some(account) = accounts.next() {
            out.push(ConfigEntry::Ready(account));
        } else if let Some(issue) = issues.next() {
            out.push(ConfigEntry::NeedsConfiguration(issue));
        } else {
            return out;
        }
    }
}

impl ParsedConfig {
    /// Every account table in document order (PRD 0051 RF1.1).
    pub fn entries(&self) -> Vec<ConfigEntry<'_>> {
        entries(&self.config.accounts, &self.issues)
    }
}

/// The one-based line and column of byte `offset` in `text`.
pub(crate) fn position(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

fn parse_error(text: &str, span: Option<Range<usize>>, path: Option<ConfigPath>) -> ConfigError {
    let (line, column) = span.map_or((1, 1), |span| position(text, span.start));
    ConfigError::Parse { line, column, path }
}

fn contains(span: Option<Range<usize>>, offset: usize) -> bool {
    span.is_some_and(|span| span.start <= offset && offset < span.end.max(span.start + 1))
}

fn path_in_value(value: &toml_edit::Value, offset: usize, path: ConfigPath) -> ConfigPath {
    match value {
        toml_edit::Value::InlineTable(table) => {
            for (key, _) in table.iter() {
                if let Some((key_node, item)) = table.get_key_value(key) {
                    if contains(key_node.span(), offset) {
                        return path.key(key);
                    }
                    if contains(item.span(), offset) {
                        return match item.as_value() {
                            Some(value) => path_in_value(value, offset, path.key(key)),
                            None => path.key(key),
                        };
                    }
                }
            }
            path
        }
        toml_edit::Value::Array(array) => {
            for (index, item) in array.iter().enumerate() {
                if contains(item.span(), offset) {
                    return path_in_value(item, offset, path.index(index));
                }
            }
            path
        }
        _ => path,
    }
}

fn path_in_table(table: &toml_edit::Table, offset: usize, path: &ConfigPath) -> Option<ConfigPath> {
    for (key, item) in table.iter() {
        let Some((key_node, _)) = table.get_key_value(key) else {
            continue;
        };
        let here = path.clone().key(key);
        match item {
            toml_edit::Item::Table(inner) => {
                if let Some(found) = path_in_table(inner, offset, &here) {
                    return Some(found);
                }
                if contains(key_node.span(), offset) || contains(inner.span(), offset) {
                    return Some(here);
                }
            }
            toml_edit::Item::ArrayOfTables(tables) => {
                for (index, inner) in tables.iter().enumerate() {
                    let slot = here.clone().index(index);
                    if let Some(found) = path_in_table(inner, offset, &slot) {
                        return Some(found);
                    }
                    if contains(inner.span(), offset) {
                        return Some(slot);
                    }
                }
            }
            toml_edit::Item::Value(value) => {
                if contains(key_node.span(), offset) {
                    return Some(here);
                }
                if contains(value.span(), offset) {
                    return Some(path_in_value(value, offset, here));
                }
            }
            toml_edit::Item::None => {}
        }
    }
    None
}

/// The key or value at byte `offset` of the parsed document `root`.
fn path_at(root: &toml_edit::Table, offset: usize) -> Option<ConfigPath> {
    path_in_table(root, offset, &ConfigPath::default())
}

/// The path of `error` in `root`. An unknown key that opens a table array
/// (`[[accounts.floor]]`) is reported at the header of its first table;
/// the key itself is what the reader did not know.
fn error_path(root: &toml_edit::Table, error: &toml_edit::de::Error) -> Option<ConfigPath> {
    let mut path = path_at(root, error.span()?.start)?;
    if error.message().starts_with("unknown field") {
        while matches!(path.0.last(), Some(Segment::Index(_))) {
            path.0.pop();
        }
    }
    Some(path)
}

/// An error raised while reading a value that only a newer release could
/// mean: a key the struct does not know, or an enum value (a provider slug)
/// it does not list. Anything else (a wrong type, a missing field) is a
/// broken document in every release.
fn newer_syntax(error: &toml_edit::de::Error) -> bool {
    let message = error.message();
    message.starts_with("unknown field") || message.starts_with("unknown variant")
}

/// `table` as a document of its own, keeping the original spans so errors
/// still point into the user's file.
fn deserialize<T: serde::de::DeserializeOwned>(
    table: toml_edit::Table,
) -> Result<T, toml_edit::de::Error> {
    toml_edit::de::from_document(toml_edit::DocumentMut::from(table))
}

/// The keys of a `[[groups]]` table this release knows ([`crate::model::GroupDef`]).
pub(crate) const GROUP_KEYS: [&str; 4] = ["id", "label", "order", "members"];

/// Remove the key at `path` (`updates.channel`, `groups[0].name`) from
/// `root`. Returns false when the path does not end on a key of a table
/// below `root`.
fn remove_at(root: &mut toml_edit::Table, path: &ConfigPath) -> bool {
    enum Node<'a> {
        Item(&'a mut toml_edit::Item),
        Table(&'a mut toml_edit::Table),
        Value(&'a mut toml_edit::Value),
    }
    let Some((Segment::Key(last), parents)) = path.0.split_last() else {
        return false;
    };
    let Some((Segment::Key(first), rest)) = parents.split_first() else {
        return false;
    };
    let Some(mut node) = root.get_mut(first).map(Node::Item) else {
        return false;
    };
    for segment in rest {
        let next = match (node, segment) {
            (Node::Item(toml_edit::Item::ArrayOfTables(tables)), Segment::Index(index)) => {
                tables.get_mut(*index).map(Node::Table)
            }
            (Node::Item(toml_edit::Item::Table(table)) | Node::Table(table), Segment::Key(key)) => {
                table.get_mut(key).map(Node::Item)
            }
            (Node::Item(toml_edit::Item::Value(value)) | Node::Value(value), segment) => {
                match (value, segment) {
                    (toml_edit::Value::Array(array), Segment::Index(index)) => {
                        array.get_mut(*index).map(Node::Value)
                    }
                    (toml_edit::Value::InlineTable(table), Segment::Key(key)) => {
                        table.get_mut(key).map(Node::Value)
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(next) = next else {
            return false;
        };
        node = next;
    }
    match node {
        Node::Item(item) => item
            .as_table_like_mut()
            .is_some_and(|table| table.remove(last).is_some()),
        Node::Table(table) => table.remove(last).is_some(),
        Node::Value(toml_edit::Value::InlineTable(table)) => table.remove(last).is_some(),
        Node::Value(_) => false,
    }
}

/// PRD 0051 RF1.3: read the known top-level table `key` of `kept` as `T`.
/// In tolerant mode a key this release does not know inside it (and only
/// that: a wrong type or an unknown value still fails) is dropped from
/// `kept` and reported as a [`ConfigWarning`]; the document keeps it.
fn read_known_table<T: serde::de::DeserializeOwned>(
    kept: &mut toml_edit::Table,
    key: &str,
    mode: ConfigParseMode,
    root: &toml_edit::Table,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<(), toml_edit::de::Error> {
    let Some(item) = kept.get(key) else {
        return Ok(());
    };
    let mut table = toml_edit::Table::new();
    table.insert(key, item.clone());
    // Each pass removes one key, so the loop ends.
    loop {
        let error = match deserialize::<T>(table.clone()) {
            Ok(_) => return Ok(()),
            Err(error) => error,
        };
        if mode == ConfigParseMode::Strict || !error.message().starts_with("unknown field") {
            return Err(error);
        }
        let Some(path) = error_path(root, &error) else {
            return Err(error);
        };
        if !remove_at(&mut table, &path) || !remove_at(kept, &path) {
            return Err(error);
        }
        warnings.push(ConfigWarning { path });
    }
}

/// A `[[groups]]` table as this release reads it: keys it does not know
/// are left out (PRD 0051 RF1.3).
pub(crate) fn known_group(table: &toml_edit::Table) -> Option<crate::model::GroupDef> {
    let mut table = table.clone();
    table.retain(|key, _| GROUP_KEYS.contains(&key));
    toml_edit::de::from_str(&table.to_string()).ok()
}

/// Read `text` in `mode`. The result is not validated: run
/// [`Config::validate`] on it (as [`Config::from_toml`] and
/// [`super::ConfigFile`] do).
pub fn parse(text: &str, mode: ConfigParseMode) -> Result<ParsedConfig, ConfigError> {
    let document = toml_edit::ImDocument::parse(text)
        .map_err(|error| parse_error(text, error.span(), None))?;
    let root = document.as_table();
    let mut kept = root.clone();
    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    let error_at =
        |error: &toml_edit::de::Error| parse_error(text, error.span(), error_path(root, error));

    // PRD 0028 RF8.2: `[updates]` belongs to the CLI update notice; it is
    // validated here and left out of `Config`.
    #[derive(serde::Deserialize)]
    struct Updates {
        #[allow(dead_code)]
        updates: crate::update_check::UpdatesSettings,
    }
    read_known_table::<Updates>(&mut kept, "updates", mode, root, &mut warnings)
        .map_err(|error| error_at(&error))?;
    kept.remove("updates");
    #[derive(serde::Deserialize)]
    struct Groups {
        #[allow(dead_code)]
        groups: Vec<crate::model::GroupDef>,
    }
    read_known_table::<Groups>(&mut kept, "groups", mode, root, &mut warnings)
        .map_err(|error| error_at(&error))?;

    for (key, _) in root.iter() {
        if KNOWN_TOP_LEVEL.contains(&key) {
            continue;
        }
        let path = ConfigPath::default().key(key);
        match mode {
            ConfigParseMode::Strict => {
                let span = root.get_key_value(key).and_then(|(node, _)| node.span());
                return Err(parse_error(text, span, Some(path)));
            }
            ConfigParseMode::Tolerant => {
                kept.remove(key);
                warnings.push(ConfigWarning { path });
            }
        }
    }

    if mode == ConfigParseMode::Tolerant {
        if let Some(tables) = kept
            .get_mut("accounts")
            .and_then(toml_edit::Item::as_array_of_tables_mut)
        {
            let mut index = 0;
            let mut original = 0;
            while index < tables.len() {
                let table = tables.get(index).expect("index checked").clone();
                match deserialize::<crate::Account>(table.clone()) {
                    Err(error) if newer_syntax(&error) => {
                        let path = error_path(root, &error).unwrap_or_else(|| {
                            ConfigPath::default().key("accounts").index(original)
                        });
                        let provider = table
                            .get("provider")
                            .and_then(toml_edit::Item::as_str)
                            .filter(|slug| crate::Provider::from_slug(slug).is_some())
                            .map(str::to_owned);
                        issues.push(ConfigAccountIssue {
                            index: original,
                            provider,
                            path,
                        });
                        tables.remove(index);
                    }
                    _ => index += 1,
                }
                original += 1;
            }
        }
    }

    let config: Config = deserialize(kept).map_err(|error| error_at(&error))?;
    Ok(ParsedConfig {
        config,
        issues,
        warnings,
    })
}

/// The redacted note of a [`ConfigError::Parse`] (PRD 0051 RF2.2).
pub(crate) fn parse_note(line: usize, column: usize, path: Option<&ConfigPath>) -> Localized {
    match path {
        // PRD 0029 (V-10): a malformed id says what an id looks like.
        Some(path) if path.is_account_id() => Localized::with_params(
            "config.error.parse_account_id",
            params([
                ("line", ParamValue::from(line)),
                ("column", ParamValue::from(column)),
                ("path", ParamValue::from(path.to_string())),
            ]),
            "The configuration has an invalid {path} at line {line}, column {column}: an id is 4 to 40 lowercase letters, digits, _ or - (Cuotai writes acc_ followed by 12 characters).",
        ),
        Some(path) => Localized::with_params(
            "config.error.parse_key",
            params([
                ("line", ParamValue::from(line)),
                ("column", ParamValue::from(column)),
                ("path", ParamValue::from(path.to_string())),
            ]),
            "The configuration has an unknown or invalid {path} at line {line}, column {column}.",
        ),
        None => Localized::with_params(
            "config.error.parse",
            params([
                ("line", ParamValue::from(line)),
                ("column", ParamValue::from(column)),
            ]),
            "The configuration is not valid TOML at line {line}, column {column}.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_error_of(text: &str, mode: ConfigParseMode) -> (usize, usize, Option<String>) {
        match parse(text, mode) {
            Err(ConfigError::Parse { line, column, path }) => {
                (line, column, path.map(|p| p.to_string()))
            }
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    /// PRD 0029 (V-10): a malformed id names the accepted form.
    #[test]
    fn a_malformed_id_says_what_an_id_looks_like() {
        let text = "[[accounts]]\nid = \"Bad ID!\"\nprovider = \"groq\"\ncredential_env = \"K\"\n";
        let Err(ConfigError::Parse { line, column, path }) = parse(text, ConfigParseMode::Tolerant)
        else {
            panic!("a malformed id must not parse");
        };
        let note = parse_note(line, column, path.as_ref());
        assert_eq!(note.id, "config.error.parse_account_id");
        assert!(note.text().contains("accounts[0].id"), "{}", note.text());
        assert!(note.text().contains("4 to 40"), "{}", note.text());
        assert!(!note.text().contains("Bad ID"), "never the value");
        assert_eq!(
            parse_note(1, 1, Some(&ConfigPath::of(&["accounts", "0", "name"]))).id,
            "config.error.parse_key"
        );
    }

    const FOUR: &str = r#"
[[accounts]]
name = "one"
provider = "open_ai_api"
credential_env = "OPENAI_ADMIN_KEY"

[[accounts]]
name = "two"
provider = "provider-from-the-future"
credential_env = "SECRET_VALUE_NAME"

[[accounts]]
name = "three"
provider = "anthropic_api"
credential_env = "ANTHROPIC_ADMIN_KEY"
weekly_cap = "hidden-value"

[[accounts]]
name = "four"
provider = "groq"
credential_env = "GROQ_API_KEY"
"#;

    #[test]
    fn tolerant_reading_isolates_unknown_providers_and_keys_in_order() {
        let parsed = parse(FOUR, ConfigParseMode::Tolerant).unwrap();
        let names: Vec<_> = parsed
            .config
            .accounts
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, ["one", "four"]);
        let order: Vec<_> = parsed
            .entries()
            .iter()
            .map(|entry| match entry {
                ConfigEntry::Ready(account) => account.name.clone(),
                ConfigEntry::NeedsConfiguration(issue) => format!("#{}", issue.index),
            })
            .collect();
        assert_eq!(order, ["one", "#1", "#2", "four"], "document order");
        assert_eq!(parsed.issues.len(), 2);
        assert_eq!(parsed.issues[0].index, 1);
        assert_eq!(
            parsed.issues[0].provider, None,
            "an unknown slug is not echoed"
        );
        assert_eq!(parsed.issues[0].path.to_string(), "accounts[1].provider");
        assert_eq!(parsed.issues[1].index, 2);
        assert_eq!(parsed.issues[1].provider.as_deref(), Some("anthropic_api"));
        assert_eq!(parsed.issues[1].path.to_string(), "accounts[2].weekly_cap");
        let note = parsed.issues[0].l10n();
        assert_eq!(note.id, "config.account.requires_newer");
        assert!(parsed.config.validate().is_ok());
    }

    #[test]
    fn strict_reading_names_the_first_unknown_path_without_its_value() {
        let (line, column, path) = parse_error_of(FOUR, ConfigParseMode::Strict);
        assert_eq!(path.as_deref(), Some("accounts[1].provider"));
        assert_eq!((line, column), (9, 12));
        let error = parse(FOUR, ConfigParseMode::Strict).unwrap_err();
        let shown = format!("{error} {:?}", crate::edit_error::config_error_note(&error));
        for secret in [
            "provider-from-the-future",
            "SECRET_VALUE_NAME",
            "hidden-value",
        ] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }

    #[test]
    fn unknown_top_level_keys_warn_in_tolerant_mode_and_fail_in_strict_mode() {
        let text = "refresh_seconds = 60\nfuture = { a = 1 }\n[[accounts]]\nprovider = \"open_ai_api\"\ncredential_env = \"K\"\n\n[later]\nx = 2\n";
        let parsed = parse(text, ConfigParseMode::Tolerant).unwrap();
        let warned: Vec<_> = parsed.warnings.iter().map(|w| w.path.to_string()).collect();
        assert_eq!(warned, ["future", "later"]);
        assert_eq!(parsed.config.accounts.len(), 1);
        let (line, column, path) = parse_error_of(text, ConfigParseMode::Strict);
        assert_eq!((line, column, path.as_deref()), (2, 1, Some("future")));
    }

    #[test]
    fn syntax_and_type_errors_fail_in_both_modes_with_a_position() {
        for mode in [ConfigParseMode::Tolerant, ConfigParseMode::Strict] {
            let (line, column, path) = parse_error_of("refresh_seconds = 60\naccounts = [\n", mode);
            assert_eq!((line, path), (3, None), "{column}");
            let (line, column, path) = parse_error_of(
                "[[accounts]]\nprovider = \"open_ai_api\"\nrefresh_seconds = \"soon\"\n",
                mode,
            );
            assert_eq!(
                (line, column, path.as_deref()),
                (3, 19, Some("accounts[0].refresh_seconds"))
            );
            let (line, _, path) = parse_error_of(
                "refresh_seconds = 60\nrefresh_seconds = 90\naccounts = []\n",
                mode,
            );
            assert_eq!((line, path), (2, None), "a duplicate key is a syntax error");
        }
    }

    #[test]
    fn strict_reading_names_unknown_keys_of_updates_and_groups() {
        let text = "accounts = []\n[updates]\ncheck = true\nsurprise = 1\n";
        let (line, column, path) = parse_error_of(text, ConfigParseMode::Strict);
        assert_eq!(
            (line, column, path.as_deref()),
            (4, 1, Some("updates.surprise"))
        );
        let text =
            "accounts = []\n[[groups]]\nid = \"a\"\n\n[[groups]]\nid = \"b\"\nname = \"x\"\n";
        let (line, column, path) = parse_error_of(text, ConfigParseMode::Strict);
        assert_eq!(
            (line, column, path.as_deref()),
            (7, 1, Some("groups[1].name"))
        );
    }

    /// PRD 0051 RF1.3 (V-01): an unknown key inside `[updates]` or a
    /// `[[groups]]` table warns and sets nothing else aside.
    #[test]
    fn tolerant_reading_warns_on_unknown_keys_of_updates_and_groups() {
        let text = format!(
            "{FOUR}\n[updates]\ncheck = true\nchannel = \"beta\"\n\n[[groups]]\nid = \"coding\"\nname = \"Coding\"\nlabel = \"Code\"\n\n[[groups]]\nid = \"chat\"\n"
        );
        let parsed = parse(&text, ConfigParseMode::Tolerant).unwrap();
        parsed.config.validate().unwrap();
        let warned: Vec<_> = parsed.warnings.iter().map(|w| w.path.to_string()).collect();
        assert_eq!(warned, ["updates.channel", "groups[0].name"]);
        let groups: Vec<_> = parsed
            .config
            .groups
            .iter()
            .map(|g| (g.id.as_str(), g.label.as_deref()))
            .collect();
        assert_eq!(groups, [("coding", Some("Code")), ("chat", None)]);
        assert_eq!(
            parsed.config.accounts.len(),
            2,
            "the FOUR accounts as before"
        );
        assert_eq!(parsed.issues.len(), 2);
        // Four known accounts and the two unknown keys: four ready, two warnings.
        let text = format!(
            "{}\n[updates]\nchannel = \"beta\"\n\n[[groups]]\nid = \"coding\"\nname = \"Coding\"\n",
            FOUR.replace("provider-from-the-future", "mistral")
                .replace("weekly_cap = \"hidden-value\"\n", "")
        );
        let parsed = parse(&text, ConfigParseMode::Tolerant).unwrap();
        assert_eq!(parsed.config.accounts.len(), 4);
        assert!(parsed.issues.is_empty());
        assert_eq!(parsed.warnings.len(), 2);
        assert!(crate::update_check::settings_from_toml(&text) == Default::default());
    }

    #[test]
    fn inline_groups_and_updates_also_drop_only_the_unknown_key() {
        let text = "updates = { check = true, channel = \"beta\" }\ngroups = [{ id = \"a\", name = \"x\" }]\naccounts = []\n";
        let parsed = parse(text, ConfigParseMode::Tolerant).unwrap();
        let warned: Vec<_> = parsed.warnings.iter().map(|w| w.path.to_string()).collect();
        assert_eq!(warned, ["updates.channel", "groups[0].name"]);
        assert_eq!(parsed.config.groups[0].id, "a");
    }

    #[test]
    fn known_keys_with_a_wrong_type_still_fail_the_tolerant_reader() {
        for text in [
            "accounts = []\n[updates]\ncheck = \"yes\"\n",
            "accounts = []\n[[groups]]\nid = \"a\"\norder = \"first\"\nname = \"x\"\n",
        ] {
            assert!(parse(text, ConfigParseMode::Tolerant).is_err(), "{text}");
        }
    }

    #[test]
    fn the_known_group_keys_match_the_group_definition() {
        let group = crate::model::GroupDef {
            id: "a".into(),
            label: Some("A".into()),
            order: Some(1),
            members: vec!["m".into()],
        };
        let table = toml_edit::ser::to_document(&group).unwrap();
        let keys: Vec<_> = table.iter().map(|(key, _)| key).collect();
        assert_eq!(keys, GROUP_KEYS);
        let mut with_extra = table.into_table();
        with_extra.insert("name", toml_edit::value("x"));
        assert_eq!(known_group(&with_extra), Some(group));
    }

    /// PRD 0051 RF6.3: the proposed-key fixture is read tolerantly (known
    /// accounts ready, the others set aside in order) and the strict reader
    /// names its first unsupported path.
    #[test]
    fn the_proposed_key_fixture_passes_the_tolerant_reader() {
        let text =
            include_str!("../../tests/fixtures/compat/config/major-1-revision-1/proposed.toml");
        let parsed = parse(text, ConfigParseMode::Tolerant).unwrap();
        parsed.config.validate().unwrap();
        let ids: Vec<_> = parsed
            .config
            .accounts
            .iter()
            .map(|account| account.id().as_str().to_owned())
            .collect();
        assert_eq!(ids, ["work-groq", "personal-mistral"]);
        let issues: Vec<_> = parsed
            .issues
            .iter()
            .map(|issue| (issue.index, issue.path.to_string()))
            .collect();
        assert_eq!(
            issues,
            [
                (1, "accounts[1].base_url".to_owned()),
                (2, "accounts[2].provider".to_owned())
            ]
        );
        assert_eq!(parsed.issues[0].provider.as_deref(), Some("nine_router"));
        assert_eq!(parsed.config.groups.len(), 1);
        let (_, _, path) = parse_error_of(text, ConfigParseMode::Strict);
        assert_eq!(path.as_deref(), Some("accounts[1].base_url"));
    }

    /// PRD 0051 RF1.3/RF6.3: the revision-2 fixture (unknown keys inside
    /// `[updates]` and `[[groups]]`) keeps every account ready.
    #[test]
    fn the_revision_2_fixture_warns_and_keeps_every_account() {
        let text =
            include_str!("../../tests/fixtures/compat/config/major-1-revision-2/proposed.toml");
        let parsed = parse(text, ConfigParseMode::Tolerant).unwrap();
        parsed.config.validate().unwrap();
        assert_eq!(parsed.config.accounts.len(), 2);
        assert!(parsed.issues.is_empty());
        let warned: Vec<_> = parsed.warnings.iter().map(|w| w.path.to_string()).collect();
        assert_eq!(warned, ["updates.channel", "groups[0].name"]);
        assert_eq!(parsed.config.groups[0].label.as_deref(), Some("Coding"));
        assert!(crate::update_check::settings_from_toml(text).check);
        let (_, _, path) = parse_error_of(text, ConfigParseMode::Strict);
        assert_eq!(path.as_deref(), Some("updates.channel"));
    }

    #[test]
    fn a_path_quotes_keys_that_are_not_bare() {
        assert_eq!(
            ConfigPath::of(&["accounts", "3", "a b"]).to_string(),
            "accounts[3].\"a b\""
        );
    }

    #[test]
    fn positions_count_characters_not_bytes() {
        assert_eq!(position("é = 1\nñ", 7), (2, 1));
        assert_eq!(position("aé", 3), (1, 3));
        assert_eq!(position("aé", 2), (1, 2), "inside a character: its start");
    }
}
