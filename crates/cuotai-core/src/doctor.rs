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
//! own checks to the same [`Report`].

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};

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
        }
    }
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
    checks.push(check_serve_token(root));
    checks.push(check_port(inputs.port));
    Report {
        schema: SCHEMA,
        checks,
        tools,
    }
}

#[cfg(test)]
mod tests;
