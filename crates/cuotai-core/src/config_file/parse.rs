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
const KNOWN_TOP_LEVEL: [&str; 6] = [
    "refresh_seconds",
    "concurrency",
    "timeout_seconds",
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
    if let Some(updates) = kept.remove("updates") {
        let mut table = toml_edit::Table::new();
        table.insert("updates", updates);
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Updates {
            #[allow(dead_code)]
            updates: crate::update_check::UpdatesSettings,
        }
        deserialize::<Updates>(table).map_err(|error| error_at(&error))?;
    }

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
    fn nested_unknown_keys_and_the_updates_table_carry_their_path() {
        let text = "accounts = []\n[updates]\ncheck = true\nsurprise = 1\n";
        let (line, column, path) = parse_error_of(text, ConfigParseMode::Tolerant);
        assert_eq!(
            (line, column, path.as_deref()),
            (4, 1, Some("updates.surprise"))
        );
    }

    /// PRD 0051 RF6.3: the proposed-key fixture is read tolerantly (known
    /// accounts ready, the others set aside in order) and the strict reader
    /// names its first unsupported path.
    #[test]
    fn the_proposed_key_fixture_passes_the_tolerant_reader() {
        let text = include_str!("../../tests/fixtures/compat/config/proposed.toml");
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
