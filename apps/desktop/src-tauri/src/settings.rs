//! Configurações do app desktop (não confundir com o `cuotai.toml` de contas, que é do core).
//! Arquivo `desktop.json` na pasta padrão de configuração: pasta de temas e idiomas e o
//! `cuotai.toml` escolhido pelo usuário (só o caminho; nunca dentro do TOML).
use cuotai_core::{
    alerts::{AlertPrefs, Lead, THRESHOLD_RANGE},
    history::Retention,
    pace::PaceUnit,
    resets::{NotifyWhen, Prefs},
    tray::{TraySelection, TrayStyle},
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
    /// Pace display (PRD 0024 RF1.3): `{"deviation_unit": "percent"}`; absent
    /// = points. Read leniently ([`pace_unit_of`]): anything else is points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pace: Option<serde_json::Value>,
    /// Best-account advice (PRD 0024 RF7.5): `{"enabled": false}`; absent =
    /// on (it only shows where a group has two or more accounts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    advice: Option<serde_json::Value>,
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
    /// Update checks of this device (PRD 0018 RF2.6/RF3.3); absent means
    /// the one-time question has not been answered and checks stay off.
    /// Read leniently like `notify` ([`UpdaterPrefs::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updater: Option<serde_json::Value>,
    /// The last size and position of the Settings window (PRD 0020 RF1.1), in
    /// logical pixels; absent until the window is first closed. Read
    /// leniently ([`WindowGeometry::from_value`]): an invalid value means
    /// "center it again" and never invalidates the other fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    settings_window: Option<serde_json::Value>,
    /// Theme id (PRD 0020 RF7.2); absent = the interface default. Free text on
    /// read: user themes only exist after the interface loads them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    /// Light/dark choice (PRD 0020 RF7.2); absent = `system`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme_mode: Option<String>,
    /// Account display order (PRD 0020 RF7.2); absent = `custom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sort: Option<String>,
    /// The "Recharging" group (PRD 0020 RF7.2); absent = off. Only `true` is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_recharging: Option<bool>,
    /// How the popover's "All" shows aggregators (PRD 0036 RF8.3, a PRD
    /// 0020 RF7.2 preference); absent = `grouped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aggregators_in_all: Option<String>,
    /// One-time migrations already done, e.g. `{"local_storage": 1}` (PRD 0020 RF7.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    migrations: Option<serde_json::Value>,
    /// PRD 0027 RF6.5/RF7.3: presence detection of local AI tools; absent
    /// means the one-time question was not answered and nothing is checked.
    /// Read leniently ([`DetectPrefs::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detect: Option<serde_json::Value>,
    /// PRD 0025 RF2.2/RF3.6/RF4.3: the menu bar (style, colored variant,
    /// selection and the popover view); absent = the defaults. Read
    /// leniently ([`TrayPrefs::from_value`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tray: Option<serde_json::Value>,
    /// PRD 0025 RF5.3: Settings window state (`advanced_open`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    settings: Option<serde_json::Value>,
    /// PRD 0025 RF6.4: the first-run record (`version`); absent = not shown yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    onboarding: Option<serde_json::Value>,
}

/// PRD 0027 RF6.5/RF7.3: `detect.enabled` (`None` until answered) and the
/// dismissed source kinds (`detect.dismissed`, core slugs).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct DetectPrefs {
    pub enabled: Option<bool>,
    pub dismissed: Vec<String>,
}

impl DetectPrefs {
    fn from_value(value: Option<&serde_json::Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        let enabled = value.get("enabled").and_then(serde_json::Value::as_bool);
        let dismissed = value
            .get("dismissed")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|slug| cuotai_core::detect::SourceKind::from_slug(slug).is_some())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Self { enabled, dismissed }
    }

    fn to_value(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": self.enabled, "dismissed": self.dismissed })
    }
}

fn detect_of(stored: &Stored) -> DetectPrefs {
    DetectPrefs::from_value(stored.detect.as_ref())
}

/// PRD 0027 RF6.5: this device's detection preferences.
pub fn detect_prefs() -> DetectPrefs {
    detect_of(&load())
}

/// PRD 0027 RF6.5: answers the check question (or changes it in Settings).
pub fn save_detect_enabled(on: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        let mut prefs = detect_of(s);
        prefs.enabled = Some(on);
        s.detect = Some(prefs.to_value());
    })
}

/// PRD 0027 RF7.3: "Don't suggest this again" for one source kind.
/// Errors: `invalid_source`, `unwritable`.
pub fn dismiss_detected(kind: &str) -> Result<(), &'static str> {
    cuotai_core::detect::SourceKind::from_slug(kind).ok_or("invalid_source")?;
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        let mut prefs = detect_of(s);
        if !prefs.dismissed.iter().any(|d| d == kind) {
            prefs.dismissed.push(kind.to_owned());
        }
        s.detect = Some(prefs.to_value());
    })
}

