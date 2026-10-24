use super::super::parse::plain_text;
use super::*;
use crate::{oauth, Account, Config, Credentials, Monitor};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct NoCredentials;
impl Credentials for NoCredentials {
    fn get(&self, name: &str) -> Option<String> {
        name.strip_prefix("GROQ_KEY_")
            .map(|account| format!("gsk_{account}"))
    }
}

struct EmptyStore;
impl oauth::TokenStore for EmptyStore {
    fn load(&self, _: &str) -> Result<Option<oauth::OAuthToken>, oauth::OAuthError> {
        Ok(None)
    }
    fn save(&self, _: &str, _: &oauth::OAuthToken) -> Result<(), oauth::OAuthError> {
        unreachable!()
    }
    fn delete(&self, _: &str) -> Result<(), oauth::OAuthError> {
        unreachable!()
    }
}

/// One account per entry, named after its index so providers may repeat.
fn monitor(accounts: &[(Provider, Option<&str>)], status: &MockServer) -> Monitor {
    let config = Config {
        accounts: accounts
            .iter()
            .enumerate()
            .map(|(index, (provider, key))| Account {
                name: format!("account-{index}"),
                provider: *provider,
                credential_env: key.map(|key| format!("GROQ_KEY_{key}")),
                ..Account::default()
            })
            .collect(),
        ..Config::default()
    };
    let creds: Arc<dyn Credentials> = Arc::new(NoCredentials);
    let session =
        Arc::new(oauth::OAuthSession::new(Arc::new(EmptyStore), Arc::clone(&creds)).unwrap());
    let monitor = Monitor::with_oauth(config, creds, session).unwrap();
    monitor.incidents().set_test_knobs(TestKnobs {
        base: Some(status.uri()),
        ..TestKnobs::default()
    });
    monitor.incidents().set_enabled(true);
    monitor
}

fn only(accounts: &[Provider], status: &MockServer) -> Monitor {
    let accounts: Vec<_> = accounts.iter().map(|p| (*p, None)).collect();
    monitor(&accounts, status)
}

/// Mount `body` at the local path of `provider`'s source URL.
async fn serve(server: &MockServer, provider: Provider, response: ResponseTemplate) {
    let url = super::super::source(provider).unwrap().url;
    Mock::given(method("GET"))
        .and(path(format!("/{}", url.trim_start_matches("https://"))))
        .respond_with(response)
        .mount(server)
        .await;
}

fn body(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(text)
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

fn status_of(statuses: &[ProviderStatus], provider: Provider) -> &ProviderStatus {
    statuses
        .iter()
        .find(|status| status.provider == provider)
        .unwrap_or_else(|| panic!("{provider:?} has no status"))
}

fn at(text: &str) -> Option<DateTime<Utc>> {
    Some(
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc),
    )
}

const CLAUDE_INCIDENT: &str = r#"{
  "page": {"id": "p", "name": "Claude", "url": "https://status.claude.com"},
  "components": [
    {"id": "rwppv331jlwc", "name": "claude.ai", "status": "partial_outage"},
    {"id": "k8w3r06qmzrp", "name": "Claude API", "status": "operational"},
    {"id": "0qbwn08sd68x", "name": "Claude Console", "status": "operational"}
  ],
  "incidents": [{
    "id": "abc123xyz", "name": "<b>Elevated errors</b> on **claude.ai** &amp; Claude Code",
    "status": "investigating", "impact": "major",
    "started_at": "2026-10-20T10:00:00.000Z",
    "shortlink": "https://stspg.io/abc",
    "components": [{"id": "rwppv331jlwc", "name": "claude.ai", "status": "partial_outage"}]
  }],
  "scheduled_maintenances": [],
  "status": {"indicator": "major", "description": "Partial outage"}
}"#;

