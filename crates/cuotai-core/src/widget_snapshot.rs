//! `widget_snapshot` (PRD 0014 RF1): the compact, versioned JSON file that
//! system widgets and third-party integrations (Waybar, GNOME, KDE panels,
//! status bars) read instead of talking to the app. It is a projection of
//! the [`export::v1`] contract — never a query — with its own `schema`, so
//! the export can evolve without breaking widgets.
//!
//! Per account the file carries the key, name, provider, the binding metric
//! (PRD 0009), up to [`MAX_METRICS`] metrics that have a fraction (a finite
//! `used` over a finite positive `limit` — a percentage is never invented
//! when the capacity is unknown), the original `checked_at`, the freshness
//! and the pace (PRD 0007) when there is one. Stale accounts show their
//! preserved reading with its original time and `state: "stale"`; an account
//! that failed without an earlier reading is `unavailable`, never borrowing
//! another account's values. Credentials, credential variable names,
//! free-form notes and source URLs never appear, and the e-mail only when
//! the export was built with [`export::v1::Options::include_identity`].
//!
//! The write is atomic (a temporary file and a rename), readable by its
//! owner alone on Unix (`0600`), and capped at [`MAX_BYTES`]: accounts are
//! sorted by the binding metric (the most restrictive first) and the tail
//! is dropped, first by the caller's account limit, then by the size cap —
//! both deterministic, so the same state always produces the same file.

use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::export::v1::{AccountExport, Export};
use crate::freshness::Freshness;
use crate::metrics::{self, Binding};
use crate::pace::Pace;
use crate::{Metric, MetricWindow, Provider, Report, Status};

/// Version of this contract, independent of `export::v1`'s. Removing a
/// field or changing its meaning needs `2`.
pub const SCHEMA: u32 = 1;

/// Largest file written: 256 KiB (RF1.2). Accounts beyond the cap are
/// dropped from the tail of the binding order.
pub const MAX_BYTES: usize = 256 * 1024;

/// Most metrics published per account (the `N` of RF1.1); the binding
/// metric is always among them.
pub const MAX_METRICS: usize = 4;

/// Most history points published per account (PRD 0006's data, RF3.2); a
/// longer series is decimated evenly, keeping the oldest and newest.
pub const MAX_HISTORY_POINTS: usize = 96;

/// What the file says about an account's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetState {
    /// The current reading stands.
    Ready,
    /// The current query failed (or is pending); the metrics are the
    /// preserved reading with its original `checked_at`.
    Stale,
    /// No usable reading, current or preserved. Never another account's
    /// values.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetSnapshot {
    /// Always [`SCHEMA`].
    pub schema: u32,
    pub generated_at: DateTime<Utc>,
    /// In the binding order (the most restrictive account first).
    pub accounts: Vec<WidgetAccount>,
    /// `true` when accounts were dropped by the caller's limit or by
    /// [`MAX_BYTES`]. Metrics capped at [`MAX_METRICS`] are not truncation.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetAccount {
    /// [`crate::Account::key`]: `<provider slug>:<name>`, stable across
    /// reordering; a pinned widget matches on it and shows the account as
    /// unavailable when the key is gone, never another account.
    pub key: String,
    pub name: String,
    pub provider: Provider,
    /// Present only when the export included identity; `null` when the
    /// e-mail is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<Option<String>>,
    pub state: WidgetState,
    /// When the reading shown was measured; its original time while stale.
    /// `null` while `unavailable`.
    pub checked_at: Option<DateTime<Utc>>,
    /// Freshness of the reading shown (`stale` for a preserved one);
    /// `null` while `unavailable`.
    pub freshness: Option<Freshness>,
    /// The binding metric of the reading shown (recomputed over the
    /// preserved metrics while stale, so an old bar can be drawn dimmed).
    pub binding: Option<Binding>,
    /// Up to [`MAX_METRICS`] metrics with a fraction, the binding one
    /// first. Empty while `unavailable`.
    pub metrics: Vec<WidgetMetric>,
    /// PRD 0014 RF3.2: the binding metric's recent series (the used
    /// fraction over time), attached by whoever records history; absent
    /// while history is off or has no samples.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryPoint>,
}

/// One metric a widget can draw a bar for: a percentage exists, so the
/// capacity is known.
#[derive(Debug, Clone, Serialize)]
pub struct WidgetMetric {
    pub label: String,
    /// `used / limit`, unclamped (overuse reads above 1).
    pub used_fraction: f64,
    /// The capacity the fraction came from, so a panel can also draw the
    /// amount. Always present here: the metric has no fraction without it.
    pub limit: f64,
    pub unit: String,
    pub window: Option<MetricWindow>,
    pub resets_at: Option<DateTime<Utc>>,
    /// The pace (PRD 0007) when the metric has one; only current readings
    /// carry one, never a preserved one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pace: Option<Pace>,
}

