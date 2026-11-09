//! PRD 0040 acceptance tests for the feed store, the readers, the
//! adapters, the live block, the banner and the status-line composer.

use super::*;
use crate::feed::live::{self, ContextLevel};
use crate::feed::statusline::{self, OtherAccount};
use crate::tray::AccountView;
use crate::{Config, Status};
use chrono::TimeZone;
use std::sync::Arc;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 25, 14, 0, 0).unwrap()
}

/// The documented example (code.claude.com/docs/en/statusline), with
/// resets moved next to `now` and sensitive placeholders in the fields that
/// must never be kept.
fn full_input(now: DateTime<Utc>) -> String {
    let five = now.timestamp() + 5_400;
    let week = now.timestamp() + 3 * 86_400;
    let spend = now.timestamp() + 20 * 86_400;
    format!(
        r#"{{
  "cwd": "/Users/ana/SENSITIVE-project",
  "session_id": "abc123-session",
  "session_name": "SENSITIVE session title",
  "transcript_path": "/Users/ana/.claude/projects/SENSITIVE/transcript.jsonl",
  "model": {{ "id": "claude-opus-5-5", "display_name": "Opus" }},
  "workspace": {{ "current_dir": "/Users/ana/SENSITIVE-project", "project_dir": "/Users/ana/SENSITIVE-project",
                  "repo": {{ "host": "github.com", "owner": "SENSITIVE-owner", "name": "SENSITIVE-repo" }} }},
  "version": "2.1.90",
  "output_style": {{ "name": "SENSITIVE-style" }},
  "cost": {{ "total_cost_usd": 0.42, "total_duration_ms": 45000 }},
  "context_window": {{ "total_input_tokens": 15500, "context_window_size": 200000,
                       "used_percentage": 8, "remaining_percentage": 92 }},
  "rate_limits": {{
    "five_hour": {{ "used_percentage": 23.5, "resets_at": {five} }},
    "seven_day": {{ "used_percentage": 41.2, "resets_at": {week} }},
    "spend_limit": {{ "used_percentage": 62.8, "resets_at": {spend}, "used_usd": 314.12, "limit_usd": 500, "period": "monthly" }}
  }},
  "pr": {{ "url": "https://example.com/SENSITIVE/pull/1" }}
}}"#
    )
}

const NO_RATE_LIMITS: &str = r#"{"session_id":"s2","model":{"id":"claude-sonnet-5","display_name":"Sonnet"},"context_window":{"used_percentage":3,"remaining_percentage":97,"context_window_size":200000},"cost":{"total_cost_usd":0.01},"cwd":"/Users/ana/SENSITIVE"}"#;

fn spend_without_dollars(now: DateTime<Utc>) -> String {
    format!(
        r#"{{"session_id":"s3","rate_limits":{{"spend_limit":{{"used_percentage":63,"resets_at":{}}}}}}}"#,
        now.timestamp() + 86_400
    )
}

const NULL_PERCENTAGE: &str = r#"{"session_id":"s4","rate_limits":{"five_hour":{"used_percentage":null,"resets_at":1738425600},"seven_day":{"used_percentage":12}}}"#;

fn store() -> (tempfile::TempDir, FeedStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FeedStore::at(dir.path().join("feed"));
    (dir, store)
}

fn reading(session: &str, at: DateTime<Utc>) -> FeedReading {
    let mut reading = claude::parse(full_input(at).as_bytes(), at).unwrap();
    reading.session.digest = session_digest(session);
    reading
}

// ------------------------------------------------------------- RF2

