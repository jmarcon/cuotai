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
//! | `prefs/desktop.json` | desktop settings | window geometry, one-time records and the data folder dropped; the chosen paths become `@config`/`@user` markers, never taken on restore |
//! | `prefs/cost-settings.json` | local cost settings | the per-machine sources (and their folders) dropped |
//! | `prefs/pricing.json` | your local prices | verbatim |
//! | `prefs/cost-labels.json` | the labels and hidden projects of the local cost index | only the person's labels (never a folder name) |
//! | `user/themes/*.css`, `user/locales/*.json` | user themes and languages | verbatim |
//! | `history/<account id>.csv` | with `--with-history`: the last 90 days of each account | the PRD 0006 CSV sample table |
//!
//! Vault entries, tokens, the server token, the cost index itself (only its
//! labels), extensions and logs are never read. Every candidate is scanned for token-like strings
//! and the backup stops when one is found (defence in depth).
//!
//! A restore reads the archive whole, refuses entries outside these
//! folders, checks every digest, then plans per file: `add`, `replace`,
//! `keep` (identical) or `skip` (the local file changed after the backup);
//! it lists the variables to set and the sign-ins to redo, because no
//! secret travels. The history and the cost labels are merged (`merge`):
//! what this computer already has wins. It writes nothing to the vault.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config_file::{self, ConfigIdentity, ConfigParseMode};
use crate::cost::{CostIndex, Labels};
use crate::history::{HistoryStore, ImportCount};
use crate::text::{home_relative, params, Localized, ParamValue};
use crate::Account;

mod zip;

/// The manifest schema this release writes and reads.
pub const SCHEMA: u32 = 1;
/// What a bundle never holds, in the manifest's `excluded`.
pub const EXCLUDED: [&str; 5] = ["vault", "serve_token", "cost_index", "extensions", "logs"];
/// Suggested file name suffix.
pub const EXTENSION: &str = ".cuotai-backup.zip";

/// How far back `--with-history` reaches (the default retention).
pub const HISTORY_DAYS: i64 = 90;
/// Above this many bytes of history the backup warns (RF2.2).
pub const HISTORY_WARN_BYTES: usize = 32 << 20;

