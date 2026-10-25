//! PRD 0028 RF4 — local contract canary. After one refresh of the owner's
//! own accounts, [`observe`] reduces the reports to a value-free *shape* per
//! provider and connection mode (status, failure category, drifted paths,
//! the metrics present by label, kind, window and unit), [`detect`] compares
//! it with the previous run, and [`markdown`], [`drafts`] and [`evidence`]
//! turn the result into a report, issue drafts and a reviewable list of
//! interfaces that answered cleanly. Nothing here reads a value, an account
//! name, an e-mail, an organization, a URL, a header or a body, and nothing
//! is filed or written outside the canary directory.

use crate::diagnostic::{ResponseShape, MAX_ENTRIES};
use crate::events::FailureCategory;
use crate::support::{self, auth_mode, AccountView, AppInfo, IssueDraft};
use crate::{access, Account, Report, Status};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Runs kept in the canary directory (RF4.1).
pub const KEEP_RUNS: usize = 30;
/// Record format version.
pub const FORMAT: u32 = 1;

/// One metric as the canary remembers it: never a value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MetricShape {
    pub label: String,
    pub kind: String,
    pub window: String,
    pub unit: String,
}

/// The shape of one provider and connection mode in one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub provider: String,
    pub mode: String,
    pub level: String,
    /// Undocumented or session interfaces are compared first (RF4.2).
    pub priority: bool,
    pub status: String,
    pub failure: Option<String>,
    /// `path (expected -> found)` of the drifted fields.
    pub paths: Vec<String>,
    pub digest: Option<String>,
    pub metrics: Vec<MetricShape>,
    /// RF4.1: the structural fingerprint of the answer (`path`, JSON type),
    /// when the query decoded a JSON response. Keys and types only.
    #[serde(default)]
    pub response: Vec<(String, String)>,
    /// SHA-256 of `response`.
    #[serde(default)]
    pub response_digest: Option<String>,
}

impl Shape {
    fn key(&self) -> (&str, &str) {
        (&self.provider, &self.mode)
    }
}

/// One run of the canary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub format: u32,
    /// The run date, `YYYY-MM-DD` (UTC).
    pub date: String,
    pub shapes: Vec<Shape>,
}

/// A change between two runs of the same provider and mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub provider: String,
    pub mode: String,
    /// `schema_changed`, `auth_required` or `metrics_changed`.
    pub category: &'static str,
    pub priority: bool,
    pub changes: Vec<String>,
    /// Index (into the observed accounts) of an account that shows it.
    pub account: usize,
}

/// An interface that answered with no drift (RF4.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub provider: String,
    pub mode: String,
    pub verified_on: String,
}

fn plain(value: &str, keep: impl Fn(char) -> bool, max: usize) -> String {
    value.chars().filter(|c| keep(*c)).take(max).collect()
}

/// A label key that cannot carry a name, an e-mail, a URL or an id.
fn label_key(label: &str) -> String {
    let kept = plain(
        label,
        |c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '/' | '%' | '(' | ')'),
        40,
    );
    let long_number = kept
        .split(|c: char| !c.is_ascii_digit())
        .any(|run| run.len() >= 6);
    if kept.trim().is_empty() || label.contains('@') || label.contains("://") || long_number {
        "label".into()
    } else {
        kept.trim().to_string()
    }
}

fn name_of<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "none".into())
}

fn status_name(status: Status) -> String {
    name_of(&status)
}

/// The shapes of `reports` (one per provider and mode, in first-seen order)
/// and, aligned with them, the index of the first account of each group.
pub fn observe(accounts: &[Account], reports: &[Report], date: &str) -> (Run, Vec<usize>) {
    observe_with(accounts, reports, &[], date)
}