/// PRD 0025 RF6.4: the version of the first-run step; raising it shows the
/// step again to every profile.
pub const ONBOARDING_VERSION: u64 = 1;

/// PRD 0025 RF4.3: how the tray popover lists the accounts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopoverView {
    #[default]
    Grid,
    List,
}

/// PRD 0025 RF2.2: the menu-bar preferences of this device. `accounts`
/// `None` is `Auto`; otherwise the stable account ids (PRD 0029) in the
/// user's order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TrayPrefs {
    pub style: TrayStyle,
    pub colored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popover_view: Option<PopoverView>,
}

impl TrayPrefs {
    /// Each field on its own: an unknown or mistyped value is its default.
    fn from_value(value: &serde_json::Value) -> Self {
        Self {
            style: value["style"]
                .as_str()
                .and_then(TrayStyle::parse)
                .unwrap_or_default(),
            colored: value["colored"].as_bool().unwrap_or(false),
            accounts: value["accounts"].as_array().map(|ids| clean_ids(ids)),
            popover_view: serde_json::from_value(value["popover_view"].clone()).ok(),
        }
    }

    pub fn selection(&self) -> TraySelection {
        match &self.accounts {
            None => TraySelection::Auto,
            Some(ids) => TraySelection::Custom(ids.clone()),
        }
    }

    /// RF2.2: a custom selection without the ids of removed accounts;
    /// `None` when nothing changes.
    pub fn pruned(&self, existing: &[String]) -> Option<Self> {
        let ids = self.accounts.as_ref()?;
        let TraySelection::Custom(kept) = self.selection().pruned(existing) else {
            return None;
        };
        (kept != *ids).then(|| Self {
            accounts: Some(kept),
            ..self.clone()
        })
    }
}

/// Valid account ids (PRD 0029), without repeats, in order.
pub fn clean_ids(ids: &[serde_json::Value]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for id in ids.iter().filter_map(|v| v.as_str()) {
        if cuotai_core::AccountId::parse(id).is_some() && !kept.iter().any(|k| k == id) {
            kept.push(id.to_owned());
        }
    }
    kept
}

/// PRD 0025 RF2.2: the menu-bar preferences stored in `file`.
pub fn read_tray(file: &Path) -> TrayPrefs {
    read(file)
        .tray
        .as_ref()
        .map(TrayPrefs::from_value)
        .unwrap_or_default()
}

/// Store the menu-bar preferences in `file`; the defaults remove the key.
pub fn write_tray(file: &Path, prefs: &TrayPrefs) -> Result<(), &'static str> {
    update(file, |s| {
        s.tray = (*prefs != TrayPrefs::default())
            .then(|| serde_json::to_value(prefs).ok())
            .flatten()
    })
}

/// The menu-bar preferences of this device.
pub fn tray_prefs() -> TrayPrefs {
    settings_file().map(|f| read_tray(&f)).unwrap_or_default()
}

/// Store the menu-bar preferences of this device. Error: `unwritable`.
pub fn save_tray(prefs: &TrayPrefs) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    write_tray(&file, prefs)
}

/// PRD 0025 RF5.3: whether the Settings window's Advanced group is open.
#[cfg(test)]
pub fn read_advanced_open(file: &Path) -> bool {
    read(file)
        .settings
        .as_ref()
        .and_then(|v| v["advanced_open"].as_bool())
        .unwrap_or(false)
}

/// Store whether the Advanced group is open, keeping other `settings` keys.
pub fn write_advanced_open(file: &Path, open: bool) -> Result<(), &'static str> {
    update(file, |s| {
        let mut value = s
            .settings
            .take()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        value["advanced_open"] = serde_json::Value::Bool(open);
        s.settings = Some(value);
    })
}

/// PRD 0025 RF6.4: the first-run version this profile has seen (0 = never).
#[cfg(test)]
pub fn read_onboarding(file: &Path) -> u64 {
    read(file)
        .onboarding
        .as_ref()
        .and_then(|v| v["version"].as_u64())
        .unwrap_or(0)
}

/// Record the first run as seen (`Some(version)`) or reset it (`None`,
/// "Show the recommended setup again").
pub fn write_onboarding(file: &Path, version: Option<u64>) -> Result<(), &'static str> {
    update(file, |s| {
        s.onboarding = version.map(|v| serde_json::json!({ "version": v }))
    })
}

/// The `desktop.json` of this device, or `unwritable`.
pub fn file() -> Result<PathBuf, &'static str> {
    settings_file().ok_or("unwritable")
}

/// PRD 0018 RF2.6/RF3.3: the update-check preferences of this device.
/// `consent_answered` is the hard privacy gate: automatic and manual checks
/// stay off until the one-time question is answered. `last_check` is the
/// RFC 3339 instant the last check finished.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct UpdaterPrefs {
    pub automatic: bool,
    pub last_check: Option<String>,
    pub consent_answered: bool,
}

