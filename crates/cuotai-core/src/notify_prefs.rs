//! The single source of notification preferences (PRD 0032 RF1): the
//! `notify`, `alerts` and `quiet` sections of `desktop.json`
//! (`<OS config dir>/cuotai/desktop.json`), their lenient codec and the
//! types every notifying process reads: the desktop (read and write),
//! `cuotai --notify` and `--watch` (read only).
//!
//! Bells and muted metrics are keyed by [`crate::Account::state_key`] (the
//! account `id`, PRD 0029), so every reader agrees on them. When the file
//! does not exist, [`Loaded::absent`] applies the recommended defaults so a
//! TUI-only person keeps the behaviour `--notify` always had.

use crate::alerts::{AlertPrefs, THRESHOLD_RANGE};
use crate::quiet::QuietSchedule;
use crate::resets::{NotifyWhen, Prefs};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Lead times offered, in minutes ("at the time" is 0).
pub const NOTIFY_LEADS: [u32; 4] = [0, 5, 15, 60];
/// "When to notify" filters, in the order the interface shows them.
pub const NOTIFY_WHENS: [&str; 3] = ["used", "exhausted", "always"];

/// PRD 0003 RF3: reset notice preferences of this device. The global switch
/// is off by default; `off_accounts` holds the `Account::state_key`s (ids,
/// PRD 0029) whose bell is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotifyPrefs {
    pub enabled: bool,
    /// One of [`NOTIFY_LEADS`].
    pub lead_minutes: u32,
    /// One of [`NOTIFY_WHENS`].
    pub when: &'static str,
    pub include_other: bool,
    pub off_accounts: BTreeSet<String>,
    /// PRD 0032 RF7.1: lock-screen-safe texts without account names. On by
    /// default (owner decision 2026-10-23): an absent key reads as on.
    pub private: bool,
}

impl Default for NotifyPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            lead_minutes: 0,
            when: NOTIFY_WHENS[0],
            include_other: false,
            off_accounts: BTreeSet::new(),
            private: true,
        }
    }
}

impl NotifyPrefs {
    /// Each field on its own: an unknown or mistyped value is its default.
    pub fn from_value(value: &serde_json::Value) -> Self {
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
            private: value["private"].as_bool().unwrap_or(default.private),
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

/// PRD 0008 RF3: usage alert preferences of this device. Off by default;
/// the account's bell ([`NotifyPrefs::off_accounts`]) mutes its alerts
/// too, and `muted` holds the metrics muted in the details per
/// [`crate::Account::state_key`].
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
    pub fn from_value(value: &serde_json::Value) -> Self {
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

/// Everything a notifying process reads from `desktop.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded {
    pub notify: NotifyPrefs,
    pub alerts: AlertSettings,
    pub quiet: QuietSchedule,
}

impl Loaded {
    /// RF1.2: with no `desktop.json`, the recommended defaults: reset
    /// notices and usage alerts on with [`AlertPrefs::recommended`]'s
    /// thresholds, exhaustion and recovery, nothing muted, private texts
    /// and no quiet hours.
    pub fn absent() -> Self {
        let recommended = AlertPrefs::recommended();
        Self {
            notify: NotifyPrefs {
                enabled: true,
                ..NotifyPrefs::default()
            },
            alerts: AlertSettings {
                enabled: true,
                thresholds: recommended.thresholds().collect(),
                exhaustion: recommended.exhaustion,
                recovery: recommended.recovery,
                muted: BTreeMap::new(),
            },
            quiet: QuietSchedule::default(),
        }
    }

    /// Lenient read of the three sections of an already parsed document.
    pub fn from_value(document: &serde_json::Value) -> Self {
        let section = |key: &str| document.get(key).filter(|value| value.is_object());
        Self {
            notify: section("notify")
                .map(NotifyPrefs::from_value)
                .unwrap_or_default(),
            alerts: section("alerts")
                .map(AlertSettings::from_value)
                .unwrap_or_default(),
            quiet: section("quiet")
                .map(QuietSchedule::from_value)
                .unwrap_or_default(),
        }
    }

    /// The evaluator's preferences (bells muting the accounts).
    pub fn evaluator(&self) -> AlertPrefs {
        self.alerts.evaluator(&self.notify.off_accounts)
    }

    /// The reset planner's preferences.
    pub fn planner(&self) -> Prefs {
        self.notify.planner()
    }
}

/// `<OS config dir>/cuotai/desktop.json`, the file the desktop writes.
pub fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("cuotai").join("desktop.json"))
}

/// Read the sections in `path`. A missing file is [`Loaded::absent`]; an
/// unreadable or invalid one reads as the plain defaults (off), like the
/// desktop does.
pub fn read(path: &Path) -> Loaded {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text)
            .map(|document| Loaded::from_value(&document))
            .unwrap_or_default(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Loaded::absent(),
        Err(_) => Loaded::default(),
    }
}

