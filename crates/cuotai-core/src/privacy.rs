//! PRD 0052 RF3.3/RF4 — the single typed manifest of what Cuotai keeps and
//! where it connects.
//!
//! - [`artifacts`] is the exhaustive registry of Cuotai-owned files,
//!   directories and vault services. Diagnostics ([`inspect`]) and total
//!   erasure read it; nothing else lists them.
//! - [`network`] has one row per outbound (and the one inbound) flow, keyed
//!   by [`Purpose`], an exhaustive enum: adding a flow means adding a row.
//! - [`inventory_markdown`] renders both into `docs/privacy.md` between
//!   generated markers; a drift test fails when the document is stale.
//!
//! Nothing here makes a network request or reads a secret value: a vault
//! service is metadata only.

pub mod erase;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::diagnostics::redact::home_abbreviated;

/// Where an artifact's path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Root {
    /// `<OS local data dir>/cuotai`.
    LocalData,
    /// `<OS config dir>/cuotai`.
    Config,
    /// The directory of the accounts TOML in use.
    AccountsDir,
    /// A service of the OS credential vault (no path).
    Vault,
}

impl Root {
    fn template(self) -> &'static str {
        match self {
            Self::LocalData => "<local data>/cuotai/",
            Self::Config => "<config>/cuotai/",
            Self::AccountsDir => "<accounts dir>/",
            Self::Vault => "vault service ",
        }
    }
}

/// What an artifact is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Directory,
    /// Every entry of one credential-vault service.
    VaultService,
}

/// How an artifact's format is versioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Schema {
    /// SQLite `PRAGMA user_version`.
    SqliteUserVersion,
    /// A top-level JSON `schema` number.
    JsonSchema,
    /// A format without a version.
    Unversioned,
    /// A directory, a log or a vault service.
    NotApplicable,
}

/// What total erasure ([PRD 0052 RF5]) does with an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Erase {
    /// Removed by `--erase-all-data` and the desktop's "Erase all".
    Always,
    /// Removed only with `--include-config` (or the desktop checkbox).
    WithConfig,
}

/// One Cuotai-owned data artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Artifact {
    /// Stable logical id.
    pub id: &'static str,
    pub root: Root,
    /// Path below [`Artifact::root`] (`/`-separated), or the vault service.
    pub path: &'static str,
    pub kind: Kind,
    /// What it holds, in a few words.
    pub content: &'static str,
    /// Expected Unix mode (`None`: inherited or not applicable).
    pub mode: Option<u32>,
    pub schema: Schema,
    /// How long it is kept.
    pub retention: &'static str,
    pub erase: Erase,
    /// It rotates into `<path>.1`, which belongs to it.
    pub rotated: bool,
    /// The plan that owns it.
    pub prd: &'static str,
}

const fn file(
    id: &'static str,
    root: Root,
    path: &'static str,
    content: &'static str,
    schema: Schema,
    retention: &'static str,
    prd: &'static str,
) -> Artifact {
    Artifact {
        id,
        root,
        path,
        kind: Kind::File,
        content,
        mode: Some(0o600),
        schema,
        retention,
        erase: Erase::Always,
        rotated: false,
        prd,
    }
}

const fn dir(
    id: &'static str,
    path: &'static str,
    content: &'static str,
    retention: &'static str,
    prd: &'static str,
) -> Artifact {
    Artifact {
        id,
        root: Root::LocalData,
        path,
        kind: Kind::Directory,
        content,
        mode: Some(0o700),
        schema: Schema::NotApplicable,
        retention,
        erase: Erase::Always,
        rotated: false,
        prd,
    }
}

const fn vault(
    id: &'static str,
    service: &'static str,
    content: &'static str,
    prd: &'static str,
) -> Artifact {
    Artifact {
        id,
        root: Root::Vault,
        path: service,
        kind: Kind::VaultService,
        content,
        mode: None,
        schema: Schema::NotApplicable,
        retention: "until the account is removed or signed out",
        erase: Erase::Always,
        rotated: false,
        prd,
    }
}

