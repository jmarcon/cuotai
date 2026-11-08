//! PRD 0046 RF4: `cuotai --doctor`, a read-only and offline check of this
//! installation. Every check names what it looked at, its status and, when
//! something is wrong, the fix; nothing is repaired, nothing goes to the
//! network and no secret is read: an environment variable is reported as
//! set or not, never its value, paths under the home folder are shown with
//! `~`, and accounts are named by their position and provider only, so the
//! report can be pasted into an issue.
//!
//! The configuration check reuses `--check-config`'s strict reader
//! ([`crate::config_file::check`]); PRD 0052's diagnostics can add their
//! own checks to the same [`Report`]. The system vault is checked for
//! availability only (no entry is read, so no token's expiry either; the
//! next refresh reports an expired sign-in). [`probe`] adds the opt-in
//! network checks of `--doctor --probe`.

pub mod probe;

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config_file::{self, ConfigFile, ConfigParseMode, PathSource, ResolvedPath};
use crate::text::{home_relative, params, Localized, ParamValue};
use crate::Account;

/// The JSON schema of [`Report`].
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Fail,
    Warn,
    Ok,
    Skip,
}

/// One check: a stable `id`, its status, what was checked (`title`), what
/// was found (`detail`) and what to do (`fix`).
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: String,
    pub status: CheckStatus,
    pub title: Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Localized>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Localized>,
}

impl Check {
    fn new(id: impl Into<String>, status: CheckStatus, title: Localized) -> Self {
        Self {
            id: id.into(),
            status,
            title,
            detail: None,
            fix: None,
        }
    }

    fn detail(mut self, detail: Localized) -> Self {
        self.detail = Some(detail);
        self
    }

    fn fix(mut self, fix: Localized) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// The doctor's report, in check order.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema: u32,
    pub checks: Vec<Check>,
    /// PRD 0054 RF6.3: how each provider CLI the accounts use resolves
    /// without a configured path (`tools.aws { status, source, path }`).
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tools: std::collections::BTreeMap<&'static str, crate::external_tool::ToolReport>,
}

impl Report {
    /// `0` without a failure (warnings included), `2` with one.
    pub fn exit_code(&self) -> u8 {
        if self.checks.iter().any(|c| c.status == CheckStatus::Fail) {
            2
        } else {
            0
        }
    }
}

/// Looks up an environment variable (only whether it is set is used).
pub type VarLookup = Box<dyn Fn(&str) -> Option<String>>;

/// Whether the system vault answers (no entry is read).
pub type VaultProbe = Box<dyn Fn() -> bool>;

/// What the process learned of the servers' clocks: the current warning
/// and whether any server date was compared at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClockState {
    pub warning: Option<crate::network::Skew>,
    pub observed: bool,
}

/// [`ClockState`] when the doctor runs (after `--probe`, if any).
pub type ClockProbe = Box<dyn Fn() -> ClockState>;

/// What the doctor looks at, injectable so tests never read the real
/// environment or OS folders.
pub struct Inputs {
    /// The configuration file the other modes would read.
    pub config: ResolvedPath,
    /// An environment variable's value (only whether it is set is used).
    pub var: VarLookup,
    /// [`crate::data_dir::check_override`].
    pub data_override: Result<Option<PathBuf>, crate::data_dir::DataDirError>,
    /// [`crate::data_dir::root`].
    pub data_root: Option<PathBuf>,
    /// The desktop app's `desktop.json`.
    pub desktop_settings: Option<PathBuf>,
    /// The local server port to probe (bound and released at once).
    pub port: u16,
    /// Version, OS and architecture, and where this copy came from.
    pub version: String,
    pub install_source: String,
    /// PRD 0054 RF6.2: where the provider CLIs are looked for.
    pub resolver: crate::external_tool::Resolver,
    /// The system vault's availability, asked only when an account keeps
    /// its sign-in there.
    pub vault: VaultProbe,
    /// [`crate::extensions::default_dir`].
    pub extensions_dir: Option<PathBuf>,
    /// The home folder and variables the local cost sources follow.
    pub cost_env: crate::cost::Env,
    /// The servers' clock as this process saw it.
    pub clock: ClockProbe,
    /// The desktop's notification permission (`None` outside the desktop).
    pub notifications: Option<bool>,
    /// The language the front end shows.
    pub locale: String,
    pub now: DateTime<Utc>,
}

