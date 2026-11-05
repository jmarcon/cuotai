//! PRD 0046 RF1–RF3/RF5: a secret-free backup of this installation's
//! settings and its restore.
//!
//! A bundle is a `.cuotai-backup.zip` with `manifest.json` (schema, date,
//! version, OS, every file with its SHA-256 and kind, and what was left out)
//! and an allow-list of files, never a folder copy:
//!
//! | Path | Source | Scrubbed |
//! |---|---|---|
//! | `config/cuotai.toml` | the accounts TOML | verbatim (it names variables, never values) |
//! | `config/hooks.toml` | beside the TOML | verbatim |
//! | `prefs/desktop.json` | desktop settings | window geometry, one-time records and machine paths dropped (paths become `@config`/`@user` markers) |
//! | `prefs/cost-settings.json` | local cost settings | the per-machine sources (and their folders) dropped |
//! | `prefs/pricing.json` | your local prices | verbatim |
//! | `user/themes/*.css`, `user/locales/*.json` | user themes and languages | verbatim |
//!
//! Vault entries, tokens, the server token, the cost index, extensions and
//! logs are never read. Every candidate is scanned for token-like strings
//! and the backup stops when one is found (defence in depth).
//!
//! A restore reads the archive whole, refuses entries outside these
//! folders, checks every digest, then plans per file: `add`, `replace`,
//! `keep` (identical) or `skip` (the local file changed after the backup);
//! it lists the variables to set and the sign-ins to redo, because no
//! secret travels. It writes nothing to the vault.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config_file::{self, ConfigParseMode};
use crate::text::{home_relative, params, Localized, ParamValue};

mod zip;

/// The manifest schema this release writes and reads.
pub const SCHEMA: u32 = 1;
/// What a bundle never holds, in the manifest's `excluded`.
pub const EXCLUDED: [&str; 5] = ["vault", "serve_token", "cost_index", "extensions", "logs"];
/// Suggested file name suffix.
pub const EXTENSION: &str = ".cuotai-backup.zip";

const MANIFEST: &str = "manifest.json";

/// Why a backup or a restore stopped. Never carries a value from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// A candidate file holds something that looks like a secret.
    Secret { path: String },
    /// The development SQLite credential store is in use.
    DevStore,
    /// Not a backup this release understands, or a damaged archive.
    Invalid,
    /// A file does not match its manifest digest.
    Digest { path: String },
    /// A manifest schema from a newer release.
    Schema { schema: u64 },
    /// A file could not be read or written.
    Io { path: String },
    /// A file changed between the plan and the write.
    Conflict { path: String },
}

impl BundleError {
    pub fn l10n(&self) -> Localized {
        match self {
            Self::Secret { path } => Localized::with_param(
                "bundle.error.secret",
                "path",
                path.as_str(),
                "{path} holds something that looks like a key or token, so no backup was written; remove it from the file and try again.",
            ),
            Self::DevStore => Localized::fixed(
                "bundle.error.dev_store",
                "The development credential store is in use; back up without --dev-credentials-sqlite.",
            ),
            Self::Invalid => Localized::fixed(
                "bundle.error.invalid",
                "This is not a Cuotai backup, or it is damaged.",
            ),
            Self::Digest { path } => Localized::with_param(
                "bundle.error.digest",
                "path",
                path.as_str(),
                "{path} in the backup does not match its digest; the backup is damaged or was edited.",
            ),
            Self::Schema { schema } => Localized::with_param(
                "bundle.error.schema",
                "schema",
                *schema,
                "This backup was made by a newer Cuotai (format {schema}); update Cuotai to restore it.",
            ),
            Self::Io { path } => Localized::with_param(
                "bundle.error.io",
                "path",
                path.as_str(),
                "Could not read or write {path}.",
            ),
            Self::Conflict { path } => Localized::with_param(
                "bundle.error.conflict",
                "path",
                path.as_str(),
                "{path} changed while the restore ran; run it again.",
            ),
        }
    }

    /// `--restore --check` exit code: 2 for a damaged or newer backup, 1
    /// for the rest.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Invalid | Self::Digest { .. } | Self::Schema { .. } => 2,
            _ => 1,
        }
    }
}