use Root::{AccountsDir, Config, LocalData};
use Schema::{JsonSchema, SqliteUserVersion, Unversioned};

const UNTIL_ERASED: &str = "until erased";

static ARTIFACTS: &[Artifact] = &[
    Artifact {
        mode: None,
        erase: Erase::WithConfig,
        ..file(
            "config.accounts",
            AccountsDir,
            "<accounts TOML>",
            "account names, providers and their parameters, environment-variable names; never a secret",
            Unversioned,
            "kept; yours to edit",
            "0001",
        )
    },
    Artifact {
        mode: None,
        erase: Erase::WithConfig,
        ..file(
            "config.hooks",
            AccountsDir,
            "hooks.toml",
            "hook rules: events and the commands they run",
            Unversioned,
            "kept; yours to edit",
            "0013",
        )
    },
    file(
        "desktop.settings",
        Config,
        "desktop.json",
        "desktop preferences: language, theme, notifications, server, widgets",
        Unversioned,
        UNTIL_ERASED,
        "0020",
    ),
    file(
        "history.db",
        LocalData,
        "history/history.sqlite3",
        "readings per account and metric (digests of names, numbers, times)",
        SqliteUserVersion,
        "90 days by default (7 to 365), 64 MiB cap",
        "0006",
    ),
    file(
        "models.db",
        LocalData,
        "models/models.sqlite3",
        "model lists returned by providers, per account digest",
        SqliteUserVersion,
        "replaced on each listing",
        "0002",
    ),
    file(
        "cost.db",
        LocalData,
        "cost/cost.sqlite3",
        "index of local coding-tool usage logs (tokens, models, times)",
        SqliteUserVersion,
        "rebuilt from the tools' logs",
        "0015",
    ),
    file(
        "cost.settings",
        LocalData,
        "cost/settings.json",
        "local cost sources and their switches",
        Unversioned,
        UNTIL_ERASED,
        "0015",
    ),
    file(
        "cost.pricing",
        LocalData,
        "cost/pricing.json",
        "price overrides per model",
        Unversioned,
        UNTIL_ERASED,
        "0015",
    ),
    file(
        "notifications.ledger",
        LocalData,
        "notified.json",
        "digests and dates of reset and usage notices already sent",
        Unversioned,
        "8 days",
        "0003",
    ),
    file(
        "widgets.snapshot",
        LocalData,
        "widget.json",
        "the latest readings shown by system widgets (desktop)",
        JsonSchema,
        "replaced on each refresh; 256 KiB cap",
        "0014",
    ),
    file(
        "last_config.tui",
        LocalData,
        "last-config-tui.json",
        "path of the accounts file the terminal last opened",
        Unversioned,
        "replaced on each start",
        "0022",
    ),
    file(
        "last_config.desktop",
        LocalData,
        "last-config-desktop.json",
        "path of the accounts file the desktop last opened",
        Unversioned,
        "replaced on each start",
        "0022",
    ),
    dir(
        "extensions.tree",
        "extensions",
        "the default extensions directory: approved extension programs you added",
        UNTIL_ERASED,
        "0017",
    ),
    file(
        "extensions.consent",
        LocalData,
        "consent/extensions.json",
        "digests of the extensions you approved",
        Unversioned,
        UNTIL_ERASED,
        "0017",
    ),
    file(
        "hooks.consent",
        LocalData,
        "hooks/consent.json",
        "digests of the hook rules you confirmed",
        Unversioned,
        UNTIL_ERASED,
        "0013",
    ),
    file(
        "hooks.claims",
        LocalData,
        "hooks/claims.json",
        "a digest and two times per hook run claimed by one process; no payload, account or command",
        Unversioned,
        "8 days",
        "0054",
    ),
    dir(
        "hooks.work",
        "hooks/work",
        "the working directory of running hooks",
        "emptied by the hooks themselves",
        "0013",
    ),
    Artifact {
        rotated: true,
        ..file(
            "hooks.runs",
            LocalData,
            "hooks/runs.log",
            "rule, event, time, duration and exit code of each hook run; no payload or output",
            Unversioned,
            "256 KiB plus one rotated copy",
            "0013",
        )
    },
    file(
        "serve.token_file",
        LocalData,
        "serve/token",
        "the local server's bearer token, when `--serve` keeps it in a file",
        Unversioned,
        "until revoked",
        "0012",
    ),
    Artifact {
        rotated: true,
        ..file(
            "app.log",
            LocalData,
            "logs/cuotai.log",
            "redacted app events: ids, levels, counts, classes; no secret, body, URL path or e-mail",
            Unversioned,
            "256 KiB plus one rotated copy",
            "0052",
        )
    },
    file(
        "vault.index",
        LocalData,
        "vault-index.json",
        "opaque keys of the vault entries Cuotai created, with dates; never a value",
        JsonSchema,
        "until the entry is deleted",
        "0029",
    ),
    file(
        "update.check",
        LocalData,
        "update-check.json",
        "the latest version seen by the terminal update notice and when",
        Unversioned,
        "24 hours",
        "0028",
    ),
    dir(
        "metered.records",
        "metered",
        "time of the last query of each metered account (digest-named)",
        UNTIL_ERASED,
        "0033",
    ),
    dir(
        "canary.records",
        "canary",
        "maintainer canary runs: response shapes without values",
        UNTIL_ERASED,
        "0028",
    ),
    vault(
        "vault.oauth",
        "cuotai.oauth",
        "OAuth tokens, pasted sessions and keys saved in the app",
        "0001",
    ),
    vault(
        "vault.serve",
        "cuotai.serve",
        "the local server's bearer token",
        "0012",
    ),
];