impl Inputs {
    /// The process's own inputs; `config` is `--config`.
    pub fn from_process(config: Option<PathBuf>, install_source: String) -> Self {
        Self {
            config: ConfigFile::resolve_path(
                config_file::Discovery::Cli { config },
                &config_file::Locations::from_process(),
            ),
            var: Box::new(|name| std::env::var(name).ok()),
            data_override: crate::data_dir::check_override(),
            data_root: crate::data_dir::root(),
            desktop_settings: dirs::config_dir().map(|dir| dir.join("cuotai").join("desktop.json")),
            port: 6738,
            version: crate::build_info::display_version(),
            install_source,
            resolver: crate::external_tool::Resolver::from_process(),
            vault: Box::new(vault_available),
            extensions_dir: crate::extensions::default_dir(),
            cost_env: crate::cost::Env::system(),
            clock: Box::new(|| {
                let skew = crate::network::clock_skew();
                ClockState {
                    warning: skew.warning(),
                    observed: skew.observed(),
                }
            }),
            notifications: None,
            locale: String::new(),
            now: Utc::now(),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn vault_available() -> bool {
    crate::oauth::SystemTokenStore::available()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn vault_available() -> bool {
    false
}

fn path_param(path: &Path) -> ParamValue {
    ParamValue::from(home_relative(path))
}

fn source_slug(source: PathSource) -> &'static str {
    match source {
        PathSource::Argument => "--config",
        PathSource::Env => "CUOTAI_CONFIG",
        PathSource::Chosen => "desktop",
        PathSource::Local => "./cuotai.toml",
        PathSource::System => "default",
    }
}

fn fixed(id: &'static str, fallback: &'static str) -> Localized {
    Localized::fixed(id, fallback)
}

fn check_config(inputs: &Inputs, checks: &mut Vec<Check>) -> Option<crate::Config> {
    let path = &inputs.config.path;
    let title = Localized::with_params(
        "doctor.title.config",
        params([
            ("path", path_param(path)),
            (
                "source",
                ParamValue::from(source_slug(inputs.config.source)),
            ),
        ]),
        "Configuration file {path} ({source})",
    );
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            checks.push(
                Check::new("config", CheckStatus::Warn, title)
                    .detail(fixed(
                        "doctor.config.missing",
                        "There is no file yet, so no account is configured.",
                    ))
                    .fix(fixed(
                        "doctor.fix.add_account",
                        "Add an account in the desktop app, or run cuotai to open the wizard.",
                    )),
            );
            return None;
        }
        Err(_) => {
            checks.push(
                Check::new("config", CheckStatus::Fail, title)
                    .detail(fixed(
                        "doctor.state.damaged",
                        "It cannot be read, or it failed its integrity check.",
                    ))
                    .fix(check_config_fix()),
            );
            return None;
        }
    };
    let strict = config_file::check_bytes(&bytes);
    let tolerant = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| config_file::parse(text, ConfigParseMode::Tolerant).ok());
    match (strict.diagnostics.first(), tolerant) {
        (None, Some(parsed)) => {
            checks.push(Check::new("config", CheckStatus::Ok, title).detail(
                Localized::with_param(
                    "doctor.config.valid",
                    "count",
                    parsed.config.accounts.len(),
                    "Valid; accounts: {count}.",
                ),
            ));
            Some(parsed.config)
        }
        // Valid for the runtime reader only: some accounts or keys need a
        // newer Cuotai (PRD 0051 RF1).
        (Some(_), Some(parsed)) if parsed.config.validate().is_ok() => {
            let mut check = Check::new("config", CheckStatus::Warn, title).fix(check_config_fix());
            check.detail = parsed
                .issues
                .first()
                .map(|issue| issue.l10n())
                .or_else(|| parsed.warnings.first().map(|warning| warning.l10n()));
            checks.push(check);
            Some(parsed.config)
        }
        (diagnostic, _) => {
            let mut check = Check::new("config", CheckStatus::Fail, title).fix(check_config_fix());
            check.detail = diagnostic.map(|diagnostic| diagnostic.note.clone());
            checks.push(check);
            None
        }
    }
}