impl UpdaterPrefs {
    /// Each field on its own: an unknown or mistyped value is its safe
    /// default. Profiles written before RF2.6 with an explicit
    /// `automatic: false` already opted out, so they count as answered and
    /// remain off without seeing the question.
    fn from_value(value: &serde_json::Value) -> Self {
        let automatic = value["automatic"].as_bool();
        let consent_answered = value["consent_answered"]
            .as_bool()
            .unwrap_or(automatic == Some(false));
        let last_check = value["last_check"].as_str().and_then(|text| {
            let text = text.trim();
            chrono::DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|_| text.to_owned())
        });
        Self {
            automatic: consent_answered && automatic.unwrap_or(false),
            last_check,
            consent_answered,
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

/// Store the update-check preferences in `file`; the unanswered defaults remove the key.
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
/// is off by default; `off_accounts` holds the `Account::state_key`s (ids, PRD 0029) whose bell is
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
    /// PRD 0023 RF4.6: notify when paid extra usage starts (`alerts.overage`
    /// in `desktop.json`, on by default whenever usage alerts are on).
    pub overage: bool,
    /// Account key → muted metric labels.
    pub muted: BTreeMap<String, BTreeSet<String>>,
    /// PRD 0036 RF6.4: aggregator accounts whose connections alert one by
    /// one; every other aggregator alerts only when a whole pool runs out.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub pool_each: BTreeSet<String>,
    /// PRD 0024 RF4.1: minutes before a projected exhaustion to warn (15,
    /// 30 or 60); absent = off, as the core requires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projected_lead_minutes: Option<u16>,
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
            overage: core.overage,
            muted: BTreeMap::new(),
            pool_each: BTreeSet::new(),
            projected_lead_minutes: None,
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
            overage: flag("overage", default.overage),
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
            pool_each: value["pool_each"]
                .as_array()
                .map(|keys| {
                    keys.iter()
                        .filter_map(|key| key.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            projected_lead_minutes: value["projected_lead_minutes"]
                .as_u64()
                .and_then(Lead::from_minutes)
                .map(Lead::minutes),
        }
    }

    /// PRD 0036 RF6.4: how the aggregator `account_key` alerts.
    pub fn pool_mode(&self, account_key: &str) -> cuotai_core::alerts::PoolAlerts {
        if self.pool_each.contains(account_key) {
            cuotai_core::alerts::PoolAlerts::EachConnection
        } else {
            cuotai_core::alerts::PoolAlerts::WholePool
        }
    }

    /// The same settings with `account_key` alerting by `mode`.
    pub fn with_pool_mode(&self, account_key: &str, mode: cuotai_core::alerts::PoolAlerts) -> Self {
        let mut next = self.clone();
        match mode {
            cuotai_core::alerts::PoolAlerts::EachConnection => {
                next.pool_each.insert(account_key.to_owned());
            }
            cuotai_core::alerts::PoolAlerts::WholePool => {
                next.pool_each.remove(account_key);
            }
        }
        next
    }

    /// The projected-exhaustion lead (PRD 0024 RF2.7), if turned on.
    pub fn projected(&self) -> Option<Lead> {
        self.projected_lead_minutes
            .and_then(|minutes| Lead::from_minutes(u64::from(minutes)))
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
        prefs.overage = self.overage;
        prefs.muted_accounts = off_accounts.clone();
        prefs.muted_metrics = self.muted.clone();
        prefs.projected = self.projected();
        // The projected warning has its own switch: the evaluator runs for
        // it too, and `Notices` keeps the other kinds to `enabled`.
        prefs.enabled |= prefs.projected.is_some();
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

/// PRD 0020 RF1.1: the Settings window's outer position and inner size in
/// logical pixels. Window state, not a preference: nothing in the interface
/// reads or edits it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WindowGeometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The largest side kept, so a hand-edited value cannot ask for a window
/// larger than any screen or placed absurdly far away.
const MAX_WINDOW_SIDE: f64 = 16_384.0;

impl WindowGeometry {
    /// `None` unless every field is a finite number, the size is at least
    /// `min` and at most [`MAX_WINDOW_SIDE`], and the position is within
    /// four times that bound.
    pub fn checked(self, min: (f64, f64)) -> Option<Self> {
        let finite = [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite());
        let sized = (min.0..=MAX_WINDOW_SIDE).contains(&self.width)
            && (min.1..=MAX_WINDOW_SIDE).contains(&self.height);
        let far = MAX_WINDOW_SIDE * 4.0;
        let placed = self.x.abs() <= far && self.y.abs() <= far;
        (finite && sized && placed).then_some(self)
    }

    fn from_value(value: &serde_json::Value) -> Option<Self> {
        let field = |key: &str| value[key].as_f64();
        Some(Self {
            x: field("x")?,
            y: field("y")?,
            width: field("width")?,
            height: field("height")?,
        })
    }
}

/// PRD 0020 RF1.1: the Settings window geometry stored in `file`, if valid
/// for a window of at least `min`.
pub fn read_settings_window(file: &Path, min: (f64, f64)) -> Option<WindowGeometry> {
    read(file)
        .settings_window
        .as_ref()
        .and_then(WindowGeometry::from_value)
        .and_then(|g| g.checked(min))
}

/// Store the Settings window geometry in `file`, keeping the other fields.
pub fn write_settings_window(file: &Path, geometry: &WindowGeometry) -> Result<(), &'static str> {
    update(file, |s| {
        s.settings_window = serde_json::to_value(geometry).ok()
    })
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

/// Account orders the dashboard offers, in the order the interface lists them.
pub const SORTS: [&str; 6] = [
    "custom",
    "binding",
    "reset-asc",
    "reset-desc",
    "pct-asc",
    "pct-desc",
];
const THEME_MODES: [&str; 3] = ["system", "light", "dark"];

fn valid_theme_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// BCP 47-ish tag, the same shape the pages accept before first paint.
fn valid_locale(code: &str) -> bool {
    let mut parts = code.split('-');
    let ok_first = parts
        .next()
        .is_some_and(|p| (2..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic()));
    code.len() <= 35
        && ok_first
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Stores one of the interface preferences that live here since PRD 0020 RF7.2;
/// the defaults remove the key. Error: `invalid_preference`.
/// PRD 0036 RF8.3: how the popover's "All" shows aggregators.
const AGGREGATOR_MODES: [&str; 3] = ["grouped", "listed", "hidden"];

fn apply_preference(
    s: &mut Stored,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), &'static str> {
    let text = value.as_str();
    match key {
        "theme" => match text {
            Some(v) if valid_theme_id(v) => s.theme = Some(v.to_owned()),
            _ => return Err("invalid_preference"),
        },
        "theme_mode" => match text {
            Some(v) if THEME_MODES.contains(&v) => {
                s.theme_mode = (v != "system").then(|| v.to_owned())
            }
            _ => return Err("invalid_preference"),
        },
        "sort" => match text {
            Some(v) if SORTS.contains(&v) => s.sort = (v != "custom").then(|| v.to_owned()),
            _ => return Err("invalid_preference"),
        },
        "group_recharging" => match value.as_bool() {
            Some(on) => s.group_recharging = on.then_some(true),
            None => return Err("invalid_preference"),
        },
        "aggregators_in_all" => match text {
            Some(v) if AGGREGATOR_MODES.contains(&v) => {
                s.aggregators_in_all = (v != "grouped").then(|| v.to_owned())
            }
            _ => return Err("invalid_preference"),
        },
        _ => return Err("invalid_preference"),
    }
    Ok(())
}

/// Saves one interface preference (`theme`, `theme_mode`, `sort`,
/// `group_recharging`, `aggregators_in_all`).
pub fn save_preference(key: &str, value: &serde_json::Value) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    apply_preference(&mut Stored::default(), key, value)?;
    update(&file, |s| {
        let _ = apply_preference(s, key, value);
    })
}

fn migrated_of(stored: &Stored) -> bool {
    stored
        .migrations
        .as_ref()
        .and_then(|m| m.get("local_storage"))
        .and_then(serde_json::Value::as_u64)
        == Some(1)
}

/// One-time copy of the old per-window `localStorage` preferences (PRD 0020
/// RF7.2): a key is taken only when this file has no value for it, an invalid
/// value is dropped, and the migration is recorded either way.
fn migrate_local(s: &mut Stored, old: &BTreeMap<String, String>) {
    if migrated_of(s) {
        return;
    }
    let text = |k: &str| serde_json::Value::String(old.get(k).cloned().unwrap_or_default());
    if s.theme.is_none() {
        let _ = apply_preference(s, "theme", &text("theme"));
    }
    if s.theme_mode.is_none() {
        let _ = apply_preference(s, "theme_mode", &text("theme_mode"));
    }
    if s.sort.is_none() {
        let _ = apply_preference(s, "sort", &text("sort"));
    }
    if s.group_recharging.is_none() && old.get("group_recharging").map(String::as_str) == Some("on")
    {
        s.group_recharging = Some(true);
    }
    if s.aggregators_in_all.is_none() {
        let _ = apply_preference(s, "aggregators_in_all", &text("aggregators_in_all"));
    }
    if s.motion.is_none() {
        if let Some(m) = old.get("motion").filter(|m| MOTIONS.contains(&m.as_str())) {
            s.motion = (m != "system").then(|| m.clone());
        }
    }
    if s.locale.is_none() {
        if let Some(l) = old.get("locale").filter(|l| valid_locale(l)) {
            s.locale = Some(l.clone());
        }
    }
    let mut m = match s.migrations.take() {
        Some(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    m.insert("local_storage".into(), 1.into());
    s.migrations = Some(serde_json::Value::Object(m));
}

/// Runs the migration of [`migrate_local`] once; `true` when it ran now.
pub fn migrate_local_storage(old: &BTreeMap<String, String>) -> Result<bool, &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    if migrated_of(&read(&file)) {
        return Ok(false);
    }
    update(&file, |s| migrate_local(s, old))?;
    Ok(true)
}

/// Whether `locale` differs from the language stored in `file`.
pub fn locale_differs(file: &Path, locale: &str) -> bool {
    read(file).locale.as_deref() != Some(locale)
}

/// Shows or hides the consumption pace; `true` (the default) removes the key.
pub fn save_show_pace(show: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.show_pace = (!show).then_some(false))
}

/// PRD 0024 RF7.5: whether the best account is shown (on unless the file
/// says `advice.enabled = false`).
fn advice_of(stored: &Stored) -> bool {
    stored
        .advice
        .as_ref()
        .and_then(|advice| advice["enabled"].as_bool())
        != Some(false)
}

/// PRD 0024 RF7.5: show (the default, removes the key) or hide the best
/// account. Error: `unwritable`.
pub fn save_advice_enabled(enabled: bool) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.advice = (!enabled).then(|| serde_json::json!({ "enabled": false }))
    })
}

