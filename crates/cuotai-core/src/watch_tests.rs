//! PRD 0013 RF2: the watch engine over the headless driver, against a local
//! `wiremock` Groq whose answers change between readings, with an
//! accelerated clock (each round jumps the cycle's `Instant` and the
//! readings' `checked_at` by one interval instead of waiting for it).

use super::*;
use crate::config_file::ConfigIdentity;
use crate::headless::Driver;
use crate::{oauth, Account, Credentials, Monitor};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Keys;
impl Credentials for Keys {
    fn get(&self, name: &str) -> Option<String> {
        (name == "GROQ_KEY").then(|| "gsk_test".to_owned())
    }
}

struct NoTokens;
impl oauth::TokenStore for NoTokens {
    fn load(&self, _: &str) -> Result<Option<oauth::OAuthToken>, oauth::OAuthError> {
        Ok(None)
    }
    fn save(&self, _: &str, _: &oauth::OAuthToken) -> Result<(), oauth::OAuthError> {
        unreachable!("watch tests never save tokens")
    }
    fn delete(&self, _: &str) -> Result<(), oauth::OAuthError> {
        unreachable!("watch tests never delete tokens")
    }
}

/// The answer the mock gives now; the test swaps it between readings.
#[derive(Clone)]
struct Answer(Arc<parking_lot::Mutex<wiremock::ResponseTemplate>>);

impl wiremock::Respond for Answer {
    fn respond(&self, _: &wiremock::Request) -> wiremock::ResponseTemplate {
        self.0.lock().clone()
    }
}

impl Answer {
    fn set(&self, template: wiremock::ResponseTemplate) {
        *self.0.lock() = template;
    }
}

fn remaining(requests: u32) -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({ "data": [] }))
        .insert_header("x-ratelimit-limit-requests", "14400")
        .insert_header("x-ratelimit-remaining-requests", requests.to_string())
        .insert_header("x-ratelimit-reset-requests", "59s")
}

fn t0() -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
}

struct Rig {
    server: wiremock::MockServer,
    answer: Answer,
    config: Config,
    identity: ConfigIdentity,
    clock: Arc<parking_lot::Mutex<DateTime<Utc>>>,
    driver: Driver,
    all: Watcher,
    failures_only: Watcher,
    start: Instant,
    /// PRD 0013 RF3: runs the events of `all`, when set.
    hooks: Option<crate::hooks::Runner>,
}

impl Rig {
    async fn new() -> Self {
        let server = wiremock::MockServer::start().await;
        let answer = Answer(Arc::new(parking_lot::Mutex::new(remaining(14_370))));
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(answer.clone())
            .mount(&server)
            .await;
        let config = Config::from_toml(
            "refresh_seconds = 60\n\n[[accounts]]\nname = \"alpha\"\nprovider = \"groq\"\ncredential_env = \"GROQ_KEY\"\n",
        )
        .unwrap();
        let creds: Arc<dyn Credentials> = Arc::new(Keys);
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::new(NoTokens), Arc::clone(&creds)).unwrap());
        let mut monitor = Monitor::with_oauth(config.clone(), creds, session).unwrap();
        let clock = Arc::new(parking_lot::Mutex::new(t0()));
        monitor.groq_url = Some(server.uri());
        monitor.clock = Some(Arc::clone(&clock));
        let start = Instant::now();
        let driver = Driver::new(&config, Arc::new(monitor), start);
        let failures = parse_events(["refresh_failed"]).unwrap();
        Self {
            server,
            answer,
            config,
            identity: ConfigIdentity::Explicit("watch".into()),
            clock,
            driver,
            all: Watcher::new(EventState::default(), None),
            failures_only: Watcher::new(EventState::default(), Some(failures)),
            start,
            hooks: None,
        }
    }

    /// Step the driver at `seconds` past the start until its queries
    /// landed; the events of both watchers, by name.
    async fn at(&mut self, seconds: u64) -> (Vec<&'static str>, Vec<&'static str>) {
        let now = self.start + Duration::from_secs(seconds);
        let wall = t0() + chrono::TimeDelta::seconds(seconds as i64);
        *self.clock.lock() = wall;
        let mut changed = self.driver.step(now).await;
        while !self.driver.is_idle() {
            tokio::time::sleep(Duration::from_millis(5)).await;
            changed |= self.driver.step(now).await;
        }
        if !changed {
            return (Vec::new(), Vec::new());
        }
        let export = self.driver.export(&self.config, &self.identity, wall);
        let names = |events: Vec<Event>| events.into_iter().map(|e| e.event.name()).collect();
        let all = self.all.observe(&export, wall);
        if let Some(hooks) = &self.hooks {
            for event in &all {
                assert_eq!(hooks.submit(event), 0);
            }
        }
        (names(all), names(self.failures_only.observe(&export, wall)))
    }

    async fn queries(&self) -> usize {
        self.server.received_requests().await.unwrap().len()
    }
}