/// Every Cuotai-owned data artifact, in a stable order.
pub fn artifacts() -> &'static [Artifact] {
    ARTIFACTS
}

/// The artifact with `id`.
pub fn artifact(id: &str) -> Option<&'static Artifact> {
    ARTIFACTS.iter().find(|a| a.id == id)
}

/// The directories artifact paths resolve against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    /// The data root: `CUOTAI_DATA_DIR` or `<OS local data dir>/cuotai`
    /// ([`crate::data_dir::root`]).
    pub local_data: Option<PathBuf>,
    /// `<OS config dir>`, without the `cuotai` part.
    pub config: Option<PathBuf>,
    /// The accounts TOML in use.
    pub accounts: Option<PathBuf>,
}

impl Roots {
    /// The OS directories and the given accounts file.
    pub fn system(accounts: Option<PathBuf>) -> Self {
        Self {
            local_data: crate::data_dir::root(),
            config: dirs::config_dir(),
            accounts,
        }
    }

    /// Where `artifact` lives; `None` for a vault service or an unknown
    /// root.
    pub fn path_of(&self, artifact: &Artifact) -> Option<PathBuf> {
        let join = |base: &Path, rel: &str| {
            rel.split('/')
                .fold(base.to_path_buf(), |path, part| path.join(part))
        };
        match artifact.root {
            Root::LocalData => self.local_data.as_deref().map(|d| join(d, artifact.path)),
            Root::Config => self
                .config
                .as_deref()
                .map(|d| join(&d.join("cuotai"), artifact.path)),
            Root::AccountsDir if artifact.id == "config.accounts" => self.accounts.clone(),
            Root::AccountsDir => self
                .accounts
                .as_deref()
                .and_then(Path::parent)
                .map(|d| join(d, artifact.path)),
            Root::Vault => None,
        }
    }
}

/// Every path that belongs to a file artifact at `path`: the file, its
/// rotated copy, and SQLite's sidecars.
pub fn companions(artifact: &Artifact, path: &Path) -> Vec<PathBuf> {
    let with = |suffix: &str| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    let mut out = vec![path.to_path_buf()];
    if artifact.rotated {
        out.push(with(".1"));
    }
    if artifact.schema == Schema::SqliteUserVersion {
        out.extend(["-wal", "-shm", "-journal"].map(with));
    }
    out
}