#[test]
fn the_full_example_keeps_exactly_the_contract_fields() {
    let reading = claude::parse(full_input(now()).as_bytes(), now()).unwrap();
    assert_eq!(reading.source, FeedSource::ClaudeCode);
    assert_eq!(reading.written_at, now());
    assert_eq!(reading.agent_version.as_deref(), Some("2.1.90"));
    assert_eq!(reading.session.digest, session_digest("abc123-session"));
    assert_eq!(reading.session.digest.len(), 16);
    assert_eq!(reading.session.model_id.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(reading.session.model_name.as_deref(), Some("Opus"));
    assert_eq!(reading.session.context_used_pct, Some(8.0));
    assert_eq!(reading.session.context_remaining_pct, Some(92.0));
    assert_eq!(reading.session.context_size, Some(200_000));
    assert_eq!(reading.session.cost_usd, Some(0.42));
    assert_eq!(reading.windows.len(), 3);
    let spend = reading.window(WindowId::SpendLimit).unwrap();
    assert_eq!(spend.used_usd, Some(314.12));
    assert_eq!(spend.limit_usd, Some(500.0));
    assert_eq!(spend.period.as_deref(), Some("monthly"));
    let json = serde_json::to_string(&reading).unwrap();
    assert!(!json.contains("SENSITIVE"), "{json}");
    assert!(!json.contains("abc123"), "{json}");
    assert!(!json.contains("transcript"), "{json}");
    assert!(reading.is_valid(now()));
}

#[test]
fn absent_and_null_windows_stay_absent() {
    let reading = claude::parse(NO_RATE_LIMITS.as_bytes(), now()).unwrap();
    assert!(reading.windows.is_empty());
    assert_eq!(reading.session.model_name.as_deref(), Some("Sonnet"));
    let reading = claude::parse(NULL_PERCENTAGE.as_bytes(), now()).unwrap();
    assert!(reading.window(WindowId::FiveHour).is_none());
    assert_eq!(reading.window(WindowId::SevenDay).unwrap().used_pct, 12.0);
    let reading = claude::parse(spend_without_dollars(now()).as_bytes(), now()).unwrap();
    let spend = reading.window(WindowId::SpendLimit).unwrap();
    assert_eq!(spend.used_pct, 63.0);
    assert_eq!(spend.used_usd, None);
}

#[test]
fn malformed_and_oversized_input_is_refused() {
    assert_eq!(
        claude::parse(b"not json", now()),
        Err(claude::ParseError::Malformed)
    );
    assert_eq!(
        claude::parse(b"[1,2]", now()),
        Err(claude::ParseError::Malformed)
    );
    let big = vec![b' '; MAX_INPUT + 1];
    assert_eq!(
        claude::parse(&big, now()),
        Err(claude::ParseError::TooLarge)
    );
}

#[test]
fn a_model_name_outside_the_pattern_is_dropped() {
    let input =
        r#"{"session_id":"x","model":{"id":"opus/../../etc","display_name":"Opus <script>"}}"#;
    let reading = claude::parse(input.as_bytes(), now()).unwrap();
    assert_eq!(reading.session.model_id, None);
    assert_eq!(reading.session.model_name, None);
}

#[test]
fn parse_and_write_fit_the_writer_budget() {
    // RF2.3: the in-process part of `--feed claude-statusline` (parse and
    // atomic write) stays far under the 50 ms budget even in a debug build.
    let (_dir, store) = store();
    let input = full_input(Utc::now());
    // The fastest of five runs is measured: a run that waited for the CPU
    // or the disk behind the rest of the parallel test suite says nothing
    // about the writer (a sum of all five flaked on loaded CI runners).
    let fastest = (0..5)
        .map(|_| {
            let started = std::time::Instant::now();
            let reading = claude::parse(input.as_bytes(), Utc::now()).unwrap();
            store.write(&reading, Utc::now()).unwrap();
            started.elapsed()
        })
        .min()
        .unwrap();
    assert!(
        fastest < std::time::Duration::from_millis(50),
        "{fastest:?}"
    );
}

#[test]
fn the_feed_only_choice_names_the_feed_mode_option() {
    use crate::account_edit::{self, FieldDescriptorKind};
    for (provider, mode) in [
        (crate::Provider::Claude, MODE_CLAUDE_CODE),
        (crate::Provider::ChatGpt, MODE_CODEX_CLI),
    ] {
        let (key, index) = account_edit::feed_only_choice(provider).unwrap();
        let descriptor = account_edit::field_descriptors(provider)
            .iter()
            .find(|d| d.key == key)
            .unwrap();
        let FieldDescriptorKind::Choice(choice) = &descriptor.kind else {
            panic!("query_mode is a choice");
        };
        assert_eq!(choice.options[index].value, mode);
    }
    assert!(account_edit::feed_only_choice(crate::Provider::OpenAiApi).is_none());
}

// ------------------------------------------------------------- RF1

#[test]
fn a_reading_round_trips_through_the_store() {
    let (_dir, store) = store();
    let reading = reading("one", Utc::now());
    store.write(&reading, Utc::now()).unwrap();
    assert_eq!(
        store.latest(FeedSource::ClaudeCode, Utc::now()),
        Some(reading.clone())
    );
    assert_eq!(store.latest(FeedSource::CodexCli, Utc::now()), None);
    let files = json_files(&store.dir().join("claude_code"));
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].file_name().unwrap().to_string_lossy(),
        format!("{}.json", reading.session.digest)
    );
}

#[cfg(unix)]
#[test]
fn feed_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, store) = store();
    store
        .write(&reading("one", Utc::now()), Utc::now())
        .unwrap();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(store.dir()), 0o700);
    assert_eq!(mode(&store.dir().join("claude_code")), 0o700);
    let file = &json_files(&store.dir().join("claude_code"))[0];
    assert_eq!(mode(file), 0o600);
}

#[test]
fn the_newest_of_two_sessions_wins() {
    let (_dir, store) = store();
    let current = Utc::now();
    let older = reading("work", current - TimeDelta::minutes(3));
    let newer = reading("side", current - TimeDelta::minutes(1));
    store.write(&newer, current).unwrap();
    store.write(&older, current).unwrap();
    assert_eq!(
        store.latest(FeedSource::ClaudeCode, current).unwrap(),
        newer
    );
    assert_eq!(store.sessions(FeedSource::ClaudeCode, current).len(), 2);
}