/// [`observe`] with the response fingerprints of
/// `Monitor::refresh_with_shapes` (RF4.1), aligned with the accounts.
pub fn observe_with(
    accounts: &[Account],
    reports: &[Report],
    responses: &[Option<ResponseShape>],
    date: &str,
) -> (Run, Vec<usize>) {
    let mut shapes: Vec<Shape> = Vec::new();
    let mut firsts = Vec::new();
    for (index, (account, report)) in accounts.iter().zip(reports).enumerate() {
        let info = access::describe(account);
        let level = name_of(&info.level);
        let provider = account.provider.slug().to_string();
        let mode = auth_mode(account).to_string();
        let drift = report.diagnostic.as_ref().map(|d| d.drift());
        let mut metrics: BTreeSet<MetricShape> = report
            .metrics
            .iter()
            .map(|m| MetricShape {
                label: label_key(&m.label),
                kind: name_of(&m.kind),
                window: m.window.map_or_else(|| "none".into(), |w| name_of(&w)),
                unit: plain(&m.unit, |c| c.is_ascii_alphanumeric() || c == '_', 12),
            })
            .collect();
        let failure = FailureCategory::of(report).map(|c| c.name().to_string());
        let paths: Vec<String> = drift
            .map(|d| {
                d.paths
                    .iter()
                    .map(|p| format!("{} ({} -> {})", p.path, p.expected, p.found))
                    .collect()
            })
            .unwrap_or_default();
        let digest = drift.map(|d| d.digest.clone());
        let response = responses.get(index).and_then(Option::as_ref);
        if let Some(existing) = shapes
            .iter_mut()
            .find(|s| s.provider == provider && s.mode == mode)
        {
            // Several accounts of one provider and mode: the worst answer
            // wins, the metrics are the union.
            metrics.extend(existing.metrics.drain(..));
            existing.metrics = metrics.into_iter().collect();
            if let Some(response) = response {
                let mut merged = ResponseShape {
                    fingerprint: std::mem::take(&mut existing.response),
                    digest: String::new(),
                };
                merged.merge(response);
                existing.response = merged.fingerprint;
                existing.response_digest = Some(merged.digest);
            }
            if failure.is_some() && existing.failure.is_none() {
                existing.status = status_name(report.status);
                existing.failure = failure;
                existing.paths = paths;
                existing.digest = digest;
            }
            continue;
        }
        shapes.push(Shape {
            priority: matches!(
                info.level,
                access::Level::Undocumented | access::Level::Derived
            ) || info.credential == access::Credential::UserSession,
            provider,
            mode,
            level,
            status: status_name(report.status),
            failure,
            paths,
            digest,
            metrics: metrics.into_iter().collect(),
            response: response.map(|r| r.fingerprint.clone()).unwrap_or_default(),
            response_digest: response.map(|r| r.digest.clone()),
        });
        firsts.push(index);
    }
    (
        Run {
            format: FORMAT,
            date: date.to_string(),
            shapes,
        },
        firsts,
    )
}

fn metric_text(metric: &MetricShape) -> String {
    format!(
        "{} ({}, {}, {})",
        metric.label, metric.kind, metric.window, metric.unit
    )
}

/// RF4.2: a field present before and gone now, or a type that changed.
/// A field that turned `null` (or came back from `null`) is an optional
/// value, not drift. A fingerprint cut at [`MAX_ENTRIES`] only vouches for
/// the paths up to its last entry.
fn response_changes(before: &[(String, String)], now: &[(String, String)]) -> Vec<String> {
    if before.is_empty() || now.is_empty() {
        return Vec::new();
    }
    let horizon = (now.len() >= MAX_ENTRIES).then(|| now.last().map(|(p, _)| p.as_str()));
    let mut changes = Vec::new();
    for (path, kind) in before {
        if horizon.flatten().is_some_and(|last| path.as_str() > last) {
            continue;
        }
        let found: Vec<&str> = now
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, k)| k.as_str())
            .collect();
        if found.is_empty() {
            changes.push(format!("field missing: {path}"));
        } else if !found.contains(&kind.as_str())
            && kind != "null"
            && found.iter().all(|k| *k != "null")
        {
            changes.push(format!(
                "type changed: {path} ({kind} -> {})",
                found.join("|")
            ));
        }
    }
    changes
}