const OPENAI_WIDGET: &str = r#"{
  "page_title": "OpenAI", "page_url": "https://status.openai.com/",
  "ongoing_incidents": [{
    "id": "01INC", "name": "Increased latency in Responses",
    "status": "investigating",
    "url": "https://status.openai.com/incidents/01INC?utm=feed#latest",
    "current_worst_impact": "full_outage",
    "started_at": "2026-10-20T09:30:00Z",
    "affected_components": [
      {"id": "01JP8CD9JR3HR6Y7G4Q75N4DVW", "name": "Responses", "current_status": "degraded_performance"},
      {"id": "01JMXBNJXG1S2D9V65P1ZZTD94", "name": "Login (ChatGPT)", "current_status": "operational"},
      {"id": "unrelated", "name": "Sora", "current_status": "full_outage"}
    ]
  }],
  "in_progress_maintenances": [],
  "scheduled_maintenances": []
}"#;

const GOOGLE_ACTIVE: &str = r#"[
  {"id": "active", "begin": "2026-10-20T08:00:00+00:00", "end": null,
   "external_desc": "Vertex Gemini API requests failing",
   "status_impact": "SERVICE_DISRUPTION", "severity": "medium",
   "affected_products": [{"title": "Vertex Gemini API", "id": "Z0FZJAMvEB4j3NbCJs6B"}],
   "uri": "incidents/active"},
  {"id": "closed", "begin": "2026-10-01T08:00:00+00:00", "end": "2026-10-01T09:00:00+00:00",
   "external_desc": "Old Cloud Monitoring incident", "status_impact": "SERVICE_OUTAGE",
   "affected_products": [{"title": "Cloud Monitoring", "id": "3zaaDb7antc73BM1UAVT"}],
   "uri": "incidents/closed"}
]"#;

const AWS_ACTIVE: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<rss version="2.0"><channel><title>AWS</title><ttl>5</ttl>
 <item>
  <title><![CDATA[Service degradation: Increased Bedrock error rates]]></title>
  <link>https://status.aws.amazon.com/</link>
  <pubDate>Tue, 20 Oct 2026 03:30:00 PDT</pubDate>
  <guid isPermaLink="false">https://status.aws.amazon.com/#bedrock-us-east-1_1792405800</guid>
 </item>
 <item>
  <title><![CDATA[Informational message: Increased Bedrock error rates]]></title>
  <link>https://status.aws.amazon.com/</link>
  <pubDate>Tue, 20 Oct 2026 03:00:00 PDT</pubDate>
  <guid isPermaLink="false">https://status.aws.amazon.com/#bedrock-us-east-1_1792404000</guid>
 </item>
 <item>
  <title><![CDATA[Service is operating normally: [RESOLVED] Bedrock latency]]></title>
  <link>https://status.aws.amazon.com/</link>
  <pubDate>Mon, 19 Oct 2026 10:00:00 PDT</pubDate>
  <guid isPermaLink="false">https://status.aws.amazon.com/#bedrock-eu-west-1_1792342800</guid>
 </item>
 <item>
  <title><![CDATA[Service disruption: EC2 instances unreachable]]></title>
  <link>https://status.aws.amazon.com/</link>
  <pubDate>Tue, 20 Oct 2026 04:00:00 PDT</pubDate>
  <guid isPermaLink="false">https://status.aws.amazon.com/#ec2-us-east-1_1792407600</guid>
 </item>
</channel></rss>"#;

const XAI_ACTIVE: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom"><channel>
 <title>xAI</title><link>https://status.x.ai</link>
 <atom:link href="https://status.x.ai/feed.xml" rel="self" type="application/rss+xml" />
 <item>
  <title>[grok.com] Elevated errors on grok.com</title>
  <link>https://status.x.ai/grok-com/ps_1</link>
  <guid isPermaLink="false">ps_1</guid>
  <pubDate>Tue, 20 Oct 2026 11:00:00 GMT</pubDate>
  <category>outage</category>
 </item>
 <item>
  <title>[Voice] Voice degraded</title>
  <link>https://status.x.ai/voice/ps_2</link>
  <guid isPermaLink="false">ps_2</guid>
  <pubDate>Tue, 20 Oct 2026 12:00:00 GMT</pubDate>
  <category>disruption</category>
 </item>
</channel></rss>"#;

