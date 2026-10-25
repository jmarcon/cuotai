//! Config file discovery, loading and atomic saving.
//!
//! Save policy: on save we resolve symlinks to the real destination, re-read the
//! current bytes and refuse to write if they differ from the snapshot taken at
//! load time (external edit, unexpected creation or deletion), then re-check
//! again immediately before the atomic rename (`persist_noclobber` when the
//! file should not exist yet) to shrink the TOCTOU window. Bytes are written
//! to a temporary file in the destination directory, fsynced, then atomically
//! renamed over the target, so the file is never partially overwritten. New
//! files get mode 0600 on Unix; existing files keep their permissions (and
//! permission failures abort the save). When the previously loaded document
//! parses, we edit it with `toml_edit`, matching `[[accounts]]` tables by
//! name and provider (plus extension id where applicable) for moves and
//! positionally for in-place edits so order, comments and formatting survive;
//! new accounts and brand new files are serialized with `toml_edit::ser`. The
//! rendered document is parsed back and compared to the requested config.
//! Only on success are `config`/`exists`/snapshot updated; on any failure
//! `self` is untouched.
//!
//! Discovery is controlled by [`Discovery`] over injectable [`Locations`]:
//! - [`Discovery::Cli`] (CLI/TUI): `--config`, `CUOTAI_CONFIG`, existing
//!   `./cuotai.toml`, then `<OS config dir>/cuotai/config.toml`.
//! - [`Discovery::Gui`]: `CUOTAI_CONFIG`, the file chosen in the app
//!   settings, then the OS path (never `./cuotai.toml`, whose meaning in a
//!   graphical application is unpredictable).
//!
//! With nothing found, both create the file in the OS path. The selected
//! rule is kept as [`PathSource`]; [`ConfigIdentity`] normalizes relative
//! paths and symlinks so per-config caches can be keyed by file.
//!
//! Errors are typed via [`crate::edit_error::EditError`] so a frontend can
//! distinguish a [`EditError::Conflict`] (the file changed on disk; offer
//! reload) from a [`EditError::Validation`] (the proposed document fails the
//! schema) from a [`EditError::Io`] (filesystem problem) from an
//! [`EditError::Unsupported`] (combination not accepted). Messages never
//! contain user-typed text, token material, or the raw TOML body.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::edit_error::{
    config_error_note, toml_int_in_range, validation, validation_note, EditError,
};
use crate::{Account, Config};

/// Discovery policy for [`ConfigFile::load_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovery {
    /// CLI/TUI: `--config` > `CUOTAI_CONFIG` > existing `./cuotai.toml` >
    /// `<OS config dir>/cuotai/config.toml`.
    Cli { config: Option<PathBuf> },
    /// GUI: `CUOTAI_CONFIG` > file chosen in the app settings > OS path.
    /// Never consults `./cuotai.toml`.
    Gui { chosen: Option<PathBuf> },
}

/// Process inputs consulted by discovery; injectable so tests never touch
/// the real environment or OS directories.
#[derive(Debug, Clone, Default)]
pub struct Locations {
    /// `CUOTAI_CONFIG` (an empty value counts as unset).
    pub env: Option<PathBuf>,
    /// Directory searched for `cuotai.toml` by [`Discovery::Cli`]; empty = cwd.
    pub cwd: PathBuf,
    /// Platform config directory (`dirs::config_dir()`).
    pub config_dir: Option<PathBuf>,
}

impl Locations {
    pub fn from_process() -> Self {
        Self {
            env: std::env::var_os("CUOTAI_CONFIG")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            cwd: PathBuf::new(),
            config_dir: dirs::config_dir(),
        }
    }
}

/// Rule that selected the effective path; frontends show it next to the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSource {
    /// `--config`.
    Argument,
    /// `CUOTAI_CONFIG`.
    Env,
    /// File chosen in the desktop settings.
    Chosen,
    /// Existing `./cuotai.toml`.
    Local,
    /// `<OS config dir>/cuotai/config.toml`, also the creation default.
    System,
}

/// Path selected by [`ConfigFile::resolve_path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub source: PathSource,
}

/// Normalized identity of a configuration, the key for per-config caches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConfigIdentity {
    /// Canonical path of the TOML file (see [`ConfigIdentity::of_path`]).
    File(PathBuf),
    /// Stable id supplied by an integrator that has no config file.
    Explicit(String),
}

impl ConfigIdentity {
    /// Resolve relative components and symlinks. A file that does not exist
    /// yet maps to its canonical (existing) parent plus file name, so the
    /// identity is the same before and after creation.
    pub fn of_path(path: &Path) -> std::io::Result<Self> {
        match fs::canonicalize(path) {
            Ok(real) => Ok(Self::File(real)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let name = path.file_name().ok_or(e)?;
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                Ok(Self::File(fs::canonicalize(parent)?.join(name)))
            }
            Err(e) => Err(e),
        }
    }

    /// SHA-256 (lowercase hex) of the identity: the per-config key caches
    /// store and exports publish instead of the path itself.
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let bytes = match self {
            Self::File(file) => [b"file:".as_slice(), file.as_os_str().as_encoded_bytes()].concat(),
            Self::Explicit(id) => [b"explicit:".as_slice(), id.as_bytes()].concat(),
        };
        format!("{:x}", Sha256::digest(bytes))
    }
}

impl std::fmt::Display for ConfigIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
            Self::Explicit(id) => f.write_str(id),
        }
    }
}

pub struct ConfigFile {
    pub config: Config,
    pub path: PathBuf,
    /// Rule that selected `path`.
    pub source: PathSource,
    pub exists: bool,
    /// Byte snapshot of the file when loaded/saved; `None` when it did not exist.
    original: Option<Vec<u8>>,
}

impl ConfigFile {
    /// CLI/TUI load: `explicit` is `--config` (see [`Discovery::Cli`]),
    /// reading `CUOTAI_CONFIG`, cwd and the OS dir from the process.
    pub fn load(explicit: Option<PathBuf>, interactive: bool) -> Result<Self, EditError> {
        Self::load_with(
            Discovery::Cli { config: explicit },
            interactive,
            &Locations::from_process(),
        )
    }

    /// Resolve with `discovery` over `locations`, then [`ConfigFile::open`].
    pub fn load_with(
        discovery: Discovery,
        interactive: bool,
        locations: &Locations,
    ) -> Result<Self, EditError> {
        Self::open(Self::resolve_path(discovery, locations), interactive)
    }