/// RF4.5: a fixed status component that the public page no longer lists is a
/// drift entry ("status component renamed"), instead of the silent `Unknown`
/// the app shows. Mode `status`, shown with an account of that provider.
fn status_drifts(
    accounts: &[Account],
    missing: &[(crate::Provider, Vec<&'static str>)],
) -> Vec<Drift> {
    missing
        .iter()
        .filter_map(|(provider, names)| {
            let account = accounts.iter().position(|a| a.provider == *provider)?;
            Some(Drift {
                provider: provider.slug().to_string(),
                mode: "status".into(),
                category: "status_component",
                priority: false,
                changes: names
                    .iter()
                    .map(|name| format!("status component renamed or removed: {name}"))
                    .collect(),
                account,
            })
        })
        .collect()
}

/// What changed from `previous` to `current` (RF4.2), priority interfaces
/// first. Two identical runs give an empty list. `firsts` is the second
/// value of [`observe`] for `current`.
pub fn detect(previous: &Run, current: &Run, firsts: &[usize]) -> Vec<Drift> {
    let mut drifts = Vec::new();
    for (position, now) in current.shapes.iter().enumerate() {
        let Some(before) = previous.shapes.iter().find(|s| s.key() == now.key()) else {
            continue;
        };
        let account = firsts.get(position).copied().unwrap_or(0);
        let mut push = |category: &'static str, changes: Vec<String>| {
            drifts.push(Drift {
                provider: now.provider.clone(),
                mode: now.mode.clone(),
                category,
                priority: now.priority,
                changes,
                account,
            });
        };
        if let Some(failure) = now.failure.as_deref() {
            if before.failure.as_deref() != Some(failure) {
                match failure {
                    "schema_changed" => {
                        let mut changes = vec!["the answer no longer matches the interface".into()];
                        changes.extend(
                            now.paths
                                .iter()
                                .filter(|p| !before.paths.contains(p))
                                .map(|p| format!("field changed: {p}")),
                        );
                        push("schema_changed", changes);
                    }
                    "auth_required" => push(
                        "auth_required",
                        vec!["credentials are refused or missing".into()],
                    ),
                    _ => {}
                }
            }
        }
        if before.status == "ready" && now.status == "ready" {
            let changes = response_changes(&before.response, &now.response);
            if !changes.is_empty() {
                push("schema_changed", changes);
            }
            let mut changes: Vec<String> = before
                .metrics
                .iter()
                .filter(|m| !now.metrics.contains(m))
                .map(|m| format!("metric absent: {}", metric_text(m)))
                .collect();
            changes.extend(
                now.metrics
                    .iter()
                    .filter(|m| !before.metrics.contains(m))
                    .map(|m| format!("metric new: {}", metric_text(m))),
            );
            if !changes.is_empty() {
                push("metrics_changed", changes);
            }
        }
    }
    drifts.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.category.cmp(b.category))
    });
    drifts
}

/// Interfaces of `current` that answered with no drift (RF4.4): ready, no
/// failure and no entry in `drifts`.
pub fn evidence(current: &Run, drifts: &[Drift]) -> Vec<Evidence> {
    current
        .shapes
        .iter()
        .filter(|s| s.status == "ready" && s.failure.is_none())
        .filter(|s| {
            !drifts
                .iter()
                .any(|d| d.provider == s.provider && d.mode == s.mode)
        })
        .map(|s| Evidence {
            provider: s.provider.clone(),
            mode: s.mode.clone(),
            verified_on: current.date.clone(),
        })
        .collect()
}

/// One issue draft per drift (RF4.3), built by [`support::canary_draft`].
pub fn drafts(
    drifts: &[Drift],
    accounts: &[Account],
    reports: &[Report],
    statuses: &[crate::incidents::ProviderStatus],
    app: &AppInfo,
) -> Vec<IssueDraft> {
    drifts
        .iter()
        .filter_map(|drift| {
            let account = accounts.get(drift.account)?;
            let report = reports.get(drift.account)?;
            Some(support::canary_draft(
                &AccountView {
                    account,
                    report,
                    last_good_age: None,
                    incident_active: crate::incidents::incident_active(
                        statuses,
                        account.provider,
                    ),
                },
                app,
                drift.category,
                &drift.changes.join("; "),
            ))
        })
        .collect()
}