/// The localized reason each [`EXCLUDED`] item is left out.
pub fn excluded_reason(item: &str) -> Localized {
    match item {
        "vault" => Localized::fixed(
            "bundle.excluded.vault",
            "Vault entries: secrets never leave the system vault.",
        ),
        "serve_token" => Localized::fixed(
            "bundle.excluded.serve_token",
            "Local server token: each computer makes its own.",
        ),
        "cost_index" => Localized::fixed(
            "bundle.excluded.cost_index",
            "Local cost index: each computer rebuilds it from its own logs.",
        ),
        "extensions" => Localized::fixed(
            "bundle.excluded.extensions",
            "Extensions: you review and approve them on each computer.",
        ),
        _ => Localized::fixed(
            "bundle.excluded.logs",
            "Logs: they describe this computer only.",
        ),
    }
}

/// One file of a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub sha256: String,
    pub kind: String,
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u64,
    pub created_at: String,
    pub cuotai_version: String,
    pub os: String,
    pub files: Vec<Entry>,
    pub excluded: Vec<String>,
}

/// A bundle in memory: its manifest and files by bundle path.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Where this installation keeps what a bundle carries.
#[derive(Debug, Clone)]
pub struct Locations {
    /// The accounts TOML (`hooks.toml` is beside it).
    pub config: PathBuf,
    /// The desktop app's `desktop.json`.
    pub desktop_settings: Option<PathBuf>,
    /// The folder of user `themes/` and `locales/`.
    pub user_dir: Option<PathBuf>,
    /// [`crate::data_dir::root`] (`cost/settings.json`, `cost/pricing.json`).
    pub data_root: Option<PathBuf>,
}

impl Locations {
    /// This process's locations; `config` is the resolved accounts TOML.
    pub fn from_process(config: PathBuf) -> Self {
        let config_dir = dirs::config_dir().map(|dir| dir.join("cuotai"));
        let desktop_settings = config_dir.as_ref().map(|dir| dir.join("desktop.json"));
        let custom_user_dir = desktop_settings
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value["user_dir"].as_str().map(PathBuf::from))
            .filter(|path| path.is_absolute());
        let user_dir = std::env::var_os("CUOTAI_USER_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or(custom_user_dir)
            .or(config_dir);
        Self {
            config,
            desktop_settings,
            user_dir,
            data_root: crate::data_dir::root(),
        }
    }

    /// Where the bundle file at `path` lives on this machine; `None` for a
    /// path outside the allow-list.
    fn target(&self, path: &str) -> Option<PathBuf> {
        let mut parts = path.split('/');
        let target = match (parts.next()?, parts.next()?, parts.next()) {
            ("config", "cuotai.toml", None) => self.config.clone(),
            ("config", "hooks.toml", None) => crate::hooks::Hooks::path_beside(&self.config),
            ("prefs", "desktop.json", None) => self.desktop_settings.clone()?,
            ("prefs", "cost-settings.json", None) => {
                self.data_root.as_ref()?.join("cost").join("settings.json")
            }
            ("prefs", "pricing.json", None) => {
                self.data_root.as_ref()?.join("cost").join("pricing.json")
            }
            ("user", folder @ ("themes" | "locales"), Some(name)) if safe_name(name) => {
                self.user_dir.as_ref()?.join(folder).join(name)
            }
            _ => return None,
        };
        (parts.next().is_none()).then_some(target)
    }
}

/// A file name of a user theme or language: one plain component.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.starts_with('.')
}