/// What one artifact looks like now (RF3.3; PRD 0046's `--doctor` embeds
/// this list). Paths are home-abbreviated; no content is read except a
/// schema number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactState {
    pub id: &'static str,
    /// Home-abbreviated path, or the vault service name.
    pub path: String,
    pub kind: Kind,
    /// `None` for a vault service (metadata only: never enumerated here).
    pub exists: Option<bool>,
    /// `file`, `directory` or `other` when it exists.
    pub found: Option<&'static str>,
    /// Bytes of the file (with its rotated copy and sidecars) or of the
    /// directory tree (bounded walk).
    pub bytes: Option<u64>,
    /// Octal Unix mode (`"0600"`); `None` elsewhere or when absent.
    pub mode: Option<String>,
    /// The expected mode, when it differs from [`ArtifactState::mode`].
    pub mode_expected: Option<String>,
    /// `user_version` or JSON `schema`; `unversioned`/`not_applicable`/
    /// `unreadable` otherwise.
    pub schema: String,
}

/// Most directory entries a size walk visits.
const WALK_LIMIT: usize = 10_000;

/// Inspect every artifact under `roots` (no network, no secret read).
pub fn inspect(roots: &Roots) -> Vec<ArtifactState> {
    ARTIFACTS.iter().map(|a| inspect_one(a, roots)).collect()
}

fn inspect_one(artifact: &'static Artifact, roots: &Roots) -> ArtifactState {
    let Some(path) = roots.path_of(artifact) else {
        return ArtifactState {
            id: artifact.id,
            path: if artifact.kind == Kind::VaultService {
                artifact.path.to_string()
            } else {
                "unavailable".to_string()
            },
            kind: artifact.kind,
            exists: None,
            found: None,
            bytes: None,
            mode: None,
            mode_expected: None,
            schema: "not_applicable".to_string(),
        };
    };
    let meta = std::fs::symlink_metadata(&path).ok();
    let found = meta.as_ref().map(|m| {
        if m.is_file() {
            "file"
        } else if m.is_dir() {
            "directory"
        } else {
            "other"
        }
    });
    let bytes = meta.as_ref().map(|m| match artifact.kind {
        Kind::Directory if m.is_dir() => tree_size(&path),
        _ => companions(artifact, &path)
            .iter()
            .filter_map(|p| std::fs::symlink_metadata(p).ok())
            .filter(std::fs::Metadata::is_file)
            .map(|m| m.len())
            .sum(),
    });
    let mode = meta.as_ref().and_then(unix_mode);
    let mode_expected = match (artifact.mode, &mode) {
        (Some(expected), Some(actual)) if format!("{expected:04o}") != *actual => {
            Some(format!("{expected:04o}"))
        }
        _ => None,
    };
    let schema = match (artifact.schema, found) {
        (_, None) | (Schema::NotApplicable, _) => "not_applicable".to_string(),
        (Schema::Unversioned, _) => "unversioned".to_string(),
        (Schema::SqliteUserVersion, Some("file")) => sqlite_version(&path),
        (Schema::JsonSchema, Some("file")) => json_schema(&path),
        _ => "unreadable".to_string(),
    };
    ArtifactState {
        id: artifact.id,
        path: home_abbreviated(&path),
        kind: artifact.kind,
        exists: Some(meta.is_some()),
        found,
        bytes,
        mode,
        mode_expected,
        schema,
    }
}

fn unix_mode(meta: &std::fs::Metadata) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(format!("{:04o}", meta.permissions().mode() & 0o7777))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

fn tree_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0;
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > WALK_LIMIT {
                return total;
            }
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

fn sqlite_version(path: &Path) -> String {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    rusqlite::Connection::open_with_flags(path, flags)
        .and_then(|db| db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)))
        .map_or_else(|_| "unreadable".to_string(), |v| v.to_string())
}

