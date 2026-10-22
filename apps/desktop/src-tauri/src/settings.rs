//! Configurações do app desktop (não confundir com o `cuotai.toml` de contas, que é do core).
//! Arquivo `desktop.json` na pasta padrão de configuração: pasta de temas e idiomas e o
//! `cuotai.toml` escolhido pelo usuário (só o caminho; nunca dentro do TOML).
use cuotai_core::{
    alerts::{AlertPrefs, THRESHOLD_RANGE},
    history::Retention,
    resets::{NotifyWhen, Prefs},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, Default, Clone)]
struct Stored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_file: Option<String>,
    /// Animações (PRD 0004 RF1.2); ausente = `system`. Texto livre na leitura: um valor
    /// desconhecido vira `system` sem invalidar os demais campos do arquivo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    motion: Option<String>,
    /// Consumption pace (PRD 0007 RF3.4); absent = shown. Only `false` is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    show_pace: Option<bool>,
    /// Days of measurement history kept (PRD 0006 RF4.1, 7 to 365); absent
    /// = the core default (90). A value out of range reads as the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history_days: Option<u32>,
    /// Recording of the measurement history (PRD 0006 RF5.3); absent = on.
    /// Only `false` is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history_recording: Option<bool>,
    /// Reset notices of this device (PRD 0003 RF3); absent = off. Read
    /// leniently ([`NotifyPrefs::from_value`]): a hand-edited value of the
    /// wrong type falls back to its default without invalidating the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notify: Option<serde_json::Value>,
    /// The language chosen in the app, which the notices sent by the backend
    /// use (PRD 0003 RF2.2); absent until the interface first reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
    /// Usage alerts of this device (PRD 0008 RF3); absent = off. Read
    /// leniently like `notify` ([`AlertSettings::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alerts: Option<serde_json::Value>,
    /// Public provider status queries (PRD 0011 RF4.3); absent = off, the
    /// owner's opt-in decision. Only `true` is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status_queries: Option<bool>,
    /// The local server (PRD 0012 RF5.1); absent = off, the owner's
    /// decision. Read leniently like `notify` ([`ServePrefs::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    serve: Option<serde_json::Value>,
    /// Hooks run in the app (PRD 0013 RF4); absent = off, as RF3.2
    /// requires. Only `true` is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hooks: Option<bool>,
    /// Widget snapshot publishing (PRD 0014 RF4.1); absent = off, the
    /// owner's choice. Read leniently like `serve`
    /// ([`WidgetPrefs::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    widgets: Option<serde_json::Value>,
    /// Update checks of this device (PRD 0018 RF2.1/RF3.3); absent = on,
    /// nothing recorded yet. Read leniently like `notify`
    /// ([`UpdaterPrefs::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updater: Option<serde_json::Value>,
}

/// PRD 0018 RF2.1/RF3.3: the update-check preferences of this device.
/// Automatic checks are on by default (RF2.1); `last_check` is the RFC 3339
/// instant the last check finished; `explanation_seen` records that the
/// first-run privacy explanation (what a check sends) was shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdaterPrefs {
    pub automatic: bool,
    pub last_check: Option<String>,
    pub explanation_seen: bool,
}

impl Default for UpdaterPrefs {
    fn default() -> Self {
        Self {
            automatic: true,
            last_check: None,
            explanation_seen: false,
        }
    }
}

impl UpdaterPrefs {
    /// Each field on its own: an unknown or mistyped value is its default.
    /// A malformed timestamp is also absent, so the UI never receives
    /// `Invalid Date` from a hand-edited preference file.
    fn from_value(value: &serde_json::Value) -> Self {
        let default = Self::default();
        let last_check = value["last_check"].as_str().and_then(|text| {
            let text = text.trim();
            chrono::DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|_| text.to_owned())
        });
        Self {
            automatic: value["automatic"].as_bool().unwrap_or(default.automatic),
            last_check,
            explanation_seen: value["explanation_seen"]
                .as_bool()
                .unwrap_or(default.explanation_seen),
        }
    }
}

fn updater_of(stored: &Stored) -> UpdaterPrefs {
    stored
        .updater
        .as_ref()
        .map(UpdaterPrefs::from_value)
        .unwrap_or_default()
}

/// PRD 0018 RF3.3: the update-check preferences stored in `file`.
pub fn read_updater(file: &Path) -> UpdaterPrefs {
    updater_of(&read(file))
}

/// Store the update-check preferences in `file`; the defaults remove the key.
pub fn write_updater(file: &Path, prefs: &UpdaterPrefs) -> Result<(), &'static str> {
    update(file, |s| {
        s.updater = (*prefs != UpdaterPrefs::default())
            .then(|| serde_json::to_value(prefs).ok())
            .flatten()
    })
}

/// Lead times offered, in minutes ("at the time" is 0).
pub const NOTIFY_LEADS: [u32; 4] = [0, 5, 15, 60];
/// "When to notify" filters, in the order the interface shows them.
pub const NOTIFY_WHENS: [&str; 3] = ["used", "exhausted", "always"];

/// PRD 0003 RF3: reset notice preferences of this device. The global switch
/// is off by default; `off_accounts` holds the `Account::key`s whose bell is
/// off (a rename changes the key and so loses the choice).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotifyPrefs {
    pub enabled: bool,
    /// One of [`NOTIFY_LEADS`].
    pub lead_minutes: u32,
    /// One of [`NOTIFY_WHENS`].
    pub when: &'static str,
    pub include_other: bool,
    pub off_accounts: BTreeSet<String>,
}