#[tokio::test]
async fn one_event_per_real_transition_over_an_accelerated_clock() {
    let mut rig = Rig::new().await;
    let none = Vec::<&str>::new();

    // The first reading is the baseline.
    assert_eq!(rig.at(0).await, (none.clone(), none.clone()));
    assert_eq!(rig.queries().await, 1);
    // Not due before the account's interval: no query, nothing to judge.
    assert_eq!(rig.at(30).await, (none.clone(), none.clone()));
    assert_eq!(rig.queries().await, 1);
    // The same values again: no event.
    assert_eq!(rig.at(60).await, (none.clone(), none.clone()));

    // A failure fires once and does not repeat while it persists.
    rig.answer.set(wiremock::ResponseTemplate::new(500));
    assert_eq!(
        rig.at(120).await,
        (vec!["refresh_failed"], vec!["refresh_failed"])
    );
    assert_eq!(rig.at(180).await, (none.clone(), none.clone()));

    // Recovery with new values: recovered and usage, once.
    rig.answer.set(remaining(13_000));
    assert_eq!(
        rig.at(240).await,
        (vec!["refresh_recovered", "usage_updated"], none.clone())
    );
    // New values within 600 s of the last usage event stay silent...
    rig.answer.set(remaining(12_000));
    for seconds in [300, 360, 420, 480, 540, 600, 660, 720, 780] {
        assert_eq!(
            rig.at(seconds).await,
            (none.clone(), none.clone()),
            "{seconds}"
        );
    }
    // ...and are reported once 600 s passed.
    assert_eq!(rig.at(840).await, (vec!["usage_updated"], none.clone()));

    // Refused credentials: a failure that requires authentication.
    rig.answer.set(wiremock::ResponseTemplate::new(401));
    assert_eq!(
        rig.at(900).await,
        (
            vec!["refresh_failed", "auth_required"],
            vec!["refresh_failed"]
        )
    );
    assert!(rig.driver.failing(), "the account ends in failure");

    // One query per due reading, none for watching (rounds at 0, 60 … 900
    // except 30).
    assert_eq!(rig.queries().await, 16);
}

fn account(name: &str, provider: Provider, refresh_seconds: Option<u64>) -> Account {
    let mut account = Account {
        name: name.into(),
        provider,
        refresh_seconds,
        ..Account::default()
    };
    if provider == Provider::AwsBedrock {
        account.params.insert("profile".into(), "dev".into());
    }
    account
}