fn kind_of(path: &str) -> &'static str {
    match path {
        "config/cuotai.toml" => "config",
        "config/hooks.toml" => "hooks",
        "prefs/desktop.json" => "desktop_settings",
        "prefs/cost-settings.json" => "cost_settings",
        "prefs/pricing.json" => "pricing",
        path if path.starts_with("user/themes/") => "theme",
        _ => "locale",
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Whether `text` holds a string shaped like a credential: a known key or
/// token prefix followed by a long run of token characters, a PEM private
/// key, or a JSON Web Token. Variable names (`OPENAI_ADMIN_KEY`) and
/// vault references never match.
pub fn looks_secret(text: &str) -> bool {
    const PREFIXES: [&str; 16] = [
        "sk-",
        "sk_live_",
        "rk_live_",
        "ghp_",
        "gho_",
        "ghs_",
        "ghu_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "AIza",
        "hf_",
        "gsk_",
        "pplx-",
    ];
    let token_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        return true;
    }
    for prefix in PREFIXES {
        for (at, _) in text.match_indices(prefix) {
            let before = text[..at].chars().next_back();
            if before.is_some_and(token_char) {
                continue;
            }
            let run = text[at + prefix.len()..]
                .chars()
                .take_while(|c| token_char(*c))
                .count();
            if run >= 16 {
                return true;
            }
        }
    }
    // A JSON Web Token: three base64url parts, the first two `eyJ`.
    text.match_indices("eyJ").any(|(at, _)| {
        let token: String = text[at..].chars().take_while(|c| token_char(*c)).collect();
        let parts: Vec<_> = token.split('.').collect();
        parts.len() == 3 && parts[1].starts_with("eyJ") && parts.iter().all(|p| p.len() >= 8)
    })
}

/// `desktop.json` without what belongs to this machine: the window
/// geometry, one-time records, and the chosen paths (kept as markers).
fn scrub_desktop(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object_mut()?;
    for key in [
        "settings_window",
        "migrations",
        "profile_existed",
        "onboarding",
        "whats_new_seen",
        "settings",
    ] {
        object.remove(key);
    }
    for (key, marker) in [("config_file", "@config"), ("user_dir", "@user")] {
        if object.contains_key(key) {
            object.insert(key.into(), marker.into());
        }
    }
    serde_json::to_vec_pretty(&value).ok()
}

/// Local cost settings without the sources, which each computer turns on
/// after its own disclosure and whose folders belong to it.
fn scrub_cost_settings(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value.as_object_mut()?.remove("sources");
    serde_json::to_vec_pretty(&value).ok()
}

/// Collect the bundle of `locations`. `created_at` and `version` are
/// parameters so the same tree always gives the same manifest.
pub fn collect(
    locations: &Locations,
    created_at: DateTime<Utc>,
    version: &str,
) -> Result<Bundle, BundleError> {
    let io = |path: &Path| BundleError::Io {
        path: home_relative(path),
    };
    let read = |path: &Path| -> Result<Option<Vec<u8>>, BundleError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(io(path)),
        }
    };
    let mut files = BTreeMap::new();
    let mut add = |path: &str, bytes: Option<Vec<u8>>| {
        if let Some(bytes) = bytes {
            files.insert(path.to_owned(), bytes);
        }
    };
    add("config/cuotai.toml", read(&locations.config)?);
    add(
        "config/hooks.toml",
        read(&crate::hooks::Hooks::path_beside(&locations.config))?,
    );
    if let Some(path) = &locations.desktop_settings {
        // A damaged file is left out rather than copied as is.
        add(
            "prefs/desktop.json",
            read(path)?.and_then(|bytes| scrub_desktop(&bytes)),
        );
    }
    if let Some(root) = &locations.data_root {
        let cost = root.join("cost");
        add(
            "prefs/cost-settings.json",
            read(&cost.join("settings.json"))?.and_then(|bytes| scrub_cost_settings(&bytes)),
        );
        add("prefs/pricing.json", read(&cost.join("pricing.json"))?);
    }
    if let Some(user) = &locations.user_dir {
        for (folder, extension) in [("themes", "css"), ("locales", "json")] {
            let Ok(entries) = std::fs::read_dir(user.join(folder)) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| {
                    safe_name(name)
                        && Path::new(name).extension().and_then(|e| e.to_str()) == Some(extension)
                })
                .collect();
            names.sort();
            for name in names {
                let path = user.join(folder).join(&name);
                add(&format!("user/{folder}/{name}"), read(&path)?);
            }
        }
    }
    for (path, bytes) in &files {
        let text = String::from_utf8_lossy(bytes);
        if looks_secret(&text) {
            return Err(BundleError::Secret { path: path.clone() });
        }
    }
    let manifest = Manifest {
        schema: u64::from(SCHEMA),
        created_at: created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        cuotai_version: version.to_owned(),
        os: std::env::consts::OS.to_owned(),
        files: files
            .iter()
            .map(|(path, bytes)| Entry {
                path: path.clone(),
                sha256: sha256(bytes),
                kind: kind_of(path).to_owned(),
            })
            .collect(),
        excluded: EXCLUDED.iter().map(|item| (*item).to_owned()).collect(),
    };
    Ok(Bundle { manifest, files })
}