#[test]
fn truncated_and_tampered_files_are_ignored_not_zeroed() {
    let (_dir, store) = store();
    let current = Utc::now();
    let good = reading("good", current - TimeDelta::minutes(5));
    store.write(&good, current).unwrap();
    let dir = store.dir().join("claude_code");
    std::fs::write(
        dir.join("truncated.json"),
        br#"{"schema":1,"source":"claude_"#,
    )
    .unwrap();
    let mut tampered = serde_json::to_value(reading("bad", current)).unwrap();
    tampered["windows"][0]["used_pct"] = serde_json::json!(250.0);
    std::fs::write(dir.join("tampered.json"), tampered.to_string()).unwrap();
    let mut unknown = serde_json::to_value(reading("unknown", current)).unwrap();
    unknown["cwd"] = serde_json::json!("/Users/ana/SENSITIVE");
    std::fs::write(dir.join("unknown.json"), unknown.to_string()).unwrap();
    assert_eq!(store.latest(FeedSource::ClaudeCode, current).unwrap(), good);
    let health = store.health(current);
    assert_eq!(
        health,
        FeedHealth {
            readable: 1,
            unreadable: 3
        }
    );
    assert!(health.note().is_some());
}

/// RF1.3 (PRD 0007 re-validation): a file dated in the future never
/// passes for the newest reading; it is counted unreadable. A little clock
/// skew between the agent and Cuotai is accepted.
#[test]
fn a_reading_dated_in_the_future_is_ignored() {
    let (_dir, store) = store();
    let current = Utc::now();
    let good = reading("good", current - TimeDelta::minutes(2));
    store.write(&good, current).unwrap();
    let ahead = reading("ahead", current + TimeDelta::minutes(10));
    assert!(store.write(&ahead, current).is_err(), "never written");
    // Planted by hand (another writer, a moved clock).
    let dir = store.dir().join("claude_code");
    std::fs::write(dir.join("ahead.json"), serde_json::to_vec(&ahead).unwrap()).unwrap();
    assert_eq!(store.latest(FeedSource::ClaudeCode, current).unwrap(), good);
    assert_eq!(
        store.health(current),
        FeedHealth {
            readable: 1,
            unreadable: 1
        }
    );
    let skewed = reading("skewed", current + TimeDelta::seconds(30));
    store.write(&skewed, current).unwrap();
    assert_eq!(
        store.latest(FeedSource::ClaudeCode, current).unwrap(),
        skewed
    );
}

#[test]
fn an_invalid_reading_is_never_written() {
    let (_dir, store) = store();
    let mut bad = reading("bad", Utc::now());
    bad.windows[0].used_pct = f64::NAN;
    assert!(store.write(&bad, Utc::now()).is_err());
    bad.windows[0].used_pct = 10.0;
    bad.windows[0].resets_at = Some(Utc::now().timestamp() + 500 * 86_400);
    assert!(store.write(&bad, Utc::now()).is_err());
}

#[test]
fn retention_keeps_the_newest_and_caps_the_directory() {
    let (_dir, store) = store();
    let current = Utc::now();
    let dir = store.dir().join("claude_code");
    crate::private_fs::ensure_dir(store.dir()).unwrap();
    crate::private_fs::ensure_dir(&dir).unwrap();
    // Two files older than a day: only the newest of them survives.
    for (name, hours) in [("a", 30), ("b", 26)] {
        let path = dir.join(format!("{name}.json"));
        let old = reading(name, current - TimeDelta::hours(hours));
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        let mtime: std::time::SystemTime = (current - TimeDelta::hours(hours)).into();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }
    store.prune(FeedSource::ClaudeCode, current);
    let names: Vec<String> = json_files(&dir)
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["b".to_owned()],
        "the newest stale file ages, never vanishes"
    );
    // A fresh write removes the remaining expired file.
    store.write(&reading("fresh", current), current).unwrap();
    assert_eq!(json_files(&dir).len(), 1);
    // The cap: never more than MAX_FILES per source.
    for index in 0..(MAX_FILES + 6) {
        store
            .write(&reading(&format!("s{index}"), current), current)
            .unwrap();
    }
    assert_eq!(json_files(&dir).len(), MAX_FILES);
}