/// RF2 acceptance: one wiremock source per format, each normalized for the
/// provider's scope only, with sanitized text and same-host public links.
#[tokio::test]
async fn every_format_normalizes_scoped_active_incidents() {
    use Provider::*;
    let server = MockServer::start().await;
    serve(&server, Claude, body(CLAUDE_INCIDENT)).await;
    serve(&server, OpenAiApi, body(OPENAI_WIDGET)).await;
    serve(&server, VertexAi, body(GOOGLE_ACTIVE)).await;
    serve(&server, AwsBedrock, body(AWS_ACTIVE)).await;
    serve(&server, Grok, body(XAI_ACTIVE)).await;
    let monitor = only(
        &[
            Claude,
            AnthropicApi,
            OpenAiApi,
            ChatGpt,
            VertexAi,
            AwsBedrock,
            Grok,
        ],
        &server,
    );
    let statuses = monitor.refresh_incidents().await;
    assert_eq!(requests(&server).await, 5, "one request per shared URL");

    let claude = status_of(&statuses, Claude);
    assert_eq!(claude.state, IncidentState::PartialOutage);
    let incident = claude.incident.as_ref().unwrap();
    assert_eq!(incident.title, "Elevated errors on claude.ai & Claude Code");
    assert_eq!(incident.started_at, at("2026-10-20T10:00:00Z"));
    assert_eq!(
        incident.url,
        "https://status.claude.com/incidents/abc123xyz"
    );
    assert_eq!(claude.page_url, "https://status.claude.com/");
    assert!(claude.read_at.is_some() && !claude.stale);
    // Same page, unaffected scope.
    let api = status_of(&statuses, AnthropicApi);
    assert_eq!(
        (api.state, api.incident.as_ref()),
        (IncidentState::Operational, None)
    );

    let openai = status_of(&statuses, OpenAiApi);
    assert_eq!(
        openai.state,
        IncidentState::Degraded,
        "not Sora's full outage"
    );
    let incident = openai.incident.as_ref().unwrap();
    assert_eq!(incident.url, "https://status.openai.com/incidents/01INC");
    assert_eq!(incident.started_at, at("2026-10-20T09:30:00Z"));
    // The incident lists ChatGPT login as operational: doubt, not health.
    assert_eq!(status_of(&statuses, ChatGpt).state, IncidentState::Unknown);

    let vertex = status_of(&statuses, VertexAi);
    assert_eq!(vertex.state, IncidentState::PartialOutage);
    let incident = vertex.incident.as_ref().unwrap();
    assert_eq!(incident.title, "Vertex Gemini API requests failing");
    assert_eq!(
        incident.url,
        "https://status.cloud.google.com/incidents/active"
    );

    // The newest update of the Bedrock event decides; resolved and
    // unrelated-service events do not count.
    let bedrock = status_of(&statuses, AwsBedrock);
    assert_eq!(bedrock.state, IncidentState::PartialOutage);
    let incident = bedrock.incident.as_ref().unwrap();
    assert_eq!(incident.title, "Increased Bedrock error rates");
    assert_eq!(incident.started_at, at("2026-10-20T10:00:00Z"));
    assert_eq!(incident.url, "https://status.aws.amazon.com/");

    let grok = status_of(&statuses, Grok);
    assert_eq!(
        grok.state,
        IncidentState::MajorOutage,
        "Voice is out of scope"
    );
    let incident = grok.incident.as_ref().unwrap();
    assert_eq!(incident.title, "Elevated errors on grok.com");
    assert_eq!(incident.url, "https://status.x.ai/grok-com/ps_1");

    // RF4.1: every page the client reports can be opened by a frontend.
    for status in &statuses {
        assert!(super::super::is_status_link(&status.page_url), "{status:?}");
        if let Some(incident) = &status.incident {
            assert!(super::super::is_status_link(&incident.url), "{status:?}");
        }
    }
}