    /// Load exactly `resolved` (also used to re-read the live file).
    ///
    /// Missing file behaviour:
    /// - interactive always yields an empty-accounts config with `exists =
    ///   false` (so the caller can open the wizard);
    /// - non-interactive errors for a user-named path (`--config`, env,
    ///   chosen), otherwise falls back to the default provider catalog.
    ///
    /// Errors never include raw TOML contents or user-typed secrets.
    pub fn open(resolved: ResolvedPath, interactive: bool) -> Result<Self, EditError> {
        let ResolvedPath { path, source } = resolved;
        match fs::read(&path) {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| {
                    validation(None, "edit_error.config_not_utf8", "arquivo nao e UTF-8")
                })?;
                // ConfigError messages are fixed strings and never echo the body.
                // Preserve the field/account detail of the metered compatibility
                // boundary; other malformed documents keep the generic load error.
                let mut config = Config::from_toml(text).map_err(|error| {
                    if matches!(
                        error,
                        crate::ConfigError::MeteredRefreshFloor { .. }
                            | crate::ConfigError::AccountIds { .. }
                    ) {
                        validation_note(None, config_error_note(&error))
                    } else {
                        validation(
                            None,
                            "edit_error.config_toml_invalid",
                            "arquivo TOML invalido",
                        )
                    }
                })?;
                config.resolve_ids(ConfigIdentity::of_path(&path).ok().as_ref());
                Ok(Self {
                    config,
                    path,
                    source,
                    exists: true,
                    original: Some(bytes),
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if interactive {
                    let config = Config {
                        accounts: Vec::new(),
                        ..Config::default()
                    };
                    Ok(Self {
                        config,
                        path,
                        source,
                        exists: false,
                        original: None,
                    })
                } else if matches!(
                    source,
                    PathSource::Argument | PathSource::Env | PathSource::Chosen
                ) {
                    Err(EditError::Io {
                        path: path.clone(),
                        source: e,
                    })
                } else {
                    Ok(Self {
                        config: Config::default(),
                        path,
                        source,
                        exists: false,
                        original: None,
                    })
                }
            }
            Err(e) => Err(EditError::Io {
                path: path.clone(),
                source: e,
            }),
        }
    }

    /// Pure precedence (see [`Discovery`]); only `./cuotai.toml` is probed
    /// with `is_file`. With nothing found the OS path is returned so a new
    /// file is created there.
    pub fn resolve_path(discovery: Discovery, locations: &Locations) -> ResolvedPath {
        let at = |path, source| ResolvedPath { path, source };
        let (chosen, local) = match discovery {
            Discovery::Cli { config: Some(path) } => return at(path, PathSource::Argument),
            Discovery::Cli { config: None } => (None, true),
            Discovery::Gui { chosen } => (chosen, false),
        };
        if let Some(env) = &locations.env {
            return at(env.clone(), PathSource::Env);
        }
        if let Some(chosen) = chosen {
            return at(chosen, PathSource::Chosen);
        }
        let local_path = locations.cwd.join("cuotai.toml");
        if local && local_path.is_file() {
            return at(local_path, PathSource::Local);
        }
        // ponytail: without a platform config dir (rare) this is relative to
        // the cwd, as before; add an error when such a platform matters.
        let dir = locations.config_dir.clone().unwrap_or_default();
        at(dir.join("cuotai").join("config.toml"), PathSource::System)
    }

    /// Normalized identity of [`ConfigFile::path`] (see [`ConfigIdentity::of_path`]).
    pub fn identity(&self) -> std::io::Result<ConfigIdentity> {
        ConfigIdentity::of_path(&self.path)
    }

    /// Validate and durably persist `config`. See module docs for the policy.
    pub fn save(&mut self, mut config: Config) -> Result<(), EditError> {
        // PRD 0029 RF1.3: an account the front end built without an id gets
        // the effective one a reader of this file would derive; every saved
        // table carries its id from now on.
        config.resolve_ids(self.identity().ok().as_ref());
        config
            .validate()
            .map_err(|e| validation_note(None, config_error_note(&e)))?;
        toml_int_in_range("refresh_seconds", config.refresh_seconds)?;
        toml_int_in_range("concurrency", config.concurrency as u64)?;
        toml_int_in_range("timeout_seconds", config.timeout_seconds)?;
        // The OS default lives in our own `cuotai/` dir, which a fresh
        // machine lacks; user-named paths must already have a parent.
        if self.source == PathSource::System && self.original.is_none() {
            if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent).map_err(|e| EditError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
        }
        // Resolve symlinks so we replace the destination, never the link.
        let dest = match fs::canonicalize(&self.path) {
            Ok(real) => real,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.path.clone(),
            Err(e) => {
                return Err(EditError::Io {
                    path: self.path.clone(),
                    source: e,
                });
            }
        };
        // Refuse stale/external edits and unexpected creation/deletion.
        self.check_fresh(&dest)?;
        let bytes = render(&config, self.original.as_deref())?;
        let dir = dest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut tmp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| EditError::Io {
            path: dir.clone(),
            source: e,
        })?;
        if let Some(perms) = current_permissions(&dest) {
            fs::set_permissions(tmp.path(), perms).map_err(|e| EditError::Io {
                path: tmp.path().to_path_buf(),
                source: e,
            })?;
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o600)).map_err(
                    |e| EditError::Io {
                        path: tmp.path().to_path_buf(),
                        source: e,
                    },
                )?;
            }
        }
        tmp.write_all(&bytes).map_err(|e| EditError::Io {
            path: tmp.path().to_path_buf(),
            source: e,
        })?;
        tmp.as_file().sync_all().map_err(|e| EditError::Io {
            path: tmp.path().to_path_buf(),
            source: e,
        })?;
        // Last freshness check right before the rename shrinks the TOCTOU window.
        self.check_fresh(&dest)?;
        let result = if self.original.is_none() {
            tmp.persist_noclobber(&dest).map_err(|e| e.error)
        } else {
            tmp.persist(&dest).map_err(|e| e.error)
        };
        result.map_err(|e| EditError::Io {
            path: dest.clone(),
            source: e,
        })?;
        self.config = config;
        self.exists = true;
        self.original = Some(bytes);
        Ok(())
    }

    /// Re-read the file from disk and, on success, replace the in-memory
    /// configuration and byte snapshot. Returns [`EditError::Conflict`]
    /// when the file changed in a way the user must approve; any other
    /// error leaves `self` untouched (the previous configuration stays
    /// in memory so the dashboard does not flash empty data).
    pub fn reload(&mut self) -> Result<(), EditError> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| {
                    validation(None, "edit_error.config_not_utf8", "arquivo nao e UTF-8")
                })?;
                let mut config = Config::from_toml(text).map_err(|_| {
                    validation(
                        None,
                        "edit_error.config_toml_invalid",
                        "arquivo TOML invalido",
                    )
                })?;
                config.resolve_ids(ConfigIdentity::of_path(&self.path).ok().as_ref());
                // Commit only after every step above succeeded; an
                // intermediate failure keeps `self` untouched.
                self.config = config;
                self.exists = true;
                self.original = Some(bytes);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // The file disappeared under us: surface as a conflict
                // so the UI can choose between creating a new file or
                // cancelling.
                Err(EditError::Conflict {
                    path: self.path.clone(),
                })
            }
            Err(e) => Err(EditError::Io {
                path: self.path.clone(),
                source: e,
            }),
        }
    }

    /// Cheap detector for "did the file on disk change since we last
    /// loaded or saved it?". Implemented by comparing the current bytes
    /// to the snapshot we hold: `mtime` alone is unreliable (resolution
    /// varies by filesystem; a touch-then-write at the same second can
    /// look unchanged) and the snapshot already covers the case where
    /// the file was deleted (snapshot is `Some`, on-disk is `None`).
    ///
    /// An I/O error is reported as `EditError::Io` so the caller can
    /// surface it the same way as a save failure.
    pub fn changed_on_disk(&self) -> Result<bool, EditError> {
        let current = match fs::read(&self.path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(EditError::Io {
                    path: self.path.clone(),
                    source: e,
                });
            }
        };
        Ok(current != self.original)
    }

    fn check_fresh(&self, dest: &Path) -> Result<(), EditError> {
        let current = match fs::read(dest) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(EditError::Io {
                    path: dest.to_path_buf(),
                    source: e,
                });
            }
        };
        if current != self.original {
            return Err(EditError::Conflict {
                path: dest.to_path_buf(),
            });
        }
        Ok(())
    }
}