/// The value-free Markdown report (RF4.3).
pub fn markdown(
    run: &Run,
    drifts: &[Drift],
    evidence: &[Evidence],
    skipped: &[String],
    first_run: bool,
) -> String {
    let mut out = format!("# Cuotai canary report {}\n\n", run.date);
    if first_run {
        out.push_str("First run: nothing to compare with yet.\n\n");
    }
    out.push_str(&format!(
        "{} interface(s) observed, {} drift(s), {} verified.\n\n",
        run.shapes.len(),
        drifts.len(),
        evidence.len()
    ));
    if drifts.is_empty() {
        out.push_str("## Drift\n\nNone.\n\n");
    } else {
        out.push_str("## Drift\n\n");
        for drift in drifts {
            out.push_str(&format!(
                "- {}`{}` ({}): {}\n",
                if drift.priority { "**priority** " } else { "" },
                drift.provider,
                drift.mode,
                drift.category
            ));
            for change in &drift.changes {
                out.push_str(&format!("  - {change}\n"));
            }
        }
        out.push('\n');
    }
    if !evidence.is_empty() {
        out.push_str("## Answered with no drift\n\n");
        for e in evidence {
            out.push_str(&format!("- `{}` ({})\n", e.provider, e.mode));
        }
        out.push('\n');
    }
    if !skipped.is_empty() {
        out.push_str("## Skipped (billed per query; use --canary-include-metered)\n\n");
        for provider in skipped {
            out.push_str(&format!("- `{provider}`\n"));
        }
    }
    out
}

/// Everything one canary run produces.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub run: Run,
    pub drifts: Vec<Drift>,
    pub evidence: Vec<Evidence>,
    pub drafts: Vec<IssueDraft>,
    pub markdown: String,
}

/// The whole analysis of one refresh, pure: observe, compare with
/// `previous`, and build the evidence, drafts and report. `skipped` lists
/// the providers left out because they bill per query.
pub fn analyze(
    previous: Option<&Run>,
    accounts: &[Account],
    reports: &[Report],
    date: &str,
    skipped: &[String],
    app: &AppInfo,
) -> Outcome {
    analyze_with(previous, accounts, reports, &[], &[], &[], date, skipped, app)
}