impl Bundle {
    /// The archive bytes: `manifest.json` first, then the files by path.
    pub fn to_zip(&self) -> Vec<u8> {
        let mut manifest = serde_json::to_vec_pretty(&self.manifest).expect("serializable");
        manifest.push(b'\n');
        let mut entries = vec![(MANIFEST.to_owned(), manifest)];
        entries.extend(self.files.iter().map(|(p, b)| (p.clone(), b.clone())));
        zip::write(&entries)
    }

    /// Read and verify an archive: known schema, safe paths only, every
    /// file listed once with its digest, nothing unlisted.
    pub fn from_zip(bytes: &[u8]) -> Result<Self, BundleError> {
        let entries = zip::read(bytes).map_err(|_| BundleError::Invalid)?;
        let mut files = BTreeMap::new();
        let mut manifest = None;
        for (name, data) in entries {
            if name == MANIFEST {
                if manifest.is_some() {
                    return Err(BundleError::Invalid);
                }
                manifest = Some(data);
            } else if files.insert(name, data).is_some() {
                return Err(BundleError::Invalid);
            }
        }
        let value: serde_json::Value = manifest
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or(BundleError::Invalid)?;
        let schema = value["schema"].as_u64().ok_or(BundleError::Invalid)?;
        if schema != u64::from(SCHEMA) {
            return Err(BundleError::Schema { schema });
        }
        let manifest: Manifest = serde_json::from_value(value).map_err(|_| BundleError::Invalid)?;
        let probe = Locations {
            config: PathBuf::from("probe").join("cuotai.toml"),
            desktop_settings: Some(PathBuf::from("probe")),
            user_dir: Some(PathBuf::from("probe")),
            data_root: Some(PathBuf::from("probe")),
        };
        let listed: BTreeSet<&str> = manifest.files.iter().map(|e| e.path.as_str()).collect();
        if listed.len() != manifest.files.len() || listed.len() != files.len() {
            return Err(BundleError::Invalid);
        }
        for entry in &manifest.files {
            if probe.target(&entry.path).is_none() {
                return Err(BundleError::Invalid);
            }
            let data = files.get(&entry.path).ok_or(BundleError::Invalid)?;
            if sha256(data) != entry.sha256 {
                return Err(BundleError::Digest {
                    path: entry.path.clone(),
                });
            }
        }
        Ok(Self { manifest, files })
    }
}

/// Write `bytes` to `path` atomically, as a private file.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), BundleError> {
    replace(path, bytes, None, true)
}

/// Replace `path` with `bytes` through a synced temporary file in the same
/// folder; with `expected`, only while the file still holds those bytes
/// (`None` inside means it must not exist). New files are `0600` when
/// `private`; an existing file keeps its permissions.
fn replace(
    path: &Path,
    bytes: &[u8],
    expected: Option<&Option<Vec<u8>>>,
    private: bool,
) -> Result<(), BundleError> {
    use std::io::Write;
    let io = || BundleError::Io {
        path: home_relative(path),
    };
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|_| io())?;
    let current = || match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(io()),
    };
    let conflict = || BundleError::Conflict {
        path: home_relative(path),
    };
    if expected.is_some_and(|expected| current().ok().as_ref() != Some(expected)) {
        return Err(conflict());
    }
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|_| io())?;
    match std::fs::metadata(path) {
        Ok(meta) => std::fs::set_permissions(tmp.path(), meta.permissions()).map_err(|_| io())?,
        Err(_) if private => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| io())?;
            }
        }
        Err(_) => {}
    }
    tmp.write_all(bytes).map_err(|_| io())?;
    tmp.as_file().sync_all().map_err(|_| io())?;
    if expected.is_some_and(|expected| current().ok().as_ref() != Some(expected)) {
        return Err(conflict());
    }
    crate::private_fs::persist(tmp, path).map_err(|_| io())?;
    Ok(())
}