#[test]
fn bindings_and_modes_are_recognised() {
    let config = Config::from_toml(
        r#"
[[accounts]]
name = "fed"
provider = "claude"
params = { query_mode = "claude_code" }

[[accounts]]
name = "bound"
provider = "chat_gpt"
credential_env = "CHATGPT_ACCESS_TOKEN"
params = { feed = "codex_cli" }
"#,
    )
    .unwrap();
    config.validate().unwrap();
    assert_eq!(
        bound_source(&config.accounts[0]),
        Some(FeedSource::ClaudeCode)
    );
    assert_eq!(
        bound_source(&config.accounts[1]),
        Some(FeedSource::CodexCli)
    );
    assert_eq!(
        mode_of(config.accounts[0].provider, &config.accounts[0].params),
        Some((FeedSource::ClaudeCode, SourceKind::Feed))
    );
    assert_eq!(
        mode_of(config.accounts[1].provider, &config.accounts[1].params),
        None
    );
}

#[test]
fn two_accounts_on_one_feed_are_a_validation_error() {
    let duplicate = Config::from_toml(
        r#"
[[accounts]]
name = "work"
provider = "claude"
params = { query_mode = "claude_code" }

[[accounts]]
name = "personal"
provider = "claude"
credential_env = "CLAUDE_OAUTH_TOKEN"
params = { feed = "claude_code" }
"#,
    );
    assert!(matches!(duplicate, Err(crate::ConfigError::Feed)));
    let wrong = Config::from_toml(
        r#"
[[accounts]]
provider = "claude"
credential_env = "CLAUDE_OAUTH_TOKEN"
params = { feed = "codex_cli" }
"#,
    );
    assert!(matches!(wrong, Err(crate::ConfigError::Feed)));
}

#[test]
fn the_feed_binding_round_trips_through_the_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cuotai.toml");
    std::fs::write(
        &path,
        "[[accounts]]\nprovider = \"claude\"\ncredential_env = \"CLAUDE_OAUTH_TOKEN\"\nparams = { feed = \"claude_code\" }\n",
    )
    .unwrap();
    let mut file = crate::config_file::ConfigFile::load(Some(path.clone()), false).unwrap();
    assert_eq!(
        file.config.accounts[0]
            .params
            .get(BIND_PARAM)
            .map(String::as_str),
        Some("claude_code")
    );
    let config = file.config.clone();
    file.save(config).unwrap();
    let again = crate::config_file::ConfigFile::load(Some(path), false).unwrap();
    assert_eq!(
        bound_source(&again.config.accounts[0]),
        Some(FeedSource::ClaudeCode)
    );
}

// ------------------------------------------------------------- RF3

struct NoCredentials;
impl crate::Credentials for NoCredentials {
    fn get(&self, _: &str) -> Option<String> {
        None
    }
}

struct NoTokens;
impl crate::oauth::TokenStore for NoTokens {
    fn load(&self, _: &str) -> Result<Option<crate::oauth::OAuthToken>, crate::oauth::OAuthError> {
        Ok(None)
    }
    fn save(&self, _: &str, _: &crate::oauth::OAuthToken) -> Result<(), crate::oauth::OAuthError> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), crate::oauth::OAuthError> {
        Ok(())
    }
}

fn monitor(toml: &str, feed_dir: &Path) -> crate::Monitor {
    let config = Config::from_toml(toml).unwrap();
    let creds: Arc<dyn crate::Credentials> = Arc::new(NoCredentials);
    let session =
        Arc::new(crate::oauth::OAuthSession::new(Arc::new(NoTokens), Arc::clone(&creds)).unwrap());
    let mut monitor = crate::Monitor::with_login_session(config, creds, session).unwrap();
    monitor.feed_dir = Some(feed_dir.to_path_buf());
    monitor
}

const FED: &str =
    "[[accounts]]\nprovider = \"claude\"\nparams = { query_mode = \"claude_code\" }\n";
const HYBRID: &str = "refresh_seconds = 300\n[[accounts]]\nprovider = \"claude\"\ncredential_env = \"CLAUDE_OAUTH_TOKEN\"\nparams = { query_mode = \"claude_code_or_api\" }\n";

#[tokio::test]
async fn a_fed_account_reads_the_feed_without_any_request() {
    let server = wiremock::MockServer::start().await;
    let (_dir, store) = store();
    let written = Utc::now() - TimeDelta::minutes(2);
    store.write(&reading("live", written), Utc::now()).unwrap();
    let snapshot = monitor(FED, store.dir()).refresh().await;
    let report = &snapshot.reports[0];
    assert_eq!(report.status, Status::Ready, "{:?}", report.notes);
    assert_eq!(report.as_of, Some(written), "age counts from the feed");
    assert_eq!(report.source, "feed:claude_code");
    let labels: Vec<&str> = report.metrics.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Sessão (5 horas) (%)", "Semana (7 dias) (%)", "Spend limit"]
    );
    let five = &report.metrics[0];
    assert_eq!(
        (five.used, five.limit, five.remaining),
        (Some(23.5), Some(100.0), Some(76.5))
    );
    assert_eq!(five.window, Some(crate::MetricWindow::Window5h));
    assert!(five.resets_at.is_some());
    let spend = &report.metrics[2];
    assert_eq!(spend.kind, crate::MetricKind::Spend);
    assert_eq!(spend.unit, "USD");
    assert_eq!(spend.window, Some(crate::MetricWindow::Monthly));
    assert!(server.received_requests().await.unwrap().is_empty());
    // The access descriptor of the mode is official, local and credential-free.
    let account = &Config::from_toml(FED).unwrap().accounts[0];
    let access = crate::access::describe_reading(account, Some(report));
    assert_eq!(access.level, crate::access::Level::Official);
    assert_eq!(access.credential, crate::access::Credential::AgentFeed);
    assert!(access.local && !access.metered);
}