/// A read-only reader that follows the file: [`Self::refresh`] re-reads it
/// when its modification time changed (RF1.2), so `--notify` and `--watch`
/// pick up the desktop's edits without a restart.
#[derive(Debug)]
pub struct Source {
    path: Option<PathBuf>,
    stamp: Option<SystemTime>,
    loaded: Loaded,
}

impl Source {
    /// Follow `path` (`None`: no file, the recommended defaults).
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut source = Self {
            path,
            stamp: None,
            loaded: Loaded::absent(),
        };
        source.refresh();
        source
    }

    /// Follow the default `desktop.json`.
    pub fn open() -> Self {
        Self::new(settings_path())
    }

    pub fn loaded(&self) -> &Loaded {
        &self.loaded
    }

    /// Re-read when the file changed (or appeared or vanished); `true` when
    /// the preferences differ now.
    pub fn refresh(&mut self) -> bool {
        let Some(path) = &self.path else {
            return false;
        };
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if stamp == self.stamp && self.stamp.is_some() {
            return false;
        }
        self.stamp = stamp;
        let loaded = read(path);
        let changed = loaded != self.loaded;
        self.loaded = loaded;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_file_is_the_recommended_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = read(&dir.path().join("desktop.json"));
        assert!(loaded.notify.enabled && loaded.notify.private);
        assert!(loaded.alerts.enabled);
        assert_eq!(loaded.alerts.thresholds, [75, 90]);
        assert!(!loaded.quiet.enabled);
        assert!(loaded.evaluator().enabled);
    }

    #[test]
    fn an_empty_document_is_the_plain_defaults_with_private_texts_on() {
        let loaded = Loaded::from_value(&json!({"motion": "off"}));
        assert_eq!(loaded, Loaded::default());
        assert!(!loaded.notify.enabled && !loaded.alerts.enabled);
        assert!(loaded.notify.private, "an absent key reads as on");
    }

    #[test]
    fn unknown_and_invalid_fields_are_read_leniently() {
        let loaded = Loaded::from_value(&json!({
            "notify": {"enabled": true, "lead_minutes": 7, "when": "never",
                       "include_other": "yes", "off_accounts": ["a", 3], "private": false,
                       "extra": 1},
            "alerts": {"enabled": true, "thresholds": [5, 50, "x", 300, 99, 50],
                       "exhaustion": "no", "muted": {"a": ["5h", 1], "b": [], "c": "5h"}},
            "quiet": {"enabled": true, "days": {"mon": [{"from": "22:00", "to": "08:00"}]}}
        }));
        assert!(loaded.notify.enabled && !loaded.notify.private);
        assert_eq!(
            (loaded.notify.lead_minutes, loaded.notify.when),
            (0, "used")
        );
        assert_eq!(loaded.notify.off_accounts, BTreeSet::from(["a".to_owned()]));
        assert_eq!(loaded.alerts.thresholds, [50, 99]);
        assert!(loaded.alerts.exhaustion && loaded.alerts.recovery);
        assert_eq!(loaded.alerts.muted.len(), 1);
        assert!(loaded.quiet.enabled && loaded.quiet.days[0].len() == 1);
        // A section of the wrong type is its default.
        let wrong = Loaded::from_value(&json!({"notify": 3, "alerts": "x", "quiet": []}));
        assert_eq!(wrong, Loaded::default());
    }

    #[test]
    fn evaluator_and_planner_follow_the_file_for_every_reader() {
        // The desktop, `--notify` and `EventState` build their preferences
        // from the same document, so they cannot disagree.
        let document = json!({
            "notify": {"enabled": true, "lead_minutes": 15, "when": "always",
                       "off_accounts": ["muted-bell"]},
            "alerts": {"enabled": true, "thresholds": [60, 85],
                       "recovery": false, "muted": {"acct": ["5h"]}}
        });
        let a = Loaded::from_value(&document);
        let b = Loaded::from_value(&document);
        let (ea, eb) = (a.evaluator(), b.evaluator());
        assert_eq!(ea, eb);
        assert_eq!(ea.thresholds().collect::<Vec<_>>(), [60, 85]);
        assert!(ea.muted_accounts.contains("muted-bell"));
        assert!(!ea.recovery);
        assert_eq!(a.planner(), b.planner());
        assert_eq!(a.planner().lead, chrono::TimeDelta::minutes(15));
    }

    #[test]
    fn the_source_follows_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        let mut source = Source::new(Some(path.clone()));
        assert_eq!(source.loaded(), &Loaded::absent());
        assert!(!source.refresh(), "nothing changed");
        std::fs::write(&path, r#"{"alerts":{"enabled":true,"thresholds":[40]}}"#).unwrap();
        assert!(source.refresh());
        assert_eq!(source.loaded().alerts.thresholds, [40]);
        assert!(!source.refresh(), "same modification time");
    }
}