/// What a restore does with one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Missing here: it is created.
    Add,
    /// Different here: it is replaced (settings are merged over).
    Replace,
    /// Identical here: nothing to do.
    Keep,
    /// Changed here after the backup was made: kept unless confirmed.
    Skip,
}

/// Accounts of the backup's TOML against this machine's, by stable id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AccountChanges {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
}

/// One file of a restore plan.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// The bundle path (`config/cuotai.toml`).
    pub path: String,
    /// Where it goes here, with `~` for the home folder.
    pub target: String,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounts: Option<AccountChanges>,
    #[serde(skip)]
    target_path: PathBuf,
    #[serde(skip)]
    current: Option<Vec<u8>>,
    #[serde(skip)]
    new: Vec<u8>,
}

/// What a restore would do, and what the person still has to provide.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub created_at: String,
    pub cuotai_version: String,
    pub items: Vec<Item>,
    /// Variables to set and sign-ins to redo, from the backup's accounts.
    pub needs: Vec<Localized>,
}

/// Which part of a bundle a restore takes (`--only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Config,
    Prefs,
    User,
}

impl Part {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "config" => Some(Self::Config),
            "prefs" => Some(Self::Prefs),
            "user" => Some(Self::User),
            _ => None,
        }
    }

    fn holds(self, path: &str) -> bool {
        let folder = match self {
            Self::Config => "config/",
            Self::Prefs => "prefs/",
            Self::User => "user/",
        };
        path.starts_with(folder)
    }
}

/// `local` with the members of `incoming` over it, leaving the members that
/// belong to this machine (the markers stand for them).
fn merge_desktop(local: Option<&[u8]>, incoming: &[u8]) -> Option<Vec<u8>> {
    let mut merged: serde_json::Map<String, serde_json::Value> = match local {
        Some(bytes) => serde_json::from_slice(bytes).ok()?,
        None => serde_json::Map::new(),
    };
    let incoming: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(incoming).ok()?;
    for (key, value) in incoming {
        if matches!(value.as_str(), Some("@config" | "@user")) {
            continue;
        }
        merged.insert(key, value);
    }
    serde_json::to_vec_pretty(&merged).ok()
}