impl Default for NotifyPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            lead_minutes: 0,
            when: NOTIFY_WHENS[0],
            include_other: false,
            off_accounts: BTreeSet::new(),
        }
    }
}

impl NotifyPrefs {
    /// Each field on its own: an unknown or mistyped value is its default.
    fn from_value(value: &serde_json::Value) -> Self {
        let default = Self::default();
        let lead = value["lead_minutes"].as_u64();
        let when = value["when"].as_str();
        Self {
            enabled: value["enabled"].as_bool().unwrap_or(default.enabled),
            lead_minutes: NOTIFY_LEADS
                .into_iter()
                .find(|m| lead == Some(u64::from(*m)))
                .unwrap_or(default.lead_minutes),
            when: NOTIFY_WHENS
                .into_iter()
                .find(|w| when == Some(w))
                .unwrap_or(default.when),
            include_other: value["include_other"]
                .as_bool()
                .unwrap_or(default.include_other),
            off_accounts: value["off_accounts"]
                .as_array()
                .map(|keys| {
                    keys.iter()
                        .filter_map(|key| key.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The core planner's preferences.
    pub fn planner(&self) -> Prefs {
        Prefs {
            lead: chrono::TimeDelta::minutes(i64::from(self.lead_minutes)),
            when: match self.when {
                "exhausted" => NotifyWhen::OnlyIfExhausted,
                "always" => NotifyWhen::Always,
                _ => NotifyWhen::OnlyIfUsed,
            },
            include_other: self.include_other,
            disabled: self.off_accounts.clone(),
        }
    }

    /// Lead time and filter from the interface. Error: `invalid_notify`.
    pub fn with_choices(
        &self,
        lead_minutes: u32,
        when: &str,
        include_other: bool,
    ) -> Result<Self, &'static str> {
        let lead_minutes = NOTIFY_LEADS
            .into_iter()
            .find(|m| *m == lead_minutes)
            .ok_or("invalid_notify")?;
        let when = NOTIFY_WHENS
            .into_iter()
            .find(|w| *w == when)
            .ok_or("invalid_notify")?;
        Ok(Self {
            lead_minutes,
            when,
            include_other,
            ..self.clone()
        })
    }
}

fn notify_of(stored: &Stored) -> NotifyPrefs {
    stored
        .notify
        .as_ref()
        .map(NotifyPrefs::from_value)
        .unwrap_or_default()
}

/// PRD 0008 RF3: usage alert preferences of this device. Off by default;
/// the account's bell ([`NotifyPrefs::off_accounts`]) mutes its alerts
/// too, and `muted` holds the metrics muted in the details per
/// `Account::key` (a rename changes the key and so loses the choice).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlertSettings {
    pub enabled: bool,
    /// Ascending, each within `alerts::THRESHOLD_RANGE`.
    pub thresholds: Vec<u8>,
    pub exhaustion: bool,
    pub recovery: bool,
    /// Account key → muted metric labels.
    pub muted: BTreeMap<String, BTreeSet<String>>,
}

impl Default for AlertSettings {
    /// The core's defaults: off, 75 and 90%, exhaustion and recovery.
    fn default() -> Self {
        let core = AlertPrefs::default();
        Self {
            enabled: core.enabled,
            thresholds: core.thresholds().collect(),
            exhaustion: core.exhaustion,
            recovery: core.recovery,
            muted: BTreeMap::new(),
        }
    }
}

impl AlertSettings {
    /// Each field on its own: an unknown or mistyped value is its default;
    /// thresholds outside the range are dropped.
    fn from_value(value: &serde_json::Value) -> Self {
        let default = Self::default();
        let flag = |key: &str, default: bool| value[key].as_bool().unwrap_or(default);
        Self {
            enabled: flag("enabled", default.enabled),
            thresholds: value["thresholds"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|t| u8::try_from(t.as_u64()?).ok())
                        .filter(|t| THRESHOLD_RANGE.contains(t))
                        .collect::<BTreeSet<u8>>()
                        .into_iter()
                        .collect()
                })
                .unwrap_or(default.thresholds),
            exhaustion: flag("exhaustion", default.exhaustion),
            recovery: flag("recovery", default.recovery),
            muted: value["muted"]
                .as_object()
                .map(|accounts| {
                    accounts
                        .iter()
                        .filter_map(|(key, labels)| {
                            let labels: BTreeSet<String> = labels
                                .as_array()?
                                .iter()
                                .filter_map(|label| label.as_str().map(str::to_owned))
                                .collect();
                            (!labels.is_empty()).then(|| (key.clone(), labels))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Thresholds (each 10 to 99; repeats collapse) and the exhaustion and
    /// recovery notices from the interface. Error: `invalid_threshold`.
    pub fn with_choices(
        &self,
        thresholds: &[u8],
        exhaustion: bool,
        recovery: bool,
    ) -> Result<Self, &'static str> {
        let checked =
            AlertPrefs::new(true, thresholds.iter().copied()).map_err(|_| "invalid_threshold")?;
        Ok(Self {
            thresholds: checked.thresholds().collect(),
            exhaustion,
            recovery,
            ..self.clone()
        })
    }

    /// Mute (`on = false`) or unmute the metric `label` of `account_key`.
    pub fn with_metric(&self, account_key: &str, label: &str, on: bool) -> Self {
        let mut next = self.clone();
        if on {
            if let Some(labels) = next.muted.get_mut(account_key) {
                labels.remove(label);
                if labels.is_empty() {
                    next.muted.remove(account_key);
                }
            }
        } else {
            next.muted
                .entry(account_key.to_owned())
                .or_default()
                .insert(label.to_owned());
        }
        next
    }

    /// The core evaluator's preferences; `off_accounts` are the accounts
    /// whose bell is off.
    pub fn evaluator(&self, off_accounts: &BTreeSet<String>) -> AlertPrefs {
        // Always valid: read and chosen within the range.
        let mut prefs =
            AlertPrefs::new(self.enabled, self.thresholds.iter().copied()).unwrap_or_default();
        prefs.exhaustion = self.exhaustion;
        prefs.recovery = self.recovery;
        prefs.muted_accounts = off_accounts.clone();
        prefs.muted_metrics = self.muted.clone();
        prefs
    }
}

fn alerts_of(stored: &Stored) -> AlertSettings {
    stored
        .alerts
        .as_ref()
        .map(AlertSettings::from_value)
        .unwrap_or_default()
}

/// PRD 0008 RF3: the usage alert preferences stored in `file`.
pub fn read_alerts(file: &Path) -> AlertSettings {
    alerts_of(&read(file))
}

/// Store the usage alert preferences in `file`; the defaults remove the key.
pub fn write_alerts(file: &Path, alerts: &AlertSettings) -> Result<(), &'static str> {
    update(file, |s| {
        s.alerts = (*alerts != AlertSettings::default())
            .then(|| serde_json::to_value(alerts).ok())
            .flatten()
    })
}

/// Ports the desktop server accepts: unprivileged ones, so a choice never
/// depends on running as an administrator.
pub const SERVE_PORTS: std::ops::RangeInclusive<u16> = 1024..=u16::MAX;

/// PRD 0012 RF5.1: the local server of this device. Off by default, on the
/// core's suggested port, refusing web pages and without a token; the token
/// itself lives in the OS vault, `token` only says one is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ServePrefs {
    pub enabled: bool,
    /// One of [`SERVE_PORTS`]; bound exactly, never moved to another one.
    pub port: u16,
    pub allow_browsers: bool,
    pub token: bool,
}

impl Default for ServePrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            port: cuotai_core::serve::DEFAULT_PORT,
            allow_browsers: false,
            token: false,
        }
    }
}

impl ServePrefs {
    /// Each field on its own: an unknown or mistyped value is its default.
    fn from_value(value: &serde_json::Value) -> Self {
        let default = Self::default();
        let flag = |key: &str, default: bool| value[key].as_bool().unwrap_or(default);
        Self {
            enabled: flag("enabled", default.enabled),
            port: value["port"]
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
                .filter(|port| SERVE_PORTS.contains(port))
                .unwrap_or(default.port),
            allow_browsers: flag("allow_browsers", default.allow_browsers),
            token: flag("token", default.token),
        }
    }