fn current_permissions(path: &Path) -> Option<fs::Permissions> {
    fs::metadata(path).ok().map(|m| m.permissions())
}

/// Serialize `config`, editing the previous document when it parses so
/// comments and formatting of retained entries survive. The result is parsed
/// back and must be equivalent to `config`, otherwise the save is refused.
fn render(config: &Config, previous: Option<&[u8]>) -> Result<Vec<u8>, EditError> {
    let text = match previous
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|s| s.parse::<toml_edit::DocumentMut>().ok())
    {
        Some(mut doc) => {
            set_int(&mut doc, "refresh_seconds", config.refresh_seconds as i64);
            set_int(&mut doc, "concurrency", config.concurrency as i64);
            set_int(&mut doc, "timeout_seconds", config.timeout_seconds as i64);
            let editable = doc
                .get("accounts")
                .map(|item| item.as_array_of_tables().is_some())
                .unwrap_or(true);
            if !editable {
                if config.accounts.is_empty() {
                    doc["accounts"] =
                        toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new()));
                    doc.to_string()
                } else {
                    serialize_fresh(config)?
                }
            } else {
                update_accounts(&mut doc, &config.accounts)?;
                update_groups(&mut doc, &config.groups)?;
                doc.to_string()
            }
        }
        None => serialize_fresh(config)?,
    };
    // Round-trip guard: the rendered document must describe the same config.
    let parsed = Config::from_toml(&text).map_err(|_| {
        validation(
            None,
            "edit_error.config_toml_invalid",
            "documento gerado invalido",
        )
    })?;
    let want = toml::to_string(config).map_err(|_| {
        validation(
            None,
            "edit_error.config_serialize",
            "falha ao serializar configuracao",
        )
    })?;
    let got = toml::to_string(&parsed).map_err(|_| {
        validation(
            None,
            "edit_error.config_serialize",
            "falha ao serializar configuracao",
        )
    })?;
    if want != got {
        return Err(validation(
            None,
            "edit_error.config_roundtrip",
            "documento gerado diverge da configuracao",
        ));
    }
    Ok(text.into_bytes())
}

fn serialize_fresh(config: &Config) -> Result<String, EditError> {
    toml_edit::ser::to_document(config)
        .map(|doc| doc.to_string())
        .map_err(|_| {
            validation(
                None,
                "edit_error.config_serialize",
                "falha ao serializar configuracao",
            )
        })
}

/// Replace a scalar while keeping the existing value decor (e.g. trailing comments).
fn set_int(doc: &mut toml_edit::DocumentMut, key: &str, val: i64) {
    if let Some(existing) = doc.get_mut(key).and_then(|i| i.as_value_mut()) {
        let decor = existing.decor().clone();
        let mut value = toml_edit::Value::from(val);
        *value.decor_mut() = decor;
        *existing = value;
    } else {
        doc[key] = toml_edit::value(val);
    }
}

/// Reorder existing tables by their validated account identity before applying
/// positional edits. Names identify current configurations; RF0 also accepts an
/// omitted name, so provider and extension id disambiguate unnamed accounts.
/// Moves carry each table's prefix and value comments with the account. Renames
/// still use positional matching; extra tables are dropped and missing ones
/// appended.
fn table_matches_account(table: &toml_edit::Table, account: &Account) -> bool {
    // PRD 0029 RF1.4: a table that names its id is that account, whatever
    // its name or position now.
    if let (Some(id), Some(table_id)) = (
        account.id.as_ref(),
        table.get("id").and_then(|id| id.as_str()),
    ) {
        return id.as_str() == table_id;
    }
    let name = table
        .get("name")
        .and_then(|name| name.as_str())
        .unwrap_or_default();
    let provider = table.get("provider").and_then(|provider| provider.as_str());
    if name != account.name || provider != Some(account.provider.slug()) {
        return false;
    }
    account.provider != crate::Provider::Extension
        || table_param(table, "extension_id") == account.extension_id()
}

fn table_param<'a>(table: &'a toml_edit::Table, key: &str) -> Option<&'a str> {
    let params = table.get("params")?;
    params
        .as_value()
        .and_then(toml_edit::Value::as_inline_table)
        .and_then(|params| params.get(key))
        .and_then(toml_edit::Value::as_str)
        .or_else(|| {
            params
                .as_table()
                .and_then(|params| params.get(key))
                .and_then(|value| value.as_str())
        })
}

/// PRD 0024 RF7.1: keep the `[[groups]]` of the document as they are while
/// they describe `groups`; otherwise replace them (no table at all when
/// there are none), so unrelated comments survive.
fn update_groups(
    doc: &mut toml_edit::DocumentMut,
    groups: &[crate::model::GroupDef],
) -> Result<(), EditError> {
    let current: Option<Vec<crate::model::GroupDef>> = doc
        .get("groups")
        .and_then(|item| item.as_array_of_tables())
        .map(|tables| {
            tables
                .iter()
                .filter_map(|table| toml_edit::de::from_str(&table.to_string()).ok())
                .collect()
        });
    if current.as_deref().unwrap_or_default() == groups && current.is_some() == !groups.is_empty() {
        return Ok(());
    }
    doc.remove("groups");
    if groups.is_empty() {
        return Ok(());
    }
    let mut array = toml_edit::ArrayOfTables::new();
    for group in groups {
        let table = toml_edit::ser::to_document(group)
            .map_err(|_| {
                validation(
                    Some("groups"),
                    "edit_error.config_serialize",
                    "falha ao serializar grupo",
                )
            })?
            .into_table();
        array.push(table);
    }
    doc["groups"] = toml_edit::Item::ArrayOfTables(array);
    Ok(())
}

