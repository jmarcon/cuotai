use super::*;
use crate::config_file::ConfigIdentity;
use crate::export::v1::{Export, Options};
use crate::incidents::{Incident, ProviderStatus};
use crate::overage::OverageState;
use crate::refresh::RefreshCycle;
use crate::text::{params, Localized, ParamValue};
use crate::{Account, Config, MetricKind};
use chrono::TimeZone;
use serde_json::json;
use std::time::Instant;
use EventKind::*;

const LABEL: &str = "Sessão (5h)";

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
}

fn at(minute: i64) -> DateTime<Utc> {
    t0() + TimeDelta::minutes(minute)
}

/// The window the readings start in ends at minute 20.
fn first_window() -> DateTime<Utc> {
    at(20)
}

/// The window after it.
fn second_window() -> DateTime<Utc> {
    at(20) + TimeDelta::hours(5)
}

fn quota(used: f64, resets_at: DateTime<Utc>) -> Metric {
    Metric {
        limit: Some(100.0),
        resets_at: Some(resets_at),
        ..Metric::usage(
            LABEL,
            used,
            "%",
            MetricKind::Quota,
            Some(MetricWindow::Window5h),
        )
    }
}

fn cycle_end() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()
}

/// The 5-hour window plus a Claude extra-usage meter in provider units
/// (`spent` `None`: the amount is missing from the response).
fn with_extra(spent: Option<f64>, cycle: DateTime<Utc>) -> Reading {
    let mut extra = Metric::usage(
        "Uso extra — utilizado",
        0.0,
        "unid. provedor",
        MetricKind::Quota,
        Some(MetricWindow::Monthly),
    )
    .overage();
    extra.used = spent;
    extra.limit = Some(50.0);
    extra.resets_at = Some(cycle);
    Reading::Ready(vec![quota(10.0, first_window()), extra])
}

enum Reading {
    Ready(Vec<Metric>),
    Failed(Status, Localized),
}

/// A `Ready` reading in the first window.
fn ready(used: f64) -> Reading {
    Reading::Ready(vec![quota(used, first_window())])
}

/// A `Ready` reading after the first window reset.
fn renewed(used: f64) -> Reading {
    Reading::Ready(vec![quota(used, second_window())])
}

fn failed(status: Status, id: &'static str) -> Reading {
    Reading::Failed(status, Localized::fixed(id, "mensagem bruta do provedor"))
}

/// One account read through a real `RefreshCycle` and `export::v1`, as a
/// front end would, with the engine judging each new export.
struct Watch {
    config: Config,
    cycle: RefreshCycle,
    state: EventState,
    prev: AccountExport,
    statuses: Vec<ProviderStatus>,
    source: String,
    session_expires_at: Option<DateTime<Utc>>,
}

impl Watch {
    fn new() -> Self {
        Self::with_account(Account {
            id: None,
            name: "Work".into(),
            provider: Provider::Claude,
            ..Account::default()
        })
    }

    fn with_account(account: Account) -> Self {
        let config = Config {
            accounts: vec![account],
            ..Config::default()
        };
        let cycle = RefreshCycle::new(&config, Instant::now());
        let prev = export(&config, &cycle, &[], t0());
        Self {
            config,
            cycle,
            // `quota_reset_soon` has its own tests; the sequences below
            // read windows minutes from their end.
            state: EventState::default().with_reset_lead(TimeDelta::zero()),
            prev,
            statuses: Vec::new(),
            source: String::new(),
            session_expires_at: None,
        }
    }

    fn read(&mut self, minute: i64, reading: Reading) -> Vec<Event> {
        let account = &self.config.accounts[0];
        let (status, metrics, notes_l10n) = match reading {
            Reading::Ready(metrics) => (Status::Ready, metrics, Vec::new()),
            Reading::Failed(status, note) => (status, Vec::new(), vec![note]),
        };
        let report = Report {
            account_id: None,
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: at(minute),
            status,
            metrics,
            notes: notes_l10n.iter().map(|n| n.fallback.to_string()).collect(),
            notes_l10n,
            source: self.source.clone(),
            session_expires_at: self.session_expires_at,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        };
        let token = self.cycle.begin(0).unwrap();
        self.cycle
            .finish_account(0, Some(report), token, Instant::now());
        self.observe(minute)
    }

    /// Judge the export at `minute` without a new reading.
    fn observe(&mut self, minute: i64) -> Vec<Event> {
        let next = export(&self.config, &self.cycle, &self.statuses, at(minute));
        let events = diff(&self.prev, &next, &mut self.state, at(minute));
        self.prev = next;
        events
    }