    /// The switch, port and browser choice from the interface, keeping the
    /// token requirement. Error: `invalid_port`.
    pub fn with_choices(
        self,
        enabled: bool,
        port: u16,
        allow_browsers: bool,
    ) -> Result<Self, &'static str> {
        if !SERVE_PORTS.contains(&port) {
            return Err("invalid_port");
        }
        Ok(Self {
            enabled,
            port,
            allow_browsers,
            ..self
        })
    }
}

fn serve_of(stored: &Stored) -> ServePrefs {
    stored
        .serve
        .as_ref()
        .map(ServePrefs::from_value)
        .unwrap_or_default()
}

/// PRD 0012 RF5.1: the local server preferences stored in `file`.
pub fn read_serve(file: &Path) -> ServePrefs {
    serve_of(&read(file))
}

/// Store the local server preferences in `file`; the defaults remove the key.
pub fn write_serve(file: &Path, prefs: &ServePrefs) -> Result<(), &'static str> {
    update(file, |s| {
        s.serve = (*prefs != ServePrefs::default())
            .then(|| serde_json::to_value(prefs).ok())
            .flatten()
    })
}

/// PRD 0012 RF5.1: the local server preferences of this device.
pub fn serve_prefs() -> ServePrefs {
    settings_file().map(|f| read_serve(&f)).unwrap_or_default()
}

/// Store the local server preferences of this device. Error: `unwritable`.
pub fn save_serve(prefs: &ServePrefs) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    write_serve(&file, prefs)
}

/// PRD 0014 RF4.1/RF5: accounts published in `widget.json`: from 1 (the most
/// restrictive) to [`MAX_WIDGET_ACCOUNTS`]; a value outside reads as the
/// default.
pub const MAX_WIDGET_ACCOUNTS: usize = 24;

/// PRD 0014 RF4.1: publishing the widget snapshot of this device. Off by
/// default (no file is written and an existing one is deleted, RF2.2), with
/// identity off (no e-mail) and at most [`DEFAULT_WIDGET_ACCOUNTS`]
/// accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WidgetPrefs {
    pub enabled: bool,
    pub include_identity: bool,
    pub max_accounts: usize,
}

/// PRD 0014 RF4.1: the account limit offered when none was chosen.
pub const DEFAULT_WIDGET_ACCOUNTS: usize = 6;

impl Default for WidgetPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            include_identity: false,
            max_accounts: DEFAULT_WIDGET_ACCOUNTS,
        }
    }
}