/// One sample of the binding metric's series: `used / limit` at a time.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HistoryPoint {
    pub taken_at: DateTime<Utc>,
    pub used_fraction: f64,
}

/// What [`write`] did, for logging and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOutcome {
    pub bytes: usize,
    pub accounts: usize,
    pub truncated: bool,
}

impl WidgetSnapshot {
    /// The projection of `export`, capped at `max_accounts` accounts: the
    /// binding order keeps the most restrictive accounts when some must be
    /// dropped. Pure: reads the export and never queries anything.
    pub fn from_export(export: &Export, max_accounts: usize) -> Self {
        let mut accounts: Vec<WidgetAccount> = export.accounts.iter().map(project).collect();
        accounts.sort_by(cmp_accounts);
        let truncated = accounts.len() > max_accounts;
        accounts.truncate(max_accounts);
        Self {
            schema: SCHEMA,
            generated_at: export.generated_at,
            accounts,
            truncated,
        }
    }

    /// The account with `key`, for attaching a history series.
    pub fn account_mut(&mut self, key: &str) -> Option<&mut WidgetAccount> {
        self.accounts.iter_mut().find(|account| account.key == key)
    }

    /// The JSON, within [`MAX_BYTES`]: accounts past the cap are dropped
    /// from the tail of the binding order and `truncated` is set. Returns
    /// the body, how many accounts survived and the truncated flag.
    fn fitted_json(&self) -> (Vec<u8>, usize, bool) {
        let mut fitted = self.clone();
        let mut oversized = false;
        loop {
            let json = serde_json::to_vec(&fitted).unwrap_or_else(|_| b"{}".to_vec());
            if json.len() <= MAX_BYTES || fitted.accounts.is_empty() {
                let truncated = fitted.truncated || oversized;
                return (json, fitted.accounts.len(), truncated);
            }
            fitted.accounts.pop();
            fitted.truncated = true;
            oversized = true;
        }
    }

    /// Write the snapshot at `path`, atomically (a temporary file in the
    /// same directory and a rename) and readable by its owner alone on
    /// Unix (`0600`, also when the file already existed).
    pub fn write(&self, path: &Path) -> std::io::Result<WriteOutcome> {
        let (json, accounts, truncated) = self.fitted_json();
        write_atomic(path, &json)?;
        Ok(WriteOutcome {
            bytes: json.len(),
            accounts,
            truncated,
        })
    }
}

/// The snapshot of `export`, capped at `max_accounts`, written at `path`.
pub fn write(export: &Export, path: &Path, max_accounts: usize) -> std::io::Result<WriteOutcome> {
    WidgetSnapshot::from_export(export, max_accounts).write(path)
}

/// Atomic write of `bytes` at `path`: a temporary file in the same
/// directory (created `0600` by `tempfile` on Unix), synced and renamed
/// over the destination; the permission is enforced after the rename so a
/// pre-existing looser file does not survive.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = match dir {
        Some(dir) => tempfile::NamedTempFile::new_in(dir)?,
        None => tempfile::NamedTempFile::new()?,
    };
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// The binding order: accounts with a binding metric first, the most
/// restrictive (highest used fraction) ahead; ties and the rest by key, so
/// the sort is total and deterministic.
fn cmp_accounts(a: &WidgetAccount, b: &WidgetAccount) -> std::cmp::Ordering {
    let used = |account: &WidgetAccount| account.binding.as_ref().map(|b| b.used_ratio);
    match (used(a), used(b)) {
        (Some(x), Some(y)) => y.total_cmp(&x).then_with(|| a.key.cmp(&b.key)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.key.cmp(&b.key),
    }
}

/// The reading an account is projected from: the current one, the
/// preserved one (stale, original time) or nothing.
struct Reading<'a> {
    state: WidgetState,
    metrics: Option<&'a [Metric]>,
    checked_at: Option<DateTime<Utc>>,
    freshness: Option<Freshness>,
    binding: Option<Binding>,
    pace: &'a [Option<Pace>],
}

impl Reading<'_> {
    /// What the file shows: no reading at all, current or preserved.
    const UNAVAILABLE: Self = Self {
        state: WidgetState::Unavailable,
        metrics: None,
        checked_at: None,
        freshness: None,
        binding: None,
        pace: &[],
    };
}