fn check_config_fix() -> Localized {
    fixed(
        "doctor.fix.check_config",
        "Run cuotai --check-config to see the line and key, then fix them in the file.",
    )
}

fn check_account(index: usize, account: &Account, inputs: &Inputs) -> Check {
    let title = Localized::with_params(
        "doctor.title.account",
        params([
            ("number", ParamValue::from(index + 1)),
            ("provider", ParamValue::from(account.provider.slug())),
        ]),
        "Account {number} ({provider})",
    );
    let id = format!("account.{}", index + 1);
    let set_env = |variable: &str| {
        Localized::with_param(
            "doctor.fix.set_env",
            "variable",
            variable,
            "Set {variable} in the environment that starts Cuotai, or store the key in the vault from the account editor.",
        )
    };
    if account.oauth.is_some() || account.session.is_some() {
        return Check::new(id, CheckStatus::Ok, title).detail(fixed(
            "doctor.account.vault",
            "It signs in through the system vault, which the next refresh checks.",
        ));
    }
    if let Some(extension) = account.extension_id() {
        let manager = inputs
            .extensions_dir
            .as_ref()
            .map(|dir| crate::extensions::management::Manager::at(dir.clone()));
        return match manager.map(|manager| manager.is_enabled(extension)) {
            Some(Ok(true)) => Check::new(id, CheckStatus::Ok, title).detail(fixed(
                "doctor.account.extension_ok",
                "Its extension is approved and unchanged since.",
            )),
            Some(Ok(false)) => Check::new(id, CheckStatus::Warn, title)
                .detail(fixed(
                    "doctor.account.extension_changed",
                    "Its extension changed since you approved it, or was never approved, so it does not run.",
                ))
                .fix(fixed(
                    "doctor.fix.extension",
                    "Review and approve the extension again in Extensions, or run cuotai --extension-check with its id.",
                )),
            Some(Err(reason)) => Check::new(id, CheckStatus::Fail, title)
                .detail(reason)
                .fix(fixed(
                    "doctor.fix.extension",
                    "Review and approve the extension again in Extensions, or run cuotai --extension-check with its id.",
                )),
            None => Check::new(id, CheckStatus::Fail, title).detail(fixed(
                "doctor.account.extension_missing",
                "There is no extensions folder on this computer.",
            )),
        };
    }
    if let Some(variable) = account.credential_env.as_deref() {
        let set = (inputs.var)(variable).is_some_and(|value| !value.is_empty());
        let detail = |id, fallback| Localized::with_param(id, "variable", variable, fallback);
        return if set {
            Check::new(id, CheckStatus::Ok, title).detail(detail(
                "doctor.account.env_set",
                "It reads {variable}, which is set.",
            ))
        } else {
            Check::new(id, CheckStatus::Fail, title)
                .detail(detail(
                    "doctor.account.env_missing",
                    "It reads {variable}, which is not set in this environment.",
                ))
                .fix(set_env(variable))
        };
    }
    let default = crate::account_edit::default_env(account.provider);
    if default.is_empty() {
        return Check::new(id, CheckStatus::Skip, title).detail(fixed(
            "doctor.account.local",
            "It uses a local tool, file or profile, which the next refresh checks.",
        ));
    }
    if (inputs.var)(default).is_some_and(|value| !value.is_empty()) {
        Check::new(id, CheckStatus::Ok, title).detail(Localized::with_param(
            "doctor.account.env_set",
            "variable",
            default,
            "It reads {variable}, which is set.",
        ))
    } else {
        Check::new(id, CheckStatus::Warn, title)
            .detail(Localized::with_param(
                "doctor.account.default_env_missing",
                "variable",
                default,
                "It names no variable, and the provider's default {variable} is not set; ignore this if it signs in another way.",
            ))
            .fix(set_env(default))
    }
}