fn json_schema(path: &Path) -> String {
    // A bounded read: these files are small; a huge one is not parsed.
    let Ok(meta) = std::fs::metadata(path) else {
        return "unreadable".to_string();
    };
    if meta.len() > 4 * 1024 * 1024 {
        return "unreadable".to_string();
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .map_or_else(
            || "unreadable".to_string(),
            |value| match value.get("schema").and_then(serde_json::Value::as_u64) {
                Some(n) => n.to_string(),
                None => "unversioned".to_string(),
            },
        )
}

/// One network flow (RF4.1). Every HTTP client Cuotai builds has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// Usage, quota, balance and model-list queries of configured accounts.
    ProviderApi,
    /// Sign-in and token refresh of OAuth accounts.
    #[serde(rename = "oauth")]
    OAuth,
    /// Public status pages (opt-in incident status).
    PublicStatus,
    /// The terminal's opt-in update notice.
    UpdateCheck,
    /// The desktop's in-app updater.
    DesktopUpdater,
    /// A 9router instance probed on loopback during setup detection.
    LocalProbe,
    /// The desktop's sign-in window that captures a provider session.
    BrowserSession,
    /// Extension programs you approved.
    Extensions,
    /// Hook commands you confirmed.
    Hooks,
    /// The optional read-only local server (inbound, loopback by default).
    LocalServer,
}

impl Purpose {
    pub const ALL: [Purpose; 10] = [
        Self::ProviderApi,
        Self::OAuth,
        Self::PublicStatus,
        Self::UpdateCheck,
        Self::DesktopUpdater,
        Self::LocalProbe,
        Self::BrowserSession,
        Self::Extensions,
        Self::Hooks,
        Self::LocalServer,
    ];
}

/// One row of the network inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Flow {
    pub purpose: Purpose,
    pub host: &'static str,
    pub trigger: &'static str,
    pub sends: &'static str,
    pub default: &'static str,
    pub prd: &'static str,
}

/// The row of `purpose` (exhaustive: a new purpose needs its row here).
pub fn flow(purpose: Purpose) -> Flow {
    let (host, trigger, sends, default, prd) = match purpose {
        Purpose::ProviderApi => (
            "each configured provider's API (or the base URL you set)",
            "each refresh of an account; a model list you open",
            "that account's credential and the parameters the provider needs",
            "on for each account you add",
            "0001",
        ),
        Purpose::OAuth => (
            "the provider's sign-in and token endpoints",
            "signing in, and refreshing an expiring token",
            "authorization codes, refresh tokens and the provider's client id",
            "only for OAuth accounts",
            "0001",
        ),
        Purpose::PublicStatus => (
            "public status pages of configured providers",
            "every 5 minutes at most, while turned on",
            "nothing but the request (no credential)",
            "off",
            "0011",
        ),
        Purpose::UpdateCheck => (
            "GitHub Releases (`latest.json`)",
            "at most once every 24 hours on a terminal",
            "the `User-Agent` `cuotai/<version>`",
            "off",
            "0028",
        ),
        Purpose::DesktopUpdater => (
            "GitHub Releases (signed manifest and installers)",
            "on opening and every 24 hours, after you allow it once",
            "nothing about you: a static manifest request, then only the installer you accept",
            "asked once; off for package-manager installs",
            "0018",
        ),
        Purpose::LocalProbe => (
            "a 9router instance on this computer (loopback only, no proxy)",
            "account setup detection",
            "a health request without credentials",
            "only while adding accounts",
            "0036",
        ),
        Purpose::BrowserSession => (
            "the provider's own sign-in page, in a private window",
            "only when you click Sign in and capture session",
            "what you type into the provider's page; Cuotai keeps only the descriptor's cookies",
            "off until clicked",
            "0048",
        ),
        Purpose::Extensions => (
            "whatever an approved extension program contacts, with your network",
            "each refresh of an extension account",
            "decided by the program you approved; Cuotai passes it only its configured parameters",
            "off until you approve an extension",
            "0017",
        ),
        Purpose::Hooks => (
            "whatever a confirmed hook command contacts, with your network",
            "each event a confirmed rule matches",
            "decided by the command you confirmed; it receives the event on standard input",
            "off until you confirm the rules",
            "0013",
        ),
        Purpose::LocalServer => (
            "inbound: clients on this computer (loopback unless you allow more with a token)",
            "while `--serve` or Settings → Integration runs it",
            "the readings already shown, without credentials; a bearer token is required by default",
            "off",
            "0012",
        ),
    };
    Flow {
        purpose,
        host,
        trigger,
        sends,
        default,
        prd,
    }
}