/// One account of the export as the file shows it: the current reading, or
/// the preserved one (stale, original time), or `unavailable` with nothing.
fn project(account: &AccountExport) -> WidgetAccount {
    let current = account
        .report
        .as_ref()
        .filter(|report| report.status == Status::Ready);
    let reading = match (&account.last_good, current) {
        (Some(last_good), _) => Reading {
            state: WidgetState::Stale,
            metrics: Some(&last_good.metrics),
            checked_at: Some(last_good.checked_at),
            freshness: Some(Freshness::Stale),
            binding: binding_of(account, &last_good.metrics, last_good.checked_at),
            pace: &[],
        },
        (None, Some(report)) => Reading {
            state: WidgetState::Ready,
            metrics: Some(&report.metrics),
            checked_at: Some(report.checked_at),
            freshness: account.freshness,
            binding: account.binding.clone(),
            pace: &account.pace,
        },
        (None, None) => Reading::UNAVAILABLE,
    };
    let metrics = reading
        .metrics
        .map(|metrics| widget_metrics(metrics, reading.binding.as_ref(), reading.pace))
        .unwrap_or_default();
    WidgetAccount {
        key: account.key.clone(),
        name: account.name.clone(),
        provider: account.provider,
        email: account.email.clone(),
        state: reading.state,
        checked_at: reading.checked_at,
        freshness: reading.freshness,
        binding: reading.binding,
        metrics,
        history: Vec::new(),
    }
}

/// The binding metric of a preserved reading: `metrics::binding` needs a
/// `Ready` report, so one is rebuilt around the metrics (status and metrics
/// are all it reads).
fn binding_of(
    account: &AccountExport,
    metrics: &[Metric],
    checked_at: DateTime<Utc>,
) -> Option<Binding> {
    let report = Report {
        account: account.name.clone(),
        provider: account.provider,
        checked_at,
        status: Status::Ready,
        metrics: metrics.to_vec(),
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
    };
    metrics::binding(&report)
}

/// A metric goes to the file only with a percentage: a finite, non-negative
/// `used` over a finite, positive `limit`. Unknown capacity stays out.
fn fraction(metric: &Metric) -> Option<f64> {
    let used = metric
        .used
        .filter(|used| used.is_finite() && *used >= 0.0)?;
    let limit = metric
        .limit
        .filter(|limit| limit.is_finite() && *limit > 0.0)?;
    Some(used / limit)
}