impl WidgetPrefs {
    /// Each field on its own: an unknown or mistyped value is its default
    /// (read leniently, like [`ServePrefs::from_value`]).
    fn from_value(value: &serde_json::Value) -> Self {
        let default = Self::default();
        let flag = |key: &str, default: bool| value[key].as_bool().unwrap_or(default);
        Self {
            enabled: flag("enabled", default.enabled),
            include_identity: flag("include_identity", default.include_identity),
            max_accounts: value["max_accounts"]
                .as_u64()
                .and_then(|accounts| usize::try_from(accounts).ok())
                .filter(|accounts| (1..=MAX_WIDGET_ACCOUNTS).contains(accounts))
                .unwrap_or(default.max_accounts),
        }
    }

    /// The choices from the interface. Error: `invalid_widget_accounts`.
    pub fn with_choices(
        mut self,
        enabled: bool,
        include_identity: bool,
        max_accounts: usize,
    ) -> Result<Self, &'static str> {
        if !(1..=MAX_WIDGET_ACCOUNTS).contains(&max_accounts) {
            return Err("invalid_widget_accounts");
        }
        self.enabled = enabled;
        self.include_identity = include_identity;
        self.max_accounts = max_accounts;
        Ok(self)
    }
}

fn widgets_of(stored: &Stored) -> WidgetPrefs {
    stored
        .widgets
        .as_ref()
        .map(WidgetPrefs::from_value)
        .unwrap_or_default()
}

/// PRD 0014 RF4.1: the widget preferences stored in `file`.
pub fn read_widgets(file: &Path) -> WidgetPrefs {
    widgets_of(&read(file))
}

/// Store the widget preferences in `file`; the defaults remove the key.
pub fn write_widgets(file: &Path, prefs: &WidgetPrefs) -> Result<(), &'static str> {
    update(file, |s| {
        s.widgets = (*prefs != WidgetPrefs::default())
            .then(|| serde_json::to_value(prefs).ok())
            .flatten()
    })
}

/// PRD 0014 RF4.1: the widget preferences of this device.
pub fn widget_prefs() -> WidgetPrefs {
    settings_file()
        .map(|f| read_widgets(&f))
        .unwrap_or_default()
}

/// Store the widget preferences of this device. Error: `unwritable`.
pub fn save_widgets(prefs: &WidgetPrefs) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    write_widgets(&file, prefs)
}

/// PRD 0003 RF3: the reset notice preferences stored in `file`.
pub fn read_notify(file: &Path) -> NotifyPrefs {
    notify_of(&read(file))
}

/// PRD 0003 RF2.2: the language last chosen in the app, stored in `file`.
pub fn read_locale(file: &Path) -> Option<String> {
    read(file).locale
}

/// `desktop.json` of this device.
pub fn settings_path() -> Option<PathBuf> {
    settings_file()
}

/// Store the notice preferences in `file`; the defaults remove the key.
pub fn write_notify(file: &Path, prefs: &NotifyPrefs) -> Result<(), &'static str> {
    update(file, |s| {
        s.notify = (*prefs != NotifyPrefs::default())
            .then(|| serde_json::to_value(prefs).ok())
            .flatten()
    })
}

/// Store the language of the notices in `file`.
pub fn write_locale(file: &Path, locale: &str) -> Result<(), &'static str> {
    update(file, |s| s.locale = Some(locale.to_owned()))
}

/// The history retention stored in `desktop.json`; the core default when
/// absent or out of range.
fn retention_of(stored: &Stored) -> Retention {
    stored
        .history_days
        .and_then(|days| Retention::days(days).ok())
        .unwrap_or_default()
}

/// PRD 0006 RF4.1: the measurement history retention of this device.
pub fn history_retention() -> Retention {
    retention_of(&load())
}

/// On unless `desktop.json` says `false`.
fn recording_of(stored: &Stored) -> bool {
    stored.history_recording != Some(false)
}

/// PRD 0006 RF5.3: whether this device records new readings.
pub fn history_recording() -> bool {
    recording_of(&load())
}

/// Off unless `desktop.json` says `true`.
fn status_queries_of(stored: &Stored) -> bool {
    stored.status_queries == Some(true)
}

/// PRD 0011 RF4.3: whether this device queries the providers' public
/// status pages (off until the user turns it on).
pub fn status_queries() -> bool {
    status_queries_of(&load())
}

/// Off unless `desktop.json` says `true`.
fn hooks_of(stored: &Stored) -> bool {
    stored.hooks == Some(true)
}

/// PRD 0013 RF4: whether this device runs the confirmed hooks (off until
/// the user turns them on).
pub fn hooks_enabled() -> bool {
    hooks_of(&load())
}

/// Valores aceitos para `motion`, na ordem exibida pela UI.
pub const MOTIONS: [&str; 4] = ["system", "always", "reduced", "off"];

fn motion_of(stored: &Stored) -> &'static str {
    MOTIONS
        .into_iter()
        .find(|m| stored.motion.as_deref() == Some(m))
        .unwrap_or("system")
}

/// Pasta padrão de configuração do Cuotai (`<config do SO>/cuotai`).
pub fn default_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cuotai"))
}

fn settings_file() -> Option<PathBuf> {
    default_dir().map(|d| d.join("desktop.json"))
}