#[test]
fn interval_is_bounded_and_respects_overrides_and_the_metered_floor() {
    let config = Config {
        refresh_seconds: 600,
        accounts: vec![
            account("plain", Provider::Groq, None),
            account("own", Provider::Groq, Some(45)),
            account("bedrock", Provider::AwsBedrock, None),
            account("bedrock-own", Provider::AwsBedrock, Some(3_600)),
        ],
        ..Config::default()
    };
    config.validate().unwrap();

    for rejected in [0, 59, MAX_REFRESH_SECONDS + 1] {
        let error = with_interval(&config, rejected).unwrap_err();
        assert_eq!(error.id, "watch.error.interval_range", "{rejected}");
    }

    let effective = |config: &Config| -> Vec<u64> {
        config
            .accounts
            .iter()
            .map(|a| a.effective_refresh_seconds(config.refresh_seconds))
            .collect()
    };
    let fast = with_interval(&config, 60).unwrap();
    fast.validate().unwrap();
    assert_eq!(effective(&fast), [60, 45, 300, 3_600]);
    let slow = with_interval(&config, 900).unwrap();
    assert_eq!(effective(&slow), [900, 45, 900, 3_600]);
}

#[test]
fn event_selection_accepts_the_payload_names_only() {
    for kind in EventKind::ALL {
        assert_eq!(
            serde_json::to_value(kind).unwrap(),
            serde_json::Value::from(kind.name())
        );
    }
    assert_eq!(
        parse_events(["quota_exhausted", " refresh_failed", "quota_exhausted"]).unwrap(),
        BTreeSet::from([EventKind::QuotaExhausted, EventKind::RefreshFailed])
    );
    for wrong in ["quota-exhausted", "", "QUOTA_EXHAUSTED"] {
        let error = parse_events([wrong]).unwrap_err();
        assert_eq!(error.id, "watch.error.unknown_event");
        assert!(error.resolve("en-US").contains("usage_updated"), "{wrong}");
    }
}

/// PRD 0013 RF3 over the real cycle: a hook gets exactly one run per real
/// transition of its events (none for the baseline or a failure that
/// persists), and a hook that fails changes neither the events nor the
/// queries.
#[cfg(unix)]
#[tokio::test]
async fn a_hook_runs_once_per_transition_and_its_failure_changes_nothing() {
    use crate::hooks::{Consent, Hooks, OnRun, Run, Runner};
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen");
    let script = dir.path().join("hook.sh");
    std::fs::write(
        &script,
        format!(
            "printf '%s %s\\n' \"$CUOTAI_EVENT\" \"$CUOTAI_ACCOUNT\" >> '{}'\nexit 7\n",
            seen.display()
        ),
    )
    .unwrap();
    let hooks = Hooks::from_toml(
        dir.path().join("hooks.toml"),
        &format!(
            "[[hooks]]\nname = \"seen\"\nevents = [\"refresh_failed\", \"refresh_recovered\"]\ncommand = [\"/bin/sh\", {:?}]\n",
            script.display().to_string()
        ),
    )
    .unwrap();
    let confirmed = Consent::at(dir.path().join("consent.json"))
        .confirm(hooks)
        .unwrap();
    let (sender, mut runs) = tokio::sync::mpsc::unbounded_channel();
    let on_run: OnRun = Arc::new(move |run: &Run, _: Option<&std::io::Error>| {
        let _ = sender.send(run.clone());
    });
    let mut rig = Rig::new().await;
    rig.hooks = Some(Runner::start(confirmed, None, on_run));
    let none = Vec::<&str>::new();

    assert_eq!(rig.at(0).await.0, none, "baseline");
    rig.answer.set(wiremock::ResponseTemplate::new(500));
    assert_eq!(rig.at(60).await.0, ["refresh_failed"]);
    assert_eq!(rig.at(120).await.0, none, "the failure persists");
    rig.answer.set(remaining(13_000));
    assert_eq!(rig.at(180).await.0, ["refresh_recovered", "usage_updated"]);
    for _ in 0..2 {
        let run = tokio::time::timeout(Duration::from_secs(20), runs.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.exit_code, Some(7));
    }
    assert_eq!(rig.at(240).await.0, none);
    assert_eq!(rig.queries().await, 5, "one query per due reading");
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap(),
        "refresh_failed alpha\nrefresh_recovered alpha\n"
    );
    assert!(runs.try_recv().is_err(), "no other run");
}