fn account_changes(local: Option<&[u8]>, incoming: &[u8]) -> Option<AccountChanges> {
    let read = |bytes: &[u8]| {
        let text = std::str::from_utf8(bytes).ok()?;
        let config = config_file::parse(text, ConfigParseMode::Tolerant)
            .ok()?
            .config;
        Some(
            config
                .accounts
                .iter()
                .map(|account| {
                    (
                        account.id().as_str().to_owned(),
                        toml::to_string(account).ok(),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        )
    };
    let incoming = read(incoming)?;
    let local = local.and_then(read).unwrap_or_default();
    Some(AccountChanges {
        added: incoming
            .keys()
            .filter(|id| !local.contains_key(*id))
            .count(),
        changed: incoming
            .iter()
            .filter(|(id, text)| local.get(*id).is_some_and(|local| local != *text))
            .count(),
        removed: local
            .keys()
            .filter(|id| !incoming.contains_key(*id))
            .count(),
    })
}

/// The variables and sign-ins the backup's accounts need on this machine.
fn needs(config: &[u8], var: &dyn Fn(&str) -> Option<String>) -> Vec<Localized> {
    let Some(parsed) = std::str::from_utf8(config)
        .ok()
        .and_then(|text| config_file::parse(text, ConfigParseMode::Tolerant).ok())
    else {
        return Vec::new();
    };
    let mut needs = Vec::new();
    for (index, account) in parsed.config.accounts.iter().enumerate() {
        let who = [
            ("number", ParamValue::from(index + 1)),
            ("provider", ParamValue::from(account.provider.slug())),
        ];
        if account.oauth.is_some() || account.session.is_some() {
            needs.push(Localized::with_params(
                "bundle.need.sign_in",
                params(who),
                "Sign in again to account {number} ({provider}).",
            ));
        } else if let Some(variable) = account.credential_env.as_deref() {
            if var(variable).is_none_or(|value| value.is_empty()) {
                let [number, provider] = who;
                needs.push(Localized::with_params(
                    "bundle.need.env",
                    params([number, provider, ("variable", ParamValue::from(variable))]),
                    "Set {variable} for account {number} ({provider}).",
                ));
            }
        }
    }
    needs
}

/// The restore plan of `bundle` over `locations`.
pub fn plan(
    bundle: &Bundle,
    locations: &Locations,
    only: Option<Part>,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<Plan, BundleError> {
    let created_at = DateTime::parse_from_rfc3339(&bundle.manifest.created_at)
        .map(|at| at.with_timezone(&Utc))
        .ok();
    let mut items = Vec::new();
    for (path, bytes) in &bundle.files {
        if only.is_some_and(|part| !part.holds(path)) {
            continue;
        }
        let Some(target) = locations.target(path) else {
            continue;
        };
        let current = match std::fs::read(&target) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => {
                return Err(BundleError::Io {
                    path: home_relative(&target),
                })
            }
        };
        let new = if path == "prefs/desktop.json" {
            merge_desktop(current.as_deref(), bytes).ok_or(BundleError::Invalid)?
        } else {
            bytes.clone()
        };
        let changed_since = || {
            let modified = std::fs::metadata(&target).and_then(|m| m.modified()).ok();
            match (modified, created_at) {
                (Some(modified), Some(created)) => DateTime::<Utc>::from(modified) > created,
                _ => false,
            }
        };
        let action = match &current {
            None => Action::Add,
            Some(current) if *current == new => Action::Keep,
            Some(_) if changed_since() => Action::Skip,
            Some(_) => Action::Replace,
        };
        let accounts = (path == "config/cuotai.toml" && action != Action::Keep)
            .then(|| account_changes(current.as_deref(), bytes))
            .flatten();
        items.push(Item {
            path: path.clone(),
            target: home_relative(&target),
            action,
            accounts,
            target_path: target,
            current,
            new,
        });
    }
    let needs = bundle
        .files
        .get("config/cuotai.toml")
        .filter(|_| only.is_none_or(|part| part == Part::Config))
        .map(|config| needs(config, var))
        .unwrap_or_default();
    Ok(Plan {
        created_at: bundle.manifest.created_at.clone(),
        cuotai_version: bundle.manifest.cuotai_version.clone(),
        items,
        needs,
    })
}

impl Plan {
    /// Apply the items `confirm` accepts among `replace` and `skip` ones;
    /// `add` items are always written and `keep` items never. Every write
    /// is atomic and refused when the file changed since the plan was
    /// made. Returns how many files were written.
    pub fn apply(&self, mut confirm: impl FnMut(&Item) -> bool) -> Result<usize, BundleError> {
        let mut written = 0;
        for item in &self.items {
            let go = match item.action {
                Action::Keep => false,
                Action::Add => true,
                Action::Replace | Action::Skip => confirm(item),
            };
            if !go {
                continue;
            }
            if item.path == "prefs/desktop.json" {
                let result = crate::private_fs::update_locked(&item.target_path, |current| {
                    if current != item.current {
                        return Err(());
                    }
                    Ok(item.new.clone())
                });
                match result {
                    Ok(()) => {}
                    Err(crate::private_fs::UpdateError::Refused(())) => {
                        return Err(BundleError::Conflict {
                            path: item.target.clone(),
                        })
                    }
                    Err(_) => {
                        return Err(BundleError::Io {
                            path: item.target.clone(),
                        })
                    }
                }
            } else {
                let private = item.path.starts_with("prefs/");
                replace(&item.target_path, &item.new, Some(&item.current), private)?;
            }
            written += 1;
        }
        Ok(written)
    }

    /// Whether applying needs a decision: a file would be replaced.
    pub fn has_conflicts(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item.action, Action::Replace | Action::Skip))
    }
}

#[cfg(test)]
mod tests;
