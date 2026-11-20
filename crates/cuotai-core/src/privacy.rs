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
    /// `<OS local data dir>/cuotai`, never moved by `CUOTAI_DATA_DIR` or
    /// the desktop's data-folder setting: machine-local billing state that
    /// every front end must share (PRD 0033 re-validation M-4).
    OsLocalData,
    /// `<OS config dir>/cuotai`.
    Config,
    /// The directory of the accounts TOML in use.
    AccountsDir,
    /// A service of the OS credential vault (no path).
    Vault,
    /// The user's home directory, for the few macOS paths that stay put
    /// when the data folder moves (the widget extension can read only
    /// those).
    MacHome,
    /// Kept by the operating system or the webview for the desktop app
    /// (PRD 0052 S-01/S-03): no path Cuotai's files resolve against.
    App,
}

impl Root {
    fn template(self) -> &'static str {
        match self {
            Self::LocalData => "<local data>/cuotai/",
            Self::OsLocalData => "<OS local data>/cuotai/",
            Self::Config => "<config>/cuotai/",
            Self::AccountsDir => "<accounts dir>/",
            Self::Vault => "vault service ",
            Self::MacHome => "~/",
            Self::App => "",
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
    /// State the desktop app itself erases (its web storage, the
    /// start-at-login entry, leftover capture profiles): metadata only.
    /// [`erase::erase_all`] never touches it; the plan marks it
    /// [`erase::PlanItem::app_only`].
    AppManaged,
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

/// A [`Kind::AppManaged`] artifact: `path` describes where it lives.
const fn app_managed(
    id: &'static str,
    path: &'static str,
    content: &'static str,
    retention: &'static str,
    prd: &'static str,
) -> Artifact {
    Artifact {
        id,
        root: Root::App,
        path,
        kind: Kind::AppManaged,
        content,
        mode: None,
        schema: Schema::NotApplicable,
        retention,
        erase: Erase::Always,
        rotated: false,
        prd,
    }
}

use Root::{AccountsDir, Config, LocalData, MacHome};
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
            "account names, providers and their parameters, environment-variable names, budgets and balance floors; never a secret",
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
        "desktop preferences: language, theme, notifications, server, widgets; the menu-bar style and the ids of the accounts it shows, the row layouts, the first-run record, and the ids (or 9router connection keys) of accounts hidden from the dashboard",
        Unversioned,
        UNTIL_ERASED,
        "0020, 0025",
    ),
    file(
        "data.marker",
        LocalData,
        crate::data_dir::MARKER,
        "a fixed note that this folder is Cuotai's data folder, and whether Cuotai created it or adopted an older one; Cuotai deletes or narrows files only in a folder that holds it (or the system's default one)",
        Unversioned,
        "until erased; removed last, and only when nothing else is left in the folder",
        "0051",
    ),
    file(
        "history.db",
        LocalData,
        "history/history.sqlite3",
        "readings per account and metric (digests of names, numbers, times), plus a settings digest per account that decides whether its last reading may be shown after a restart (no secret), and an event journal: event name, metric label, threshold, share, reset and projection times, failure category, incident state, extra-usage amount and currency (no message, URL, e-mail or token)",
        SqliteUserVersion,
        "90 days by default (7 to 365), 64 MiB cap that trims the oldest readings and journal rows together; its pre-upgrade copy `history.sqlite3.bak` is kept 7 days within the cap",
        "0006, 0026",
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
        "index of local coding-tool usage logs (tokens, models, times, and fingerprints of sessions, requests, log files and projects keyed to this computer), your session and project labels and hidden projects, and the last folder name of a project per source only while that source's switch is on",
        SqliteUserVersion,
        "rebuilt from the tools' logs; usage, projects and folder names follow the retention; labels and hidden flags kept until erased; a source's folder names are removed with the source and when its switch is turned off, also from the pre-upgrade copy `cost.sqlite3.bak`, which is otherwise kept 7 days and removed on erase or turn-off",
        "0015, 0042",
    ),
    file(
        "cost.key",
        LocalData,
        "cost/fingerprint.key",
        "the random key of this computer's cost-index fingerprints (sessions, requests, log files and projects), so a known session id, message id or log path, or a guessed folder path, cannot be confirmed from them; carried with the labels in a backup",
        Unversioned,
        "replaced when the index is erased",
        "0015, 0046",
    ),
    file(
        "cost.settings",
        LocalData,
        "cost/settings.json",
        "local cost sources and their switches, block lengths, folder-name hint switches, plan prices per account id, a second currency and its rate",
        Unversioned,
        UNTIL_ERASED,
        "0015, 0030",
    ),
    Artifact {
        // Written only by the person; Cuotai reads it and never writes it.
        mode: None,
        ..file(
            "cost.pricing",
            LocalData,
            "cost/pricing.json",
            "price overrides per model, as you created it (never written by Cuotai)",
            Unversioned,
            UNTIL_ERASED,
            "0015, 0030",
        )
    },
    file(
        "notifications.ledger",
        LocalData,
        "notified.json",
        "digests and dates of reset and usage notices and weekly digests already sent",
        Unversioned,
        "8 days",
        "0003",
    ),
    Artifact {
        // Created with the default mode: it holds no personal data.
        mode: None,
        ..file(
            "notifications.lock",
            LocalData,
            "notified.lock",
            "a process id and a counter for a moment while a notice is claimed",
            Unversioned,
            "deleted on release, milliseconds after it is taken; a lock left by a crash is broken after 10 seconds",
            "0032",
        )
    },
    file(
        "widgets.snapshot",
        LocalData,
        "widget.json",
        "the latest readings, a short history of each account's tightest quota, any paid extra usage and, if turned on, account e-mails, for system widgets (desktop)",
        JsonSchema,
        "replaced on each refresh; 256 KiB cap",
        "0014",
    ),
    file(
        "widgets.snapshot_macos",
        MacHome,
        crate::widget_snapshot::MACOS_PATH,
        "the widget snapshot on macOS, where the widget extension reads it; it stays here when the data folder moves (macOS desktop)",
        JsonSchema,
        "replaced on each refresh; 256 KiB cap",
        "0014",
    ),
    file(
        "widgets.snapshot_legacy",
        MacHome,
        crate::widget_snapshot::MACOS_LEGACY_PATH,
        "the snapshot as releases before 0.82.2 wrote it (macOS desktop)",
        JsonSchema,
        "no longer written; removed with the widget data",
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
        root: Root::OsLocalData,
        ..dir(
        "feed.tree",
        "feed",
        "agent-fed readings: one JSON file per agent session with rate-limit percentages, reset times, the context share, the agent's cost estimate, a model name and a digest of the session id; never a path, prompt or raw id. Codex CLI's are read only with its local cost source on and erased when it is turned off; `--feed claude-statusline --forget` erases Claude Code's",
        "24 hours (the newest file of an agent an account reads is kept), 64 files per agent",
        "0040",
    )
    },
    dir(
        "feed.tree_legacy",
        "feed",
        "agent-fed readings left in a moved data folder by builds before PRD 0040's re-validation; no longer read; only its agents' folders are erased",
        UNTIL_ERASED,
        "0040",
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
    Artifact {
        rotated: true,
        ..file(
            "serve.access_log",
            LocalData,
            "logs/serve-access.log",
            "desktop local-server requests: UTC time, loopback or not, method, route template, status, elapsed; no address, header, Origin, token, query or account",
            Unversioned,
            "1 MiB plus one rotated copy; none while the desktop access log is off",
            "0050",
        )
    },
    file(
        "vault.index",
        LocalData,
        "vault-index.json",
        "opaque keys of the vault entries Cuotai created, with dates, kinds and owning-file digests (`ConfigIdentity`, never a path); never a value",
        JsonSchema,
        "until the entry is deleted",
        "0029",
    ),
    dir(
        "oauth.locks",
        "oauth-locks",
        "empty lock files named by the vault's opaque keys, held during a token refresh",
        UNTIL_ERASED,
        "0029",
    ),
    file(
        "update.check",
        LocalData,
        "update-check.json",
        "the latest version seen by the terminal update notice and when",
        Unversioned,
        "replaced at most once a day; kept until erased",
        "0028",
    ),
    Artifact {
        root: Root::OsLocalData,
        ..dir(
            "metered.records",
            "metered",
            "time of the last query of each metered account and its redacted reading (account name, figures, notes, source), with a digest of the account's settings; digest-named; not moved by the data-folder setting",
            UNTIL_ERASED,
            "0033",
        )
    },
    Artifact {
        root: Root::OsLocalData,
        ..dir(
            "process.presence",
            "running",
            "empty lock files naming which Cuotai programs run now (the desktop app, `--serve`, `--watch`, the terminal dashboard) and their process ids; erasing all data refuses while one is held",
            "removed when the program ends; a file a crash left is removed by the next check",
            "0052",
        )
    },
    dir(
        "metered.records_legacy",
        "metered",
        "metered records left in a moved data folder by builds before PRD 0033's re-validation; no longer read",
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
    file(
        "channels.log",
        LocalData,
        "channels/log.jsonl",
        "time, channel id, notification kind, outcome, HTTP status and attempt of each channel delivery; never the body, address or label",
        Unversioned,
        "7 days",
        "0044",
    ),
    app_managed(
        "webview.storage",
        "the app's web storage (WebKit on macOS and Linux, WebView2 on Windows) for `dev.cuotai.desktop`",
        "display choices mirrored from `desktop.json`, the ids of hidden quotas (provider, account name and metric), the accounts-file paths already announced, language-pack notices, the last Settings page",
        UNTIL_ERASED,
        "0020, 0052",
    ),
    app_managed(
        "autostart.entry",
        "`~/Library/LaunchAgents/<app>.plist` (macOS), a `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run` value (Windows), `~/.config/autostart/<app>.desktop` (Linux)",
        "the path of the Cuotai app, to start it at login",
        "while **Open at login** is on",
        "0022, 0054",
    ),
    app_managed(
        "capture.profiles",
        "`<temp>/cuotai-session-capture-*`",
        "temporary browser profiles of a web-session capture",
        "removed when the capture ends, at the next start or by the erase",
        "0048, 0056",
    ),
    vault(
        "vault.oauth",
        "cuotai.oauth",
        "OAuth tokens, pasted sessions, keys saved in the app and notification channel addresses (`channel_<id>`)",
        "0001",
    ),
    Artifact {
        // The serve token belongs to no account: it lasts until it is
        // revoked or replaced (PRD 0012 RF5).
        retention: "until revoked or replaced",
        ..vault(
            "vault.serve",
            "cuotai.serve",
            "the local server's bearer token",
            "0012",
        )
    },
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
    /// The home directory, on macOS only ([`Root::MacHome`]); `None`
    /// elsewhere, where those artifacts do not exist.
    pub mac_home: Option<PathBuf>,
    /// The metered records' fixed folder ([`Root::OsLocalData`]):
    /// `<OS local data dir>/cuotai/metered`.
    pub metered: Option<PathBuf>,
    /// The agent feed's fixed folder ([`Root::OsLocalData`]):
    /// `<OS local data dir>/cuotai/feed` ([`crate::feed::FeedStore::default_dir`]).
    pub feed: Option<PathBuf>,
    /// PRD 0052 re-validation S-02: the other data roots an erase also
    /// clears (the desktop's chosen folder, data left in the OS default
    /// one), each Cuotai's own ([`crate::data_dir::owned`]) and apart from
    /// [`Roots::local_data`]. Empty outside [`Roots::for_erase`].
    pub other_local_data: Vec<PathBuf>,
    /// Data roots an erase found but leaves alone, because Cuotai did not
    /// create them (no marker); listed as kept with the reason.
    pub kept_local_data: Vec<PathBuf>,
}

impl Roots {
    /// The OS directories and the given accounts file.
    pub fn system(accounts: Option<PathBuf>) -> Self {
        Self {
            local_data: crate::data_dir::root(),
            config: dirs::config_dir(),
            accounts,
            mac_home: if cfg!(target_os = "macos") {
                dirs::home_dir()
            } else {
                None
            },
            metered: crate::metered_record::Store::default_dir(),
            feed: crate::feed::FeedStore::default_dir(),
            other_local_data: Vec::new(),
            kept_local_data: Vec::new(),
        }
    }

    /// The roots of an erase (PRD 0052 re-validation S-02): [`Roots::system`]
    /// plus the desktop's chosen data folder
    /// ([`crate::data_dir::desktop_choice`]) and the OS default one, when
    /// they exist apart from the data root in use. One without Cuotai's
    /// marker is never emptied; it is listed as kept.
    pub fn for_erase(accounts: Option<PathBuf>) -> Self {
        let roots = Self::system(accounts);
        let choice = roots
            .config
            .as_deref()
            .and_then(crate::data_dir::desktop_choice);
        roots.with_erase_roots(
            [choice, crate::data_dir::default_root()]
                .into_iter()
                .flatten(),
        )
    }

    /// [`Roots::for_erase`] over any candidate roots (tests): each existing
    /// directory not already listed (compared canonically) joins
    /// [`Roots::other_local_data`] when it is Cuotai's, else
    /// [`Roots::kept_local_data`].
    pub fn with_erase_roots(mut self, candidates: impl IntoIterator<Item = PathBuf>) -> Self {
        let same = |a: &Path, b: &Path| {
            a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
        };
        for candidate in candidates {
            if !std::fs::symlink_metadata(&candidate).is_ok_and(|meta| meta.is_dir()) {
                continue;
            }
            let listed = self
                .local_data
                .iter()
                .chain(&self.other_local_data)
                .chain(&self.kept_local_data)
                .any(|known| same(known, &candidate));
            if listed {
                continue;
            }
            if crate::data_dir::owned(&candidate) {
                self.other_local_data.push(candidate);
            } else {
                self.kept_local_data.push(candidate);
            }
        }
        self
    }

    /// These roots with `root` as the data root and no other one: how an
    /// erase resolves an extra root's items.
    pub(crate) fn with_local_data(&self, root: &Path) -> Self {
        Self {
            local_data: Some(root.to_path_buf()),
            other_local_data: Vec::new(),
            kept_local_data: Vec::new(),
            ..self.clone()
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
            // The records left in a moved data folder; none when the data
            // folder is the OS default (the same folder, listed once).
            Root::LocalData if artifact.id == "metered.records_legacy" => self
                .local_data
                .as_deref()
                .map(|d| join(d, artifact.path))
                .filter(|path| self.metered.as_deref() != Some(path.as_path())),
            Root::LocalData if artifact.id == "feed.tree_legacy" => self
                .local_data
                .as_deref()
                .map(|d| join(d, artifact.path))
                .filter(|path| self.feed.as_deref() != Some(path.as_path())),
            Root::LocalData => self.local_data.as_deref().map(|d| join(d, artifact.path)),
            Root::OsLocalData if artifact.id == "feed.tree" => self.feed.clone(),
            // The other fixed folders sit beside the metered records, in
            // `<OS local data dir>/cuotai` ([`crate::presence::default_dir`]).
            Root::OsLocalData if artifact.id != "metered.records" => self
                .metered
                .as_deref()
                .and_then(Path::parent)
                .map(|d| join(d, artifact.path)),
            Root::OsLocalData => self.metered.clone(),
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
            Root::Vault | Root::App => None,
            // A home path inside the data root is that root's own artifact
            // (the default macOS data folder), listed once.
            Root::MacHome => self
                .mac_home
                .as_deref()
                .map(|d| join(d, artifact.path))
                .filter(|path| {
                    self.local_data
                        .as_deref()
                        .is_none_or(|data| !path.starts_with(data))
                }),
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
    // PRD 0051 RF3.3/RF4.2 and PRD 0003 COMP-02: the copies Cuotai keeps of
    // the file (`<file>.bak`, the numbered `desktop.json.bak.N`, the
    // `notified.json.corrupt-<time>` set-asides) hold the same data, and
    // so does a `notified.lock.stale-<pid>-<n>` a crash left while breaking
    // a stale lock (PRD 0032 RF4.2). PRD 0052 re-validation S-05: so do
    // its writer's lock (`.<file>.lock`) and the temporaries a crashed
    // writer left (`.<file>.<random>.tmp`).
    out.push(with(".bak"));
    out.push(crate::private_fs::lock_path(path));
    if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
        let name = name.to_string_lossy();
        let (bak, corrupt, stale) = (
            format!("{name}.bak."),
            format!("{name}.corrupt"),
            format!("{name}.stale-"),
        );
        let (temp_prefix, temp_suffix) = crate::private_fs::temp_affixes(path);
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut copies: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    let entry = entry.file_name();
                    let entry = entry.to_string_lossy();
                    entry.starts_with(&bak)
                        || entry.starts_with(&corrupt)
                        || entry.starts_with(&stale)
                        || (entry.starts_with(&temp_prefix) && entry.ends_with(temp_suffix))
                })
                .map(|entry| entry.path())
                .collect();
            copies.sort();
            out.extend(copies);
        }
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
    /// Windows permission summary (`private`, `open_to_others`,
    /// `writable_by_others` or `unknown`: whether a broad principal such as
    /// Everyone or Users can reach it); `None` elsewhere or when absent.
    pub acl: Option<&'static str>,
    /// `user_version` or JSON `schema`; `unversioned`/`not_applicable`/
    /// `unreadable` otherwise.
    pub schema: String,
}

/// Most directory entries a size walk visits.
const WALK_LIMIT: usize = 10_000;

/// Inspect every artifact under `roots` (no network, no secret read).
pub fn inspect(roots: &Roots) -> Vec<ArtifactState> {
    ARTIFACTS
        .iter()
        // macOS-only paths: absent from the report elsewhere, and while
        // they are the data root's own files.
        .filter(|a| {
            (a.root != Root::MacHome
                && a.id != "metered.records_legacy"
                && a.id != "feed.tree_legacy")
                || roots.path_of(a).is_some()
        })
        .map(|a| inspect_one(a, roots))
        .collect()
}

fn inspect_one(artifact: &'static Artifact, roots: &Roots) -> ArtifactState {
    let Some(path) = roots.path_of(artifact) else {
        return ArtifactState {
            id: artifact.id,
            path: if matches!(artifact.kind, Kind::VaultService | Kind::AppManaged) {
                artifact.path.replace('`', "")
            } else {
                "unavailable".to_string()
            },
            kind: artifact.kind,
            exists: None,
            found: None,
            bytes: None,
            mode: None,
            mode_expected: None,
            acl: None,
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
        path: crate::text::strip_disguising(&home_abbreviated(&path)).into_owned(),
        kind: artifact.kind,
        exists: Some(meta.is_some()),
        found,
        bytes,
        mode,
        mode_expected,
        acl: meta
            .as_ref()
            .and_then(|_| crate::private_fs::acl_summary(&path)),
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
    /// PRD 0044: notification channels the person added.
    NotificationChannels,
}

impl Purpose {
    pub const ALL: [Purpose; 11] = [
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
        Self::NotificationChannels,
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
            "each configured provider's API (or the base URL you set; a plain-HTTP, local or private-network address is reached directly, never through an environment proxy)",
            "each refresh of an account; a model list you open; `cuotai --doctor --probe` (one `HEAD` per host, without the credential)",
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
            "public status pages of configured providers, listed in `docs/providers.md` (public incident source registry) and in Settings › Data & privacy",
            "every 5 minutes at most, while turned on; `cuotai --doctor --probe` while turned on; `cuotai --canary --status`",
            "a `GET` with the `User-Agent` `cuotai/<version> (+https://github.com/jmarcon/cuotai)`; no credential, cookie or account data",
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
            "GitHub Releases and its asset download host (release manifest `latest.json` and signed installers)",
            "on opening and every 24 hours, after you allow it once",
            "a `GET` with `accept` and the `User-Agent` `tauri-plugin-updater/<version>` (the updater plugin's, not Cuotai's); no query, body, cookie or identifier. Then only the installer you accept",
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
            "decided by the program you approved; Cuotai passes it only `PATH`, `HOME`, `LANG` (`SystemRoot` on Windows), the environment variables you approved and the extension's own declared secrets",
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
            "inbound: programs on this computer (loopback only)",
            "while `--serve` or Settings → Integration runs it",
            "the readings already shown, without credentials; a bearer token is always required by the desktop and by default by `--serve`",
            "off",
            "0012",
        ),
        Purpose::NotificationChannels => (
            "only the webhook, ntfy, Slack or Discord address you add as a channel (a plain-HTTP, local or private-network one directly, never through an environment proxy)",
            "a notification Cuotai would show on this computer, after the same preferences and quiet hours, at most 20 per hour per channel; Send test",
            "the notification's title and text, its kind and severity; for a webhook also the provider, the stable account id, the metric window, the percent used and the reset time; the metric label and the account name only if you choose to show names; with the digest kind on, the weekly digest text (a section per account when names are shown), covering only the accounts the channel's filter admits; never readings in bulk or a credential",
            "off: nothing until you add a channel",
            "0044",
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
        // PRD 0052 S-01/S-03: only the app can reach these.
        (_, Kind::AppManaged) => match artifact.id {
            "autostart.entry" => {
                "desktop: **Erase all Cuotai data**, or turn off **Open at login**"
            }
            "capture.profiles" => "desktop: **Erase all Cuotai data**, or the next start",
            _ => "desktop: **Erase all Cuotai data**",
        },
        (Erase::WithConfig, _) => {
            "`--erase-all-data --include-config`; desktop: **Erase all Cuotai data** with the accounts-file box"
        }
        (Erase::Always, _) => "`--erase-all-data`; desktop: **Erase all Cuotai data**",
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
        let location = if a.root == Root::App {
            // A description that marks its own code spans.
            a.path.to_string()
        } else {
            format!(
                "`{}{}`{}",
                a.root.template(),
                a.path,
                if a.rotated { " (and `.1`)" } else { "" }
            )
        };
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
            "notifications.lock",
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
            "serve.access_log",
            "vault.oauth",
            "vault.serve",
            "channels.log",
            "process.presence",
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
            mac_home: None,
            metered: None,
            feed: None,
            ..Roots::default()
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
        #[cfg(feature = "serve")]
        assert_eq!(
            path("serve.access_log"),
            crate::serve::access_log_path(Path::new("/data/cuotai"))
        );
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

    /// PRD 0033 re-validation M-4: the metered records live in one fixed
    /// place; a moved data folder's old copy is listed (and erased) as a
    /// leftover, and only when it is a different folder.
    #[test]
    fn metered_records_stay_put_and_a_moved_copy_is_a_leftover() {
        let fixed = PathBuf::from("/os/cuotai/metered");
        let moved = Roots {
            local_data: Some(PathBuf::from("/moved/cuotai")),
            metered: Some(fixed.clone()),
            ..Roots::default()
        };
        let path = |roots: &Roots, id: &str| roots.path_of(artifact(id).unwrap());
        assert_eq!(path(&moved, "metered.records"), Some(fixed.clone()));
        // PRD 0052 S-04: the presence locks sit beside them, whatever the
        // data folder.
        assert_eq!(
            path(&moved, "process.presence"),
            Some(PathBuf::from("/os/cuotai/running"))
        );
        assert_eq!(
            path(&moved, "metered.records_legacy"),
            Some(PathBuf::from("/moved/cuotai/metered"))
        );
        let default = Roots {
            local_data: Some(PathBuf::from("/os/cuotai")),
            ..moved
        };
        assert_eq!(path(&default, "metered.records_legacy"), None);
        assert!(!inspect(&default)
            .iter()
            .any(|state| state.id == "metered.records_legacy"));
    }

    #[test]
    fn macos_widget_copies_live_below_the_home_unless_the_data_root_holds_them() {
        let moved = Roots {
            local_data: Some(PathBuf::from("/moved/cuotai")),
            config: None,
            accounts: None,
            mac_home: Some(PathBuf::from("/home/ana")),
            metered: None,
            feed: None,
            ..Roots::default()
        };
        let path = |roots: &Roots, id: &str| roots.path_of(artifact(id).unwrap());
        let fixed = Path::new("/home/ana/Library/Application Support/cuotai/widget.json");
        assert_eq!(
            path(&moved, "widgets.snapshot_macos").as_deref(),
            Some(fixed)
        );
        assert_eq!(
            path(&moved, "widgets.snapshot_legacy").as_deref(),
            Some(Path::new(
                "/home/ana/Library/Group Containers/WL9XSC6H36.dev.cuotai.desktop/widget.json"
            ))
        );
        // The default macOS data folder already is that folder: one row.
        let default = Roots {
            local_data: Some(PathBuf::from(
                "/home/ana/Library/Application Support/cuotai",
            )),
            ..moved.clone()
        };
        assert_eq!(path(&default, "widgets.snapshot_macos"), None);
        assert_eq!(path(&default, "widgets.snapshot").as_deref(), Some(fixed));
        let ids: Vec<_> = inspect(&default).iter().map(|s| s.id).collect();
        assert!(!ids.contains(&"widgets.snapshot_macos"));
        assert!(ids.contains(&"widgets.snapshot_legacy"));
        // Elsewhere there is no such path at all.
        let other = Roots {
            mac_home: None,
            ..moved
        };
        assert_eq!(path(&other, "widgets.snapshot_legacy"), None);
        assert!(!inspect(&other)
            .iter()
            .any(|s| s.id.starts_with("widgets.snapshot_")));
    }

    #[test]
    fn inspection_reports_kind_size_mode_and_schema_without_content() {
        let dir = tempfile::tempdir().unwrap();
        let roots = Roots {
            local_data: Some(dir.path().join("cuotai")),
            config: Some(dir.path().join("conf")),
            accounts: None,
            mac_home: None,
            metered: None,
            feed: None,
            ..Roots::default()
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

    /// PRD 0032 RF4.2: `notified.lock` sits beside the ledger, and a
    /// `notified.lock.stale-<pid>-<n>` left by a crash belongs to it.
    #[test]
    fn the_ledger_lock_and_its_stale_leftovers_are_companions() {
        let roots = Roots {
            local_data: Some(PathBuf::from("/data/cuotai")),
            ..Roots::default()
        };
        let lock = artifact("notifications.lock").unwrap();
        let ledger = roots.path_of(artifact("notifications.ledger").unwrap());
        assert_eq!(
            roots.path_of(lock),
            ledger.as_deref().map(crate::resets::lock_path)
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notified.lock");
        let leftover = dir.path().join("notified.lock.stale-1-0");
        std::fs::write(&path, "1:0").unwrap();
        std::fs::write(&leftover, "1:0").unwrap();
        let found = companions(lock, &path);
        assert!(found.contains(&path));
        assert!(found.contains(&leftover));
    }

    /// PRD 0054 re-validation S-01: a damaged hook claims file set aside as
    /// `claims.json.corrupt-<time>` is part of the claims artifact.
    #[test]
    fn a_set_aside_claims_file_is_a_companion() {
        let claims = artifact("hooks.claims").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let path = hooks.join("claims.json");
        let aside = hooks.join("claims.json.corrupt-20261118T000000Z");
        std::fs::write(&aside, "{not json").unwrap();
        assert!(companions(claims, &path).contains(&aside));
    }
}