/// Every network row, in [`Purpose::ALL`] order.
pub fn network() -> Vec<Flow> {
    Purpose::ALL.into_iter().map(flow).collect()
}

fn purpose_id(purpose: Purpose) -> String {
    serde_json::to_value(purpose)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn erase_text(artifact: &Artifact) -> &'static str {
    match (artifact.erase, artifact.kind) {
        (Erase::WithConfig, _) => "`--erase-all-data --include-config`",
        (Erase::Always, _) => "`--erase-all-data`",
    }
}

fn schema_text(schema: Schema) -> &'static str {
    match schema {
        Schema::SqliteUserVersion => "SQLite `user_version`",
        Schema::JsonSchema => "JSON `schema`",
        Schema::Unversioned => "unversioned",
        Schema::NotApplicable => "—",
    }
}

/// The generated part of `docs/privacy.md` (deterministic, LF).
pub fn inventory_markdown() -> String {
    let mut out = String::new();
    out.push_str("### Network\n\n");
    out.push_str(
        "| Purpose | Host | When | What is sent | Default | PRD |\n|---|---|---|---|---|---|\n",
    );
    for row in network() {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            purpose_id(row.purpose),
            row.host,
            row.trigger,
            row.sends,
            row.default,
            row.prd
        ));
    }
    out.push_str("\n### Local data\n\n");
    out.push_str("| Id | Location | Holds | Mode | Schema | Kept | Erased by | PRD |\n|---|---|---|---|---|---|---|---|\n");
    for a in ARTIFACTS {
        let location = format!(
            "`{}{}`{}",
            a.root.template(),
            a.path,
            if a.rotated { " (and `.1`)" } else { "" }
        );
        let mode = a
            .mode
            .map_or_else(|| "—".to_string(), |m| format!("`{m:04o}`"));
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
            a.id,
            location,
            a.content,
            mode,
            schema_text(a.schema),
            a.retention,
            erase_text(a),
            a.prd
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const PRIVACY_DOC: &str = "docs/privacy.md";
    const START: &str = "<!-- privacy-inventory:start (generated by `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core privacy_doc_matrix`; do not edit) -->";
    const END: &str = "<!-- privacy-inventory:end -->";
    const REGENERATE: &str = "CUOTAI_REGENERATE_DOCS";

    #[test]
    fn ids_are_unique_and_every_artifact_has_one_local_row() {
        let ids: BTreeSet<_> = ARTIFACTS.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), ARTIFACTS.len());
        let doc = inventory_markdown();
        for a in ARTIFACTS {
            assert_eq!(
                doc.matches(&format!("| `{}` |", a.id)).count(),
                1,
                "{} needs exactly one local row",
                a.id
            );
        }
        for purpose in Purpose::ALL {
            assert_eq!(
                doc.matches(&format!("| `{}` |", purpose_id(purpose)))
                    .count(),
                1,
                "{purpose:?} needs exactly one network row"
            );
        }
    }

    /// RF3.3: the registry names every artifact the requirement lists.
    #[test]
    fn the_registry_covers_the_required_artifacts() {
        for id in [
            "config.accounts",
            "config.hooks",
            "desktop.settings",
            "history.db",
            "models.db",
            "cost.db",
            "cost.settings",
            "cost.pricing",
            "notifications.ledger",
            "widgets.snapshot",
            "last_config.tui",
            "last_config.desktop",
            "extensions.tree",
            "extensions.consent",
            "hooks.consent",
            "hooks.claims",
            "hooks.work",
            "hooks.runs",
            "serve.token_file",
            "app.log",
            "vault.oauth",
            "vault.serve",
        ] {
            assert!(artifact(id).is_some(), "{id}");
        }
    }

    /// The registry's paths are the ones the owning modules write.
    #[test]
    fn registry_paths_match_the_modules() {
        let roots = Roots {
            local_data: Some(PathBuf::from("/data/cuotai")),
            config: Some(PathBuf::from("/conf")),
            accounts: Some(PathBuf::from("/acc/cuotai.toml")),
        };
        let path = |id: &str| roots.path_of(artifact(id).unwrap()).unwrap();
        let local = |rel: &str| Path::new("/data").join("cuotai").join(rel);
        assert_eq!(
            path("app.log"),
            crate::diagnostics::logging::log_path(Path::new("/data/cuotai"))
        );
        assert_eq!(path("hooks.runs"), local("hooks").join("runs.log"));
        assert_eq!(
            path("hooks.claims"),
            local("hooks").join(crate::hooks::CLAIMS_FILE_NAME)
        );
        assert_eq!(path("serve.token_file"), local("serve").join("token"));
        assert_eq!(path("config.accounts"), PathBuf::from("/acc/cuotai.toml"));
        assert_eq!(path("config.hooks"), PathBuf::from("/acc/hooks.toml"));
        assert_eq!(
            path("desktop.settings"),
            Path::new("/conf").join("cuotai").join("desktop.json")
        );
        assert_eq!(roots.path_of(artifact("vault.oauth").unwrap()), None);
        if let Some(index) = crate::oauth::vault_index::default_path() {
            let real = Roots::system(None);
            assert_eq!(real.path_of(artifact("vault.index").unwrap()), Some(index));
        }
    }

    #[test]
    fn inspection_reports_kind_size_mode_and_schema_without_content() {
        let dir = tempfile::tempdir().unwrap();
        let roots = Roots {
            local_data: Some(dir.path().join("cuotai")),
            config: Some(dir.path().join("conf")),
            accounts: None,
        };
        let base = dir.path().join("cuotai");
        std::fs::create_dir_all(base.join("history")).unwrap();
        let db = rusqlite::Connection::open(base.join("history").join("history.sqlite3")).unwrap();
        db.execute_batch("PRAGMA user_version = 7; CREATE TABLE t(x);")
            .unwrap();
        drop(db);
        crate::private_fs::write_file(
            &base.join("widget.json"),
            br#"{"schema":2,"secret":"SEEDVALUE"}"#,
        )
        .unwrap();
        let states = inspect(&roots);
        let state = |id: &str| states.iter().find(|s| s.id == id).unwrap().clone();
        let history = state("history.db");
        assert_eq!(history.exists, Some(true));
        assert_eq!(history.schema, "7");
        assert!(history.bytes.unwrap() > 0);
        let ledger = state("widgets.snapshot");
        assert_eq!(ledger.schema, "2");
        #[cfg(unix)]
        assert_eq!(ledger.mode.as_deref(), Some("0600"));
        assert_eq!(state("app.log").exists, Some(false));
        assert_eq!(state("vault.oauth").exists, None);
        assert_eq!(state("config.accounts").path, "unavailable");
        let json = serde_json::to_string(&states).unwrap();
        assert!(!json.contains("SEEDVALUE"), "no content leaks");
    }

    #[test]
    fn privacy_doc_matrix_matches_the_registry() {
        let _guard = crate::catalog::DOC_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(PRIVACY_DOC);
        let doc = std::fs::read_to_string(&path)
            .expect("privacy doc")
            .replace("\r\n", "\n");
        let (head, rest) = doc
            .split_once(START)
            .expect("start marker in docs/privacy.md");
        let (current, tail) = rest.split_once(END).expect("end marker in docs/privacy.md");
        let expected = format!("\n{}", inventory_markdown());
        if std::env::var_os(REGENERATE).is_some() {
            std::fs::write(&path, format!("{head}{START}{expected}{END}{tail}"))
                .expect("write privacy doc");
            return;
        }
        assert_eq!(
            current, expected,
            "{PRIVACY_DOC} is out of date with privacy::artifacts/network. Regenerate it with \
             `{REGENERATE}=1 cargo test -p cuotai-core privacy_doc_matrix` and commit the result."
        );
    }
}