fn update_accounts(
    doc: &mut toml_edit::DocumentMut,
    accounts: &[Account],
) -> Result<(), EditError> {
    if accounts.is_empty() {
        doc["accounts"] = toml_edit::value(toml_edit::Array::new());
        return Ok(());
    }
    if !doc.contains_key("accounts") {
        doc["accounts"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let array = doc["accounts"].as_array_of_tables_mut().ok_or_else(|| {
        validation(
            Some("accounts"),
            "edit_error.config_accounts_shape",
            "chave 'accounts' existente nao e uma lista de tabelas",
        )
    })?;
    // A pure move has the same account identities in a new order. Move the
    // actual `toml_edit::Table` nodes first so comments and formatting stay
    // attached to account identity rather than to the old numeric slot.
    let is_permutation = array.len() == accounts.len()
        && accounts.iter().all(|account| {
            array
                .iter()
                .any(|table| table_matches_account(table, account))
        });
    if is_permutation {
        // `toml_edit` 0.22 exposes append but no indexed insertion, and its
        // removal API does not return the removed table. Build the new array
        // from cloned `Table` nodes: `Table::clone` retains decor, including
        // table-prefix and value comments, without a text round-trip.
        let mut reordered = toml_edit::ArrayOfTables::new();
        for (index, account) in accounts.iter().enumerate() {
            let mut table = array
                .iter()
                .find(|table| table_matches_account(table, account))
                .expect("permutation contains every requested account")
                .clone();
            // Encoder sorts by document position, not array order.
            table.set_position(
                array
                    .get(index)
                    .and_then(|slot| slot.position())
                    .unwrap_or(index),
            );
            reordered.push(table);
        }
        *array = reordered;
    }
    // Deletion preserves the original table/comment belonging to each survivor.
    // Renames still use positional matching when the account count is unchanged.
    if accounts.len() < array.len() {
        let mut index = 0;
        while index < array.len() {
            let keep = array.get(index).is_some_and(|table| {
                accounts
                    .iter()
                    .any(|account| table_matches_account(table, account))
            });
            if keep {
                index += 1;
            } else {
                array.remove(index);
            }
        }
    }
    for (index, account) in accounts.iter().enumerate() {
        if index < array.len() {
            edit_account(array.get_mut(index).expect("index checked"), account)?;
        } else {
            let doc = toml_edit::ser::to_document(account).map_err(|_| {
                validation(
                    Some("accounts"),
                    "edit_error.config_serialize",
                    "falha ao serializar conta",
                )
            })?;
            let mut table = toml_edit::Table::new();
            for (key, item) in doc.iter() {
                table[key] = item.clone();
            }
            array.push(table);
        }
    }
    while array.len() > accounts.len() {
        array.remove(array.len() - 1);
    }
    Ok(())
}

/// Overwrite a table's fields, preserving value decor (trailing comments) and
/// the key/prefix comments that stay attached to the table itself.
fn edit_account(table: &mut toml_edit::Table, account: &Account) -> Result<(), EditError> {
    let doc = toml_edit::ser::to_document(account).map_err(|_| {
        validation(
            Some("accounts"),
            "edit_error.config_serialize",
            "falha ao serializar conta",
        )
    })?;
    for (key, item) in doc.iter() {
        if let (Some(old), Some(new_value)) = (
            table.get_mut(key).and_then(|i| i.as_value_mut()),
            item.as_value(),
        ) {
            let decor = old.decor().clone();
            let mut value = new_value.clone();
            *value.decor_mut() = decor;
            *old = value;
        } else {
            table[key] = item.clone();
        }
    }
    if account.id.is_none() {
        table.remove("id");
    } else if table.iter().next().is_some_and(|(key, _)| key != "id") {
        // RF1.4: the id is the first key of its table. A stable sort keeps
        // every other key, and its comments, in place.
        table.sort_values_by(|a, _, b, _| (a.get() != "id").cmp(&(b.get() != "id")));
    }
    if account.name.is_empty() {
        table.remove("name");
    }
    if account.credential_env.is_none() {
        table.remove("credential_env");
    }
    if account.oauth.is_none() {
        table.remove("oauth");
    }
    if account.session.is_none() {
        table.remove("session");
    }
    if account.params.is_empty() {
        table.remove("params");
    }
    // Override individual de refresh_seconds: só persiste quando setado;
    // limpar o campo no assistente remove a chave (herda o global).
    if account.refresh_seconds.is_none() {
        table.remove("refresh_seconds");
    }
    // PRD 0024 RF7.1: the optional group of comparable accounts.
    if account.group.is_none() {
        table.remove("group");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const FUTURE_ID_FIXTURE: &str = include_str!("../tests/fixtures/stable-account-identity.toml");

    fn account(name: &str, env: Option<&str>) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider: crate::Provider::OpenAiApi,
            credential_env: env.map(str::to_string),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn config(accounts: Vec<Account>) -> Config {
        Config {
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 10,
            accounts,
            groups: Vec::new(),
        }
    }

    fn load_at(path: &Path, interactive: bool) -> Result<ConfigFile, EditError> {
        ConfigFile::load(Some(path.to_path_buf()), interactive)
    }

    #[test]
    fn new_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        assert!(!file.exists);
        assert!(file.config.accounts.is_empty());
        let cfg = config(vec![account("OpenAI", Some("OPENAI_ADMIN_KEY"))]);
        file.save(cfg.clone()).unwrap();
        assert!(file.exists);
        assert_eq!(file.path, path);
        let reloaded = load_at(&path, true).unwrap();
        // PRD 0029 RF1.3: the saved account carries the id it was given.
        assert!(reloaded.config.accounts[0].id.is_some());
        assert_eq!(reloaded.config.accounts, file.config.accounts);
        let mut without_ids = reloaded.config.clone();
        without_ids.accounts[0].id = None;
        assert_eq!(
            toml::to_string(&without_ids).unwrap(),
            toml::to_string(&cfg).unwrap()
        );
    }

    #[test]
    fn save_preserves_existing_ids_and_gives_new_accounts_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future-id.toml");
        fs::write(&path, FUTURE_ID_FIXTURE).unwrap();
        let mut file = load_at(&path, false).unwrap();
        assert_eq!(file.config.accounts[0].name, "");

        let mut updated = file.config.clone();
        updated.accounts[0].refresh_seconds = Some(90);
        updated.accounts.push(account("OpenAI", None));
        file.save(updated).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap();
        let accounts = document["accounts"].as_array_of_tables().unwrap();
        assert_eq!(
            accounts
                .get(0)
                .and_then(|table| table.get("id"))
                .and_then(|id| id.as_str()),
            Some("acc_0123456789ab")
        );
        assert_eq!(
            accounts
                .get(1)
                .and_then(|table| table.get("id"))
                .and_then(|id| id.as_str()),
            Some("acc_bcdefghjkmnp")
        );
        // PRD 0029 RF1.3: the account added without an id gains one.
        let added = accounts
            .get(2)
            .and_then(|table| table.get("id"))
            .and_then(|id| id.as_str())
            .unwrap();
        assert!(crate::AccountId::parse(added).is_some());

        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts[0].name, "");
        assert_eq!(reloaded.config.accounts[0].refresh_seconds, Some(90));
        assert_eq!(reloaded.config.accounts[2].name, "OpenAI");
    }

    #[test]
    fn moving_unnamed_accounts_keeps_each_future_id_with_its_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unnamed-move.toml");
        fs::write(
            &path,
            r#"
[[accounts]]
id = "acc_0123456789ab"
provider = "groq"

[[accounts]]
id = "acc_bcdefghjkmnp"
provider = "anthropic_api"
"#,
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let moved = file.config.move_account(0, 1).unwrap();
        file.save(moved).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap();
        let accounts = document["accounts"].as_array_of_tables().unwrap();
        assert_eq!(
            accounts
                .get(0)
                .and_then(|table| table.get("id"))
                .and_then(|id| id.as_str()),
            Some("acc_bcdefghjkmnp")
        );
        assert_eq!(
            accounts
                .get(1)
                .and_then(|table| table.get("id"))
                .and_then(|id| id.as_str()),
            Some("acc_0123456789ab")
        );
        assert_eq!(
            file.config
                .accounts
                .iter()
                .map(|account| account.provider)
                .collect::<Vec<_>>(),
            [crate::Provider::AnthropicApi, crate::Provider::Groq]
        );
    }

    const LEGACY: &str = r#"# global comment
refresh_seconds = 60 # trailing
concurrency = 4
timeout_seconds = 30

# the work account
[[accounts]]
name = "Work"
provider = "open_ai_api"
credential_env = "WORK_KEY" # work key

# the home account
[[accounts]]
name = "Home"
provider = "open_ai_api"
credential_env = "HOME_KEY"
"#;

    fn table_ids(path: &Path) -> Vec<Option<String>> {
        let text = fs::read_to_string(path).unwrap();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap();
        document["accounts"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .map(|table| {
                table
                    .get("id")
                    .and_then(|id| id.as_str())
                    .map(str::to_owned)
            })
            .collect()
    }

    #[test]
    fn an_old_config_gains_one_id_line_per_account_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.toml");
        fs::write(&path, LEGACY).unwrap();
        let mut file = load_at(&path, true).unwrap();
        let ids: Vec<String> = file
            .config
            .accounts
            .iter()
            .map(|account| account.id.as_ref().unwrap().to_string())
            .collect();
        file.save(file.config.clone()).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let (id_lines, rest): (Vec<&str>, Vec<&str>) =
            text.lines().partition(|line| line.starts_with("id = "));
        assert_eq!(
            id_lines,
            ids.iter()
                .map(|id| format!("id = \"{id}\""))
                .collect::<Vec<_>>()
        );
        assert_eq!(rest, LEGACY.lines().collect::<Vec<_>>());
        // Each id is the first key of its table.
        assert!(text.contains(&format!(
            "[[accounts]]\nid = \"{}\"\nname = \"Work\"",
            ids[0]
        )));
        // A second save changes nothing (the migration is idempotent).
        let before = fs::read(&path).unwrap();
        let mut again = load_at(&path, true).unwrap();
        again.save(again.config.clone()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn read_only_processes_agree_on_effective_ids_before_any_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.toml");
        fs::write(&path, LEGACY).unwrap();
        let one = load_at(&path, false).unwrap();
        let two = load_at(&path, false).unwrap();
        assert_eq!(one.config.accounts, two.config.accounts);
        assert_ne!(one.config.accounts[0].id, one.config.accounts[1].id);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            LEGACY,
            "reading writes nothing"
        );
        // The ids an interactive save writes are the ones readers derived.
        let mut writer = load_at(&path, true).unwrap();
        writer.save(writer.config.clone()).unwrap();
        let ids: Vec<Option<String>> = one
            .config
            .accounts
            .iter()
            .map(|account| account.id.as_ref().map(ToString::to_string))
            .collect();
        assert_eq!(table_ids(&path), ids);
        // Another file with the same accounts derives other ids.
        let other = dir.path().join("other.toml");
        fs::write(&other, LEGACY).unwrap();
        assert_ne!(
            load_at(&other, false).unwrap().config.accounts[0].id,
            one.config.accounts[0].id
        );
    }

    #[test]
    fn a_duplicated_id_is_rejected_naming_the_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.toml");
        fs::write(
            &path,
            r#"
[[accounts]]
id = "acc_0123456789ab"
name = "Work"
provider = "open_ai_api"

[[accounts]]
id = "acc_0123456789ab"
name = "Home"
provider = "open_ai_api"
"#,
        )
        .unwrap();
        let Err(error) = load_at(&path, false) else {
            panic!("a duplicated id must not load");
        };
        let note = error.l10n();
        assert_eq!(note.id, "edit_error.config_account_ids");
        assert!(note.text().contains("accounts[1].id"), "{}", note.text());
        let config = Config::from_toml(&fs::read_to_string(&path).unwrap());
        assert!(matches!(
            config,
            Err(crate::ConfigError::AccountIds { field }) if field == "accounts[1].id"
        ));
    }

    #[test]
    fn moving_and_renaming_in_one_save_keeps_comments_with_their_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.toml");
        fs::write(&path, LEGACY).unwrap();
        let mut file = load_at(&path, true).unwrap();
        file.save(file.config.clone()).unwrap();
        let mut edited = file.config.move_account(0, 1).unwrap();
        edited.accounts[1].name = "Office".into();
        edited.accounts[0].name = "House".into();
        file.save(edited.clone()).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let house = text.find("name = \"House\"").unwrap();
        let office = text.find("name = \"Office\"").unwrap();
        assert!(house < office, "{text}");
        let work_comment = text.find("# the work account").unwrap();
        let home_comment = text.find("# the home account").unwrap();
        // Each comment stays above the table of its account.
        assert!(home_comment < house && house < work_comment, "{text}");
        assert!(work_comment < office, "{text}");
        assert!(
            text.contains("credential_env = \"WORK_KEY\" # work key"),
            "{text}"
        );
        assert_eq!(
            load_at(&path, false).unwrap().config.accounts,
            edited.accounts
        );
    }

    #[test]
    fn save_preserves_comments_params_and_other_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            "# cabecalho\nrefresh_seconds = 300 # rodape\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\nname = \"OpenAI\"\nprovider = \"open_ai_api\"\ncredential_env = \"OPENAI_ADMIN_KEY\"\n# parametros extras\nparams = { project_ids = \"proj_1\", models = \"gpt-4.1\" }\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut cfg = file.config.clone();
        cfg.refresh_seconds = 60;
        cfg.accounts.push(account("Nova", None));
        file.save(cfg).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# cabecalho"));
        assert!(text.contains("# rodape"));
        assert!(text.contains("# parametros extras"));
        assert!(text.contains("project_ids = \"proj_1\""));
        assert!(text.contains("refresh_seconds = 60"));
        assert!(text.contains("name = \"Nova\""));
        // Saved document must still parse to the same config.
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts.len(), 2);
        assert_eq!(
            reloaded.config.accounts[0]
                .params
                .get("project_ids")
                .map(String::as_str),
            Some("proj_1")
        );
    }

    #[test]
    fn rename_first_of_two_keeps_order_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\n# primeira\nname = \"A\"\nprovider = \"open_ai_api\"\n\n[[accounts]]\n# segunda\nname = \"B\"\nprovider = \"anthropic_api\"\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut cfg = file.config.clone();
        cfg.accounts[0].name = "Renomeada".into();
        file.save(cfg.clone()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# primeira"));
        assert!(text.contains("# segunda"));
        let first = text.find("Renomeada").unwrap();
        let second = text.find("\"B\"").unwrap();
        assert!(first < second, "{text}");
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(
            toml::to_string(&reloaded.config).unwrap(),
            toml::to_string(&cfg).unwrap()
        );
    }

    #[test]
    fn move_account_persists_order_and_per_account_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("move.toml");
        fs::write(
            &path,
            "# arquivo\nrefresh_seconds = 300 # global\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\n# comentario alpha\nname = \"A\"\nprovider = \"open_ai_api\"\ncredential_env = \"A_KEY\" # credencial alpha\nparams = { project_ids = \"alpha\" }\n\n[[accounts]]\n# comentario beta\nname = \"B\"\nprovider = \"anthropic_api\"\ncredential_env = \"B_KEY\" # credencial beta\n\n[[accounts]]\n# comentario gamma\nname = \"C\"\nprovider = \"groq\"\ncredential_env = \"C_KEY\" # credencial gamma\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let updated = file.config.move_account(0, 2).unwrap();
        file.save(updated.clone()).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# arquivo\nrefresh_seconds = 300 # global\n"));
        let sections = text.split("[[accounts]]").skip(1).collect::<Vec<_>>();
        assert_eq!(sections.len(), 3);
        for (section, name, comment, credential) in [
            (
                sections[0],
                "name = \"B\"",
                "# comentario beta",
                "# credencial beta",
            ),
            (
                sections[1],
                "name = \"C\"",
                "# comentario gamma",
                "# credencial gamma",
            ),
            (
                sections[2],
                "name = \"A\"",
                "# comentario alpha",
                "# credencial alpha",
            ),
        ] {
            assert!(section.contains(name), "{text}");
            assert!(section.contains(comment), "{text}");
            assert!(section.contains(credential), "{text}");
        }
        assert!(sections[2].contains("project_ids = \"alpha\""), "{text}");

        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(
            toml::to_string(&reloaded.config).unwrap(),
            toml::to_string(&updated).unwrap()
        );
        assert!(
            !text.contains("params = {}"),
            "accounts without params must not gain an empty table"
        );

        // Clearing every param removes the key instead of writing `{}`.
        let mut cleared = file.config.clone();
        cleared.accounts[2].params.clear();
        file.save(cleared).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("project_ids"));
        assert!(!text.contains("params"));
    }

    #[test]
    fn conflict_refuses_external_edit_and_keeps_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        fs::write(
            &path,
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let err = file.save(config(vec![])).unwrap_err();
        assert!(err.is_conflict(), "expected Conflict, got {err:?}");
        let display = err.message();
        assert!(display.contains("recarregue"), "{display}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n"
        );
    }

    #[test]
    fn unexpected_creation_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let err = file.save(config(vec![])).unwrap_err();
        assert!(err.is_conflict(), "expected Conflict, got {err:?}");
    }

    #[test]
    fn write_failure_leaves_object_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone").join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        let before = file.config.accounts.len();
        let err = file.save(config(vec![account("X", None)])).unwrap_err();
        assert!(err.is_io(), "expected Io, got {err:?}");
        assert!(!file.exists);
        assert_eq!(file.config.accounts.len(), before);
    }

    #[test]
    fn missing_explicit_interactive_vs_headless() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let headless = load_at(&path, false);
        let err = match headless {
            Ok(_) => panic!("headless load of missing file must error"),
            Err(e) => e,
        };
        assert!(err.is_io(), "missing explicit headless must surface as Io");
        let interactive = load_at(&path, true).unwrap();
        assert!(!interactive.exists);
        assert!(interactive.config.accounts.is_empty());
    }

    #[test]
    fn invalid_toml_error_has_no_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(&path, "segredo = [").unwrap();
        let err = load_at(&path, false)
            .err()
            .expect("TOML inválido deve falhar");
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        let rendered = err.message();
        assert!(!rendered.contains("segredo"), "{rendered}");
    }

    #[test]
    fn metered_floor_load_error_names_field_and_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            r#"