    fn status(&mut self, state: IncidentState, stale: bool) {
        self.statuses = vec![ProviderStatus {
            provider: Provider::Claude,
            state,
            incident: state.is_incident().then(|| Incident {
                title: "Elevated errors on Claude Code".into(),
                started_at: Some(t0()),
                url: "https://status.claude.com/incidents/abc123".into(),
            }),
            page_url: "https://status.claude.com/".into(),
            read_at: Some(t0()),
            stale,
            expired: false,
        }];
    }
}

fn export(
    config: &Config,
    cycle: &RefreshCycle,
    statuses: &[ProviderStatus],
    now: DateTime<Utc>,
) -> AccountExport {
    let identity = ConfigIdentity::Explicit("events".into());
    Export::from_cycle(config, &identity, cycle, statuses, now, Options::default())
        .accounts
        .into_iter()
        .next()
        .expect("the export holds the first account")
}

/// Kinds in order, leaving out `usage_updated` (its throttle has a test of
/// its own).
fn transitions(events: &[Event]) -> Vec<EventKind> {
    events
        .iter()
        .map(|event| event.event)
        .filter(|kind| *kind != UsageUpdated)
        .collect()
}

/// RF1.2/RF1.4: sequences of readings and the edge events each one fires.
#[test]
fn reading_sequences_fire_one_event_per_transition() {
    type Step = (i64, Reading, &'static [EventKind]);
    let cases: Vec<(&str, Vec<Step>)> = vec![
        (
            "a first reading only sets the baseline",
            vec![(0, ready(95.0), &[])],
        ),
        (
            "an exhausted first reading only sets the baseline",
            vec![(0, ready(100.0), &[]), (2, ready(100.0), &[])],
        ),
        (
            "a rise crosses each threshold once and exhaustion blocks",
            vec![
                (0, ready(50.0), &[]),
                (2, ready(80.0), &[QuotaThresholdCrossed]),
                (4, ready(85.0), &[]),
                (6, ready(92.0), &[QuotaThresholdCrossed]),
                (8, ready(100.0), &[QuotaExhausted, AccountBlocked]),
                (10, ready(100.0), &[]),
            ],
        ),
        (
            "a jump past every threshold fires only the highest",
            vec![
                (0, ready(10.0), &[]),
                (2, ready(95.0), &[QuotaThresholdCrossed]),
            ],
        ),
        (
            "a fall without a reset fires nothing",
            vec![
                (0, ready(92.0), &[]),
                (2, ready(60.0), &[]),
                (4, ready(40.0), &[]),
            ],
        ),
        (
            "oscillating around a threshold fires once",
            vec![
                (0, ready(70.0), &[]),
                (2, ready(76.0), &[QuotaThresholdCrossed]),
                (4, ready(74.0), &[]),
                (6, ready(77.0), &[]),
                (8, ready(73.0), &[]),
                (10, ready(76.0), &[]),
            ],
        ),
        (
            "falling five points below re-arms a threshold",
            vec![
                (0, ready(70.0), &[]),
                (2, ready(76.0), &[QuotaThresholdCrossed]),
                (4, ready(69.0), &[]),
                (6, ready(76.0), &[QuotaThresholdCrossed]),
            ],
        ),
        (
            "a reset after exhaustion recovers and unblocks",
            vec![
                (0, ready(60.0), &[]),
                (2, ready(100.0), &[QuotaExhausted, AccountBlocked]),
                (
                    25,
                    renewed(5.0),
                    &[QuotaReset, QuotaRecovered, AccountUnblocked],
                ),
                (27, renewed(6.0), &[]),
            ],
        ),
        (
            "an exhaustion held in the baseline recovers too",
            vec![
                (0, ready(100.0), &[]),
                (
                    25,
                    renewed(5.0),
                    &[QuotaReset, QuotaRecovered, AccountUnblocked],
                ),
            ],
        ),
        (
            "an idle quota reports its reset alone",
            vec![(0, ready(30.0), &[]), (25, renewed(0.0), &[QuotaReset])],
        ),
        (
            "a failure fires once whatever its category, then recovers",
            vec![
                (0, ready(30.0), &[]),
                (
                    2,
                    failed(Status::Error, "query.transport"),
                    &[RefreshFailed],
                ),
                (4, failed(Status::Error, "query.timeout"), &[]),
                (6, failed(Status::RateLimited, "query.http"), &[]),
                (8, ready(30.0), &[RefreshRecovered]),
                (10, ready(30.0), &[]),
            ],
        ),
        (
            "an authentication failure also asks for authentication",
            vec![
                (0, ready(30.0), &[]),
                (
                    2,
                    failed(Status::AuthenticationError, "query.http"),
                    &[RefreshFailed, AuthRequired],
                ),
                (4, failed(Status::AuthenticationError, "query.http"), &[]),
                (6, ready(30.0), &[RefreshRecovered]),
            ],
        ),
        (
            "authentication lost during another failure",
            vec![
                (0, ready(30.0), &[]),
                (
                    2,
                    failed(Status::Error, "query.transport"),
                    &[RefreshFailed],
                ),
                (
                    4,
                    failed(Status::AuthenticationError, "query.http"),
                    &[AuthRequired],
                ),
                (6, ready(30.0), &[RefreshRecovered]),
            ],
        ),
        (
            "a failed first reading is the baseline; the first success sets the quota one",
            vec![
                (0, failed(Status::Error, "query.transport"), &[]),
                (2, ready(80.0), &[RefreshRecovered]),
                (4, ready(92.0), &[QuotaThresholdCrossed]),
            ],
        ),
        (
            "a crossing during a failure shows on recovery",
            vec![
                (0, ready(50.0), &[]),
                (2, failed(Status::Error, "query.schema"), &[RefreshFailed]),
                (4, ready(80.0), &[RefreshRecovered, QuotaThresholdCrossed]),
            ],
        ),
    ];
    for (case, steps) in cases {
        let mut watch = Watch::new();
        for (minute, reading, expected) in steps {
            let events = watch.read(minute, reading);
            assert_eq!(transitions(&events), expected, "{case}, minute {minute}");
        }
    }
}

/// PRD 0023 RF5: `overage_started` on the alert edge, `overage_ended` when
/// an active extra usage goes back to armed or a new cycle starts empty.
#[test]
fn extra_usage_fires_started_and_ended_on_its_edges() {
    let mut watch = Watch::new();
    assert!(watch.read(0, with_extra(Some(0.0), cycle_end())).is_empty());
    let started = watch.read(5, with_extra(Some(12.5), cycle_end()));
    assert_eq!(transitions(&started), [OverageStarted]);
    let event = &started[0];
    assert!(event.metric.is_none());
    assert_eq!(event.current.overage_state, Some(OverageState::Active));
    assert_eq!(event.current.spent, Some(12.5));
    assert_eq!(event.current.cap, Some(50.0));
    assert_eq!(event.current.unit.as_deref(), Some("unid. provedor"));
    assert_eq!(event.current.currency, None);
    assert_eq!(event.current.cycle_ends_at, Some(cycle_end()));
    let before = event.previous.as_ref().unwrap();
    assert_eq!(before.overage_state, Some(OverageState::Armed));
    assert_eq!(before.spent, Some(0.0));
    // Persisting or oscillating inside the cycle fires neither again.
    assert!(
        !transitions(&watch.read(8, with_extra(Some(13.0), cycle_end()))).contains(&OverageStarted)
    );
    // A new cycle with nothing spent ends it; the next spend starts again.
    let next_cycle = cycle_end() + TimeDelta::days(30);
    let ended = watch.read(40, with_extra(Some(0.0), next_cycle));
    assert!(transitions(&ended).contains(&OverageEnded));
    let ended = ended.iter().find(|e| e.event == OverageEnded).unwrap();
    assert_eq!(ended.current.overage_state, Some(OverageState::Armed));
    assert_eq!(
        ended.previous.as_ref().unwrap().overage_state,
        Some(OverageState::Active)
    );
    let again = watch.read(50, with_extra(Some(1.0), next_cycle));
    assert!(transitions(&again).contains(&OverageStarted));
}

#[test]
fn a_baseline_in_extra_usage_or_a_missing_amount_fires_nothing() {
    let mut watch = Watch::new();
    // Already active when the engine starts: only the baseline.
    assert!(watch.read(0, with_extra(Some(9.0), cycle_end())).is_empty());
    assert!(
        !transitions(&watch.read(5, with_extra(Some(9.5), cycle_end()))).contains(&OverageStarted)
    );
    // A missing amount is unknown, neither started nor ended.
    let unknown = watch.read(10, with_extra(None, cycle_end()));
    assert!(!transitions(&unknown).contains(&OverageEnded));
    assert!(unknown.iter().all(|e| e.current.spent.is_none()));
}

#[test]
fn an_export_without_a_new_reading_fires_nothing() {
    let mut watch = Watch::new();
    watch.read(0, ready(50.0));
    watch.read(2, ready(80.0));
    assert!(watch.observe(3).is_empty(), "same report");
    assert!(watch.observe(4).is_empty(), "still the same report");
}

#[test]
fn a_restarted_engine_or_a_forgotten_account_starts_a_new_baseline() {
    let mut watch = Watch::new();
    watch.read(0, ready(50.0));
    watch.state = EventState::default();
    assert!(watch.read(2, ready(80.0)).is_empty(), "restart: baseline");
    assert_eq!(
        transitions(&watch.read(4, ready(92.0))),
        [QuotaThresholdCrossed]
    );

    watch.state.retain_accounts([sk("claude:Other").as_str()]);
    assert!(
        watch.read(6, ready(100.0)).is_empty(),
        "forgotten: baseline"
    );
    watch.state.retain_accounts([sk("claude:Work").as_str()]);
    assert!(
        watch
            .read(8, failed(Status::Error, "query.transport"))
            .iter()
            .any(|event| event.event == RefreshFailed),
        "kept accounts keep their baseline"
    );
}

/// RF1.4: `usage_updated` after a change, at most once per 600 s, and the
/// change it reports is the one since the last `usage_updated`.
#[test]
fn usage_updates_are_spaced_by_ten_minutes_per_account() {
    let far = at(600);
    let usage = |used: f64| Reading::Ready(vec![quota(used, far)]);
    let used = |values: &Option<Values>| -> Vec<Option<f64>> {
        values
            .iter()
            .flat_map(|values| values.metrics.iter().map(|metric| metric.used))
            .collect()
    };
    let mut watch = Watch::new();
    assert!(watch.read(0, usage(10.0)).is_empty(), "baseline");
    let first = watch.read(2, usage(20.0));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].event, UsageUpdated);
    assert_eq!(used(&first[0].previous), [Some(10.0)]);
    assert_eq!(used(&Some(first[0].current.clone())), [Some(20.0)]);
    assert!(watch.read(5, usage(30.0)).is_empty(), "3 min later");
    assert!(watch.read(11, usage(31.0)).is_empty(), "9 min later");
    let second = watch.read(12, usage(32.0));
    assert_eq!(second.len(), 1, "exactly 600 s later");
    assert_eq!(used(&second[0].previous), [Some(20.0)]);
    assert_eq!(used(&Some(second[0].current.clone())), [Some(32.0)]);
    assert!(watch.read(30, usage(32.0)).is_empty(), "unchanged");
    let drifted = Reading::Ready(vec![quota(32.0, far + TimeDelta::minutes(1))]);
    assert!(watch.read(40, drifted).is_empty(), "only resets_at drifted");
}