fn read(file: &Path) -> Stored {
    fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn load() -> Stored {
    settings_file().map(|f| read(&f)).unwrap_or_default()
}

/// Altera um campo preservando os demais. Escrita atômica (temporário + rename).
fn update(file: &Path, change: impl FnOnce(&mut Stored)) -> Result<(), &'static str> {
    let parent = file.parent().ok_or("unwritable")?;
    fs::create_dir_all(parent).map_err(|_| "unwritable")?;
    let mut stored = read(file);
    change(&mut stored);
    let tmp = parent.join(".desktop.json.tmp");
    let text = serde_json::to_string_pretty(&stored).map_err(|_| "unwritable")?;
    fs::write(&tmp, text).map_err(|_| "unwritable")?;
    fs::rename(&tmp, file).map_err(|_| "unwritable")
}

/// Pasta personalizada salva pelo usuário (sem considerar `CUOTAI_USER_DIR`).
pub fn custom_user_dir() -> Option<PathBuf> {
    load()
        .user_dir
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// A variável de ambiente tem prioridade e bloqueia a edição pela interface.
pub fn env_user_dir() -> Option<PathBuf> {
    std::env::var_os("CUOTAI_USER_DIR").map(PathBuf::from)
}

/// Pasta efetiva: variável de ambiente → configuração do usuário → padrão.
pub fn effective_user_dir() -> Option<PathBuf> {
    env_user_dir().or_else(custom_user_dir).or_else(default_dir)
}

/// Erros com código estável (a UI traduz): `invalid_path`, `not_dir`, `unwritable`.
pub fn validate_dir(path: &Path) -> Result<(), &'static str> {
    if !path.is_absolute() || path.parent().is_none() || path.components().count() < 3 {
        return Err("invalid_path"); // relativo ou raiz do disco / pasta de topo
    }
    if path.exists() && !path.is_dir() {
        return Err("not_dir");
    }
    fs::create_dir_all(path).map_err(|_| "unwritable")?;
    let probe = path.join(".cuotai-write-test");
    fs::File::create(&probe)
        .and_then(|mut f| f.write_all(b"ok"))
        .map_err(|_| "unwritable")?;
    let _ = fs::remove_file(probe);
    Ok(())
}

/// Grava (ou, com `None`, remove) a pasta personalizada.
pub fn save_user_dir(dir: Option<&Path>) -> Result<(), &'static str> {
    if let Some(d) = dir {
        validate_dir(d)?;
    }
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.user_dir = dir.map(|d| d.display().to_string()))
}

/// `cuotai.toml` escolhido nas configurações (sem considerar `CUOTAI_CONFIG`).
pub fn chosen_config() -> Option<PathBuf> {
    load()
        .config_file
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Erro com código estável (a UI traduz): `not_file` (relativo, ausente ou pasta).
pub fn validate_config_file(path: &Path) -> Result<(), &'static str> {
    if path.is_absolute() && path.is_file() {
        Ok(())
    } else {
        Err("not_file")
    }
}

/// Grava (ou, com `None`, remove) o arquivo escolhido; o chamador já validou e carregou.
pub fn save_config_file(path: Option<&Path>) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.config_file = path.map(|p| p.display().to_string())
    })
}

/// Grava a preferência de animações. Erro com código estável: `invalid_motion`.
pub fn save_motion(motion: &str) -> Result<(), &'static str> {
    if !MOTIONS.contains(&motion) {
        return Err("invalid_motion");
    }
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.motion = (motion != "system").then(|| motion.to_owned())
    })
}

/// Shows or hides the consumption pace; `true` (the default) removes the key.
pub fn save_show_pace(show: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.show_pace = (!show).then_some(false))
}

/// Keeps `days` of measurement history (the default, 90, removes the key).
/// Errors: `invalid_history_days` (outside 7 to 365), `unwritable`.
pub fn save_history_days(days: u32) -> Result<Retention, &'static str> {
    let retention = Retention::days(days).map_err(|_| "invalid_history_days")?;
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.history_days = (retention != Retention::default()).then_some(days)
    })?;
    Ok(retention)
}

/// Records new readings or not; `true` (the default) removes the key.
pub fn save_history_recording(on: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.history_recording = (!on).then_some(false))
}

/// Queries the public status pages or not; `false` (the default) removes
/// the key.
pub fn save_status_queries(on: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.status_queries = on.then_some(true))
}

/// Runs the confirmed hooks or not; `false` (the default) removes the key.
pub fn save_hooks_enabled(on: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.hooks = on.then_some(true))
}