/// PRD 0024 RF1.3: how pace deviations read, "points" (the default) or
/// "percent".
fn pace_unit_of(stored: &Stored) -> &'static str {
    match stored
        .pace
        .as_ref()
        .and_then(|pace| pace["deviation_unit"].as_str())
        .and_then(PaceUnit::parse)
    {
        Some(PaceUnit::Percent) => "percent",
        _ => "points",
    }
}

/// PRD 0024 RF1.3: choose `points` (removes the key) or `percent`.
/// Errors: `invalid_pace_unit`, `unwritable`.
pub fn save_pace_unit(unit: &str) -> Result<(), &'static str> {
    let unit = PaceUnit::parse(unit).ok_or("invalid_pace_unit")?;
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.pace =
            (unit == PaceUnit::Percent).then(|| serde_json::json!({ "deviation_unit": "percent" }))
    })
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
    /// PRD 0020 RF7.2: theme id, light/dark mode, account order, the "Recharging" group, the
    /// interface language and whether the `localStorage` migration already ran.
    theme: Option<String>,
    theme_mode: &'static str,
    sort: &'static str,
    group_recharging: bool,
    /// PRD 0036 RF8.3: `grouped`, `listed` or `hidden`.
    aggregators_in_all: &'static str,
    locale: Option<String>,
    local_storage_migrated: bool,
    /// PRD 0024 RF1.3: `points` or `percent`.
    pace_unit: &'static str,
    /// PRD 0024 RF7.5: show the best account of each group (on by default).
    advice_enabled: bool,
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
    /// PRD 0027 RF6.5: local-tool detection (`enabled` is `None` until asked).
    detect: DetectPrefs,
    /// PRD 0025 RF2.2/RF3.6/RF4.3: the menu bar.
    tray: TrayPrefs,
    /// PRD 0025 RF5.3: the Advanced group is open.
    advanced_open: bool,
    /// PRD 0025 RF6.4: the first-run version seen (0 = never) and the current one.
    onboarding: u64,
    onboarding_version: u64,
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
        theme: stored.theme.clone().filter(|t| valid_theme_id(t)),
        theme_mode: THEME_MODES
            .into_iter()
            .find(|m| stored.theme_mode.as_deref() == Some(m))
            .unwrap_or("system"),
        sort: SORTS
            .into_iter()
            .find(|m| stored.sort.as_deref() == Some(m))
            .unwrap_or("custom"),
        group_recharging: stored.group_recharging == Some(true),
        aggregators_in_all: AGGREGATOR_MODES
            .into_iter()
            .find(|m| stored.aggregators_in_all.as_deref() == Some(m))
            .unwrap_or("grouped"),
        locale: stored.locale.clone().filter(|l| valid_locale(l)),
        local_storage_migrated: migrated_of(&stored),
        pace_unit: pace_unit_of(&stored),
        advice_enabled: advice_of(&stored),
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
        detect: detect_of(&stored),
        tray: stored
            .tray
            .as_ref()
            .map(TrayPrefs::from_value)
            .unwrap_or_default(),
        advanced_open: stored
            .settings
            .as_ref()
            .and_then(|v| v["advanced_open"].as_bool())
            .unwrap_or(false),
        onboarding: stored
            .onboarding
            .as_ref()
            .and_then(|v| v["version"].as_u64())
            .unwrap_or(0),
        onboarding_version: ONBOARDING_VERSION,
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
    fn detect_prefs_read_leniently_and_keep_known_kinds() {
        assert_eq!(DetectPrefs::from_value(None), DetectPrefs::default());
        let value = serde_json::json!({ "enabled": "yes", "dismissed": ["cursor_app", "nope", 3] });
        let prefs = DetectPrefs::from_value(Some(&value));
        assert_eq!(prefs.enabled, None);
        assert_eq!(prefs.dismissed, vec!["cursor_app".to_string()]);
        let saved = DetectPrefs {
            enabled: Some(true),
            dismissed: vec!["aws_cli".into()],
        };
        let round = DetectPrefs::from_value(Some(&saved.to_value()));
        assert_eq!(round, saved);
    }

    #[test]
    fn tray_prefs_round_trip_and_read_leniently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(read_tray(&file), TrayPrefs::default());
        assert_eq!(read_tray(&file).style, TrayStyle::TightestPercent);
        let prefs = TrayPrefs {
            style: TrayStyle::Meters,
            colored: true,
            accounts: Some(vec!["acc_b".into(), "acc_a".into()]),
            popover_view: Some(PopoverView::List),
        };
        write_tray(&file, &prefs).unwrap();
        assert_eq!(read_tray(&file), prefs);
        // The defaults remove the key.
        write_tray(&file, &TrayPrefs::default()).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("tray"));
        // Mistyped values fall back one by one; invalid and repeated ids drop.
        fs::write(
            &file,
            r#"{"tray":{"style":"nope","colored":"yes","accounts":["acc_a","BAD ID","acc_a",3],"popover_view":"list"}}"#,
        )
        .unwrap();
        let read_back = read_tray(&file);
        assert_eq!(read_back.style, TrayStyle::TightestPercent);
        assert!(!read_back.colored);
        assert_eq!(read_back.accounts, Some(vec!["acc_a".to_owned()]));
        assert_eq!(read_back.popover_view, Some(PopoverView::List));
    }

    #[test]
    fn tray_selection_prunes_removed_accounts_and_keeps_renamed_ones() {
        // A rename keeps the stable id, so the account stays selected.
        let prefs = TrayPrefs {
            accounts: Some(vec!["acc_keep".into(), "acc_gone".into()]),
            ..TrayPrefs::default()
        };
        let existing = vec!["acc_new".to_owned(), "acc_keep".to_owned()];
        let pruned = prefs.pruned(&existing).unwrap();
        assert_eq!(pruned.accounts, Some(vec!["acc_keep".to_owned()]));
        assert_eq!(pruned.pruned(&existing), None);
        // Auto never changes (new accounts appear only there).
        assert_eq!(TrayPrefs::default().pruned(&existing), None);
    }

    #[test]
    fn advanced_group_and_onboarding_records() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert!(!read_advanced_open(&file));
        write_advanced_open(&file, true).unwrap();
        assert!(read_advanced_open(&file));
        assert_eq!(read_onboarding(&file), 0);
        write_onboarding(&file, Some(ONBOARDING_VERSION)).unwrap();
        assert_eq!(read_onboarding(&file), ONBOARDING_VERSION);
        assert!(read_advanced_open(&file));
        write_onboarding(&file, None).unwrap();
        assert_eq!(read_onboarding(&file), 0);
    }

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
    fn preferences_validate_store_and_drop_defaults() {
        let mut s = Stored::default();
        assert_eq!(apply_preference(&mut s, "theme", &"win98".into()), Ok(()));
        assert_eq!(
            apply_preference(&mut s, "theme", &"a b".into()),
            Err("invalid_preference")
        );
        assert_eq!(
            apply_preference(&mut s, "theme_mode", &"dark".into()),
            Ok(())
        );
        assert_eq!(
            apply_preference(&mut s, "theme_mode", &"neon".into()),
            Err("invalid_preference")
        );
        assert_eq!(apply_preference(&mut s, "sort", &"pct-asc".into()), Ok(()));
        assert_eq!(
            apply_preference(&mut s, "sort", &"x".into()),
            Err("invalid_preference")
        );
        assert_eq!(
            apply_preference(&mut s, "group_recharging", &true.into()),
            Ok(())
        );
        assert_eq!(
            apply_preference(&mut s, "group_recharging", &"on".into()),
            Err("invalid_preference")
        );
        assert_eq!(
            apply_preference(&mut s, "nope", &true.into()),
            Err("invalid_preference")
        );
        assert_eq!(
            (
                s.theme.as_deref(),
                s.theme_mode.as_deref(),
                s.sort.as_deref()
            ),
            (Some("win98"), Some("dark"), Some("pct-asc"))
        );
        assert_eq!(s.group_recharging, Some(true));
        apply_preference(&mut s, "theme_mode", &"system".into()).unwrap();
        apply_preference(&mut s, "sort", &"custom".into()).unwrap();
        apply_preference(&mut s, "group_recharging", &false.into()).unwrap();
        assert_eq!(
            (s.theme_mode, s.sort, s.group_recharging),
            (None, None, None)
        );
    }

    #[test]
    fn local_storage_migration_fills_gaps_once_and_keeps_existing_values() {
        let old: BTreeMap<String, String> = [
            ("theme", "nord"),
            ("theme_mode", "light"),
            ("sort", "bogus"),
            ("group_recharging", "on"),
            ("motion", "off"),
            ("locale", "es"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let mut s = Stored {
            theme: Some("win98".into()),
            migrations: Some(serde_json::json!({"other": 2})),
            ..Stored::default()
        };
        migrate_local(&mut s, &old);
        assert_eq!(
            s.theme.as_deref(),
            Some("win98"),
            "a value already in desktop.json wins"
        );
        assert_eq!(s.theme_mode.as_deref(), Some("light"));
        assert_eq!(s.sort, None, "invalid values are dropped");
        assert_eq!(s.group_recharging, Some(true));
        assert_eq!(
            (s.motion.as_deref(), s.locale.as_deref()),
            (Some("off"), Some("es"))
        );
        assert!(migrated_of(&s));
        assert_eq!(s.migrations.as_ref().unwrap()["other"], 2);
        let again: BTreeMap<String, String> = [("theme_mode".to_owned(), "dark".to_owned())].into();
        migrate_local(&mut s, &again);
        assert_eq!(s.theme_mode.as_deref(), Some("light"), "runs once");
    }

    #[test]
    fn locale_tags_are_checked() {
        assert!(valid_locale("pt-BR") && valid_locale("es") && valid_locale("zh-Hant-TW"));
        assert!(
            !valid_locale("")
                && !valid_locale("x")
                && !valid_locale("en_US")
                && !valid_locale("en-")
        );
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
    fn settings_window_geometry_round_trips_and_ignores_invalid_values() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        let min = (640.0, 460.0);
        assert_eq!(read_settings_window(&file, min), None);
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let g = WindowGeometry {
            x: 120.0,
            y: -40.5,
            width: 900.0,
            height: 700.0,
        };
        write_settings_window(&file, &g).unwrap();
        assert_eq!(read_settings_window(&file, min), Some(g));
        assert_eq!(motion_of(&read(&file)), "off");
        // Below the minimum size, missing, mistyped or absurd values: centered again.
        for text in [
            r#"{"settings_window":{"x":0,"y":0,"width":200,"height":700}}"#,
            r#"{"settings_window":{"x":0,"y":0,"width":900}}"#,
            r#"{"settings_window":{"x":"0","y":0,"width":900,"height":700}}"#,
            r#"{"settings_window":{"x":1e12,"y":0,"width":900,"height":700}}"#,
            r#"{"settings_window":{"x":0,"y":0,"width":1e9,"height":700}}"#,
            r#"{"settings_window":7}"#,
        ] {
            fs::write(&file, text).unwrap();
            assert_eq!(read_settings_window(&file, min), None, "{text}");
        }
        // A bad value never invalidates the other fields.
        fs::write(&file, r#"{"motion":"off","settings_window":"x"}"#).unwrap();
        assert_eq!(motion_of(&read(&file)), "off");
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

    /// PRD 0018 RF2.6/RF3.3: absent and malformed preferences are safely
    /// unanswered and off; an explicit legacy opt-out stays answered and off.
    #[test]
    fn updater_consent_round_trips_and_migrates_an_existing_opt_out() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        // Absent = no answer, no automatic or manual request.
        assert_eq!(read_updater(&file), UpdaterPrefs::default());
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let prefs = UpdaterPrefs {
            automatic: true,
            last_check: Some("2026-10-22T12:00:00Z".into()),
            consent_answered: true,
        };
        write_updater(&file, &prefs).unwrap();
        assert_eq!(read_updater(&file), prefs);
        assert_eq!(motion_of(&read(&file)), "off");

        // Returning to the unanswered safe default removes the key without
        // touching unrelated settings.
        write_updater(&file, &UpdaterPrefs::default()).unwrap();
        assert_eq!(read(&file).updater, None);
        assert_eq!(motion_of(&read(&file)), "off");

        // A wrong type falls back to the safe field default and never opens
        // the network gate.
        for text in [
            r#"{"motion":"off","updater":{"automatic":"yes","last_check":7,"consent_answered":"yes"}}"#,
            r#"{"motion":"off","updater":[]}"#,
            r#"{"motion":"off","updater":null}"#,
        ] {
            fs::write(&file, text).unwrap();
            assert_eq!(read_updater(&file), UpdaterPrefs::default(), "{text}");
            assert_eq!(motion_of(&read(&file)), "off", "{text}");
        }

        // Profiles that explicitly turned the old preference off keep it off
        // and do not get asked again, regardless of the old explanation flag.
        for explanation_seen in [false, true] {
            fs::write(
                &file,
                format!(
                    r#"{{"updater":{{"automatic":false,"explanation_seen":{explanation_seen}}}}}"#
                ),
            )
            .unwrap();
            assert_eq!(
                read_updater(&file),
                UpdaterPrefs {
                    automatic: false,
                    last_check: None,
                    consent_answered: true,
                }
            );
        }

        // A malformed date is discarded without changing an answered choice.
        fs::write(
            &file,
            r#"{"updater":{"automatic":true,"last_check":"when?","consent_answered":true}}"#,
        )
        .unwrap();
        assert_eq!(
            read_updater(&file),
            UpdaterPrefs {
                automatic: true,
                last_check: None,
                consent_answered: true,
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
        assert!(defaults.exhaustion && defaults.recovery && defaults.overage);
        let off = AlertSettings {
            overage: false,
            ..AlertSettings::default()
        };
        assert!(!off.evaluator(&BTreeSet::new()).overage);
        write_alerts(&file, &off).unwrap();
        assert!(!read_alerts(&file).overage);
        write_alerts(&file, &AlertSettings::default()).unwrap();
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
        assert!(alerts.overage, "extra usage alerts default on");
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

    /// PRD 0036 RF8.3 under PRD 0020 RF7.2: the aggregator display choice
    /// is a desktop.json preference; the default is not stored and an old
    /// `localStorage` value migrates once.
    #[test]
    fn aggregators_in_all_is_a_stored_preference() {
        let mut s = Stored::default();
        apply_preference(&mut s, "aggregators_in_all", &"listed".into()).unwrap();
        assert_eq!(s.aggregators_in_all.as_deref(), Some("listed"));
        apply_preference(&mut s, "aggregators_in_all", &"grouped".into()).unwrap();
        assert_eq!(s.aggregators_in_all, None);
        assert!(apply_preference(&mut s, "aggregators_in_all", &"nope".into()).is_err());
        let mut fresh = Stored::default();
        let old = BTreeMap::from([("aggregators_in_all".to_string(), "hidden".to_string())]);
        migrate_local(&mut fresh, &old);
        assert_eq!(fresh.aggregators_in_all.as_deref(), Some("hidden"));
    }

    /// PRD 0024 RF4.1: the projected-exhaustion lead is absent (off) by
    /// default, stored only as 15, 30 or 60 and read leniently.
    #[test]
    fn the_projected_lead_is_off_by_default_and_only_15_30_or_60() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(read_alerts(&file).projected(), None);
        assert!(!AlertSettings::default().evaluator(&BTreeSet::new()).enabled);
        let on = AlertSettings {
            projected_lead_minutes: Some(30),
            ..AlertSettings::default()
        };
        write_alerts(&file, &on).unwrap();
        assert_eq!(read_alerts(&file).projected(), Some(Lead::Min30));
        let prefs = on.evaluator(&BTreeSet::new());
        assert!(prefs.enabled, "the projected warning runs the evaluator");
        assert_eq!(prefs.projected, Some(Lead::Min30));
        for bad in [r#"20"#, r#""30""#, r#"0"#, r#"null"#] {
            fs::write(
                &file,
                format!(r#"{{"alerts":{{"enabled":true,"projected_lead_minutes":{bad}}}}}"#),
            )
            .unwrap();
            let read = read_alerts(&file);
            assert!(read.enabled && read.projected().is_none(), "{bad}");
        }
    }

    /// PRD 0024 RF1.3: points unless the file says percent; anything else
    /// reads as points and the rest of the file stays.
    #[test]
    fn the_pace_unit_defaults_to_points_and_reads_leniently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(pace_unit_of(&read(&file)), "points");
        fs::write(
            &file,
            r#"{"motion":"off","pace":{"deviation_unit":"percent"}}"#,
        )
        .unwrap();
        assert_eq!(pace_unit_of(&read(&file)), "percent");
        for bad in [
            r#"{"deviation_unit":"loud"}"#,
            r#"7"#,
            r#"{"deviation_unit":3}"#,
        ] {
            fs::write(&file, format!(r#"{{"motion":"off","pace":{bad}}}"#)).unwrap();
            assert_eq!(pace_unit_of(&read(&file)), "points", "{bad}");
            assert_eq!(motion_of(&read(&file)), "off");
        }
        assert_eq!(PaceUnit::parse("percent"), Some(PaceUnit::Percent));
        assert_eq!(PaceUnit::parse("%"), None);
    }

    /// PRD 0024 RF7.5: on unless the file says otherwise.
    #[test]
    fn the_best_account_is_shown_unless_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert!(advice_of(&read(&file)));
        fs::write(&file, r#"{"advice":{"enabled":false}}"#).unwrap();
        assert!(!advice_of(&read(&file)));
        for odd in [
            r#"{"advice":{"enabled":"no"}}"#,
            r#"{"advice":7}"#,
            r#"{"advice":{}}"#,
        ] {
            fs::write(&file, odd).unwrap();
            assert!(advice_of(&read(&file)), "{odd}");
        }
    }
}