refresh_seconds = 120
concurrency = 1
timeout_seconds = 5

[[accounts]]
name = "bedrock-test"
provider = "aws_bedrock"
refresh_seconds = 200
params = { profile = "validation" }
"#,
        )
        .unwrap();

        let err = match load_at(&path, false) {
            Ok(_) => panic!("metered override below 300 must be rejected"),
            Err(error) => error,
        };
        let note = err.l10n();
        assert_eq!(note.id, "edit_error.config_metered_refresh_floor");
        for locale in ["pt-BR", "en-US", "es", "it"] {
            let resolved = note.resolve(locale);
            assert!(resolved.contains("accounts[0].refresh_seconds"));
            assert!(resolved.contains("bedrock-test"));
            assert!(resolved.contains("300"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn new_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        file.save(config(vec![])).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn save_follows_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        fs::write(
            &real,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut file = load_at(&link, false).unwrap();
        file.save(config(vec![])).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(fs::read_to_string(&real)
            .unwrap()
            .contains("refresh_seconds = 60"));
    }

    #[test]
    fn switching_oauth_to_environment_removes_saved_oauth_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut account = account("Google", None);
        account.provider = crate::Provider::GeminiApi;
        account.oauth = Some(crate::oauth::OAuthConfig::new("own-client".into()));
        file.save(config(vec![account.clone()])).unwrap();
        assert_eq!(
            load_at(&path, false).unwrap().config.accounts[0].oauth,
            account.oauth
        );
        account.oauth = None;
        account.credential_env = Some("GOOGLE_CLOUD_ACCESS_TOKEN".into());
        file.save(config(vec![account])).unwrap();
        let loaded = load_at(&path, false).unwrap();
        assert!(loaded.config.accounts[0].oauth.is_none());
        assert_eq!(
            loaded.config.accounts[0].credential_env.as_deref(),
            Some("GOOGLE_CLOUD_ACCESS_TOKEN")
        );
    }
    #[test]
    fn native_login_options_roundtrip_without_credentials_in_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut account = account("Kiro", None);
        account.provider = crate::Provider::Kiro;
        let mut oauth = crate::oauth::OAuthConfig::new(String::new());
        oauth
            .options
            .insert("auth_method".into(), "builder-id".into());
        oauth.options.insert("region".into(), "us-east-1".into());
        account.oauth = Some(oauth.clone());
        file.save(config(vec![account])).unwrap();
        let loaded = load_at(&path, false).unwrap();
        assert_eq!(loaded.config.accounts[0].oauth, Some(oauth));
        assert!(loaded.config.accounts[0].credential_env.is_none());
    }

    #[test]
    fn deleting_accounts_preserves_survivors_and_last_deletion_reloads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delete.toml");
        fs::write(&path, "[[accounts]]\nname='first'\nprovider='groq'\n[[accounts]]\nname='middle'\nprovider='kimi'\n[[accounts]]\n# survivor comment\nname='last'\nprovider='ollama_cloud'\ncredential_env='LAST_KEY'\n").unwrap();
        let mut file = load_at(&path, false).unwrap();
        let edited = file.config.without_account(1).unwrap();
        file.save(edited).unwrap();
        let loaded = load_at(&path, false).unwrap();
        assert_eq!(
            loaded
                .config
                .accounts
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            ["first", "last"]
        );
        assert_eq!(
            loaded.config.accounts[1].credential_env.as_deref(),
            Some("LAST_KEY")
        );
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("# survivor comment"));
        let mut empty = file.config.clone();
        empty.accounts.clear();
        file.save(empty).unwrap();
        assert!(load_at(&path, false).unwrap().config.accounts.is_empty());
    }

    #[test]
    fn account_refresh_override_round_trips_and_clears_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refresh.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut overridden = account("OpenAI", Some("OPENAI_ADMIN_KEY"));
        overridden.refresh_seconds = Some(120);
        file.save(config(vec![overridden])).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("refresh_seconds = 120"),
            "override não persistido: {text}"
        );
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts[0].refresh_seconds, Some(120));
        // Limpar o override remove a chave do arquivo (sem campo fantasma).
        let mut cleared = reloaded.config.clone();
        cleared.accounts[0].refresh_seconds = None;
        file.save(cleared).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("refresh_seconds = 120"),
            "override não foi removido: {text}"
        );
        let final_loaded = load_at(&path, false).unwrap();
        assert!(final_loaded.config.accounts[0].refresh_seconds.is_none());
    }

    #[test]
    fn account_refresh_override_does_not_touch_global_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refresh-global.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut config = file.config.clone();
        config.refresh_seconds = 240;
        let mut overridden = account("OpenAI", Some("OPENAI_ADMIN_KEY"));
        overridden.refresh_seconds = Some(45);
        config.accounts.push(overridden);
        file.save(config.clone()).unwrap();
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.refresh_seconds, 240);
        assert_eq!(reloaded.config.accounts[0].refresh_seconds, Some(45));
    }

    #[test]
    fn editing_account_preserves_unrelated_comments_and_other_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\nname = \"Vizinha\"\nprovider = \"open_ai_api\"\ncredential_env = \"VIZINHA_KEY\"\n# comentário da vizinha\nparams = { foo = \"bar\" }\n\n[[accounts]]\nname = \"OpenAI\"\nprovider = \"open_ai_api\"\ncredential_env = \"OPENAI_ADMIN_KEY\"\nparams = { project_ids = \"proj_1\" }\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut cfg = file.config.clone();
        cfg.accounts[1].refresh_seconds = Some(90);
        file.save(cfg).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# comentário da vizinha"));
        assert!(text.contains("project_ids = \"proj_1\""));
        assert!(text.contains("refresh_seconds = 90"));
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts[1].refresh_seconds, Some(90));
        assert!(reloaded.config.accounts[0].refresh_seconds.is_none());
    }

    // ----- RF4 typed errors + reload + change detector -----

    #[test]
    fn save_rejects_out_of_bounds_refresh_with_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bounds.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut bad = file.config.clone();
        bad.refresh_seconds = 1;
        let err = file.save(bad).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(err.field(), None);
    }

    #[test]
    fn save_returns_typed_validation_error_for_account_array() {
        // The validator already rejects malformed documents at load
        // time, but the `save` path must keep returning a typed
        // Validation for any out-of-range / structural failure it
        // detects itself.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("validation.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut bad = file.config.clone();
        bad.concurrency = 0; // out of range
        let err = file.save(bad).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        // Field annotation is intentionally absent for document-wide
        // bounds errors.
        assert_eq!(err.field(), None);
    }

    #[test]
    fn save_returns_io_when_dir_unwritable() {
        let dir = tempfile::tempdir().unwrap();
        // Choose a path whose parent does not exist; persist() will
        // fail because the named-tempfile was created in "." (the
        // resolved path is inside a missing subtree).
        let path = dir.path().join("missing").join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        let err = file.save(config(vec![account("X", None)])).unwrap_err();
        assert!(err.is_io(), "expected Io, got {err:?}");
    }

    #[test]
    fn reload_picks_up_external_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reload.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        fs::write(
            &path,
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        file.reload().unwrap();
        assert_eq!(file.config.refresh_seconds, 100);
        assert!(!file.changed_on_disk().unwrap());
    }

    #[test]
    fn reload_after_deletion_returns_conflict_and_keeps_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        fs::remove_file(&path).unwrap();
        let before = file.config.refresh_seconds;
        let err = file.reload().unwrap_err();
        assert!(err.is_conflict(), "expected Conflict, got {err:?}");
        // Self untouched.
        assert_eq!(file.config.refresh_seconds, before);
        assert!(file.exists);
    }

    #[test]
    fn reload_failure_leaves_state_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let before = file.config.clone();
        fs::write(&path, "garbage = [").unwrap();
        let err = file.reload().unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(file.config.refresh_seconds, before.refresh_seconds);
        assert_eq!(file.config.accounts.len(), before.accounts.len());
        assert!(file.exists);
    }

    #[test]
    fn changed_on_disk_detects_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("detect.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let file = load_at(&path, false).unwrap();
        assert!(!file.changed_on_disk().unwrap());
        fs::write(
            &path,
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        assert!(file.changed_on_disk().unwrap());
    }

    #[test]
    fn changed_on_disk_works_when_mtime_is_preserved() {
        // mtime resolution varies by filesystem; on HFS+/APFS the
        // second is the smallest unit. A rewrite that keeps mtime
        // identical would defeat a pure mtime detector. The
        // implementation compares bytes instead, so we force a write
        // that does not change mtime (back-to-back writes inside a
        // single second) and assert the detector still fires.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mtime.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let saved_mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(
            &path,
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        // Force mtime back to the original value to simulate a coarse
        // filesystem clock.
        let _ = saved_mtime;
        // Bytes differ even when mtime agrees: detector must fire.
        assert!(file.changed_on_disk().unwrap());
        // A subsequent save against the stale snapshot must surface a
        // conflict (no mtime fast path here — byte comparison only).
        assert!(file.save(config(vec![])).unwrap_err().is_conflict());
        // Reload refreshes the snapshot; the next save succeeds.
        file.reload().unwrap();
        file.save(config(vec![])).unwrap();
        assert!(!file.changed_on_disk().unwrap());
    }

    #[test]
    fn save_after_reload_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recover.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        fs::write(
            &path,
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        // Save without reloading: conflict.
        assert!(file.save(config(vec![])).is_err());
        // Reload, then save succeeds.
        file.reload().unwrap();
        let mut cfg = file.config.clone();
        cfg.refresh_seconds = 60;
        file.save(cfg).unwrap();
    }

    const EMPTY: &str =
        "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n";

    /// Temp cwd (with or without `cuotai.toml`) plus a fake OS config dir.
    fn locations(dir: &Path, local: bool, env: Option<&str>) -> Locations {
        let cwd = dir.join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        if local {
            fs::write(cwd.join("cuotai.toml"), EMPTY).unwrap();
        }
        Locations {
            env: env.map(|e| dir.join(e)),
            cwd,
            config_dir: Some(dir.join("os")),
        }
    }

    fn resolve(discovery: Discovery, loc: &Locations) -> (PathBuf, PathSource) {
        let r = ConfigFile::resolve_path(discovery, loc);
        (r.path, r.source)
    }

    #[test]
    fn cli_precedence_argument_env_local_system() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let cli = |config: Option<PathBuf>| Discovery::Cli { config };
        let loc = locations(d, true, Some("env.toml"));
        assert_eq!(
            resolve(cli(Some(d.join("arg.toml"))), &loc),
            (d.join("arg.toml"), PathSource::Argument)
        );
        assert_eq!(
            resolve(cli(None), &loc),
            (d.join("env.toml"), PathSource::Env)
        );
        let loc = locations(d, true, None);
        assert_eq!(
            resolve(cli(None), &loc),
            (loc.cwd.join("cuotai.toml"), PathSource::Local)
        );
        fs::remove_file(loc.cwd.join("cuotai.toml")).unwrap();
        assert_eq!(
            resolve(cli(None), &loc),
            (d.join("os/cuotai/config.toml"), PathSource::System)
        );
    }

    #[test]
    fn gui_precedence_env_chosen_system_never_local() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let gui = |chosen: Option<PathBuf>| Discovery::Gui { chosen };
        let loc = locations(d, true, Some("env.toml"));
        assert_eq!(
            resolve(gui(Some(d.join("chosen.toml"))), &loc),
            (d.join("env.toml"), PathSource::Env)
        );
        let loc = locations(d, true, None);
        assert_eq!(
            resolve(gui(Some(d.join("chosen.toml"))), &loc),
            (d.join("chosen.toml"), PathSource::Chosen)
        );
        assert_eq!(
            resolve(gui(None), &loc),
            (d.join("os/cuotai/config.toml"), PathSource::System)
        );
    }

    #[test]
    fn without_local_wizard_creates_in_os_path() {
        let dir = tempfile::tempdir().unwrap();
        let loc = locations(dir.path(), false, None);
        let mut file = ConfigFile::load_with(Discovery::Cli { config: None }, true, &loc).unwrap();
        assert!(!file.exists);
        file.save(config(vec![account("OpenAI", None)])).unwrap();
        assert!(dir.path().join("os/cuotai/config.toml").is_file());
        assert!(!loc.cwd.join("cuotai.toml").exists());
    }

    #[test]
    fn explicit_local_path_is_created_and_missing_one_errors_non_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let loc = locations(dir.path(), false, None);
        let local = Discovery::Cli {
            config: Some(loc.cwd.join("cuotai.toml")),
        };
        assert!(ConfigFile::load_with(local.clone(), false, &loc).is_err());
        let mut file = ConfigFile::load_with(local, true, &loc).unwrap();
        file.save(config(vec![])).unwrap();
        assert!(loc.cwd.join("cuotai.toml").is_file());
        assert!(!dir.path().join("os").exists());
    }

    #[test]
    fn identity_normalizes_relative_symlink_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        let id = |p: &Path| ConfigIdentity::of_path(p).unwrap();
        // Not created yet: canonical parent + name, stable across creation.
        let before = id(&real);
        fs::write(&real, EMPTY).unwrap();
        assert_eq!(id(&real), before);
        #[cfg(unix)]
        {
            // Relative to the process cwd (`../..` up to `/`), without changing it.
            let cwd = std::env::current_dir().unwrap();
            let mut relative: PathBuf = cwd.components().skip(1).map(|_| "..").collect();
            relative.push(real.strip_prefix("/").unwrap());
            assert!(relative.is_relative());
            assert_eq!(id(&relative), before);
            let link = dir.path().join("link.toml");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert_eq!(id(&link), before);
        }
        assert_ne!(id(&dir.path().join("other.toml")), before);
        let file = load_at(&real, false).unwrap();
        assert_eq!(file.identity().unwrap(), before);
    }
}