/// RF1.2: `provider_incident` on a change of the known public status; an
/// unreadable or stale status is no transition.
#[test]
fn provider_incidents_fire_on_known_status_changes() {
    let mut watch = Watch::new();
    watch.status(IncidentState::Operational, false);
    assert!(watch.read(0, ready(10.0)).is_empty(), "baseline");
    watch.status(IncidentState::Degraded, false);
    let degraded = watch.observe(1);
    assert_eq!(transitions(&degraded), [ProviderIncident]);
    assert_eq!(degraded[0].current.incident, Some(IncidentState::Degraded));
    assert_eq!(
        degraded[0].previous.as_ref().unwrap().incident,
        Some(IncidentState::Operational)
    );
    assert!(watch.observe(2).is_empty(), "persists");
    watch.status(IncidentState::Unknown, true);
    assert!(watch.observe(3).is_empty(), "unreadable");
    watch.status(IncidentState::Operational, true);
    assert!(watch.observe(4).is_empty(), "stale");
    watch.statuses.clear();
    assert!(watch.observe(5).is_empty(), "status queries off");
    watch.status(IncidentState::Operational, false);
    assert_eq!(transitions(&watch.observe(6)), [ProviderIncident]);
}

#[test]
fn failures_are_classified_by_status_and_note_id() {
    let report = |status: Status, id: &'static str| Report {
        account_id: None,
        diagnostic: None,
        account: "Work".into(),
        provider: Provider::Claude,
        checked_at: t0(),
        status,
        metrics: Vec::new(),
        notes: Vec::new(),
        notes_l10n: vec![Localized::fixed(id, "x")],
        source: String::new(),
        session_expires_at: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
    };
    use FailureCategory as C;
    let cases = [
        (Status::Ready, "query.timeout", None),
        (Status::Error, "query.timeout", Some(C::Timeout)),
        (Status::Error, "query.transport", Some(C::Offline)),
        (Status::Error, "query.oauth.network", Some(C::Offline)),
        (Status::Error, "query.schema", Some(C::SchemaChanged)),
        (
            Status::Error,
            "extensions.output.invalid",
            Some(C::InvalidOutput),
        ),
        (Status::Error, "query.http", Some(C::ProviderError)),
        (
            Status::AuthenticationError,
            "query.http",
            Some(C::AuthRequired),
        ),
        (
            Status::NeedsConfiguration,
            "query.missing_credential",
            Some(C::AuthRequired),
        ),
        (Status::RateLimited, "query.http", Some(C::RateLimited)),
        (Status::Unsupported, "query.x", Some(C::ProviderError)),
    ];
    for (status, id, expected) in cases {
        assert_eq!(
            FailureCategory::of(&report(status, id)),
            expected,
            "{status:?} {id}"
        );
    }
}