/// PRD 0054 RF6.3: the program a CLI-backed account starts, found or not.
fn check_tool(index: usize, account: &Account, inputs: &Inputs) -> Option<Check> {
    let tool = crate::external_tool::Tool::for_provider(account.provider)?;
    let configured = account
        .params
        .get(crate::external_tool::CLI_PATH_PARAM)
        .map(String::as_str);
    let result = inputs.resolver.resolve(tool, configured);
    let title = Localized::with_params(
        "doctor.title.tool",
        params([
            ("tool", ParamValue::from(tool.name())),
            ("number", ParamValue::from(index + 1)),
        ]),
        "{tool} program of account {number}",
    );
    let id = format!("account.{}.tool", index + 1);
    Some(match &result {
        Ok(resolved) => Check::new(id, CheckStatus::Ok, title).detail(Localized::with_params(
            "doctor.tool.found",
            params([
                ("source", ParamValue::from(resolved.source.slug())),
                ("path", path_param(&resolved.program)),
            ]),
            "Found ({source}): {path}.",
        )),
        Err(error) => Check::new(id, CheckStatus::Fail, title)
            .detail(error.l10n())
            .fix(Localized::with_param(
                "doctor.fix.tool",
                "tool",
                tool.name(),
                "Install {tool}, or set params.cli_path to the full path of the program in the account editor.",
            )),
    })
}

fn check_data_dir(inputs: &Inputs) -> Check {
    match (&inputs.data_override, &inputs.data_root) {
        (Err(error), _) => Check::new(
            "data_dir",
            CheckStatus::Fail,
            Localized::with_param(
                "doctor.title.data_dir",
                "path",
                crate::data_dir::ENV,
                "Data folder {path}",
            ),
        )
        .detail(error.l10n())
        .fix(fixed(
            "doctor.fix.data_dir",
            "Unset CUOTAI_DATA_DIR, or point it to an absolute folder of your own.",
        )),
        (Ok(_), Some(root)) => Check::new(
            "data_dir",
            CheckStatus::Ok,
            Localized::with_param(
                "doctor.title.data_dir",
                "path",
                path_param(root),
                "Data folder {path}",
            ),
        ),
        (Ok(_), None) => Check::new(
            "data_dir",
            CheckStatus::Fail,
            Localized::with_param("doctor.title.data_dir", "path", "—", "Data folder {path}"),
        )
        .fix(fixed(
            "doctor.fix.data_dir",
            "Unset CUOTAI_DATA_DIR, or point it to an absolute folder of your own.",
        )),
    }
}

/// Human file size with binary units, as the history ceiling.
fn size_text(bytes: u64) -> String {
    if bytes < 1 << 20 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / f64::from(1 << 20))
    }
}

fn absent() -> Localized {
    fixed("doctor.state.absent", "It has not been created yet.")
}

fn newer() -> Localized {
    fixed(
        "doctor.state.newer",
        "A newer version of Cuotai wrote it in a format this one does not know; it is left untouched.",
    )
}

fn damaged() -> Localized {
    fixed(
        "doctor.state.damaged",
        "It cannot be read, or it failed its integrity check.",
    )
}

fn update_fix() -> Localized {
    fixed("doctor.fix.update", "Update Cuotai.")
}

fn damaged_fix() -> Localized {
    fixed(
        "doctor.fix.damaged",
        "Quit Cuotai and move the file aside; a .bak copy beside it may hold the previous version.",
    )
}

/// A SQLite store opened read-only: its size, migration count and state
/// major, and `PRAGMA quick_check`.
fn check_sqlite(id: &str, title: Localized, path: Option<PathBuf>, known: i64) -> Check {
    let Some(path) = path.filter(|path| path.is_file()) else {
        return Check::new(id, CheckStatus::Skip, title).detail(absent());
    };
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let opened = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    );
    let read = opened.and_then(|db| {
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let major = crate::private_fs::sqlite_major(&db)?;
        let integrity: String = db.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        Ok((version, major, integrity))
    });
    match read {
        Ok((version, major, _))
            if major.is_some_and(|major| major > crate::private_fs::SQLITE_MAJOR)
                || (version > known && major.is_none()) =>
        {
            Check::new(id, CheckStatus::Warn, title)
                .detail(newer())
                .fix(update_fix())
        }
        Ok((version, _, integrity)) if integrity == "ok" => Check::new(id, CheckStatus::Ok, title)
            .detail(Localized::with_params(
                "doctor.state.ok",
                params([
                    ("size", ParamValue::from(size_text(size))),
                    ("version", ParamValue::from(version.max(0) as u64)),
                ]),
                "{size}, version {version}, intact.",
            )),
        _ => Check::new(id, CheckStatus::Fail, title)
            .detail(damaged())
            .fix(damaged_fix()),
    }
}