/// [`analyze`] with the response fingerprints of `Monitor::refresh_with_shapes`
/// (RF4.1) and the status components `Monitor::missing_status_components`
/// found gone (RF4.5).
#[allow(clippy::too_many_arguments)]
pub fn analyze_with(
    previous: Option<&Run>,
    accounts: &[Account],
    reports: &[Report],
    responses: &[Option<ResponseShape>],
    status_missing: &[(crate::Provider, Vec<&'static str>)],
    statuses: &[crate::incidents::ProviderStatus],
    date: &str,
    skipped: &[String],
    app: &AppInfo,
) -> Outcome {
    let (run, firsts) = observe_with(accounts, reports, responses, date);
    let mut drifts = previous.map_or_else(Vec::new, |previous| detect(previous, &run, &firsts));
    drifts.extend(status_drifts(accounts, status_missing));
    let evidence = evidence(&run, &drifts);
    let drafts = drafts(&drifts, accounts, reports, statuses, app);
    let markdown = markdown(&run, &drifts, &evidence, skipped, previous.is_none());
    Outcome {
        run,
        drifts,
        evidence,
        drafts,
        markdown,
    }
}

/// The canary directory: records, report and evidence list.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `<data dir>/cuotai/canary`.
    pub fn in_data_dir() -> Option<Self> {
        dirs::data_local_dir().map(|dir| Self::new(dir.join("cuotai").join("canary")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn runs(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("run-") && n.ends_with(".json"))
            })
            .collect();
        files.sort();
        files
    }

    /// The most recent stored run, if readable.
    pub fn latest(&self) -> Option<Run> {
        let path = self.runs().pop()?;
        let run: Run = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (run.format == FORMAT).then_some(run)
    }

    /// Write the run (`0600`), the report and the evidence list, and keep
    /// only the last [`KEEP_RUNS`] runs. `stamp` orders the files
    /// (`YYYYMMDDTHHMMSSZ`).
    pub fn save(
        &self,
        stamp: &str,
        run: &Run,
        report: &str,
        evidence: &[Evidence],
    ) -> std::io::Result<()> {
        let stamp = plain(stamp, |c| c.is_ascii_alphanumeric(), 20);
        let json = serde_json::to_vec_pretty(run).map_err(std::io::Error::other)?;
        crate::private_fs::write_file(&self.dir.join(format!("run-{stamp}.json")), &json)?;
        crate::private_fs::write_file(&self.dir.join("report.md"), report.as_bytes())?;
        let list = serde_json::to_vec_pretty(evidence).map_err(std::io::Error::other)?;
        crate::private_fs::write_file(&self.dir.join("verified_on.json"), &list)?;
        let runs = self.runs();
        for old in runs.iter().take(runs.len().saturating_sub(KEEP_RUNS)) {
            let _ = std::fs::remove_file(old);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic;
    use crate::{Metric, MetricKind, Provider};
    use serde_json::json;

    fn account(provider: Provider) -> Account {
        Account {
            name: "ana@example.com SEEDED-NAME".into(),
            provider,
            credential_env: Some("SEEDED_CREDENTIAL_ENV".into()),
            params: [("org".to_string(), "SEEDED-ORG".to_string())].into(),
            ..Default::default()
        }
    }

    fn report(account: &Account, status: Status, labels: &[&str]) -> Report {
        Report {
            overage_hint: Default::default(),
            account_id: None,
            as_of: Default::default(),
            sub_reports: Vec::new(),
            account: account.name.clone(),
            provider: account.provider,
            checked_at: chrono::Utc::now(),
            status,
            metrics: labels
                .iter()
                .map(|l| Metric::usage(*l, 123456.789, "tokens", MetricKind::Usage, None))
                .collect(),
            notes: vec!["sk-live-SEEDED https://x.example.com/?key=SEEDED".into()],
            notes_l10n: if status == Status::Error {
                vec![crate::text::Localized::fixed(
                    crate::providers::SCHEMA_NOTE,
                    "ana@example.com SEEDED",
                )]
            } else {
                Vec::new()
            },
            source: "https://secret.example.com/SEEDED".into(),
            session_expires_at: None,
            diagnostic: None,
        }
    }

    fn app() -> AppInfo {
        AppInfo {
            version: "0.80.0".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            install_channel: "homebrew".into(),
        }
    }

    fn run(accounts: &[Account], reports: &[Report]) -> (Run, Vec<usize>) {
        observe(accounts, reports, "2026-10-25")
    }

    #[test]
    fn identical_runs_raise_no_alert_and_give_evidence() {
        let accounts = [account(Provider::Claude)];
        let reports = [report(&accounts[0], Status::Ready, &["5h", "7d"])];
        let (before, _) = run(&accounts, &reports);
        let (now, firsts) = run(&accounts, &reports);
        let drifts = detect(&before, &now, &firsts);
        assert!(drifts.is_empty());
        let ev = evidence(&now, &drifts);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].verified_on, "2026-10-25");
    }

    #[test]
    fn a_successful_answer_that_lost_a_field_or_changed_a_type_is_drift() {
        let accounts = [account(Provider::Claude)];
        let reports = [report(&accounts[0], Status::Ready, &["5h"])];
        let shape = |value: serde_json::Value| vec![Some(ResponseShape::of(&value))];
        let day = |value: serde_json::Value| observe_with(&accounts, &reports, &shape(value), "d");
        let (before, _) = day(json!({"a": {"x": 1, "y": "SEEDED-VALUE"}, "opt": 1, "z": 2}));
        // Identical shape (different values) and an optional field turning
        // null are not drift.
        let (same, firsts) = day(json!({"a": {"x": 9, "y": "other"}, "opt": null, "z": 3}));
        assert!(detect(&before, &same, &firsts).is_empty());
        // `y` renamed and `z` retyped.
        let (now, firsts) = day(json!({"a": {"x": 1, "w": "v"}, "opt": 1, "z": "2"}));
        let drifts = detect(&before, &now, &firsts);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].category, "schema_changed");
        assert_eq!(
            drifts[0].changes,
            ["field missing: a.y", "type changed: z (number -> string)"]
        );
        assert!(evidence(&now, &drifts).is_empty());
        // The record holds keys and types only.
        let text = serde_json::to_string(&before).unwrap();
        assert!(text.contains("\"response\"") && !text.contains("SEEDED-VALUE"));
    }

    #[test]
    fn a_renamed_status_component_is_a_drift_with_its_own_draft() {
        let accounts = [account(Provider::Claude), account(Provider::OpenAiApi)];
        let reports = [
            report(&accounts[0], Status::Ready, &["5h"]),
            report(&accounts[1], Status::Ready, &["month"]),
        ];
        let outcome = analyze_with(
            None,
            &accounts,
            &reports,
            &[],
            &[(Provider::OpenAiApi, vec!["Responses"])],
            &[],
            "2026-10-25",
            &[],
            &app(),
        );
        assert_eq!(outcome.drifts.len(), 1);
        assert_eq!(outcome.drifts[0].category, "status_component");
        assert_eq!(outcome.drifts[0].mode, "status");
        assert_eq!(
            outcome.drifts[0].changes,
            ["status component renamed or removed: Responses"]
        );
        assert_eq!(outcome.drafts.len(), 1);
        assert!(outcome.markdown.contains("status_component"));
        // It does not take the interface's own evidence away.
        assert_eq!(outcome.evidence.len(), 2);
    }

    #[test]
    fn a_fingerprint_cut_at_the_cap_only_vouches_for_its_own_paths() {
        let wide = |from: usize| -> Vec<(String, String)> {
            (from..from + MAX_ENTRIES)
                .map(|i| (format!("k{i:03}"), "number".to_string()))
                .collect()
        };
        // Everything the new, truncated fingerprint cannot see is ignored.
        assert!(response_changes(&wide(0), &wide(0)).is_empty());
        let shifted = response_changes(&wide(0), &wide(1));
        assert_eq!(shifted, ["field missing: k000"]);
    }

    #[test]
    fn a_mutated_answer_gives_one_drift_one_draft_and_no_evidence() {
        let accounts = [account(Provider::Claude), account(Provider::OpenAiApi)];
        let good = [
            report(&accounts[0], Status::Ready, &["5h"]),
            report(&accounts[1], Status::Ready, &["month"]),
        ];
        let (before, _) = run(&accounts, &good);
        let mut broken = report(&accounts[0], Status::Error, &[]);
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct Window {
            utilization: f64,
        }
        broken.diagnostic = Some(diagnostic::Diagnostic::SchemaDrift(
            diagnostic::decode::<Window>(&json!({"utilization": "sk-live-SEEDED"})).unwrap_err(),
        ));
        let now_reports = [broken, good[1].clone()];
        let (now, firsts) = run(&accounts, &now_reports);
        let drifts = detect(&before, &now, &firsts);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].category, "schema_changed");
        assert!(drifts[0]
            .changes
            .iter()
            .any(|c| c.contains("utilization (number -> string)")));
        let ev = evidence(&now, &drifts);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].provider, "open_ai_api");
        let made = drafts(&drifts, &accounts, &now_reports, &[], &app());
        assert_eq!(made.len(), 1);
        assert!(made[0].title.contains("canary schema_changed"));
        assert!(made[0].url.contains("shape_diff="));
    }

    #[test]
    fn drafts_state_the_provider_incident_from_the_statuses() {
        use crate::incidents::{incident_active, IncidentState, ProviderStatus};
        let status = |provider, state| ProviderStatus {
            provider,
            state,
            incident: None,
            page_url: "https://status.example.com".into(),
            read_at: None,
            stale: false,
            expired: false,
        };
        let statuses = [
            status(Provider::OpenAiApi, IncidentState::PartialOutage),
            status(Provider::Claude, IncidentState::Unknown),
        ];
        assert_eq!(incident_active(&statuses, Provider::OpenAiApi), Some(true));
        assert_eq!(incident_active(&statuses, Provider::Claude), None);
        assert_eq!(incident_active(&statuses, Provider::Groq), None);
        assert_eq!(
            incident_active(
                &[status(Provider::OpenAiApi, IncidentState::Operational)],
                Provider::OpenAiApi
            ),
            Some(false)
        );
        let accounts = [account(Provider::OpenAiApi)];
        let (before, _) = run(
            &accounts,
            &[report(&accounts[0], Status::Ready, &["month"])],
        );
        let reports = [report(&accounts[0], Status::Ready, &["day"])];
        let (now, firsts) = run(&accounts, &reports);
        let drifts = detect(&before, &now, &firsts);
        let made = drafts(&drifts, &accounts, &reports, &statuses, &app());
        assert!(made[0].body.contains("Provider incident active: yes"));
        let made = drafts(&drifts, &accounts, &reports, &[], &app());
        assert!(!made[0].body.contains("Provider incident active: yes"));
    }

    #[test]
    fn metrics_that_appear_or_vanish_are_drift_but_a_failure_is_not_a_removal() {
        let accounts = [account(Provider::Claude)];
        let (before, _) = run(
            &accounts,
            &[report(&accounts[0], Status::Ready, &["5h", "7d"])],
        );
        let (now, firsts) = run(
            &accounts,
            &[report(&accounts[0], Status::Ready, &["5h", "30d"])],
        );
        let drifts = detect(&before, &now, &firsts);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].category, "metrics_changed");
        assert_eq!(drifts[0].changes.len(), 2);
        let offline = report(&accounts[0], Status::AuthenticationError, &[]);
        let (failed, firsts) = run(&accounts, &[offline]);
        let drifts = detect(&before, &failed, &firsts);
        assert!(drifts.iter().all(|d| d.category == "auth_required"));
    }

    #[test]
    fn nothing_seeded_reaches_any_record_report_draft_or_list() {
        let accounts = [account(Provider::Claude)];
        let mut reports = [report(
            &accounts[0],
            Status::Ready,
            &["ana@example.com", "5h"],
        )];
        reports[0].metrics[1].label = "5h SEEDED-ORG 1234567890".into();
        let (before, _) = run(&accounts, &[report(&accounts[0], Status::Ready, &["5h"])]);
        let (now, firsts) = run(&accounts, &reports);
        let drifts = detect(&before, &now, &firsts);
        let ev = evidence(&now, &drifts);
        let all = format!(
            "{}{}{}{}",
            serde_json::to_string(&now).unwrap(),
            markdown(&now, &drifts, &ev, &[], false),
            serde_json::to_string(&ev).unwrap(),
            drafts(&drifts, &accounts, &reports, &[], &app())
                .iter()
                .map(|d| format!("{}{}{}", d.title, d.body, d.url))
                .collect::<String>()
        );
        for secret in [
            "SEEDED",
            "ana@",
            "example.com",
            "sk-live",
            "key=",
            "1234567890",
            "123456.789",
        ] {
            assert!(!all.contains(secret), "{secret} leaked:\n{all}");
        }
    }

    #[test]
    fn the_store_keeps_thirty_private_runs_and_reads_the_latest() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("canary"));
        assert!(store.latest().is_none());
        let accounts = [account(Provider::Claude)];
        for i in 0..(KEEP_RUNS + 3) {
            let (run, _) = observe(
                &accounts,
                &[report(&accounts[0], Status::Ready, &["5h"])],
                &format!("2026-10-{:02}", i % 28 + 1),
            );
            store
                .save(&format!("20261025T{i:06}Z"), &run, "report", &[])
                .unwrap();
        }
        assert_eq!(store.runs().len(), KEEP_RUNS);
        assert!(store.latest().is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.runs().pop().unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
    }
}