/// RF1.3: the payload's shape for a metric event and an account event.
#[test]
fn payloads_carry_the_common_fields_values_and_previous() {
    let mut watch = Watch::new();
    watch.read(0, ready(50.0));
    let crossed = watch.read(2, ready(80.0));
    assert_eq!(
        serde_json::to_value(&crossed[0]).unwrap(),
        json!({
            "schema": 1,
            "event": "quota_threshold_crossed",
            "occurred_at": "2026-10-20T12:02:00Z",
            "account_key": "claude:Work",
            "account_id": sk("claude:Work"),
            "account_name": "Work",
            "provider": "claude",
            "metric": { "label": LABEL, "window": "window_5h" },
            "current": {
                "threshold": 75,
                "used": 80.0,
                "limit": 100.0,
                "unit": "%",
                "used_ratio": 0.8,
                "resets_at": "2026-10-20T12:20:00Z"
            },
            "previous": {
                "used": 50.0,
                "limit": 100.0,
                "unit": "%",
                "used_ratio": 0.5,
                "resets_at": "2026-10-20T12:20:00Z"
            }
        })
    );
    let failed = watch.read(4, failed(Status::Error, "query.timeout"));
    assert_eq!(
        serde_json::to_value(&failed[0]).unwrap(),
        json!({
            "schema": 1,
            "event": "refresh_failed",
            "occurred_at": "2026-10-20T12:04:00Z",
            "account_key": "claude:Work",
            "account_id": sk("claude:Work"),
            "account_name": "Work",
            "provider": "claude",
            "metric": null,
            "current": { "status": "error", "category": "timeout" },
            "previous": { "status": "ready" }
        })
    );
    let blocked = watch.read(6, ready(100.0));
    let blocked = blocked.iter().find(|e| e.event == AccountBlocked).unwrap();
    assert_eq!(blocked.current.blocked_until, Some(first_window()));
    assert_eq!(blocked.current.used_ratio, Some(1.0));
}