fn check_hooks(config: &Path) -> Check {
    let path = crate::hooks::Hooks::path_beside(config);
    let title = Localized::with_param(
        "doctor.title.hooks",
        "path",
        path_param(&path),
        "Hook rules {path}",
    );
    if !path.is_file() {
        return Check::new("hooks", CheckStatus::Skip, title).detail(absent());
    }
    match crate::hooks::Hooks::load(&path) {
        Ok(hooks) => Check::new("hooks", CheckStatus::Ok, title).detail(Localized::with_param(
            "doctor.hooks.ok",
            "count",
            hooks.rules().len(),
            "Valid; rules: {count}.",
        )),
        Err(error) => Check::new("hooks", CheckStatus::Fail, title)
            .detail(error)
            .fix(fixed(
                "doctor.fix.hooks",
                "Fix the file or remove it; hooks run only after you confirm their rules.",
            )),
    }
}

fn check_desktop_settings(path: Option<&Path>) -> Check {
    let title = Localized::with_param(
        "doctor.title.desktop_settings",
        "path",
        path.map_or(ParamValue::from("—"), path_param),
        "Desktop settings {path}",
    );
    let Some(bytes) = path.and_then(|path| std::fs::read(path).ok()) else {
        return Check::new("desktop_settings", CheckStatus::Skip, title).detail(absent());
    };
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) if value.is_object() => {
            if value["schema"].as_u64().is_some_and(|major| major > 1) {
                Check::new("desktop_settings", CheckStatus::Warn, title)
                    .detail(newer())
                    .fix(update_fix())
            } else {
                Check::new("desktop_settings", CheckStatus::Ok, title)
            }
        }
        _ => Check::new("desktop_settings", CheckStatus::Warn, title)
            .detail(damaged())
            .fix(damaged_fix()),
    }
}

/// PRD 0003 COMP-02 / PRD 0051 RF3: the notice ledger. A damaged one was
/// set aside as `notified.json.corrupt-<time>`, which stays until the user
/// deletes it; a later schema major is never rewritten.
fn check_ledger(root: Option<&Path>) -> Check {
    let title = fixed("doctor.title.ledger", "Sent-notice ledger (notified.json)");
    let Some(root) = root else {
        return Check::new("ledger", CheckStatus::Skip, title).detail(absent());
    };
    let mut aside: Vec<String> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("notified.json.corrupt"))
                .collect()
        })
        .unwrap_or_default();
    aside.sort();
    if let Some(latest) = aside.last() {
        return Check::new("ledger", CheckStatus::Warn, title)
            .detail(Localized::with_params(
                "doctor.ledger.quarantined",
                params([
                    ("count", ParamValue::from(aside.len())),
                    ("file", ParamValue::from(latest.clone())),
                ]),
                "A damaged ledger was set aside ({count} file(s), the latest {file}); a notice may repeat once.",
            ))
            .fix(fixed(
                "doctor.fix.ledger",
                "Nothing to restore: delete the notified.json.corrupt-* files in the data folder once you have looked at them.",
            ));
    }
    match std::fs::read(root.join("notified.json")) {
        Err(_) => Check::new("ledger", CheckStatus::Skip, title).detail(absent()),
        Ok(bytes) => {
            match crate::private_fs::json_state_version(&bytes, crate::resets::LEDGER_SCHEMA) {
                Some(version) if version.major > crate::resets::LEDGER_SCHEMA => {
                    Check::new("ledger", CheckStatus::Warn, title)
                        .detail(newer())
                        .fix(update_fix())
                }
                Some(_) => Check::new("ledger", CheckStatus::Ok, title),
                None => Check::new("ledger", CheckStatus::Warn, title)
                    .detail(damaged())
                    .fix(damaged_fix()),
            }
        }
    }
}