#[derive(Serialize)]
pub struct SettingsView {
    /// Pasta em uso agora.
    user_dir: Option<String>,
    default_dir: Option<String>,
    /// Pasta escolhida pelo usuário (vazia = usa a padrão).
    custom_dir: Option<String>,
    /// `CUOTAI_USER_DIR` definida: a escolha pela interface não tem efeito.
    env_locked: bool,
    /// `cuotai.toml` escolhido (vazio = padrão do SO).
    config_file: Option<String>,
    /// `CUOTAI_CONFIG` definida: tem prioridade sobre o arquivo escolhido.
    config_env_locked: bool,
    /// Animações: `system`, `always`, `reduced` ou `off`.
    motion: &'static str,
    /// Consumption pace on cards, details and tray (on by default).
    show_pace: bool,
    /// Days of measurement history kept, and the accepted range.
    history_days: u16,
    history_days_min: u16,
    history_days_max: u16,
    /// New readings enter the history (on by default).
    history_recording: bool,
    /// PRD 0011 RF4.3: public status queries (off by default).
    status_queries: bool,
    /// PRD 0003 RF3: reset notices (off by default) and the choices offered.
    notify: NotifyPrefs,
    notify_leads: [u32; 4],
    notify_whens: [&'static str; 3],
    /// PRD 0008 RF3: usage alerts (off by default) and the threshold range.
    alerts: AlertSettings,
    alert_min: u8,
    alert_max: u8,
    /// PRD 0014 RF4.1: widget publishing (off by default) and the accepted
    /// account limit.
    widgets: WidgetPrefs,
    widget_accounts_min: usize,
    widget_accounts_max: usize,
    version: &'static str,
}

pub fn view() -> SettingsView {
    let stored = load();
    SettingsView {
        user_dir: effective_user_dir().map(|p| p.display().to_string()),
        default_dir: default_dir().map(|p| p.display().to_string()),
        custom_dir: custom_user_dir().map(|p| p.display().to_string()),
        env_locked: env_user_dir().is_some(),
        config_file: chosen_config().map(|p| p.display().to_string()),
        config_env_locked: cuotai_core::config_file::Locations::from_process()
            .env
            .is_some(),
        motion: motion_of(&stored),
        show_pace: stored.show_pace != Some(false),
        history_days: retention_of(&stored).keep_days(),
        history_days_min: Retention::MIN_DAYS,
        history_days_max: Retention::MAX_DAYS,
        history_recording: recording_of(&stored),
        status_queries: status_queries_of(&stored),
        notify: notify_of(&stored),
        notify_leads: NOTIFY_LEADS,
        notify_whens: NOTIFY_WHENS,
        alerts: alerts_of(&stored),
        alert_min: *THRESHOLD_RANGE.start(),
        alert_max: *THRESHOLD_RANGE.end(),
        widgets: widgets_of(&stored),
        widget_accounts_min: 1,
        widget_accounts_max: MAX_WIDGET_ACCOUNTS,
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// Abre a pasta no gerenciador de arquivos do sistema. Só a pasta efetiva, nunca um caminho vindo da UI.
pub fn open_user_dir() -> Result<(), &'static str> {
    let dir = effective_user_dir().ok_or("invalid_path")?;
    fs::create_dir_all(&dir).map_err(|_| "unwritable")?;
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    cmd.arg(&dir).spawn().map(|_| ()).map_err(|_| "unwritable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_root_and_files() {
        assert_eq!(
            validate_dir(Path::new("relativo/pasta")),
            Err("invalid_path")
        );
        assert_eq!(validate_dir(Path::new("/")), Err("invalid_path"));
        assert_eq!(validate_dir(Path::new("/tmp")), Err("invalid_path"));
        let file = std::env::temp_dir().join(format!("cuotai-file-{}", std::process::id()));
        fs::write(&file, "x").unwrap();
        assert_eq!(validate_dir(&file), Err("not_dir"));
        let _ = fs::remove_file(file);
    }

    #[test]
    fn accepts_writable_dir() {
        let dir = std::env::temp_dir().join(format!("cuotai-ok-{}", std::process::id()));
        assert_eq!(validate_dir(&dir), Ok(()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn choices_are_updated_independently_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        update(&file, |s| s.user_dir = Some("/u".into())).unwrap();
        update(&file, |s| s.config_file = Some("/c/cuotai.toml".into())).unwrap();
        let stored = read(&file);
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        assert_eq!(stored.config_file.as_deref(), Some("/c/cuotai.toml"));
        update(&file, |s| s.config_file = None).unwrap();
        let stored = read(&file);
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        assert_eq!(stored.config_file, None);
    }

    #[test]
    fn motion_defaults_to_system_and_round_trips_without_touching_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(motion_of(&read(&file)), "system");
        update(&file, |s| s.user_dir = Some("/u".into())).unwrap();
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let stored = read(&file);
        assert_eq!(motion_of(&stored), "off");
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        // Valor desconhecido (edição manual) cai no padrão e não apaga o resto.
        fs::write(&file, r#"{"user_dir":"/u","motion":"fast"}"#).unwrap();
        let stored = read(&file);
        assert_eq!(motion_of(&stored), "system");
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        assert_eq!(save_motion("fast"), Err("invalid_motion"));
    }

    #[test]
    fn pace_is_shown_unless_turned_off_and_keeps_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(read(&file).show_pace, None);
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        update(&file, |s| s.show_pace = Some(false)).unwrap();
        let stored = read(&file);
        assert_eq!(stored.show_pace, Some(false));
        assert_eq!(motion_of(&stored), "off");
        // A hand-edited `true` or a removed key both mean "shown".
        fs::write(&file, r#"{"show_pace":true}"#).unwrap();
        assert_ne!(read(&file).show_pace, Some(false));
    }

    #[test]
    fn history_retention_defaults_to_90_days_and_ignores_values_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(retention_of(&read(&file)).keep_days(), 90);
        update(&file, |s| s.history_days = Some(365)).unwrap();
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let stored = read(&file);
        assert_eq!(retention_of(&stored).keep_days(), 365);
        assert_eq!(motion_of(&stored), "off");
        // Hand-edited outside 7 to 365: the default, the other fields stay.
        for days in ["6", "366", "7000000"] {
            fs::write(
                &file,
                format!(r#"{{"motion":"off","history_days":{days}}}"#),
            )
            .unwrap();
            let stored = read(&file);
            assert_eq!(retention_of(&stored).keep_days(), 90, "{days}");
            assert_eq!(motion_of(&stored), "off", "{days}");
        }
        assert_eq!(save_history_days(6), Err("invalid_history_days"));
        assert_eq!(save_history_days(400), Err("invalid_history_days"));
    }

    #[test]
    fn history_recording_is_on_unless_turned_off_and_keeps_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert!(recording_of(&read(&file)), "on by default");
        update(&file, |s| s.history_days = Some(30)).unwrap();
        update(&file, |s| s.history_recording = Some(false)).unwrap();
        let stored = read(&file);
        assert!(!recording_of(&stored));
        assert_eq!(retention_of(&stored).keep_days(), 30);
        // A hand-edited `true` means on, like a removed key.
        fs::write(&file, r#"{"history_recording":true}"#).unwrap();
        assert!(recording_of(&read(&file)));
    }

    #[test]
    fn status_queries_are_off_unless_turned_on_and_keep_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert!(!status_queries_of(&read(&file)), "off by default");
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        update(&file, |s| s.status_queries = Some(true)).unwrap();
        let stored = read(&file);
        assert!(status_queries_of(&stored));
        assert_eq!(motion_of(&stored), "off");
        // Turning it off removes the key; a hand-edited `false` is off too.
        update(&file, |s| s.status_queries = None).unwrap();
        assert!(!fs::read_to_string(&file)
            .unwrap()
            .contains("status_queries"));
        fs::write(&file, r#"{"status_queries":false}"#).unwrap();
        assert!(!status_queries_of(&read(&file)));
    }

    #[test]
    fn hooks_are_off_unless_turned_on_and_keep_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert!(!hooks_of(&read(&file)), "off by default");
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        update(&file, |s| s.hooks = Some(true)).unwrap();
        let stored = read(&file);
        assert!(hooks_of(&stored));
        assert_eq!(motion_of(&stored), "off");
        fs::write(&file, r#"{"hooks":false}"#).unwrap();
        assert!(!hooks_of(&read(&file)), "a hand-edited false is off");
    }

    /// PRD 0018 RF2.1/RF3.3: automático ligado por padrão, `last_check` e a explicação
    /// aceitada lenientes, e a gravação preservando os demais campos do arquivo.
    #[test]
    fn updater_is_on_by_default_round_trips_and_reads_leniently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        // Ausente = ligado, nada verificado, explicação ainda pendente (RF2.1).
        assert_eq!(
            read_updater(&file),
            UpdaterPrefs {
                automatic: true,
                last_check: None,
                explanation_seen: false
            }
        );
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let prefs = UpdaterPrefs {
            automatic: false,
            last_check: Some("2026-10-22T12:00:00Z".into()),
            explanation_seen: true,
        };
        write_updater(&file, &prefs).unwrap();
        assert_eq!(read_updater(&file), prefs);
        assert_eq!(motion_of(&read(&file)), "off");
        // Voltar ao padrão remove a chave, sem tocar nos demais campos.
        write_updater(&file, &UpdaterPrefs::default()).unwrap();
        assert_eq!(read(&file).updater, None);
        assert_eq!(motion_of(&read(&file)), "off");
        // Um valor editado à mão no tipo errado cai no padrão do campo, não invalida o arquivo.
        for text in [
            r#"{"motion":"off","updater":{"automatic":"sim","last_check":7,"explanation_seen":"x"}}"#,
            r#"{"motion":"off","updater":[]}"#,
            r#"{"motion":"off","updater":null}"#,
        ] {
            fs::write(&file, text).unwrap();
            assert_eq!(
                read_updater(&file),
                UpdaterPrefs {
                    automatic: true,
                    last_check: None,
                    explanation_seen: false
                },
                "{text}"
            );
            assert_eq!(motion_of(&read(&file)), "off", "{text}");
        }
        // Uma data ilegível é preservada como texto (a leitura trata como ausente).
        fs::write(
            &file,
            r#"{"updater":{"automatic":true,"last_check":"quando?","explanation_seen":true}}"#,
        )
        .unwrap();
        assert_eq!(
            read_updater(&file),
            UpdaterPrefs {
                automatic: true,
                last_check: None,
                explanation_seen: true
            }
        );
    }

    #[test]
    fn local_server_is_off_on_6738_by_default_round_trips_and_reads_leniently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        let default = read_serve(&file);
        assert_eq!(
            default,
            ServePrefs {
                enabled: false,
                port: 6738,
                allow_browsers: false,
                token: false,
            }
        );
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let on = ServePrefs {
            token: true,
            ..default.with_choices(true, 7001, true).unwrap()
        };
        write_serve(&file, &on).unwrap();
        assert_eq!(read_serve(&file), on);
        assert_eq!(motion_of(&read(&file)), "off", "other fields kept");
        // Back to the defaults removes the key.
        write_serve(&file, &default).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("serve"));
        // A privileged or impossible port is refused and keeps the choice.
        for port in [0, 80, 1023] {
            assert_eq!(on.with_choices(true, port, false), Err("invalid_port"));
        }
        assert_eq!(on.with_choices(false, 1024, false).unwrap().port, 1024);
        // A hand-edited value of the wrong type is its default, alone.
        fs::write(
            &file,
            r#"{"serve":{"enabled":true,"port":"7000","allow_browsers":1,"token":true},"motion":"off"}"#,
        )
        .unwrap();
        assert_eq!(
            read_serve(&file),
            ServePrefs {
                enabled: true,
                token: true,
                ..ServePrefs::default()
            }
        );
        fs::write(&file, r#"{"serve":{"enabled":true,"port":80}}"#).unwrap();
        assert_eq!(read_serve(&file).port, 6738);
        assert_eq!(motion_of(&read(&file)), "system");
    }

    #[test]
    fn widgets_are_off_without_identity_and_six_accounts_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        let default = read_widgets(&file);
        assert_eq!(
            default,
            WidgetPrefs {
                enabled: false,
                include_identity: false,
                max_accounts: 6,
            }
        );
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let on = default.with_choices(true, true, 3).unwrap();
        write_widgets(&file, &on).unwrap();
        assert_eq!(read_widgets(&file), on);
        assert_eq!(motion_of(&read(&file)), "off", "other fields kept");
        // Back to the defaults removes the key.
        write_widgets(&file, &default).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("widgets"));
        // The account limit is bounded; the other fields stay.
        for limit in [0, MAX_WIDGET_ACCOUNTS + 1] {
            assert_eq!(
                default.with_choices(true, false, limit),
                Err("invalid_widget_accounts"),
                "{limit}"
            );
        }
        assert_eq!(
            default.with_choices(true, false, 1).unwrap().max_accounts,
            1
        );
        // A hand-edited value of the wrong type is its default, alone.
        fs::write(
            &file,
            r#"{"widgets":{"enabled":true,"include_identity":"yes","max_accounts":0},"motion":"off"}"#,
        )
        .unwrap();
        assert_eq!(
            read_widgets(&file),
            WidgetPrefs {
                enabled: true,
                ..WidgetPrefs::default()
            }
        );
        assert_eq!(motion_of(&read(&file)), "off");
    }