/// RF1.3 acceptance: no payload carries credentials, credential variable
/// names, e-mails, URLs, notes or raw messages, across every event kind.
#[test]
fn no_payload_carries_credentials_urls_or_raw_messages() {
    let mut account = Account {
        id: None,
        name: "Work".into(),
        provider: Provider::Claude,
        credential_env: Some("CLAUDE_SECRET_KEY".into()),
        ..Account::default()
    };
    account
        .params
        .insert("email".into(), "person@example.com".into());
    let mut watch = Watch::with_account(account);
    watch.source = "https://api.example.com/usage?token=sk-live-123".into();
    let raw = |status| {
        Reading::Failed(
            status,
            Localized::with_params(
                "query.http",
                params([("code", ParamValue::from(503u16))]),
                "HTTP 503 em https://api.example.com/v1?key=sk-live-123 corpo {\"error\":\"boom\"} CLAUDE_SECRET_KEY person@example.com",
            ),
        )
    };
    watch.status(IncidentState::Operational, false);
    let mut events = watch.read(0, ready(50.0));
    events.extend(watch.read(5, ready(80.0)));
    events.extend(watch.read(10, ready(100.0)));
    events.extend(watch.read(12, raw(Status::Error)));
    events.extend(watch.read(14, raw(Status::AuthenticationError)));
    watch.status(IncidentState::MajorOutage, false);
    events.extend(watch.read(25, renewed(5.0)));
    events.extend(watch.read(30, with_extra(Some(0.0), cycle_end())));
    events.extend(watch.read(35, with_extra(Some(3.5), cycle_end())));
    events.extend(watch.read(40, with_extra(Some(0.0), cycle_end() + TimeDelta::days(30))));

    let kinds: BTreeSet<EventKind> = events.iter().map(|event| event.event).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([
            QuotaThresholdCrossed,
            QuotaExhausted,
            QuotaRecovered,
            QuotaReset,
            AccountBlocked,
            AccountUnblocked,
            RefreshFailed,
            RefreshRecovered,
            AuthRequired,
            ProviderIncident,
            UsageUpdated,
            OverageStarted,
            OverageEnded,
        ]),
        "every event kind is covered (the PRD 0031 RF7 ones have their own tests)"
    );
    for event in &events {
        let text = serde_json::to_string(event).unwrap();
        for leak in [
            "http",
            "example.com",
            "sk-live",
            "CLAUDE_SECRET_KEY",
            "person@",
            "boom",
            "HTTP 503",
            "Elevated errors",
            "status.claude.com",
        ] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
    }
    let failure = events.iter().find(|e| e.event == RefreshFailed).unwrap();
    assert_eq!(
        failure.current.category,
        Some(FailureCategory::ProviderError)
    );
}