fn check_serve_token(root: Option<&Path>) -> Check {
    let title = fixed("doctor.title.serve_token", "Local server token");
    let Some(meta) = root
        .map(|root| root.join("serve").join("token"))
        .and_then(|path| std::fs::symlink_metadata(path).ok())
    else {
        return Check::new("serve_token", CheckStatus::Skip, title).detail(absent());
    };
    if !meta.is_file()
        || crate::private_fs::open_to_others(&meta)
        || crate::private_fs::foreign_owner(&meta)
    {
        Check::new("serve_token", CheckStatus::Fail, title)
            .detail(fixed(
                "doctor.token.open",
                "Other users can read or replace it.",
            ))
            .fix(fixed(
                "doctor.fix.token",
                "Delete the file; the local server creates a private one on its next start.",
            ))
    } else {
        Check::new("serve_token", CheckStatus::Ok, title)
            .detail(fixed("doctor.token.private", "It is private to your user."))
    }
}

fn check_port(port: u16) -> Check {
    let title = Localized::with_param(
        "doctor.title.port",
        "port",
        port,
        "Local server port {port}",
    );
    match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => {
            drop(listener);
            Check::new("port", CheckStatus::Ok, title)
                .detail(fixed("doctor.port.free", "It is free."))
        }
        Err(_) => Check::new("port", CheckStatus::Warn, title)
            .detail(fixed(
                "doctor.port.taken",
                "It is in use, perhaps by a running Cuotai server.",
            ))
            .fix(fixed(
                "doctor.fix.port",
                "Stop the other server, or start this one with --port and another port.",
            )),
    }
}

/// The system vault, when an account keeps its sign-in there.
fn check_vault(config: Option<&crate::Config>, inputs: &Inputs) -> Check {
    let title = fixed("doctor.title.vault", "System vault");
    let uses = config.is_some_and(|config| {
        config
            .accounts
            .iter()
            .any(|account| account.oauth.is_some() || account.session.is_some())
    });
    if !uses {
        return Check::new("vault", CheckStatus::Skip, title).detail(fixed(
            "doctor.vault.unused",
            "No account keeps its sign-in there.",
        ));
    }
    if (inputs.vault)() {
        Check::new("vault", CheckStatus::Ok, title)
            .detail(fixed("doctor.vault.ok", "It answers; no entry was read."))
    } else {
        Check::new("vault", CheckStatus::Fail, title)
            .detail(fixed(
                "doctor.vault.unavailable",
                "It does not answer, so the accounts that sign in through it cannot be read.",
            ))
            .fix(fixed(
                "doctor.fix.vault",
                "Unlock the system keychain (on Linux, start a Secret Service such as GNOME Keyring or KWallet), then try again.",
            ))
    }
}

/// The local cost sources that are on, and whether their folders can be
/// listed (nothing in them is read).
fn check_cost_sources(inputs: &Inputs) -> Vec<Check> {
    let Some(root) = inputs.data_root.as_ref() else {
        return Vec::new();
    };
    let store = crate::cost::SettingsStore::at(root.join("cost").join("settings.json"));
    let Ok(settings) = store.load() else {
        return vec![Check::new(
            "cost_settings",
            CheckStatus::Warn,
            fixed("doctor.title.cost_settings", "Local cost settings"),
        )
        .detail(damaged())
        .fix(damaged_fix())];
    };
    settings
        .enabled_sources(&inputs.cost_env)
        .into_iter()
        .map(|source| {
            let title = Localized::with_param(
                "doctor.title.cost_source",
                "source",
                source.slug(),
                "Local cost source {source}",
            );
            let id = format!("cost_source.{}", source.slug());
            let readable = settings
                .roots(source, &inputs.cost_env)
                .iter()
                .any(|dir| std::fs::read_dir(dir).is_ok());
            if readable {
                Check::new(id, CheckStatus::Ok, title).detail(fixed(
                    "doctor.cost_source.ok",
                    "Its folder can be read.",
                ))
            } else {
                Check::new(id, CheckStatus::Warn, title)
                    .detail(fixed(
                        "doctor.cost_source.unreadable",
                        "None of its folders exists or can be read, so it adds nothing.",
                    ))
                    .fix(fixed(
                        "doctor.fix.cost_source",
                        "Choose the right folder in Settings › Advanced › Local cost, or turn the source off.",
                    ))
            }
        })
        .collect()
}