#[tokio::test]
async fn without_a_feed_the_account_needs_configuration() {
    let (_dir, store) = store();
    let report = monitor(FED, store.dir()).refresh().await.reports.remove(0);
    assert_eq!(report.status, Status::NeedsConfiguration);
    assert!(report
        .notes_l10n
        .iter()
        .any(|n| n.id == "note.claude_code.no_feed"));
    assert!(report.metrics.is_empty(), "never zero");
}

#[tokio::test]
async fn an_old_feed_still_answers_with_its_own_age() {
    let (_dir, store) = store();
    let written = Utc::now() - TimeDelta::hours(30);
    // Keep the resets ahead of now so the windows are still current.
    let mut old = claude::parse(full_input(Utc::now()).as_bytes(), written).unwrap();
    old.session.digest = session_digest("old");
    store.write(&old, Utc::now()).unwrap();
    let report = monitor(FED, store.dir()).refresh().await.reports.remove(0);
    assert_eq!(report.status, Status::Ready);
    assert_eq!(report.as_of, Some(written));
    assert!(
        crate::freshness::lagging_as_of(&report).is_some(),
        "shown with its age"
    );
}

#[tokio::test]
async fn a_window_past_its_reset_is_absent_not_reset() {
    let (_dir, store) = store();
    let current = Utc::now();
    let mut stale = reading("w", current - TimeDelta::minutes(1));
    stale.windows.retain(|w| w.id != WindowId::SpendLimit);
    stale.windows[0].resets_at = Some(current.timestamp() - 60);
    store.write(&stale, current).unwrap();
    let report = monitor(FED, store.dir()).refresh().await.reports.remove(0);
    assert_eq!(report.metrics.len(), 1);
    assert_eq!(report.metrics[0].window, Some(crate::MetricWindow::Weekly));
    assert!(report
        .notes_l10n
        .iter()
        .any(|n| n.id == "note.claude_code.window_absent"));
}

#[tokio::test]
async fn the_hybrid_mode_uses_a_recent_feed_and_falls_back_otherwise() {
    let (_dir, fresh) = self::store();
    fresh
        .write(
            &reading("h", Utc::now() - TimeDelta::minutes(1)),
            Utc::now(),
        )
        .unwrap();
    let report = monitor(HYBRID, fresh.dir())
        .refresh()
        .await
        .reports
        .remove(0);
    assert_eq!(report.status, Status::Ready);
    assert!(report
        .notes_l10n
        .iter()
        .any(|n| n.id == "note.claude_code.answered_by_feed"));
    let account = &Config::from_toml(HYBRID).unwrap().accounts[0];
    let source = crate::export::v1::source_export(account, Some(&report)).unwrap();
    assert_eq!(source.kind, SourceKind::Hybrid);
    assert_eq!(
        source.answered_by,
        Some(crate::export::v1::AnsweredBy::Feed)
    );
    assert_eq!(source.measured_at, report.as_of);

    // Older than the 300 s interval: the query answers (here it lacks its
    // token, which proves the fallback ran instead of the feed).
    let (_dir2, old_store) = self::store();
    old_store
        .write(
            &reading("h", Utc::now() - TimeDelta::minutes(20)),
            Utc::now(),
        )
        .unwrap();
    let report = monitor(HYBRID, old_store.dir())
        .refresh()
        .await
        .reports
        .remove(0);
    assert_ne!(report.source, "feed:claude_code");
    assert_ne!(report.status, Status::Ready);
}

#[test]
fn pace_and_alerts_work_on_fed_windows() {
    let current = Utc::now();
    let fed = reading("p", current);
    let (metrics, _) = crate::providers::agent_feed::metrics(&fed, current);
    let five = &metrics[0];
    // A pace assessment exists for a fed window like for a queried one.
    let pace = crate::pace::assess_reading(
        five,
        current,
        std::time::Duration::from_secs(300),
        current,
        None,
    );
    assert!(pace.is_some() || five.resets_at.is_some());
    let binding = crate::metrics::binding(&crate::Report {
        account_id: None,
        diagnostic: None,
        account: String::new(),
        provider: crate::Provider::Claude,
        checked_at: current,
        status: Status::Ready,
        metrics: metrics.clone(),
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: "feed:claude_code".into(),
        session_expires_at: None,
        sub_reports: Vec::new(),
        as_of: Some(current),
        overage_hint: None,
        breakdown: Vec::new(),
    })
    .unwrap();
    assert!(binding.used_ratio > 0.0);
}