/// PRD 0013 RF2.3: the readable line of every event and category is
/// translated (no event falls back to Portuguese) and fully interpolated.
#[test]
fn every_summary_reads_in_each_catalog() {
    let reading = MetricReading {
        label: LABEL.into(),
        window: Some(MetricWindow::Window5h),
        used: Some(42.0),
        limit: Some(100.0),
        remaining: Some(58.0),
        unit: "%".into(),
        used_ratio: Some(0.42),
        resets_at: None,
    };
    for kind in EventKind::ALL {
        let event = Event {
            schema: SCHEMA,
            event: kind,
            occurred_at: t0(),
            account_key: "claude:Work".into(),
            account_id: "acc_test00000000".into(),
            account_name: "Work".into(),
            provider: Provider::Claude,
            accounts: Vec::new(),
            parent: None,
            metric: Some(MetricRef {
                label: LABEL.into(),
                window: Some(MetricWindow::Window5h),
            }),
            current: Values {
                threshold: Some(90),
                category: Some(FailureCategory::Timeout),
                incident: Some(IncidentState::Degraded),
                metrics: vec![reading.clone()],
                ..Values::default()
            },
            previous: None,
            sample: false,
        };
        let summary = event.summary();
        assert!(!summary.fallback.contains('{'), "{kind:?}");
        for locale in ["en-US", "es", "it"] {
            let text = summary.resolve(locale);
            assert_ne!(text, summary.fallback, "{kind:?} in {locale}");
            assert!(!text.contains('{'), "{text}");
        }
    }
    assert_eq!(
        Event {
            schema: SCHEMA,
            event: UsageUpdated,
            occurred_at: t0(),
            account_key: "claude:Work".into(),
            account_id: "acc_test00000000".into(),
            account_name: "Work".into(),
            provider: Provider::Claude,
            accounts: Vec::new(),
            parent: None,
            metric: None,
            current: Values {
                metrics: vec![reading],
                ..Values::default()
            },
            previous: None,
            sample: false,
        }
        .summary()
        .resolve("en-US"),
        format!("Usage updated: {LABEL} 42%")
    );
    for category in [
        FailureCategory::Timeout,
        FailureCategory::Offline,
        FailureCategory::AuthRequired,
        FailureCategory::RateLimited,
        FailureCategory::SchemaChanged,
        FailureCategory::InvalidOutput,
        FailureCategory::ProviderError,
    ] {
        let label = category.label();
        assert_eq!(
            serde_json::to_value(category).unwrap(),
            serde_json::Value::from(category.name())
        );
        for locale in ["en-US", "es", "it"] {
            assert_ne!(label.resolve(locale), label.fallback, "{category:?}");
        }
    }
}

fn sk(key: &str) -> String {
    crate::AccountId::derive(key, None).to_string()
}

/// PRD 0031 RF8.2: `Unknown` is tracked. A quiet state that goes unknown
/// and back is silent; an incident whose source is lost resolves once with
/// `unknown`, and the way back is silent again.
#[test]
fn an_incident_that_ends_with_the_source_lost_resolves_once() {
    let mut watch = Watch::new();
    watch.status(IncidentState::Operational, false);
    assert!(watch.read(0, ready(10.0)).is_empty(), "baseline");
    watch.status(IncidentState::Unknown, false);
    assert!(watch.observe(1).is_empty(), "operational to unknown");
    watch.status(IncidentState::Operational, false);
    assert!(watch.observe(2).is_empty(), "and back");

    watch.status(IncidentState::MajorOutage, false);
    assert_eq!(transitions(&watch.observe(3)), [ProviderIncident]);
    watch.status(IncidentState::Unknown, false);
    let resolved = watch.observe(4);
    assert_eq!(transitions(&resolved), [ProviderIncident]);
    assert_eq!(resolved[0].current.incident, Some(IncidentState::Unknown));
    assert_eq!(
        resolved[0].previous.as_ref().unwrap().incident,
        Some(IncidentState::MajorOutage)
    );
    assert!(watch.observe(5).is_empty(), "persists");
    watch.status(IncidentState::Operational, false);
    assert!(watch.observe(6).is_empty(), "already resolved");
    watch.status(IncidentState::Degraded, false);
    assert_eq!(transitions(&watch.observe(7)), [ProviderIncident]);
}

/// PRD 0031 RF8.2: a provider without a status source (an `unknown` that
/// was never read, as AWS or xAI) emits nothing at all.
#[test]
fn a_provider_without_a_status_source_emits_nothing() {
    let mut watch = Watch::new();
    watch.status(IncidentState::Unknown, false);
    watch.statuses[0].read_at = None;
    assert!(watch.read(0, ready(10.0)).is_empty());
    for minute in 1..4 {
        assert!(watch.observe(minute).is_empty());
    }
    watch.status(IncidentState::Operational, false);
    assert!(watch.observe(5).is_empty(), "the first read is a baseline");
}