const MANIFEST: &str = "manifest.json";
const COST_LABELS: &str = "prefs/cost-labels.json";

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

    /// The CLI's locations for the accounts file `resolved`. A file named
    /// by `--config` or `CUOTAI_CONFIG` is a configuration of its own (a
    /// test, a second profile): its backup and restore never read or write
    /// this computer's desktop settings, user folder or data folder, unless
    /// `CUOTAI_USER_DIR` or `CUOTAI_DATA_DIR` names one explicitly. What
    /// then has no place here is listed in [`Plan::unplaced`].
    pub fn for_cli(resolved: crate::config_file::ResolvedPath) -> Self {
        let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let data_root = env(crate::data_dir::ENV)
            .and_then(|_| crate::data_dir::root())
            .map(Some);
        Self::for_cli_with(
            resolved,
            env("CUOTAI_USER_DIR").map(PathBuf::from),
            data_root,
            Self::from_process,
        )
    }

    /// [`Self::for_cli`] over injected inputs: `user_dir` is
    /// `CUOTAI_USER_DIR`, `data_root` is `Some` when `CUOTAI_DATA_DIR` is
    /// set (holding the folder it resolves to), and `process` gives the
    /// locations of a configuration found by the default rules.
    fn for_cli_with(
        resolved: crate::config_file::ResolvedPath,
        user_dir: Option<PathBuf>,
        data_root: Option<Option<PathBuf>>,
        process: impl FnOnce(PathBuf) -> Self,
    ) -> Self {
        use crate::config_file::PathSource;
        match resolved.source {
            PathSource::Argument | PathSource::Env => Self {
                config: resolved.path,
                desktop_settings: None,
                user_dir,
                data_root: data_root.flatten(),
            },
            _ => process(resolved.path),
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

/// The account id of a `history/<id>.csv` bundle path.
fn history_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("history/")?.strip_suffix(".csv")?;
    crate::account_id::AccountId::parse(id).map(|_| id)
}

/// Whether `path` may appear in a bundle.
fn allowed(path: &str) -> bool {
    let probe = Locations {
        config: PathBuf::from("probe").join("cuotai.toml"),
        desktop_settings: Some(PathBuf::from("probe")),
        user_dir: Some(PathBuf::from("probe")),
        data_root: Some(PathBuf::from("probe")),
    };
    path == COST_LABELS || history_id(path).is_some() || probe.target(path).is_some()
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
        COST_LABELS => "cost_labels",
        path if path.starts_with("history/") => "history",
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

/// Members of `desktop.json` that belong to one machine: the window
/// geometry, one-time records and the data folder. A backup drops them and
/// a restore never takes them, whatever the archive holds.
const DESKTOP_MACHINE_KEYS: [&str; 8] = [
    "settings_window",
    "main_window",
    "migrations",
    "profile_existed",
    "onboarding",
    "whats_new_seen",
    "settings",
    "data_dir",
];

/// The chosen paths of `desktop.json`, which travel as markers only and
/// are never taken from a backup.
const DESKTOP_PATH_KEYS: [(&str, &str); 2] = [("config_file", "@config"), ("user_dir", "@user")];

/// `desktop.json` without what belongs to this machine (see
/// [`DESKTOP_MACHINE_KEYS`]), and the chosen paths kept as markers.
fn scrub_desktop(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object_mut()?;
    for key in DESKTOP_MACHINE_KEYS {
        object.remove(key);
    }
    for (key, marker) in DESKTOP_PATH_KEYS {
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

/// The history a backup carries (`--with-history`, RF2.2): the samples of
/// each of `accounts` taken since `since`.
pub struct HistorySource<'a> {
    pub store: &'a HistoryStore,
    pub accounts: &'a [Account],
    pub since: DateTime<Utc>,
}

/// Collect the bundle of `locations`. `created_at` and `version` are
/// parameters so the same tree always gives the same manifest.
pub fn collect(
    locations: &Locations,
    created_at: DateTime<Utc>,
    version: &str,
) -> Result<Bundle, BundleError> {
    collect_with(locations, created_at, version, None)
}

/// [`collect`], with the history of each account when `history` is set.
pub fn collect_with(
    locations: &Locations,
    created_at: DateTime<Utc>,
    version: &str,
    history: Option<HistorySource<'_>>,
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
        // Only the person's labels: an index that cannot be read (absent,
        // newer, damaged) simply has none to carry.
        let labels = CostIndex::new(cost.join("cost.sqlite3"))
            .labels()
            .ok()
            .filter(|labels| !labels.projects.is_empty() || !labels.sessions.is_empty());
        add(
            COST_LABELS,
            labels.and_then(|labels| serde_json::to_vec_pretty(&labels).ok()),
        );
    }
    if let Some(history) = history {
        for account in history.accounts {
            let csv = history
                .store
                .export_account_csv(account, history.since)
                .map_err(|_| BundleError::Io {
                    path: "history".into(),
                })?;
            add(&format!("history/{}.csv", account.id().as_str()), csv);
        }
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
    /// Bytes of history the bundle carries (RF2.2 warns above
    /// [`HISTORY_WARN_BYTES`]).
    pub fn history_bytes(&self) -> usize {
        self.files
            .iter()
            .filter(|(path, _)| path.starts_with("history/"))
            .map(|(_, bytes)| bytes.len())
            .sum()
    }

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
        let listed: BTreeSet<&str> = manifest.files.iter().map(|e| e.path.as_str()).collect();
        if listed.len() != manifest.files.len() || listed.len() != files.len() {
            return Err(BundleError::Invalid);
        }
        for entry in &manifest.files {
            if !allowed(&entry.path) {
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
    /// History or cost labels: merged, what this computer has wins.
    Merge,
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
    /// For a `merge`: the rows in the backup and how many are new here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge: Option<ImportCount>,
    #[serde(skip)]
    merge_with: Option<MergeWith>,
    #[serde(skip)]
    target_path: PathBuf,
    #[serde(skip)]
    current: Option<Vec<u8>>,
    #[serde(skip)]
    new: Vec<u8>,
}

/// What a `merge` item merges into.
#[derive(Debug, Clone)]
enum MergeWith {
    /// The history of this account (as this computer names it).
    History(Box<Account>),
    Labels(Labels),
}

/// One thing to do after the restore (RF3.3 **Next steps**), beside its
/// text in [`Plan::needs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextStep {
    /// `env` (a variable to set, or in the desktop a key to type),
    /// `sign_in`, or `channel` (a notification channel's address to type
    /// again, PRD 0044).
    pub kind: &'static str,
    /// The account's position in the backup's file (0-based); for a
    /// `channel`, the channel's position in the desktop settings.
    pub account: usize,
    /// The provider slug; for a `channel`, its kind (`ntfy`, `slack`…).
    pub provider: String,
}

/// RF1.2/RF3.1: the notification channels of the backup's desktop settings
/// (PRD 0044). Their definitions travel (id, kind, label, host, filters);
/// the address and signing secret stay in the vault, so each one is a step.
fn channel_needs(desktop: &[u8]) -> Vec<(Localized, NextStep)> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(desktop) else {
        return Vec::new();
    };
    let Some(channels) = value["channels"].as_array() else {
        return Vec::new();
    };
    channels
        .iter()
        .enumerate()
        .filter_map(|(index, channel)| {
            let kind = channel["kind"].as_str()?;
            let label = channel["label"].as_str().unwrap_or(kind);
            Some((
                Localized::with_params(
                    "bundle.need.channel",
                    params([
                        ("label", ParamValue::from(label)),
                        ("kind", ParamValue::from(kind)),
                    ]),
                    "Type the address of the notification channel {label} ({kind}) again.",
                ),
                NextStep {
                    kind: "channel",
                    account: index,
                    provider: kind.to_owned(),
                },
            ))
        })
        .collect()
}

/// What a restore would do, and what the person still has to provide.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub created_at: String,
    pub cuotai_version: String,
    pub items: Vec<Item>,
    /// Variables to set and sign-ins to redo, from the backup's accounts.
    pub needs: Vec<Localized>,
    /// The same, as actions (one per entry of `needs`, in order).
    pub next_steps: Vec<NextStep>,
    /// Bundle paths left out because this configuration has no place for
    /// them (a `--config` without the desktop settings, user folder or data
    /// folder of this computer, see [`Locations::for_cli`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unplaced: Vec<String>,
    #[serde(skip)]
    locations: Locations,
    #[serde(skip)]
    cost_index: Option<PathBuf>,
}

/// Which part of a bundle a restore takes (`--only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Config,
    Prefs,
    User,
    History,
}

impl Part {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "config" => Some(Self::Config),
            "prefs" => Some(Self::Prefs),
            "user" => Some(Self::User),
            "history" => Some(Self::History),
            _ => None,
        }
    }

    fn holds(self, path: &str) -> bool {
        let folder = match self {
            Self::Config => "config/",
            Self::Prefs => "prefs/",
            Self::User => "user/",
            Self::History => "history/",
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
        // This machine's members and paths stay as they are here: a
        // crafted or foreign backup could otherwise move every store.
        if DESKTOP_MACHINE_KEYS.contains(&key.as_str())
            || DESKTOP_PATH_KEYS.iter().any(|(path, _)| *path == key)
        {
            continue;
        }
        // PRD 0044 channels merge by id: this computer's stay (their
        // addresses are in its vault), the backup adds the ones it lacks.
        if key == "channels" {
            if let (Some(local), Some(incoming)) = (
                merged.get("channels").and_then(|v| v.as_array()).cloned(),
                value.as_array(),
            ) {
                let mut channels = local.clone();
                for channel in incoming {
                    if !local.iter().any(|known| known["id"] == channel["id"]) {
                        channels.push(channel.clone());
                    }
                }
                merged.insert(key, serde_json::Value::Array(channels));
                continue;
            }
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
fn needs(config: &[u8], var: &dyn Fn(&str) -> Option<String>) -> Vec<(Localized, NextStep)> {
    let Some(parsed) = std::str::from_utf8(config)
        .ok()
        .and_then(|text| config_file::parse(text, ConfigParseMode::Tolerant).ok())
    else {
        return Vec::new();
    };
    let mut needs = Vec::new();
    let step = |kind, index: usize, account: &Account| NextStep {
        kind,
        account: index,
        provider: account.provider.slug().to_owned(),
    };
    for (index, account) in parsed.config.accounts.iter().enumerate() {
        let who = [
            ("number", ParamValue::from(index + 1)),
            ("provider", ParamValue::from(account.provider.slug())),
        ];
        if account.oauth.is_some() || account.session.is_some() {
            needs.push((
                Localized::with_params(
                    "bundle.need.sign_in",
                    params(who),
                    "Sign in again to account {number} ({provider}).",
                ),
                step("sign_in", index, account),
            ));
        } else if let Some(variable) = account.credential_env.as_deref() {
            if var(variable).is_none_or(|value| value.is_empty()) {
                let [number, provider] = who;
                needs.push((
                    Localized::with_params(
                        "bundle.need.env",
                        params([number, provider, ("variable", ParamValue::from(variable))]),
                        "Set {variable} for account {number} ({provider}).",
                    ),
                    step("env", index, account),
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
    let history = history_store(locations);
    let cost_index = locations
        .data_root
        .as_ref()
        .map(|root| root.join("cost").join("cost.sqlite3"));
    // The accounts a history file may belong to: this computer's, then the
    // backup's (restored in the same run), each with the ids this
    // computer gives them.
    let identity = ConfigIdentity::of_path(&locations.config).ok();
    let read_accounts = |bytes: &[u8]| -> Vec<Account> {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| config_file::parse(text, ConfigParseMode::Tolerant).ok())
            .map(|parsed| {
                let mut config = parsed.config;
                config.resolve_ids(identity.as_ref());
                config.accounts
            })
            .unwrap_or_default()
    };
    let mut known = std::fs::read(&locations.config)
        .map(|bytes| read_accounts(&bytes))
        .unwrap_or_default();
    if let Some(bytes) = bundle.files.get("config/cuotai.toml") {
        known.extend(read_accounts(bytes));
    }
    let mut unplaced = Vec::new();
    for (path, bytes) in &bundle.files {
        if only.is_some_and(|part| !part.holds(path)) {
            continue;
        }
        if path == COST_LABELS || history_id(path).is_some() {
            if locations.data_root.is_none() {
                unplaced.push(path.clone());
                continue;
            }
            if let Some(item) =
                merge_item(path, bytes, &known, history.as_ref(), cost_index.as_deref())?
            {
                items.push(item);
            }
            continue;
        }
        let Some(target) = locations.target(path) else {
            unplaced.push(path.clone());
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
            merge: None,
            merge_with: None,
            target_path: target,
            current,
            new,
        });
    }
    let mut steps = bundle
        .files
        .get("config/cuotai.toml")
        .filter(|_| only.is_none_or(|part| part == Part::Config))
        .map(|config| needs(config, var))
        .unwrap_or_default();
    steps.extend(
        bundle
            .files
            .get("prefs/desktop.json")
            .filter(|_| only.is_none_or(|part| part == Part::Prefs))
            .filter(|_| locations.desktop_settings.is_some())
            .map(|desktop| channel_needs(desktop))
            .unwrap_or_default(),
    );
    let (needs, next_steps) = steps.into_iter().unzip();
    Ok(Plan {
        created_at: bundle.manifest.created_at.clone(),
        cuotai_version: bundle.manifest.cuotai_version.clone(),
        items,
        needs,
        next_steps,
        unplaced,
        locations: locations.clone(),
        cost_index,
    })
}

/// The history store a restore merges into: this configuration's, in the
/// data folder.
fn history_store(locations: &Locations) -> Option<HistoryStore> {
    // A folder that does not exist yet (a fresh computer) is created by
    // the restore; until then its path stands for itself.
    let identity = ConfigIdentity::of_path(&locations.config)
        .unwrap_or_else(|_| ConfigIdentity::File(locations.config.clone()));
    let root = locations.data_root.as_ref()?;
    Some(HistoryStore::new(
        root.join("history").join("history.sqlite3"),
        &identity,
    ))
}

/// The `merge` item of a history file or of the cost labels; `None` when
/// there is nothing to merge into (no account of that id or name, no data
/// folder).
fn merge_item(
    path: &str,
    bytes: &[u8],
    known: &[Account],
    history: Option<&HistoryStore>,
    cost_index: Option<&Path>,
) -> Result<Option<Item>, BundleError> {
    let invalid = || BundleError::Invalid;
    let (merge_with, merge, target) = if let Some(id) = history_id(path) {
        let Some(store) = history else {
            return Ok(None);
        };
        // By id, then (an id derived from another computer's path) by
        // provider and name, as the rows name them.
        let by_name = crate::history::csv_account(bytes);
        let account = known
            .iter()
            .find(|account| account.id().as_str() == id)
            .or_else(|| {
                let (provider, name) = by_name.as_ref()?;
                known
                    .iter()
                    .find(|a| a.provider.slug() == provider && a.name == *name)
            });
        let Some(account) = account else {
            return Ok(None);
        };
        let count = store
            .import_csv(account, bytes, false)
            .map_err(|_| invalid())?;
        (
            MergeWith::History(Box::new(account.clone())),
            count,
            "history",
        )
    } else {
        let Some(index) = cost_index else {
            return Ok(None);
        };
        let labels: Labels = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let rows = labels.projects.len() + labels.sessions.len();
        let new = CostIndex::new(index.to_path_buf())
            .import_labels(&labels, false)
            .unwrap_or(0);
        (
            MergeWith::Labels(labels),
            ImportCount { rows, new },
            "cost_index",
        )
    };
    Ok(Some(Item {
        path: path.to_owned(),
        target: target.to_owned(),
        action: if merge.new == 0 {
            Action::Keep
        } else {
            Action::Merge
        },
        accounts: None,
        merge: Some(merge),
        merge_with: Some(merge_with),
        target_path: PathBuf::new(),
        current: None,
        // A history item keeps its CSV for the merge.
        new: bytes.to_vec(),
    }))
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
                Action::Add | Action::Merge => true,
                Action::Replace | Action::Skip => confirm(item),
            };
            if !go {
                continue;
            }
            if let Some(merge_with) = &item.merge_with {
                let failed = || BundleError::Io {
                    path: item.target.clone(),
                };
                match merge_with {
                    MergeWith::History(account) => {
                        // Again now: the restore may just have created the
                        // configuration's folder.
                        let store = history_store(&self.locations).ok_or_else(failed)?;
                        store
                            .import_csv(account, &item.new, true)
                            .map_err(|_| failed())?;
                    }
                    MergeWith::Labels(labels) => {
                        let index = self.cost_index.as_ref().ok_or_else(failed)?;
                        CostIndex::new(index.clone())
                            .import_labels(labels, true)
                            .map_err(|_| failed())?;
                    }
                }
            } else if item.path == "prefs/desktop.json" {
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