/// The clock: the servers' dates this process compared (RF7 of PRD 0052),
/// else the newest stored reading dated more than five minutes ahead.
fn check_clock(inputs: &Inputs) -> Check {
    let title = fixed("doctor.title.clock", "Clock");
    let state = (inputs.clock)();
    if let Some(skew) = state.warning {
        return Check::new("clock", CheckStatus::Warn, title)
            .detail(skew.l10n())
            .fix(fixed(
                "doctor.fix.clock",
                "Turn on automatic date and time in the system settings.",
            ));
    }
    if state.observed {
        return Check::new("clock", CheckStatus::Ok, title).detail(fixed(
            "doctor.clock.ok",
            "It agrees with the servers' clocks.",
        ));
    }
    let newest = inputs
        .data_root
        .as_ref()
        .map(|root| root.join("history").join("history.sqlite3"))
        .filter(|path| path.is_file())
        .and_then(|path| {
            rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .ok()
        })
        .and_then(|db| {
            db.query_row("SELECT MAX(taken_at) FROM samples", [], |row| {
                row.get::<_, Option<i64>>(0)
            })
            .ok()
            .flatten()
        });
    let ahead = newest.is_some_and(|newest| {
        newest > inputs.now.timestamp_millis() + crate::network::SKEW_THRESHOLD.as_millis() as i64
    });
    if ahead {
        Check::new("clock", CheckStatus::Warn, title)
            .detail(fixed(
                "doctor.clock.history_ahead",
                "The newest stored reading is dated more than five minutes from now, so this clock went back.",
            ))
            .fix(fixed(
                "doctor.fix.clock",
                "Turn on automatic date and time in the system settings.",
            ))
    } else {
        Check::new("clock", CheckStatus::Skip, title).detail(fixed(
            "doctor.clock.unchecked",
            "No server's clock was compared in this run; cuotai --doctor --probe compares one.",
        ))
    }
}

/// PRD 0044 channels: each one's last outcome from the delivery log, named
/// by position and kind (never its label, host or address).
fn check_channels(inputs: &Inputs) -> Vec<Check> {
    let Some(path) = inputs.desktop_settings.as_deref() else {
        return Vec::new();
    };
    let channels = crate::channels::read(path);
    if channels.is_empty() {
        return Vec::new();
    }
    let entries = inputs
        .data_root
        .as_ref()
        .map(|root| {
            crate::channels::log::DeliveryLog::at(root.join("channels").join("log.jsonl")).read()
        })
        .unwrap_or_default();
    channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            let title = Localized::with_params(
                "doctor.title.channel",
                params([
                    ("number", ParamValue::from(index + 1)),
                    ("kind", ParamValue::from(channel.kind.slug())),
                ]),
                "Notification channel {number} ({kind})",
            );
            let id = format!("channel.{}", index + 1);
            if !channel.enabled {
                return Check::new(id, CheckStatus::Skip, title)
                    .detail(fixed("doctor.channel.off", "It is turned off."));
            }
            if crate::channels::log::failing_since(&entries, &channel.id).is_some() {
                return Check::new(id, CheckStatus::Warn, title)
                    .detail(fixed(
                        "doctor.channel.failing",
                        "Its last deliveries failed.",
                    ))
                    .fix(fixed(
                        "doctor.fix.channel",
                        "Open the channel in Settings › Alerts, check its address and send a test (cuotai --channel-test).",
                    ));
            }
            match crate::channels::log::last_delivery(&entries, &channel.id) {
                Some(_) => Check::new(id, CheckStatus::Ok, title).detail(fixed(
                    "doctor.channel.ok",
                    "Its last delivery was accepted.",
                )),
                None => Check::new(id, CheckStatus::Skip, title).detail(fixed(
                    "doctor.channel.none",
                    "Nothing was sent to it yet.",
                )),
            }
        })
        .collect()
}