/// The verified captures through HTTP: quiet pages are operational, feeds
/// whose history cannot prove health stay unknown, and a page-wide outage
/// does not leak into an operational scoped component.
#[tokio::test]
async fn captured_fixtures_never_assert_health_without_a_scoped_reading() {
    use Provider::*;
    let server = MockServer::start().await;
    for (provider, fixture) in [
        (Claude, include_str!("fixtures/claude.json")),
        (OpenAiApi, include_str!("fixtures/widget-empty.json")),
        (VertexAi, include_str!("fixtures/google-cloud.json")),
        (AwsBedrock, include_str!("fixtures/aws-health.rss")),
        (Grok, include_str!("fixtures/xai.rss")),
        (CloudflareAi, include_str!("fixtures/cloudflare.json")),
        (Cursor, include_str!("fixtures/cursor.json")),
        (Copilot, include_str!("fixtures/github.json")),
        (Windsurf, include_str!("fixtures/windsurf.json")),
        (MiniMax, include_str!("fixtures/minimax.json")),
        (Kimi, include_str!("fixtures/moonshot.json")),
        (NvidiaNim, include_str!("fixtures/nvidia.json")),
    ] {
        serve(&server, provider, body(fixture)).await;
    }
    let providers = [
        Claude,
        OpenAiApi,
        VertexAi,
        AwsBedrock,
        Grok,
        CloudflareAi,
        Cursor,
        Copilot,
        Windsurf,
        MiniMax,
        Kimi,
        NvidiaNim,
    ];
    let statuses = only(&providers, &server).refresh_incidents().await;
    for provider in providers {
        let status = status_of(&statuses, provider);
        let expected = match provider {
            AwsBedrock | Grok => IncidentState::Unknown,
            _ => IncidentState::Operational,
        };
        assert_eq!(status.state, expected, "{provider:?}");
        assert!(status.incident.is_none(), "{provider:?}");
        assert!(status.read_at.is_some() && !status.stale, "{provider:?}");
    }
}