/// PRD 0031 RF8.1: two accounts of one provider share one incident, so one
/// event names the first and lists both; a second provider is separate.
#[test]
fn accounts_of_one_provider_share_one_incident_event() {
    let account = |name: &str, provider| Account {
        name: name.into(),
        provider,
        ..Account::default()
    };
    let config = Config {
        accounts: vec![
            account("Work", Provider::Claude),
            account("Home", Provider::Claude),
            account("Other", Provider::Kimi),
        ],
        ..Config::default()
    };
    let cycle = RefreshCycle::new(&config, Instant::now());
    let identity = ConfigIdentity::Explicit("events".into());
    let status = |state: IncidentState| {
        [Provider::Claude, Provider::Kimi]
            .into_iter()
            .map(|provider| ProviderStatus {
                provider,
                state: if provider == Provider::Claude {
                    state
                } else {
                    IncidentState::Operational
                },
                incident: None,
                page_url: format!("https://status.example.com/{}", provider.slug()),
                read_at: Some(t0()),
                stale: false,
                expired: false,
            })
            .collect::<Vec<_>>()
    };
    let observe = |watcher: &mut crate::watch::Watcher, state, minute| {
        let export = Export::from_cycle(
            &config,
            &identity,
            &cycle,
            &status(state),
            at(minute),
            Options::default(),
        );
        watcher.observe(&export, at(minute))
    };
    let mut watcher = crate::watch::Watcher::new(EventState::default(), None);
    assert!(observe(&mut watcher, IncidentState::Operational, 0).is_empty());
    let events = observe(&mut watcher, IncidentState::MajorOutage, 1);
    assert_eq!(transitions(&events), [ProviderIncident]);
    assert_eq!(events[0].account_key, "claude:Work");
    assert_eq!(events[0].accounts, ["claude:Work", "claude:Home"]);
    assert!(observe(&mut watcher, IncidentState::MajorOutage, 2).is_empty());
    let resolved = observe(&mut watcher, IncidentState::Operational, 3);
    assert_eq!(transitions(&resolved), [ProviderIncident]);
}

/// PRD 0024 RF5: `quota_exhaustion_projected` on the RF2 edge, only with
/// the readings' pace input, with the projection's values and no notes.
#[test]
fn a_steep_recent_pace_projects_exhaustion_once() {
    let run = |with_input: bool| {
        let mut watch = Watch::new();
        let mut points = Vec::new();
        let mut out = Vec::new();
        for (i, minute) in [140, 145, 150, 155, 160].into_iter().enumerate() {
            let used = 40.0 + 5.0 * i as f64;
            points.push(crate::pace::Point {
                taken_at: at(minute),
                used,
            });
            let report = Report {
                overage_hint: Default::default(),
                as_of: Default::default(),
                sub_reports: Vec::new(),
                diagnostic: Default::default(),
                account_id: None,
                account: "Work".into(),
                provider: Provider::Claude,
                checked_at: at(minute),
                status: Status::Ready,
                metrics: vec![quota(used, second_window())],
                notes: Vec::new(),
                notes_l10n: Vec::new(),
                source: String::new(),
                session_expires_at: None,
            };
            let token = watch.cycle.begin(0).unwrap();
            watch
                .cycle
                .finish_account(0, Some(report), token, Instant::now());
            let next = export(&watch.config, &watch.cycle, &[], at(minute));
            let lists = vec![points.clone()];
            let input = PaceInput {
                interval: std::time::Duration::from_secs(300),
                recent: &lists,
            };
            let events = diff_with(
                &watch.prev,
                &next,
                &mut watch.state,
                at(minute),
                with_input.then_some(&input),
            );
            watch.prev = next;
            out.push(
                events
                    .into_iter()
                    .filter(|event| event.event == QuotaExhaustionProjected)
                    .collect::<Vec<_>>(),
            );
        }
        out
    };
    assert!(run(false).iter().all(Vec::is_empty));
    let fired = run(true);
    let counts: Vec<usize> = fired.iter().map(Vec::len).collect();
    assert_eq!(counts, vec![0, 0, 0, 1, 0]);
    let event = serde_json::to_value(&fired[3][0]).unwrap();
    assert_eq!(event["event"], "quota_exhaustion_projected");
    assert_eq!(event["current"]["lead_minutes"], 60);
    assert_eq!(event["current"]["source"], "recent");
    assert_eq!(event["current"]["used_ratio"], 0.55);
    assert!(event["current"]["projected_at"].is_string());
    assert!(event["current"]["resets_at"].is_string());
    assert!(event["current"].get("used").is_none());
    assert!(event["previous"].is_null());
    assert_eq!(
        EventKind::from_name("quota_exhaustion_projected"),
        Some(QuotaExhaustionProjected)
    );
}