/// The language and time zone, for a report.
fn check_locale(inputs: &Inputs) -> Check {
    let zone = chrono::Local::now().offset().to_string();
    Check::new(
        "locale",
        CheckStatus::Ok,
        Localized::with_params(
            "doctor.title.locale",
            params([
                (
                    "locale",
                    ParamValue::from(if inputs.locale.is_empty() {
                        "en-US".to_owned()
                    } else {
                        inputs.locale.clone()
                    }),
                ),
                ("zone", ParamValue::from(format!("UTC{zone}"))),
            ]),
            "Language {locale}, time zone {zone}",
        ),
    )
}

/// The desktop's notification permission.
fn check_notifications(granted: bool) -> Check {
    let title = fixed("doctor.title.notifications", "Notifications");
    if granted {
        Check::new("notifications", CheckStatus::Ok, title).detail(fixed(
            "doctor.notifications.ok",
            "The system allows Cuotai's notifications.",
        ))
    } else {
        Check::new("notifications", CheckStatus::Warn, title)
            .detail(fixed(
                "doctor.notifications.denied",
                "The system does not allow Cuotai's notifications, so alerts stay silent.",
            ))
            .fix(fixed(
                "doctor.fix.notifications",
                "Allow notifications for Cuotai in the system settings.",
            ))
    }
}

/// Run every check over `inputs`. Read-only and offline.
pub fn run(inputs: &Inputs) -> Report {
    let mut checks = vec![Check::new(
        "version",
        CheckStatus::Ok,
        Localized::with_params(
            "doctor.title.version",
            params([
                ("version", ParamValue::from(inputs.version.clone())),
                ("os", ParamValue::from(std::env::consts::OS)),
                ("arch", ParamValue::from(std::env::consts::ARCH)),
                ("source", ParamValue::from(inputs.install_source.clone())),
            ]),
            "Cuotai {version} on {os} {arch} ({source})",
        ),
    )];
    let config = check_config(inputs, &mut checks);
    let mut tools = std::collections::BTreeMap::new();
    if let Some(config) = &config {
        for (index, account) in config.accounts.iter().enumerate() {
            checks.push(check_account(index, account, inputs));
            checks.extend(check_tool(index, account, inputs));
            if let Some(tool) = crate::external_tool::Tool::for_provider(account.provider) {
                tools.entry(tool.name()).or_insert_with(|| {
                    crate::external_tool::ToolReport::of(&inputs.resolver.resolve(tool, None))
                });
            }
        }
    }
    checks.push(check_vault(config.as_ref(), inputs));
    checks.push(check_data_dir(inputs));
    let root = inputs.data_root.as_deref();
    let store = |dir: &str, file: &str| root.map(|root| root.join(dir).join(file));
    checks.push(check_sqlite(
        "history",
        fixed("doctor.title.history", "Measurement history"),
        store("history", "history.sqlite3"),
        crate::history::SCHEMA_REVISION,
    ));
    checks.push(check_sqlite(
        "models",
        fixed("doctor.title.models", "Model cache"),
        store("models", "models.sqlite3"),
        1,
    ));
    checks.push(check_sqlite(
        "cost_index",
        fixed("doctor.title.cost_index", "Local cost index"),
        store("cost", "cost.sqlite3"),
        crate::cost::index::SCHEMA_REVISION,
    ));
    checks.push(check_hooks(&inputs.config.path));
    checks.push(check_desktop_settings(inputs.desktop_settings.as_deref()));
    checks.push(check_ledger(root));
    checks.push(check_serve_token(root));
    checks.push(check_port(inputs.port));
    checks.extend(check_cost_sources(inputs));
    checks.extend(check_channels(inputs));
    if let Some(granted) = inputs.notifications {
        checks.push(check_notifications(granted));
    }
    checks.push(check_clock(inputs));
    checks.push(check_locale(inputs));
    Report {
        schema: SCHEMA,
        checks,
        tools,
    }
}

#[cfg(test)]
mod tests;