#[tokio::test]
async fn invalid_responses_read_as_unknown_and_stale() {
    use Provider::*;
    let server = MockServer::start().await;
    serve(&server, Claude, body("<html>maintenance</html>")).await;
    serve(&server, OpenAiApi, body(r#"{"page_title": "OpenAI"}"#)).await;
    serve(&server, VertexAi, body(r#"{"incidents": []}"#)).await;
    serve(&server, AwsBedrock, body(r#"{"not": "rss"}"#)).await;
    serve(&server, Grok, ResponseTemplate::new(503)).await;
    // A redirect is never followed to another host.
    serve(
        &server,
        Cursor,
        ResponseTemplate::new(302).insert_header("location", "https://example.invalid/"),
    )
    .await;
    let providers = [Claude, OpenAiApi, VertexAi, AwsBedrock, Grok, Cursor];
    let statuses = only(&providers, &server).refresh_incidents().await;
    for provider in providers {
        let status = status_of(&statuses, provider);
        assert_eq!(status.state, IncidentState::Unknown, "{provider:?}");
        assert!(status.incident.is_none() && status.read_at.is_none());
        assert!(status.stale, "{provider:?}");
    }
}

#[tokio::test]
async fn responses_above_the_size_limit_are_refused() {
    let fixture = include_str!("fixtures/claude.json");
    let padded = |len: usize| format!("{fixture}{}", " ".repeat(len - fixture.len()));
    for (len, readable) in [(MAX_BODY_BYTES, true), (MAX_BODY_BYTES + 1, false)] {
        let server = MockServer::start().await;
        serve(&server, Provider::Claude, body(&padded(len))).await;
        let statuses = only(&[Provider::Claude], &server).refresh_incidents().await;
        let expected = if readable {
            IncidentState::Operational
        } else {
            IncidentState::Unknown
        };
        assert_eq!(statuses[0].state, expected, "{len} bytes");
        assert_eq!(statuses[0].stale, !readable, "{len} bytes");
    }
}

/// RF2.2: a timeout after a valid reading keeps it, marked stale.
#[tokio::test]
async fn a_timeout_keeps_the_last_known_reading_marked_stale() {
    let server = MockServer::start().await;
    let url = super::super::source(Provider::Claude).unwrap().url;
    let route = format!("/{}", url.trim_start_matches("https://"));
    Mock::given(path(route.clone()))
        .respond_with(body(CLAUDE_INCIDENT))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path(route))
        .respond_with(body(CLAUDE_INCIDENT).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    let monitor = only(&[Provider::Claude], &server);
    monitor.incidents().set_test_knobs(TestKnobs {
        base: Some(server.uri()),
        ttl: Some(Duration::ZERO),
        timeout: Some(Duration::from_millis(200)),
    });
    let first = monitor.refresh_incidents().await.remove(0);
    assert_eq!(first.state, IncidentState::PartialOutage);
    assert!(!first.stale);

    let started = Instant::now();
    let second = monitor.refresh_incidents().await.remove(0);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the timeout bounds it"
    );
    assert_eq!(second.state, IncidentState::PartialOutage);
    assert_eq!(second.incident, first.incident);
    assert_eq!(second.read_at, first.read_at, "the reading's own time");
    assert!(second.stale);
}

/// RF2.2: one request per source per TTL, however many accounts and
/// products share the URL and however many callers race for it.
#[tokio::test]
async fn sources_are_cached_and_coalesced_per_url() {
    use Provider::*;
    let server = MockServer::start().await;
    serve(
        &server,
        OpenAiApi,
        body(include_str!("fixtures/widget-empty.json")).set_delay(Duration::from_millis(100)),
    )
    .await;
    let monitor = only(&[OpenAiApi, OpenAiApi, ChatGpt], &server);
    let (a, b) = tokio::join!(monitor.refresh_incidents(), monitor.refresh_incidents());
    assert_eq!(requests(&server).await, 1);
    assert_eq!(a, b);
    assert_eq!(
        a.iter().map(|status| status.provider).collect::<Vec<_>>(),
        [OpenAiApi, ChatGpt],
        "one status per configured provider, in configuration order"
    );
    monitor.refresh_incidents().await;
    assert_eq!(monitor.incident_statuses(), a, "cached reading, no network");
    assert_eq!(requests(&server).await, 1, "within the TTL");

    // A monitor rebuilt for an edited configuration keeps the cache.
    let edited = monitor.with_config(monitor.config.clone()).unwrap();
    edited.refresh_incidents().await;
    assert_eq!(requests(&server).await, 1);

    monitor.incidents().set_test_knobs(TestKnobs {
        base: Some(server.uri()),
        ttl: Some(Duration::ZERO),
        ..TestKnobs::default()
    });
    monitor.refresh_incidents().await;
    assert_eq!(requests(&server).await, 2, "after the TTL");
}

/// RF2.4 and the owner decision: off by default and while off nothing is
/// requested; on, only configured providers with a verified source are.
#[tokio::test]
async fn queries_only_when_enabled_and_only_configured_sources() {
    use Provider::*;
    let server = MockServer::start().await;
    for provider in [Claude, OpenAiApi] {
        serve(
            &server,
            provider,
            body(include_str!("fixtures/claude.json")),
        )
        .await;
    }
    let monitor = only(&[Claude, GeminiApi], &server);
    monitor.incidents().set_enabled(false);
    assert!(!IncidentMonitor::new().is_enabled(), "off until enabled");
    monitor.refresh().await;
    assert!(monitor.refresh_incidents().await.is_empty());
    assert!(monitor.incident_statuses().is_empty());
    assert_eq!(requests(&server).await, 0);

    monitor.incidents().set_enabled(true);
    let statuses = monitor.refresh_incidents().await;
    // Gemini API has no verified product-scoped source: absent, never "operational".
    assert_eq!(
        statuses.iter().map(|s| s.provider).collect::<Vec<_>>(),
        [Claude]
    );
    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(
        received[0].url.path(),
        "/status.claude.com/api/v2/summary.json"
    );
}

/// RF4.3: the domains a frontend lists before the user opts in, without
/// any request: configured providers with a source, one entry per host.
#[tokio::test]
async fn status_domains_are_listed_while_off_without_a_request() {
    use Provider::*;
    let server = MockServer::start().await;
    let monitor = only(&[Claude, GeminiApi, AnthropicApi, Groq, ChatGpt], &server);
    monitor.incidents().set_enabled(false);
    assert_eq!(
        monitor.status_domains(),
        ["status.claude.com", "groqstatus.com", "status.openai.com"]
    );
    assert!(only(&[GeminiApi, Kiro], &server)
        .status_domains()
        .is_empty());
    assert_eq!(requests(&server).await, 0);
}

/// RF2.1: public requests identify the client and carry nothing from the
/// accounts (no credential headers, cookies or query parameters).
#[tokio::test]
async fn status_requests_are_identified_and_carry_no_account_data() {
    let quota = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .mount(&quota)
        .await;
    let status = MockServer::start().await;
    serve(
        &status,
        Provider::Groq,
        body(include_str!("fixtures/widget-empty.json")),
    )
    .await;
    let mut monitor = monitor(&[(Provider::Groq, Some("ALPHA"))], &status);
    monitor.groq_url = Some(quota.uri());
    monitor.refresh().await;
    let received = status.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let request = &received[0];
    let user_agent = request.headers.get("user-agent").unwrap().to_str().unwrap();
    assert!(user_agent.starts_with("cuotai/") && user_agent.contains("github.com/jmarcon/cuotai"));
    for name in ["authorization", "cookie", "x-api-key"] {
        assert!(request.headers.get(name).is_none(), "{name}");
    }
    assert_eq!(request.url.query(), None);
}

/// RF2.2: a failing status source changes neither a successful quota
/// reading nor a failed one.
#[tokio::test]
async fn a_status_failure_does_not_affect_the_quota_query() {
    let quota = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("authorization", "Bearer gsk_BETA"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&quota)
        .await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": [] }))
                .insert_header("x-ratelimit-limit-requests", "14400")
                .insert_header("x-ratelimit-remaining-requests", "14370")
                .insert_header("x-ratelimit-reset-requests", "2m59s"),
        )
        .mount(&quota)
        .await;
    let status = MockServer::start().await;
    serve(&status, Provider::Groq, ResponseTemplate::new(500)).await;
    let accounts = [
        (Provider::Groq, Some("ALPHA")),
        (Provider::Groq, Some("BETA")),
    ];
    let mut off = monitor(&accounts, &status);
    off.incidents().set_enabled(false);
    off.groq_url = Some(quota.uri());
    let mut on = monitor(&accounts, &status);
    on.groq_url = Some(quota.uri());

    let baseline = off.refresh().await;
    let snapshot = on.refresh().await;
    assert_eq!(requests(&status).await, 1, "only the enabled monitor asks");
    // `resets_at` follows the wall clock; the readings themselves must match.
    let values = |report: &crate::Report| {
        report
            .metrics
            .iter()
            .map(|m| (m.label.clone(), m.used, m.limit, m.remaining))
            .collect::<Vec<_>>()
    };
    for (expected, report) in baseline.reports.iter().zip(&snapshot.reports) {
        assert_eq!(report.status, expected.status);
        assert_eq!(report.notes, expected.notes);
        assert_eq!(report.notes_l10n, expected.notes_l10n);
        assert_eq!(values(report), values(expected));
    }
    assert_eq!(snapshot.reports[0].status, Status::Ready);
    assert!(!snapshot.reports[0].metrics.is_empty());
    assert_eq!(snapshot.reports[1].status, Status::AuthenticationError);
    let statuses = on.incident_statuses();
    assert_eq!(
        (statuses[0].state, statuses[0].stale),
        (IncidentState::Unknown, true)
    );
}

const GROQ_INCIDENT: &str = r#"{
  "page_title": "Groq", "page_url": "https://groqstatus.com/",
  "ongoing_incidents": [{
    "id": "01INC", "name": "API errors", "status": "identified",
    "url": "https://groqstatus.com/incidents/01INC",
    "current_worst_impact": "partial_outage",
    "affected_components": [{"id": "01K053E2FAKWKEYHXEV7WAHJBM", "name": "API", "current_status": "partial_outage"}]
  }],
  "in_progress_maintenances": [],
  "scheduled_maintenances": []
}"#;

/// RF3 end to end: failed queries during an incident get the explanatory
/// note (and, for authentication, the suggestion to wait) while keeping
/// their status; the ready account gets nothing; one status request serves
/// the whole refresh.
#[tokio::test]
async fn failed_queries_during_an_incident_are_annotated_without_changing_status() {
    let quota = MockServer::start().await;
    for (key, code) in [("BETA", 401), ("GAMMA", 429), ("DELTA", 500)] {
        Mock::given(method("GET"))
            .and(header(
                "authorization",
                format!("Bearer gsk_{key}").as_str(),
            ))
            .respond_with(ResponseTemplate::new(code))
            .mount(&quota)
            .await;
    }
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .mount(&quota)
        .await;
    let status = MockServer::start().await;
    serve(&status, Provider::Groq, body(GROQ_INCIDENT)).await;
    let keys = ["ALPHA", "BETA", "GAMMA", "DELTA"];
    let accounts: Vec<_> = keys
        .iter()
        .map(|key| (Provider::Groq, Some(*key)))
        .collect();
    let mut on = monitor(&accounts, &status);
    on.groq_url = Some(quota.uri());
    let mut off = monitor(&accounts, &status);
    off.incidents().set_enabled(false);
    off.groq_url = Some(quota.uri());

    let baseline = off.refresh().await;
    let snapshot = on.refresh().await;
    assert_eq!(requests(&status).await, 1);
    let added = |index: usize| {
        let report = &snapshot.reports[index];
        let before = &baseline.reports[index];
        assert_eq!(report.status, before.status, "the status never changes");
        assert_eq!(report.notes.len(), report.notes_l10n.len());
        assert_eq!(report.notes[..before.notes.len()], before.notes[..]);
        report.notes_l10n[before.notes.len()..]
            .iter()
            .map(|note| note.id.as_ref())
            .collect::<Vec<_>>()
    };
    let ongoing = "note.incident.ongoing";
    let wait = "note.incident.auth_wait";
    assert_eq!(snapshot.reports[0].status, Status::Ready);
    assert!(added(0).is_empty());
    assert_eq!(snapshot.reports[1].status, Status::AuthenticationError);
    assert_eq!(added(1), [ongoing, wait]);
    assert_eq!(snapshot.reports[2].status, Status::RateLimited);
    assert_eq!(added(2), [ongoing]);
    assert_eq!(snapshot.reports[3].status, Status::Error);
    assert_eq!(added(3), [ongoing]);
    // A single-account refresh reuses the cached reading.
    let report = on.refresh_account(1).await;
    assert_eq!(report.status, Status::AuthenticationError);
    assert_eq!(requests(&status).await, 1);
    let note = report.notes_l10n.last().unwrap();
    assert_eq!(
        note.resolve("en-US"),
        "While the incident lasts, consider waiting before signing in again; reauthenticating is still available."
    );
}

/// RF3 acceptance: table by query status and incident state.
#[test]
fn failure_notes_by_status_and_incident_state() {
    use IncidentState as I;
    let states = [
        I::Operational,
        I::Degraded,
        I::PartialOutage,
        I::MajorOutage,
        I::Maintenance,
        I::Unknown,
    ];
    let statuses = [
        Status::Ready,
        Status::NeedsConfiguration,
        Status::Unsupported,
        Status::AuthenticationError,
        Status::RateLimited,
        Status::Error,
    ];
    for status in statuses {
        for state in states {
            let incident = matches!(state, I::Degraded | I::PartialOutage | I::MajorOutage);
            let expected: &[&str] = match status {
                Status::AuthenticationError if incident => {
                    &["note.incident.ongoing", "note.incident.auth_wait"]
                }
                Status::RateLimited | Status::Error if incident => &["note.incident.ongoing"],
                _ => &[],
            };
            let (texts, l10n) = failure_notes(status, state).finish();
            let ids: Vec<_> = l10n.iter().map(|note| note.id.as_ref()).collect();
            assert_eq!(ids, expected, "{status:?} × {state:?}");
            assert_eq!(texts.len(), l10n.len());
        }
    }
    let (texts, _) = failure_notes(Status::Error, I::MajorOutage).finish();
    assert_eq!(
        texts,
        ["O provedor informa um incidente em andamento; a falha pode não ser da sua conta."]
    );
}

#[test]
fn provider_text_loses_markup_and_is_truncated() {
    assert_eq!(
        plain_text(
            "<p>API &amp; <a href=\"x\">Console</a></p>\n\t**down** for [some](https://x.invalid) `users` &#x2014; a &lt; b",
            200
        ),
        "API & Console down for some users — a < b"
    );
    assert_eq!(
        plain_text("<script>alert(1)</script>Title", 200),
        "alert(1) Title"
    );
    assert_eq!(plain_text("a\u{0}b\u{7}c", 200), "a b c");
    let long = "word ".repeat(40);
    let truncated = plain_text(&long, 20);
    assert_eq!(truncated.chars().count(), 20);
    assert!(truncated.ends_with("word…"));
    assert_eq!(
        plain_text("ação", 4),
        "ação",
        "counts characters, not bytes"
    );
}