    #[test]
    fn notices_are_off_by_default_round_trip_and_read_leniently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(read_notify(&file), NotifyPrefs::default());
        assert!(!read_notify(&file).enabled, "off by default");
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let chosen = NotifyPrefs {
            enabled: true,
            ..NotifyPrefs::default()
        }
        .with_choices(15, "exhausted", true)
        .unwrap();
        write_notify(&file, &chosen).unwrap();
        assert_eq!(read_notify(&file), chosen);
        assert_eq!(motion_of(&read(&file)), "off");
        // Back to the defaults removes the key.
        write_notify(&file, &NotifyPrefs::default()).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("notify"));
        // Hand-edited: each bad field falls back alone; the rest of the file stays.
        fs::write(
            &file,
            r#"{"motion":"off","notify":{"enabled":true,"lead_minutes":7,"when":"never","include_other":"yes","off_accounts":["claude:Work",3]}}"#,
        )
        .unwrap();
        let prefs = read_notify(&file);
        assert!(prefs.enabled);
        assert_eq!(
            (prefs.lead_minutes, prefs.when, prefs.include_other),
            (0, "used", false)
        );
        assert_eq!(
            prefs.off_accounts,
            BTreeSet::from(["claude:Work".to_string()])
        );
        assert_eq!(motion_of(&read(&file)), "off");
        let base = NotifyPrefs::default();
        assert_eq!(base.with_choices(10, "used", false), Err("invalid_notify"));
        assert_eq!(
            base.with_choices(5, "sometimes", false),
            Err("invalid_notify")
        );
    }

    #[test]
    fn usage_alerts_are_off_by_default_round_trip_and_read_leniently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        let defaults = read_alerts(&file);
        assert!(!defaults.enabled, "off by default");
        assert_eq!(defaults.thresholds, [75, 90]);
        assert!(defaults.exhaustion && defaults.recovery);
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let chosen = AlertSettings {
            enabled: true,
            ..AlertSettings::default()
        }
        .with_choices(&[95, 10, 95], false, true)
        .unwrap()
        .with_metric("claude:Work", "Week", false);
        assert_eq!(chosen.thresholds, [10, 95]);
        write_alerts(&file, &chosen).unwrap();
        assert_eq!(read_alerts(&file), chosen);
        assert_eq!(motion_of(&read(&file)), "off");
        // Unmuting the last metric of an account drops its key.
        assert!(chosen
            .with_metric("claude:Work", "Week", true)
            .muted
            .is_empty());
        // Back to the defaults removes the key.
        write_alerts(&file, &AlertSettings::default()).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("alerts"));
        // Hand-edited: each bad field falls back alone, out-of-range
        // thresholds are dropped and the rest of the file stays.
        fs::write(
            &file,
            r#"{"motion":"off","alerts":{"enabled":true,"thresholds":[5,50,"x",300,99,50],"exhaustion":"no","muted":{"claude:Work":["5h",1],"claude:Home":[],"x":"5h"}}}"#,
        )
        .unwrap();
        let alerts = read_alerts(&file);
        assert!(alerts.enabled);
        assert_eq!(alerts.thresholds, [50, 99]);
        assert!(alerts.exhaustion && alerts.recovery);
        assert_eq!(
            alerts.muted,
            BTreeMap::from([(
                "claude:Work".to_string(),
                BTreeSet::from(["5h".to_string()])
            )])
        );
        assert_eq!(motion_of(&read(&file)), "off");
        let base = AlertSettings::default();
        for bad in [[9u8].as_slice(), &[100], &[50, 0]] {
            assert_eq!(
                base.with_choices(bad, true, true),
                Err("invalid_threshold"),
                "{bad:?}"
            );
        }
    }
}