/// PRD 0031 RF7.1: `session_expiring` once per account and expiry date,
/// with the date and the days left, never the session itself.
#[test]
fn a_session_nearing_its_expiry_fires_once_per_expiry_date() {
    let mut watch = Watch::new();
    watch.session_expires_at = Some(t0() + TimeDelta::days(10));
    assert!(transitions(&watch.read(0, ready(10.0))).is_empty());
    assert!(
        transitions(&watch.read(2, ready(10.0))).is_empty(),
        "still far"
    );

    let near = t0() + TimeDelta::days(2) + TimeDelta::hours(1);
    watch.session_expires_at = Some(near);
    let events = watch.read(4, ready(10.0));
    assert_eq!(transitions(&events), [SessionExpiring]);
    assert_eq!(events[0].current.expires_at, Some(near));
    assert_eq!(events[0].current.days_left, Some(2));
    assert!(events[0].metric.is_none() && events[0].previous.is_none());
    let json = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(json["event"], "session_expiring");
    assert_eq!(json["current"]["days_left"], 2);

    assert!(
        transitions(&watch.read(6, ready(10.0))).is_empty(),
        "same date"
    );
    watch.session_expires_at = Some(t0() + TimeDelta::days(1));
    assert_eq!(
        transitions(&watch.read(8, ready(10.0))),
        [SessionExpiring],
        "a pasted session with a new date is noticed again"
    );
}

/// RF7.1: a restart that already sees a near expiry is a baseline, not a
/// repeat.
#[test]
fn a_restarted_engine_does_not_repeat_a_near_expiry() {
    let mut watch = Watch::new();
    watch.session_expires_at = Some(t0() + TimeDelta::days(1));
    assert!(transitions(&watch.read(0, ready(10.0))).is_empty());
    assert!(transitions(&watch.read(2, ready(10.0))).is_empty());
}

/// PRD 0031 RF7.2: `quota_reset_soon` once per window instance inside the
/// lead, naming the window and its reset.
#[test]
fn a_window_about_to_reset_fires_once_per_instance() {
    let mut watch = Watch::new();
    watch.state = EventState::default().with_reset_lead(TimeDelta::minutes(15));
    // The window ends at minute 20; the lead opens at minute 5.
    assert!(
        transitions(&watch.read(0, ready(30.0))).is_empty(),
        "baseline"
    );
    assert!(
        transitions(&watch.read(3, ready(30.0))).is_empty(),
        "outside the lead"
    );
    let events = watch.read(6, ready(30.0));
    assert_eq!(transitions(&events), [QuotaResetSoon]);
    assert_eq!(events[0].current.resets_at, Some(first_window()));
    let metric = events[0].metric.as_ref().unwrap();
    assert_eq!(metric.label, LABEL);
    assert_eq!(metric.window, Some(MetricWindow::Window5h));
    assert!(
        transitions(&watch.read(9, ready(30.0))).is_empty(),
        "once per instance"
    );
    // A drifting `resets_at` is the same instance.
    let drift = Reading::Ready(vec![quota(30.0, first_window() + TimeDelta::minutes(1))]);
    assert!(transitions(&watch.read(12, drift)).is_empty());
    assert_eq!(transitions(&watch.read(25, renewed(0.0))), [QuotaReset]);
    // The next window opens its own lead.
    assert!(transitions(&watch.read(30, renewed(0.0))).is_empty());
    let late = 20 + 5 * 60 - 10;
    assert_eq!(
        transitions(&watch.read(late, renewed(5.0))),
        [QuotaResetSoon]
    );
}

/// RF7.2: a restart inside the lead and an idle quota report nothing.
#[test]
fn reset_soon_ignores_a_restart_inside_the_lead_and_idle_quotas() {
    let mut watch = Watch::new();
    watch.state = EventState::default().with_reset_lead(TimeDelta::minutes(15));
    assert!(
        transitions(&watch.read(10, ready(30.0))).is_empty(),
        "baseline inside the lead"
    );
    assert!(
        transitions(&watch.read(12, ready(30.0))).is_empty(),
        "not repeated"
    );

    let mut idle = Watch::new();
    idle.state = EventState::default().with_reset_lead(TimeDelta::minutes(15));
    idle.read(0, ready(0.0));
    assert!(
        transitions(&idle.read(10, ready(0.0))).is_empty(),
        "nothing consumed"
    );
}

#[test]
fn the_new_events_are_known_by_name() {
    assert_eq!(
        EventKind::from_name("session_expiring"),
        Some(SessionExpiring)
    );
    assert_eq!(
        EventKind::from_name("quota_reset_soon"),
        Some(QuotaResetSoon)
    );
    assert_eq!(EventKind::ALL.len(), 16);
}