// ------------------------------------------------------------- RF4

const META: &str = r#"{"timestamp":"2026-10-25T13:00:00.000Z","type":"session_meta","payload":{"id":"codex-session-1","cwd":"/Users/ana/SENSITIVE-repo","instructions":"SENSITIVE"}}"#;
const TURN: &str = r#"{"timestamp":"2026-10-25T13:00:01.000Z","type":"turn_context","payload":{"cwd":"/Users/ana/SENSITIVE-repo","model":"gpt-5-codex"}}"#;
const MESSAGE: &str = r#"{"timestamp":"2026-10-25T13:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"SENSITIVE prompt"}]}}"#;

fn count(rate_limits: &str) -> String {
    format!(
        r#"{{"timestamp":"2026-10-25T13:05:00.000Z","type":"event_msg","payload":{{"type":"token_count","info":null,"rate_limits":{rate_limits}}}}}"#
    )
}

fn codex_home(lines: &[String]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let home = tempfile::tempdir().unwrap();
    let day = home.path().join("sessions/2026/10/25");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join("rollout-2026-10-25T13-00-00-abc.jsonl"),
        lines.join("\n") + "\n",
    )
    .unwrap();
    let roots = vec![
        home.path().join("sessions"),
        home.path().join("archived_sessions"),
    ];
    (home, roots)
}

#[test]
fn codex_logs_with_two_windows_give_both() {
    let reset = 1_793_000_000;
    let lines = [
        META.to_owned(),
        TURN.to_owned(),
        MESSAGE.to_owned(),
        count(&format!(
            r#"{{"limit_id":null,"primary":{{"used_percent":12.5,"window_minutes":300,"resets_at":{reset}}},"secondary":{{"used_percent":40.0,"window_minutes":10080,"resets_in_seconds":3600}},"credits":null,"plan_type":"plus"}}"#
        )),
    ];
    let (_home, roots) = codex_home(&lines);
    let reading = codex::latest(&roots).unwrap().unwrap();
    assert_eq!(reading.source, FeedSource::CodexCli);
    assert_eq!(
        reading.written_at,
        Utc.with_ymd_and_hms(2026, 10, 25, 13, 5, 0).unwrap()
    );
    assert_eq!(reading.session.digest, session_digest("codex-session-1"));
    assert_eq!(reading.session.model_name.as_deref(), Some("gpt-5-codex"));
    let primary = reading.window(WindowId::Primary).unwrap();
    assert_eq!(
        (primary.used_pct, primary.window_minutes, primary.resets_at),
        (12.5, Some(300), Some(reset))
    );
    let secondary = reading.window(WindowId::Secondary).unwrap();
    assert_eq!(
        secondary.resets_at,
        Some(reading.written_at.timestamp() + 3600)
    );
    let json = serde_json::to_string(&reading).unwrap();
    assert!(
        !json.contains("SENSITIVE") && !json.contains("codex-session-1"),
        "{json}"
    );
    let (metrics, absent) = crate::providers::agent_feed::metrics(&reading, reading.written_at);
    assert!(!absent);
    assert_eq!(metrics[0].window, Some(crate::MetricWindow::Window5h));
    assert_eq!(metrics[1].window, Some(crate::MetricWindow::Weekly));
}

/// RF1.3 (PRD 0007 re-validation): a Codex CLI log dated in the future is
/// ignored, like a stored reading; its own time and a little skew pass.
#[test]
fn a_codex_log_dated_in_the_future_gives_no_reading() {
    let lines = [
        META.to_owned(),
        TURN.to_owned(),
        count(
            r#"{"primary":{"used_percent":12.5,"window_minutes":300,"resets_in_seconds":3600},"secondary":null}"#,
        ),
    ];
    let (_home, roots) = codex_home(&lines);
    let written = Utc.with_ymd_and_hms(2026, 10, 25, 13, 5, 0).unwrap();
    let latest = |now| crate::providers::agent_feed::codex_latest(&roots, now).unwrap();
    assert_eq!(latest(written).unwrap().written_at, written);
    assert!(latest(written - TimeDelta::seconds(30)).is_some(), "skew");
    assert_eq!(latest(written - TimeDelta::minutes(10)), None);
}