/// Up to [`MAX_METRICS`] metrics with a fraction, the binding one first,
/// the rest in the report's order. `pace` aligns with `metrics` (the
/// export's per-metric pace) and is empty for a preserved reading.
fn widget_metrics(
    metrics: &[Metric],
    binding: Option<&Binding>,
    pace: &[Option<Pace>],
) -> Vec<WidgetMetric> {
    let mut order: Vec<usize> = Vec::new();
    if let Some(binding) = binding {
        if metrics.get(binding.index).and_then(fraction).is_some() {
            order.push(binding.index);
        }
    }
    let binding_index = order.first().copied();
    order.extend(
        metrics
            .iter()
            .enumerate()
            .filter(|(index, metric)| Some(*index) != binding_index && fraction(metric).is_some())
            .map(|(index, _)| index),
    );
    order
        .into_iter()
        .take(MAX_METRICS)
        .map(|index| {
            let metric = &metrics[index];
            WidgetMetric {
                label: metric.label.clone(),
                used_fraction: fraction(metric).unwrap_or(0.0),
                limit: metric.limit.unwrap_or_default(),
                unit: metric.unit.clone(),
                window: metric.window,
                resets_at: metric.resets_at,
                pace: pace.get(index).cloned().flatten(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::ConfigIdentity;
    use crate::export::v1::Options;
    use crate::refresh::RefreshCycle;
    use crate::{Account, Config, MetricKind};
    use chrono::TimeZone;
    use serde_json::Value;
    use std::time::Instant;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            name: name.into(),
            provider,
            ..Account::default()
        }
    }

    fn quota(label: &str, used: f64, limit: f64, resets_in_hours: i64) -> Metric {
        Metric {
            limit: Some(limit),
            resets_at: Some(now() + chrono::TimeDelta::hours(resets_in_hours)),
            ..Metric::usage(
                label,
                used,
                "%",
                MetricKind::Quota,
                Some(MetricWindow::Weekly),
            )
        }
    }

    fn report(account: &Account, status: Status, metrics: Vec<Metric>, minutes_ago: i64) -> Report {
        Report {
            account: account.name.clone(),
            provider: account.provider,
            checked_at: now() - chrono::TimeDelta::minutes(minutes_ago),
            status,
            metrics,
            notes: vec!["note with ana@example.com and GROQ_API_KEY".into()],
            notes_l10n: Vec::new(),
            source: "https://user:secret@example.com/usage?q=1".into(),
            session_expires_at: None,
        }
    }

    fn finish(cycle: &mut RefreshCycle, index: usize, report: Report) {
        let token = cycle.begin(index).unwrap();
        cycle.finish_account(index, Some(report), token, Instant::now());
    }

    fn build(config: &Config, cycle: &RefreshCycle, options: Options) -> Export {
        Export::from_cycle(
            config,
            &ConfigIdentity::Explicit("widgets".into()),
            cycle,
            &[],
            now(),
            options,
        )
    }

    fn ready_export(accounts: &[(&str, f64, f64)]) -> Export {
        let config = Config {
            accounts: accounts
                .iter()
                .map(|(name, _, _)| account(name, Provider::Groq))
                .collect(),
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        for (index, (name, used, limit)) in accounts.iter().enumerate() {
            let a = account(name, Provider::Groq);
            finish(
                &mut cycle,
                index,
                report(&a, Status::Ready, vec![quota("Week", *used, *limit, 24)], 3),
            );
        }
        build(&config, &cycle, Options::default())
    }

    #[test]
    fn ready_accounts_project_binding_metrics_and_no_secrets() {
        let export = ready_export(&[("Work", 40.0, 100.0)]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("widget.json");
        let outcome = write(&export, &path, 6).unwrap();
        assert_eq!(outcome.accounts, 1);
        assert!(!outcome.truncated);
        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["schema"], SCHEMA);
        assert_eq!(json["truncated"], false);
        let work = &json["accounts"][0];
        assert_eq!(work["key"], "groq:Work");
        assert_eq!(work["state"], "ready");
        assert_eq!(work["binding"]["used_ratio"], 0.4);
        assert_eq!(work["metrics"][0]["used_fraction"], 0.4);
        assert_eq!(work["metrics"][0]["window"], "weekly");
        assert!(work["checked_at"].is_string());
        assert_eq!(work["freshness"], "fresh");
        assert!(work.get("email").is_none(), "identity is off by default");
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in [
            "ana@example.com",
            "GROQ_API_KEY",
            "secret@",
            "note with",
            "notes",
            "example.com/usage",
        ] {
            assert!(
                !text.contains(secret),
                "the file must not contain {secret:?}"
            );
        }
    }

    #[test]
    fn identity_appears_only_when_included() {
        let mut with_email = account("Work", Provider::Groq);
        with_email
            .params
            .insert("email".into(), "ana@example.com".into());
        let config = Config {
            accounts: vec![with_email.clone()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        finish(
            &mut cycle,
            0,
            report(
                &with_email,
                Status::Ready,
                vec![quota("Week", 10.0, 100.0, 24)],
                3,
            ),
        );
        let off = build(&config, &cycle, Options::default());
        assert!(WidgetSnapshot::from_export(&off, 6).accounts[0]
            .email
            .is_none());
        let on = build(
            &config,
            &cycle,
            Options {
                include_identity: true,
            },
        );
        assert_eq!(
            WidgetSnapshot::from_export(&on, 6).accounts[0].email,
            Some(Some("ana@example.com".into()))
        );
    }

    #[test]
    fn the_account_limit_keeps_the_most_restrictive_first() {
        let export = ready_export(&[
            ("calm", 10.0, 100.0),
            ("tight", 90.0, 100.0),
            ("middle", 50.0, 100.0),
        ]);
        let snapshot = WidgetSnapshot::from_export(&export, 2);
        assert!(snapshot.truncated);
        let keys: Vec<&str> = snapshot.accounts.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["groq:tight", "groq:middle"]);
    }

    #[test]
    fn the_size_cap_drops_the_tail_deterministically() {
        let export = ready_export(&[("a", 10.0, 100.0), ("b", 20.0, 100.0)]);
        let mut snapshot = WidgetSnapshot::from_export(&export, 6);
        // Balloon the names until the file cannot fit two accounts.
        for account in &mut snapshot.accounts {
            account.name = "x".repeat(MAX_BYTES / 2);
        }
        let (json, surviving, truncated) = snapshot.fitted_json();
        assert!(truncated);
        assert!(json.len() <= MAX_BYTES);
        assert_eq!(surviving, 1, "only one oversized account fits");
        let (again, ..) = snapshot.fitted_json();
        assert_eq!(json, again, "the same state fits the same way");
    }

    #[test]
    fn stale_keeps_the_original_time_and_reading() {
        let a = account("Work", Provider::Groq);
        let config = Config {
            accounts: vec![a.clone()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        finish(
            &mut cycle,
            0,
            report(&a, Status::Ready, vec![quota("Week", 30.0, 100.0, 24)], 60),
        );
        finish(&mut cycle, 0, report(&a, Status::Error, Vec::new(), 1));
        let export = build(&config, &cycle, Options::default());
        assert!(
            export.accounts[0].stale,
            "fixture: the failure kept the reading"
        );
        let snapshot = WidgetSnapshot::from_export(&export, 6);
        let work = &snapshot.accounts[0];
        assert_eq!(work.state, WidgetState::Stale);
        assert_eq!(work.freshness, Some(Freshness::Stale));
        assert_eq!(
            work.checked_at,
            Some(now() - chrono::TimeDelta::minutes(60)),
            "the preserved reading keeps its original time"
        );
        assert_eq!(work.metrics.len(), 1);
        assert_eq!(work.metrics[0].used_fraction, 0.3);
        assert!(work.metrics[0].pace.is_none(), "old data has no pace");
        let ratio = work.binding.as_ref().map(|b| b.used_ratio).unwrap();
        assert!(
            (ratio - 0.3).abs() < 1e-9,
            "the dimmed bar comes from the preserved metrics"
        );
    }

    #[test]
    fn unavailable_never_borrows_another_accounts_values() {
        let ok = account("Fine", Provider::Groq);
        let bad = account("Broken", Provider::Kimi);
        let config = Config {
            accounts: vec![ok.clone(), bad.clone()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        finish(
            &mut cycle,
            0,
            report(&ok, Status::Ready, vec![quota("Week", 25.0, 100.0, 24)], 3),
        );
        finish(&mut cycle, 1, report(&bad, Status::Error, Vec::new(), 1));
        let snapshot = WidgetSnapshot::from_export(&build(&config, &cycle, Options::default()), 6);
        let broken = &snapshot
            .accounts
            .iter()
            .find(|a| a.key == "kimi:Broken")
            .unwrap();
        assert_eq!(broken.state, WidgetState::Unavailable);
        assert!(broken.metrics.is_empty());
        assert!(broken.binding.is_none());
        assert!(broken.checked_at.is_none());
        assert!(broken.freshness.is_none());
    }

    #[test]
    fn metrics_without_a_usable_cap_are_not_invented() {
        let a = account("Work", Provider::Groq);
        let uncapped = Metric::usage("Requests", 12.0, "", MetricKind::Usage, None);
        let config = Config {
            accounts: vec![a.clone()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        finish(&mut cycle, 0, report(&a, Status::Ready, vec![uncapped], 3));
        let snapshot = WidgetSnapshot::from_export(&build(&config, &cycle, Options::default()), 6);
        assert!(snapshot.accounts[0].metrics.is_empty());
        assert!(snapshot.accounts[0].binding.is_none());
        assert_eq!(snapshot.accounts[0].state, WidgetState::Ready);
    }

    #[test]
    #[cfg(unix)]
    fn the_file_is_private_and_the_write_atomic() {
        use std::os::unix::fs::PermissionsExt;
        let export = ready_export(&[("Work", 40.0, 100.0)]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("widget.json");
        write(&export, &path, 6).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // A looser pre-existing file is tightened, and no temporary lingers.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write(&export, &path, 6).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "only widget.json remains");
    }

    #[test]
    fn history_is_attached_by_key_and_absent_by_default() {
        let export = ready_export(&[("Work", 40.0, 100.0)]);
        let mut snapshot = WidgetSnapshot::from_export(&export, 6);
        assert!(snapshot.accounts[0].history.is_empty());
        snapshot.account_mut("groq:Work").unwrap().history = vec![
            HistoryPoint {
                taken_at: now() - chrono::TimeDelta::hours(1),
                used_fraction: 0.3,
            },
            HistoryPoint {
                taken_at: now(),
                used_fraction: 0.4,
            },
        ];
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["accounts"][0]["history"][1]["used_fraction"], 0.4);
        assert_eq!(
            json["accounts"][0]["metrics"][0]["limit"], 100.0,
            "a panel can draw the amount as well as the percentage"
        );
        assert!(snapshot.account_mut("groq:Other").is_none());
    }
}