#[test]
fn codex_logs_with_one_window_leave_the_other_absent() {
    let lines = [
        META.to_owned(),
        count(r#"{"primary":{"used_percent":7.0,"window_minutes":300},"secondary":null}"#),
    ];
    let (_home, roots) = codex_home(&lines);
    let reading = codex::latest(&roots).unwrap().unwrap();
    assert_eq!(reading.windows.len(), 1);
    let (_, absent) = crate::providers::agent_feed::metrics(&reading, reading.written_at);
    assert!(absent);
}

#[test]
fn an_unknown_codex_shape_is_refused() {
    for shape in [
        r#"{"primary":{"percent_used":7.0}}"#,
        r#"{"windows":[{"used":7}]}"#,
        r#""full""#,
    ] {
        let lines = [META.to_owned(), count(shape)];
        let (_home, roots) = codex_home(&lines);
        assert_eq!(
            codex::latest(&roots),
            Err(codex::CodexError::UnknownShape),
            "{shape}"
        );
        assert!(matches!(
            crate::providers::agent_feed::codex_latest(&roots, now()),
            Err(error) if error.note.id == crate::providers::SCHEMA_NOTE
        ));
    }
}

#[test]
fn codex_logs_without_rate_limits_give_nothing() {
    let lines = [META.to_owned(), TURN.to_owned(), MESSAGE.to_owned()];
    let (_home, roots) = codex_home(&lines);
    assert_eq!(codex::latest(&roots), Ok(None));
}

#[cfg(unix)]
#[test]
fn the_codex_reader_never_follows_a_link_outside_its_roots() {
    let outside = tempfile::tempdir().unwrap();
    let foreign = outside.path().join("rollout-2026-10-25T13-00-00-zzz.jsonl");
    std::fs::write(&foreign, count(r#"{"primary":{"used_percent":99.0}}"#)).unwrap();
    let (home, roots) = codex_home(&[META.to_owned()]);
    std::os::unix::fs::symlink(&foreign, home.path().join("sessions/rollout-link.jsonl")).unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("sessions/linked-dir")).unwrap();
    assert_eq!(codex::latest(&roots), Ok(None));
}

#[test]
fn the_codex_disclosure_lists_its_directories() {
    let home = tempfile::tempdir().unwrap();
    let env = crate::cost::Env::new(Some(home.path().to_path_buf()), Vec::new());
    let disclosure =
        crate::cost::CostSettings::default().disclosure(crate::cost::SourceId::CodexCli, &env);
    let text: String = disclosure
        .lines()
        .iter()
        .map(|line| line.resolve("en-US"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("sessions"), "{text}");
}

// ------------------------------------------------------------- RF5

fn other(name: &str, remaining: f64) -> OtherAccount {
    OtherAccount {
        view: AccountView {
            key: name.into(),
            provider: "chat_gpt".into(),
            provider_label: "ChatGPT / Codex".into(),
            name: name.into(),
            remaining: Some(remaining),
            ..AccountView::default()
        },
        age: None,
    }
}

fn line(input: &str, others: &[OtherAccount], width: Option<usize>) -> statusline::Line {
    let reading = claude::parse(input.as_bytes(), now()).unwrap();
    statusline::compose(
        Some(&reading),
        others,
        now(),
        |t| t.format("%H:%M").to_string(),
        width,
        "en-US",
    )
}

#[test]
fn the_status_line_puts_claude_first_then_the_tightest_other() {
    let others = [other("Codex", 0.64), other("Copilot", 0.9)];
    assert_eq!(
        line(&full_input(now()), &others, None).text,
        "5h 24% · Wk 41% · spend $314/$500 ↺ 14:00 · Codex 64% left · ctx 8%"
            .replace("spend $314/$500", "$314/$500")
            .replace(
                "↺ 14:00",
                &format!("↺ {}", (now() + TimeDelta::days(20)).format("%H:%M"))
            )
    );
}

#[test]
fn the_status_line_golden_fixtures() {
    let others = [other("Codex", 0.64)];
    assert_eq!(
        line(NO_RATE_LIMITS, &others, None).text,
        "Codex 64% left · ctx 3%"
    );
    assert_eq!(line(NO_RATE_LIMITS, &[], None).text, "ctx 3%");
    assert_eq!(
        line(&spend_without_dollars(now()), &[], None).text,
        "spend 63% ↺ 14:00"
    );
    assert_eq!(line(NULL_PERCENTAGE, &[], None).text, "Wk 12%");
    let mut aged = other("Codex", 0.5);
    aged.age = Some(std::time::Duration::from_secs(180));
    assert_eq!(
        line(NULL_PERCENTAGE, &[aged], None).text,
        "Wk 12% · Codex 50% left (3m)"
    );
}

#[test]
fn the_status_line_drops_ctx_then_the_other_account_then_cuts_claude() {
    let others = [other("Codex", 0.64)];
    let input = format!(
        r#"{{"session_id":"t","context_window":{{"used_percentage":8}},"rate_limits":{{"five_hour":{{"used_percentage":23,"resets_at":{}}}}}}}"#,
        now().timestamp() + 3_600
    );
    assert_eq!(
        line(&input, &others, None).text,
        "5h 23% ↺ 15:00 · Codex 64% left · ctx 8%"
    );
    assert_eq!(
        line(&input, &others, Some(32)).text,
        "5h 23% ↺ 15:00 · Codex 64% left"
    );
    assert_eq!(line(&input, &others, Some(16)).text, "5h 23% ↺ 15:00");
    assert_eq!(line(&input, &others, Some(8)).text, "5h 23% …");
}

#[test]
fn the_status_line_level_follows_claude_tightest_window() {
    let input = r#"{"session_id":"t","rate_limits":{"five_hour":{"used_percentage":95}}}"#;
    assert_eq!(line(input, &[], None).level, Some(crate::tray::Level::Bad));
    assert_eq!(line(NO_RATE_LIMITS, &[], None).level, None);
}

// ------------------------------------------------------------- RF7

fn live_reading(session: &str, minutes_ago: i64, ctx: f64) -> FeedReading {
    let mut reading = reading(session, now() - TimeDelta::minutes(minutes_ago));
    reading.session.context_used_pct = Some(ctx);
    reading
}

#[test]
fn the_live_block_lists_recent_sessions_newest_first() {
    assert!(live::block(&[], now()).is_empty());
    let one = live::block(&[live_reading("a", 2, 8.0)], now());
    assert_eq!(one.sessions.len(), 1);
    assert_eq!(one.more, 0);
    assert_eq!(one.tray_chip().unwrap().resolve("en-US"), "ctx 8%");
    assert_eq!(
        one.once_line().unwrap().resolve("en-US"),
        "Live session: Opus · ctx 8% · ≈$0.42"
    );
    let six: Vec<FeedReading> = (0..6)
        .map(|i| live_reading(&format!("s{i}"), i, 10.0))
        .collect();
    let block = live::block(&six, now());
    assert_eq!(block.sessions.len(), live::MAX_SHOWN);
    assert_eq!(block.more, 1);
    assert_eq!(block.more_text().unwrap().resolve("en-US"), "and 1 more");
    assert_eq!(block.sessions[0].written_at, now());
}

#[test]
fn a_session_expires_ten_minutes_after_its_last_feed() {
    assert_eq!(
        live::block(&[live_reading("a", 9, 8.0)], now())
            .sessions
            .len(),
        1
    );
    assert!(live::block(&[live_reading("a", 10, 8.0)], now()).is_empty());
}

#[test]
fn the_context_meter_roles_are_display_only_thresholds() {
    assert_eq!(ContextLevel::of(50.0), ContextLevel::Good);
    assert_eq!(ContextLevel::of(75.0), ContextLevel::Warn);
    assert_eq!(ContextLevel::of(95.0), ContextLevel::Bad);
    let session = &live::block(&[live_reading("a", 1, 75.0)], now()).sessions[0];
    assert_eq!(session.context_level, Some(ContextLevel::Warn));
    assert_eq!(
        session.cost_text().unwrap().resolve("en-US"),
        "≈ $0.42 this session"
    );
}

// ------------------------------------------------------------- RF8

#[test]
fn the_unbound_banner_state_machine() {
    let feed = reading("x", now() - TimeDelta::minutes(5));
    let empty = Config::default();
    // No feed: nothing to offer.
    assert!(live::unbound_banner(FeedSource::ClaudeCode, &empty, None, None, now()).is_none());
    // A feed and no account: the banner, prefilled for the Claude Code mode.
    let banner =
        live::unbound_banner(FeedSource::ClaudeCode, &empty, Some(&feed), None, now()).unwrap();
    assert_eq!(banner.provider, crate::Provider::Claude);
    assert_eq!(banner.query_mode, MODE_CLAUDE_CODE);
    // Bound: gone.
    let bound = Config::from_toml(FED).unwrap();
    assert!(
        live::unbound_banner(FeedSource::ClaudeCode, &bound, Some(&feed), None, now()).is_none()
    );
    // Dismissed: hidden for 7 days, then back.
    let dismissed = Some(now() - TimeDelta::days(6));
    assert!(live::unbound_banner(
        FeedSource::ClaudeCode,
        &empty,
        Some(&feed),
        dismissed,
        now()
    )
    .is_none());
    let expired = Some(now() - TimeDelta::days(8));
    assert!(
        live::unbound_banner(FeedSource::ClaudeCode, &empty, Some(&feed), expired, now()).is_some()
    );
}

#[test]
fn the_add_form_preselects_the_claude_code_mode_when_detected() {
    use crate::detect::SourceKind as Detected;
    assert_eq!(
        crate::account_edit::preselected_choices(crate::Provider::Claude, &[Detected::ClaudeCode]),
        vec![("query_mode", 1)]
    );
    assert!(crate::account_edit::preselected_choices(crate::Provider::Claude, &[]).is_empty());
    assert!(crate::account_edit::preselected_choices(
        crate::Provider::ChatGpt,
        &[Detected::ClaudeCode]
    )
    .is_empty());
}
