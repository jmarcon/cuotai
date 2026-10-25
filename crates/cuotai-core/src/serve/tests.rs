//! PRD 0012 RF2/RF3 acceptance: a real server on an ephemeral loopback port
//! driven by `reqwest` (and raw TCP where a client would fix the request).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use reqwest::{header, Method, StatusCode};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::config_file::ConfigIdentity;
use crate::export::v1::Options;
use crate::refresh::RefreshCycle;
use crate::text::core_catalog;
use crate::{
    Account, Config, Credentials, Metric, MetricKind, MetricWindow, Monitor, Provider, Report,
    Status,
};

fn account(name: &str, provider: Provider) -> Account {
    Account {
        id: None,
        name: name.into(),
        provider,
        ..Account::default()
    }
}

fn report(account: &Account, status: Status) -> Report {
    Report {
        account_id: None,
        account: account.name.clone(),
        provider: account.provider,
        checked_at: Utc::now() - chrono::TimeDelta::minutes(2),
        status,
        metrics: Vec::new(),
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
    }
}

fn finish(cycle: &mut RefreshCycle, index: usize, report: Report) {
    let token = cycle.begin(index).unwrap();
    cycle.finish_account(index, Some(report), token, std::time::Instant::now());
}

/// A healthy Claude account (with a space in its name), an OpenAI one whose
/// failure note names its credential variable and whose source carries
/// credentials, and a Kimi account still waiting for its first report.
fn fixture() -> (Config, RefreshCycle) {
    let mut work = account("Work", Provider::OpenAiApi);
    work.credential_env = Some("OPENAI_ADMIN_KEY".into());
    let config = Config {
        accounts: vec![
            account("Personal Max", Provider::Claude),
            work,
            account("Pending", Provider::Kimi),
        ],
        ..Config::default()
    };
    let mut cycle = RefreshCycle::new(&config, std::time::Instant::now());
    let mut ready = report(&config.accounts[0], Status::Ready);
    ready.metrics = vec![Metric {
        limit: Some(100.0),
        resets_at: Some(Utc::now() + chrono::TimeDelta::hours(2)),
        ..Metric::usage(
            "Sessão (5h)",
            30.0,
            "%",
            MetricKind::Quota,
            Some(MetricWindow::Window5h),
        )
    }];
    finish(&mut cycle, 0, ready);
    let mut failed = report(&config.accounts[1], Status::AuthenticationError);
    failed.notes = vec!["Credencial OPENAI_ADMIN_KEY recusada".into()];
    failed.source = "https://user:hunter2@api.openai.com/v1/usage?api_key=sk-SOURCE".into();
    finish(&mut cycle, 1, failed);
    (config, cycle)
}

fn source_of(config: Config, cycle: RefreshCycle) -> impl Source {
    let identity = ConfigIdentity::Explicit("serve".into());
    move || {
        Some(Export::from_cycle(
            &config,
            &identity,
            &cycle,
            &[],
            Utc::now(),
            Options::default(),
        ))
    }
}

fn fixture_source() -> impl Source {
    let (config, cycle) = fixture();
    source_of(config, cycle)
}

/// `config` on an ephemeral port.
async fn start(config: ServeConfig, source: impl Source) -> LocalServer {
    LocalServer::start(ServeConfig { port: 0, ..config }, source)
        .await
        .expect("loopback server starts")
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn request(server: &LocalServer, method: Method, path: &str) -> reqwest::RequestBuilder {
    client().request(method, format!("{}{path}", server.url()))
}

async fn get(server: &LocalServer, path: &str) -> reqwest::Response {
    request(server, Method::GET, path).send().await.unwrap()
}

/// RF2.4: the headers every answer carries; no CORS by default (RF3.3).
fn assert_json_no_store(response: &reqwest::Response) {
    let headers = response.headers();
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
}

async fn json(response: reqwest::Response) -> Value {
    response.json().await.unwrap()
}

/// Write `request` as is and read the whole answer.
async fn raw(addr: SocketAddr, request: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

fn encoded_key(account: &Account) -> String {
    account.key().replace(' ', "%20")
}

#[tokio::test]
async fn routes_serve_the_export_as_json_without_caching() {
    let (config, _) = fixture();
    let server = start(ServeConfig::default(), fixture_source()).await;
    assert_eq!(server.addr().ip(), IpAddr::from([127, 0, 0, 1]));

    let health = get(&server, "/health").await;
    assert_eq!(health.status(), StatusCode::OK);
    assert_json_no_store(&health);
    assert_eq!(
        json(health).await,
        serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION"), "schema": 1, "schemas": [1] })
    );

    let snapshot = get(&server, "/v1/snapshot").await;
    assert_eq!(snapshot.status(), StatusCode::OK);
    assert_json_no_store(&snapshot);
    let snapshot = json(snapshot).await;
    assert_eq!(snapshot["schema"], 1);
    assert_eq!(snapshot["accounts"].as_array().unwrap().len(), 3);
    assert_eq!(snapshot["accounts"][0]["report"]["status"], "ready");
    assert!(snapshot["accounts"][2]["report"].is_null());

    let one = get(
        &server,
        &format!("/v1/accounts/{}", encoded_key(&config.accounts[0])),
    )
    .await;
    assert_eq!(one.status(), StatusCode::OK);
    assert_json_no_store(&one);
    let one = json(one).await;
    assert_eq!(one["key"], "claude:Personal Max");
    assert_eq!(one["class"], "available");
    // A configured account still waiting for its report is not an error.
    let pending = get(
        &server,
        &format!("/v1/accounts/{}", config.accounts[2].key()),
    )
    .await;
    assert_eq!(pending.status(), StatusCode::OK);
    assert!(json(pending).await["report"].is_null());

    let limits = get(&server, "/v1/limits").await;
    assert_eq!(limits.status(), StatusCode::OK);
    assert_json_no_store(&limits);
    let limits = json(limits).await;
    assert_eq!(limits["accounts"][0]["binding"]["label"], "Sessão (5h)");
    assert_eq!(limits["accounts"][0]["age_seconds"], 120);
    assert!(limits["accounts"][0].get("report").is_none());

    for path in [
        "/",
        "/nope",
        "/v1/snapshot/",
        "/v1/accounts/",
        "/v1/accounts/claude:Unknown",
        "/v1/accounts/claude:Personal%2",
        "/v1/accounts/claude:Personal Max/extra",
        "/V1/SNAPSHOT",
    ] {
        let response = get(&server, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_json_no_store(&response);
        assert_eq!(json(response).await["error"], "not_found");
    }
}

#[tokio::test]
async fn only_get_is_allowed() {
    let server = start(ServeConfig::default(), fixture_source()).await;
    for method in [
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::PATCH,
        Method::OPTIONS,
    ] {
        for path in ["/health", "/v1/snapshot", "/v1/limits", "/nope"] {
            let response = request(&server, method.clone(), path)
                .body("{}")
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path}"
            );
            assert_json_no_store(&response);
            assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
            let body = response.text().await.unwrap();
            assert_eq!(body, r#"{"error":"method_not_allowed"}"#);
        }
    }
}

/// PRD 0031 RF9.2: `HEAD` has the status and headers of `GET` on every
/// route (data, `404`, `401`) and never a body.
#[tokio::test]
async fn head_answers_like_get_without_a_body() {
    let (config, _) = fixture();
    let paths = [
        "/health".to_string(),
        "/v1/snapshot".into(),
        "/v1/limits".into(),
        format!("/v1/accounts/{}", encoded_key(&config.accounts[0])),
        "/nope".into(),
    ];
    let guarded = ServeConfig {
        token: Some(Token::generate().unwrap()),
        ..ServeConfig::default()
    };
    for (config, expect_401) in [(ServeConfig::default(), false), (guarded, true)] {
        let server = start(config, fixture_source()).await;
        for path in &paths {
            let got = request(&server, Method::GET, path).send().await.unwrap();
            let head = request(&server, Method::HEAD, path).send().await.unwrap();
            assert_eq!(head.status(), got.status(), "{path}");
            assert_eq!(head.status() == StatusCode::UNAUTHORIZED, expect_401);
            for name in [
                header::CONTENT_TYPE,
                header::CACHE_CONTROL,
                header::WWW_AUTHENTICATE,
            ] {
                assert_eq!(
                    head.headers().get(&name),
                    got.headers().get(&name),
                    "{path}"
                );
            }
            assert!(head.text().await.unwrap().is_empty(), "{path}");
        }
    }
}

#[tokio::test]
async fn data_routes_answer_503_until_a_reading_exists() {
    let config = Config {
        accounts: vec![account("Pending", Provider::Kimi)],
        ..Config::default()
    };
    let key = config.accounts[0].key();
    let cycle = RefreshCycle::new(&config, std::time::Instant::now());
    let pending = source_of(config, cycle);
    for source in [
        Box::new(|| None) as Box<dyn Fn() -> Option<Export> + Send + Sync>,
        Box::new(move || pending.export()),
    ] {
        let server = start(ServeConfig::default(), source).await;
        let paths = ["/v1/snapshot", "/v1/limits", &format!("/v1/accounts/{key}")];
        for (index, path) in paths.into_iter().enumerate() {
            let response = get(&server, path).await;
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "endpoint #{index}"
            );
            assert_json_no_store(&response);
            assert_eq!(
                response.headers()[header::RETRY_AFTER],
                RETRY_AFTER_SECONDS.to_string()
            );
            assert_eq!(json(response).await["error"], "no_data");
        }
        assert_eq!(get(&server, "/health").await.status(), StatusCode::OK);
    }
}

struct Keys;

impl Credentials for Keys {
    fn get(&self, _: &str) -> Option<String> {
        Some("gsk_test".into())
    }
}

/// RF2.3: the server only reads the cycle; 100 requests of every data route
/// (with the nonexistent `?refresh=1` too) reach the provider zero times.
#[tokio::test]
async fn a_hundred_requests_never_query_a_provider() {
    let provider = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "data": [] }))
                .insert_header("x-ratelimit-limit-requests", "14400")
                .insert_header("x-ratelimit-remaining-requests", "14370")
                .insert_header("x-ratelimit-reset-requests", "2m59s")
                .insert_header("x-ratelimit-limit-tokens", "18000")
                .insert_header("x-ratelimit-remaining-tokens", "17997")
                .insert_header("x-ratelimit-reset-tokens", "2m59s"),
        )
        .mount(&provider)
        .await;
    let config = Config::from_toml(
        r#"
[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY"
"#,
    )
    .unwrap();
    let mut monitor = Monitor::with_credentials(config.clone(), Arc::new(Keys)).unwrap();
    monitor.groq_url = Some(provider.uri());
    let cycle = Arc::new(Mutex::new(RefreshCycle::new(&config, Instant::now())));
    let token = cycle.lock().begin(0).unwrap();
    let reading = monitor.refresh_account(0).await;
    assert_eq!(reading.status, Status::Ready);
    cycle
        .lock()
        .finish_account(0, Some(reading), token, Instant::now());
    let queried = provider.received_requests().await.unwrap().len();
    assert!(queried > 0);

    let shared = Arc::clone(&cycle);
    let identity = ConfigIdentity::Explicit("serve".into());
    let key = config.accounts[0].key();
    let server = start(ServeConfig::default(), move || {
        let cycle = shared.lock();
        Some(Export::from_cycle(
            &config,
            &identity,
            &cycle,
            &[],
            Utc::now(),
            Options::default(),
        ))
    })
    .await;
    let paths = [
        "/v1/snapshot".to_string(),
        "/v1/limits".into(),
        format!("/v1/accounts/{key}"),
        "/v1/snapshot?refresh=1".into(),
    ];
    for n in 0..100 {
        let response = get(&server, &paths[n % paths.len()]).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(
        provider.received_requests().await.unwrap().len(),
        queried,
        "no request reached the provider"
    );
    let snapshot = json(get(&server, "/v1/snapshot").await).await;
    assert_eq!(snapshot["accounts"][0]["report"]["status"], "ready");
}

#[tokio::test]
async fn connections_over_the_limit_are_closed_until_a_slot_frees() {
    let server = start(
        ServeConfig {
            bounds: Bounds {
                max_connections: 2,
                request_timeout: Duration::from_secs(30),
                ..Bounds::default()
            },
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    // Two idle clients hold both slots (accepted in order, before ours).
    let first = TcpStream::connect(server.addr()).await.unwrap();
    let _second = TcpStream::connect(server.addr()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        request(&server, Method::GET, "/health")
            .send()
            .await
            .is_err(),
        "a third connection is closed unanswered"
    );

    drop(first);
    let mut served = None;
    for _ in 0..100 {
        if let Ok(response) = request(&server, Method::GET, "/health").send().await {
            served = Some(response.status());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(served, Some(StatusCode::OK), "the freed slot serves again");
}

#[tokio::test]
async fn slow_and_oversized_requests_are_bounded() {
    let server = start(
        ServeConfig {
            bounds: Bounds {
                max_connections: 1,
                request_timeout: Duration::from_millis(300),
                ..Bounds::default()
            },
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    // A head that never ends is cut at the request timeout, unanswered,
    // and its slot is freed.
    let started = Instant::now();
    let answer = raw(
        server.addr(),
        b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n",
    )
    .await;
    assert!(answer.is_empty(), "{answer}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(get(&server, "/health").await.status(), StatusCode::OK);

    let big_header = request(&server, Method::GET, "/health")
        .header("x-pad", "a".repeat(9 * 1024))
        .send()
        .await
        .unwrap();
    assert_eq!(
        big_header.status(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    assert_json_no_store(&big_header);
    assert_eq!(json(big_header).await["error"], "headers_too_large");

    let mut many = request(&server, Method::GET, "/health");
    for n in 0..40 {
        many = many.header(format!("x-h{n}"), "1");
    }
    let many = many.send().await.unwrap();
    assert_eq!(many.status(), StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);

    let long_url = get(&server, &format!("/health?{}", "a".repeat(3 * 1024))).await;
    assert_eq!(long_url.status(), StatusCode::URI_TOO_LONG);
    assert_json_no_store(&long_url);

    let garbage = raw(server.addr(), b"NOT HTTP\r\n\r\n").await;
    assert!(garbage.starts_with("HTTP/1.1 400 "), "{garbage}");
    assert!(garbage.contains("Content-Type: application/json\r\n"));
    assert!(garbage.contains("Cache-Control: no-store\r\n"));
}

/// RF3.3: DNS rebinding sends the attacker's name in `Host`.
#[tokio::test]
async fn a_foreign_host_is_refused() {
    let server = start(ServeConfig::default(), fixture_source()).await;
    let port = server.addr().port();
    for host in [
        "evil.example".to_string(),
        format!("evil.example:{port}"),
        format!("localhost.evil.example:{port}"),
        format!("127.0.0.1.nip.io:{port}"),
        "192.168.1.20".into(),
        "localhost:".into(),
        "localhost:http".into(),
        "[::1".into(),
        String::new(),
    ] {
        let response = request(&server, Method::GET, "/v1/snapshot")
            .header(header::HOST, &host)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{host}");
        assert_json_no_store(&response);
        assert_eq!(json(response).await["error"], "host_not_allowed");
    }
    for host in [
        format!("localhost:{port}"),
        "LocalHost".into(),
        format!("127.0.0.1:{port}"),
        "127.0.0.1".into(),
        format!("[::1]:{port}"),
        "[::1]".into(),
        // A tunnel (`ssh -L 9000:localhost:6738`) keeps a loopback name.
        "localhost:9000".into(),
    ] {
        let response = request(&server, Method::GET, "/v1/snapshot")
            .header(header::HOST, &host)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{host}");
    }
    let addr = server.addr();
    for request in [
        "GET /health HTTP/1.0\r\n\r\n".to_string(),
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nHost: evil.example\r\n\r\n".into(),
    ] {
        let answer = raw(addr, request.as_bytes()).await;
        assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");
    }
}

/// RF3.2/RF3.3: a page's request carries `Origin` or a cross-site
/// `Sec-Fetch-Site`; without "allow browsers" it is refused.
#[tokio::test]
async fn browser_requests_are_refused_unless_allowed() {
    let strict = start(ServeConfig::default(), fixture_source()).await;
    for (name, value) in [
        ("origin", "https://evil.example"),
        ("origin", "null"),
        ("origin", "http://localhost:3000"),
        ("sec-fetch-site", "cross-site"),
        ("sec-fetch-site", "same-site"),
        ("sec-fetch-site", "same-origin"),
    ] {
        let response = request(&strict, Method::GET, "/v1/snapshot")
            .header(name, value)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{name}: {value}");
        assert_json_no_store(&response);
        assert_eq!(json(response).await["error"], "browser_not_allowed");
    }
    // Typed in the address bar: no page involved.
    let typed = request(&strict, Method::GET, "/v1/snapshot")
        .header("sec-fetch-site", "none")
        .send()
        .await
        .unwrap();
    assert_eq!(typed.status(), StatusCode::OK);
    assert_json_no_store(&typed);

    let open = start(
        ServeConfig {
            allow_browsers: true,
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    let page = request(&open, Method::GET, "/v1/snapshot")
        .header("origin", "https://dashboard.example")
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    let preflight = request(&open, Method::OPTIONS, "/v1/snapshot")
        .header("origin", "https://dashboard.example")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "Authorization"
    );
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
        "GET, HEAD"
    );
    // Rebinding stays blocked even with browsers allowed.
    let rebound = request(&open, Method::GET, "/v1/snapshot")
        .header(header::HOST, "evil.example")
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);
    assert_eq!(rebound.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    let writing = request(&open, Method::POST, "/v1/snapshot")
        .send()
        .await
        .unwrap();
    assert_eq!(writing.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_configured_token_is_required_on_every_route() {
    let token = Token::generate().unwrap();
    let server = start(
        ServeConfig {
            token: Some(token.clone()),
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    let wrong = Token::generate().unwrap();
    for path in ["/health", "/v1/snapshot", "/v1/limits", "/nope"] {
        for authorization in [
            None,
            Some(format!("Bearer {}", wrong.expose())),
            Some(format!("Basic {}", token.expose())),
            Some(format!("Bearer {}x", token.expose())),
            Some("Bearer".into()),
        ] {
            let mut builder = request(&server, Method::GET, path);
            if let Some(value) = &authorization {
                builder = builder.header(header::AUTHORIZATION, value);
            }
            let response = builder.send().await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{path} {authorization:?}"
            );
            assert_json_no_store(&response);
            assert_eq!(
                response.headers()[header::WWW_AUTHENTICATE],
                "Bearer realm=\"cuotai\""
            );
            let body = response.text().await.unwrap();
            assert_eq!(body, r#"{"error":"unauthorized"}"#);
        }
    }
    for scheme in ["Bearer", "bearer"] {
        let response = request(&server, Method::GET, "/v1/snapshot")
            .header(
                header::AUTHORIZATION,
                format!("{scheme} {}", token.expose()),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    // Two credentials are ambiguous, even if one is right.
    let twice = raw(
        server.addr(),
        format!(
            "GET /health HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\n\
             Authorization: Bearer {}\r\n\r\n",
            token.expose(),
            wrong.expose()
        )
        .as_bytes(),
    )
    .await;
    assert!(twice.starts_with("HTTP/1.1 401 "), "{twice}");
}

#[tokio::test]
async fn a_non_loopback_bind_needs_the_explicit_flag_and_a_token() {
    for bind in ["192.0.2.10", "0.0.0.0", "::"] {
        let bind: IpAddr = bind.parse().unwrap();
        let refused = LocalServer::start(
            ServeConfig {
                bind,
                port: 0,
                ..ServeConfig::default()
            },
            fixture_source(),
        )
        .await;
        assert_eq!(
            refused.err(),
            Some(ServeError::NonLoopback { addr: bind }),
            "{bind}"
        );
        let flagged = ServeConfig {
            bind,
            allow_non_loopback: true,
            ..ServeConfig::default()
        };
        assert_eq!(flagged.validate(), Err(ServeError::TokenRequired));
        let with_token = ServeConfig {
            token: Some(Token::generate().unwrap()),
            ..flagged
        };
        assert_eq!(with_token.validate(), Ok(()));
    }
    for loopback in ["127.0.0.1", "::1"] {
        let config = ServeConfig {
            bind: loopback.parse().unwrap(),
            ..ServeConfig::default()
        };
        assert_eq!(config.validate(), Ok(()), "{loopback}");
    }
}

/// A held port below the usual ephemeral ranges (where the other tests'
/// sockets live), whose next ports are free right now.
fn held_port() -> std::net::TcpListener {
    (20_000u16..30_000)
        .step_by(37)
        .find_map(|port| {
            let held = std::net::TcpListener::bind(("127.0.0.1", port)).ok()?;
            (1..PORT_ATTEMPTS)
                .all(|n| std::net::TcpListener::bind(("127.0.0.1", port + n)).is_ok())
                .then_some(held)
        })
        .expect("a free port range")
}

#[tokio::test]
async fn a_taken_port_falls_back_to_the_next_one_unless_strict() {
    let taken = held_port();
    let port = taken.local_addr().unwrap().port();
    let server = LocalServer::start(
        ServeConfig {
            port,
            port_fallback: true,
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await
    .unwrap();
    assert!(server.fell_back());
    assert!(
        (port + 1..port + PORT_ATTEMPTS).contains(&server.addr().port()),
        "{} after {port}",
        server.addr()
    );
    assert_eq!(get(&server, "/health").await.status(), StatusCode::OK);

    let strict = LocalServer::start(
        ServeConfig {
            port,
            port_fallback: false,
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    assert_eq!(
        strict.err(),
        Some(ServeError::PortInUse {
            first: port,
            last: port
        })
    );
    let ephemeral = start(ServeConfig::default(), fixture_source()).await;
    assert!(!ephemeral.fell_back());
}

/// RF3.4: neither answers nor the access log carry the token, credentials,
/// credential variable names or query parameters.
#[tokio::test]
async fn answers_and_access_log_carry_no_secrets() {
    let (config, _) = fixture();
    let token = Token::generate().unwrap();
    let entries = Arc::new(Mutex::new(Vec::<AccessEntry>::new()));
    let log = Arc::clone(&entries);
    let server = start(
        ServeConfig {
            token: Some(token.clone()),
            access_log: Some(Arc::new(move |entry| log.lock().push(entry.clone()))),
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    let bearer = format!("Bearer {}", token.expose());
    let mut bodies = Vec::new();
    for (path, authorization) in [
        (
            "/v1/snapshot?api_key=sk-QUERY&token=t0p".to_string(),
            bearer.clone(),
        ),
        (
            format!("/v1/accounts/{}?x=sk-QUERY", config.accounts[1].key()),
            bearer.clone(),
        ),
        ("/v1/limits".into(), bearer.clone()),
        (
            "/health?token=sk-QUERY".into(),
            "Bearer sk-WRONG-GUESS".into(),
        ),
    ] {
        let response = request(&server, Method::GET, &path)
            .header(header::AUTHORIZATION, authorization)
            .send()
            .await
            .unwrap();
        bodies.push(response.text().await.unwrap());
    }
    let snapshot: Value = serde_json::from_str(&bodies[0]).unwrap();
    assert_eq!(
        snapshot["accounts"][1]["report"]["status"],
        "authentication_error"
    );
    for body in &bodies {
        for needle in [
            token.expose(),
            "OPENAI_ADMIN_KEY",
            "hunter2",
            "sk-SOURCE",
            "sk-QUERY",
            "sk-WRONG",
        ] {
            assert!(!body.contains(needle), "{needle} in {body}");
        }
    }

    let entries = entries.lock().clone();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].path, "/v1/snapshot");
    assert_eq!(entries[0].status, 200);
    assert_eq!(
        entries[1].path,
        format!("/v1/accounts/{}", config.accounts[1].key())
    );
    assert_eq!(entries[3].status, 401);
    for entry in &entries {
        let line = entry.to_string();
        assert!(line.contains(" GET /"), "{line}");
        for needle in [token.expose(), "Bearer", "sk-QUERY", "sk-WRONG", "t0p", "?"] {
            assert!(!line.contains(needle), "{needle} in {line}");
        }
    }
}

#[tokio::test]
async fn shutdown_closes_the_port() {
    let server = start(ServeConfig::default(), fixture_source()).await;
    let addr = server.addr();
    assert_eq!(get(&server, "/health").await.status(), StatusCode::OK);
    server.shutdown().await;
    assert!(TcpStream::connect(addr).await.is_err(), "nothing listens");
}

/// `LocalServer::reconfigure` is the seam that fixes the macOS CI race:
/// Settings → Integration's `allow_browsers` flip on the same port must not
/// close and reopen the listener. The new policy must be observable on the
/// very next request after `reconfigure` returns.
#[tokio::test]
async fn reconfigure_keeps_the_listener_and_swaps_policy_in_place() {
    let server = start(ServeConfig::default(), fixture_source()).await;
    let addr = server.addr();
    // Without `allow_browsers`, a browser-origin request is refused: this
    // proves the running policy is the initial one.
    let refused = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .request(Method::GET, format!("{}{}", server.url(), "/health"))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    // Hot swap to allow browsers on the same listener. We pass `port: 0`
    // (matches the requested port on the running server) and the same
    // bind and `port_fallback` to keep the binding identity.
    let result = server
        .reconfigure(
            ServeConfig {
                port: 0,
                allow_browsers: true,
                ..ServeConfig::default()
            },
            fixture_source(),
        )
        .await
        .expect("same-binding reconfigure is live");
    assert!(matches!(result, Reconfigure::Replaced));

    // The bound address is identical, so the listener fd was never dropped.
    assert_eq!(server.addr(), addr);

    // The next request observes the swapped policy: browser-origin succeeds.
    let page = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .request(Method::GET, format!("{}{}", server.url(), "/health"))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
}

/// A change in binding identity must NOT be applied in place: the listener is
/// still serving the old binding and the caller is told it must start a new
/// `LocalServer` rather than accepting a wrong-port snapshot.
#[tokio::test]
async fn reconfigure_refuses_a_binding_change_and_does_not_swap() {
    let server = start(ServeConfig::default(), fixture_source()).await;
    let addr_before = server.addr();
    let result = server
        .reconfigure(
            ServeConfig {
                port: 1,
                ..ServeConfig::default()
            },
            fixture_source(),
        )
        .await
        .expect("identity check is a config validation, not a bind");
    assert_eq!(
        result,
        Reconfigure::RestartRequired,
        "bind change must refuse"
    );
    assert_eq!(server.addr(), addr_before, "listener identity unchanged");
}

/// `max_connections` and `request_timeout` are accept-loop tunables that
/// own live resources (a semaphore and the connection drain timeout);
/// changing either without recreating the loop would silently keep the old
/// values. Force a restart instead.
#[tokio::test]
async fn reconfigure_refuses_max_connections_or_request_timeout_change() {
    let server = start(ServeConfig::default(), fixture_source()).await;
    let addr_before = server.addr();
    let result = server
        .reconfigure(
            ServeConfig {
                bounds: Bounds {
                    max_connections: 4,
                    ..Bounds::default()
                },
                ..ServeConfig::default()
            },
            fixture_source(),
        )
        .await
        .expect("max_connections check is config-only");
    assert_eq!(result, Reconfigure::RestartRequired);
    assert_eq!(server.addr(), addr_before);

    let result = server
        .reconfigure(
            ServeConfig {
                bounds: Bounds {
                    request_timeout: Duration::from_secs(2),
                    ..Bounds::default()
                },
                ..ServeConfig::default()
            },
            fixture_source(),
        )
        .await
        .expect("request_timeout check is config-only");
    assert_eq!(result, Reconfigure::RestartRequired);
    assert_eq!(server.addr(), addr_before);
}

#[test]
fn generated_tokens_are_random_parseable_and_never_printed() {
    let token = Token::generate().unwrap();
    let other = Token::generate().unwrap();
    assert_ne!(token, other);
    assert!(token.expose().starts_with("cuotai_"));
    assert_eq!(token.expose().len(), "cuotai_".len() + 43);
    assert_eq!(
        Token::parse(&format!("  {}\n", token.expose())),
        Ok(token.clone())
    );

    let config = ServeConfig {
        token: Some(token.clone()),
        ..ServeConfig::default()
    };
    for printed in [format!("{token:?}"), format!("{config:?}")] {
        assert!(!printed.contains(token.expose()), "{printed}");
        assert!(printed.contains("[redacted]"));
    }

    assert!(Token::parse("abcdefghijklmnop").is_ok());
    assert!(Token::parse("dGhpcyBpcyBhIHRva2Vu==").is_ok());
    for invalid in [
        "",
        "short",
        "has a space inside it",
        "tabbed\ttoken-value-1234",
        "acentuação-não-vale-1234",
        "================",
        &"a".repeat(513),
    ] {
        assert_eq!(
            Token::parse(invalid),
            Err(ServeError::InvalidToken),
            "{invalid}"
        );
    }
}

#[derive(Default)]
struct MemoryVault(Mutex<Option<Token>>);

impl TokenVault for MemoryVault {
    fn load(&self) -> Result<Option<Token>, ServeError> {
        Ok(self.0.lock().clone())
    }
    fn save(&self, token: &Token) -> Result<(), ServeError> {
        *self.0.lock() = Some(token.clone());
        Ok(())
    }
    fn delete(&self) -> Result<(), ServeError> {
        *self.0.lock() = None;
        Ok(())
    }
}

struct BrokenVault;

impl TokenVault for BrokenVault {
    fn load(&self) -> Result<Option<Token>, ServeError> {
        Ok(None)
    }
    fn save(&self, _: &Token) -> Result<(), ServeError> {
        Err(ServeError::Vault)
    }
    fn delete(&self) -> Result<(), ServeError> {
        Err(ServeError::Vault)
    }
}

#[test]
fn ensure_token_generates_once_and_keeps_it_until_revoked() {
    let vault = MemoryVault::default();
    let first = ensure_token(&vault).unwrap();
    assert_eq!(vault.load().unwrap(), Some(first.clone()), "stored");
    assert_eq!(ensure_token(&vault).unwrap(), first, "reused, not rotated");
    vault.delete().unwrap();
    let second = ensure_token(&vault).unwrap();
    assert_ne!(second, first, "a revoked token never comes back");
    // A token the vault could not keep is never handed out.
    assert_eq!(ensure_token(&BrokenVault), Err(ServeError::Vault));
}

#[test]
fn errors_and_warning_are_translated() {
    let texts = [
        ServeError::NonLoopback {
            addr: "192.0.2.10".parse().unwrap(),
        }
        .l10n(),
        ServeError::TokenRequired.l10n(),
        ServeError::PortInUse {
            first: 6738,
            last: 6747,
        }
        .l10n(),
        ServeError::Bind { port: 80 }.l10n(),
        ServeError::InvalidToken.l10n(),
        ServeError::Vault.l10n(),
        ServeError::Random.l10n(),
        allow_browsers_warning(),
        ServeError::PortInUse {
            first: 6738,
            last: 6738,
        }
        .l10n(),
        ServeError::TokenMissing.l10n(),
        ServeError::TokenFile.l10n(),
        no_token_warning(),
    ];
    for locale in ["en-US", "es", "it"] {
        let catalog = core_catalog(locale).unwrap();
        for text in &texts {
            assert!(
                catalog.contains_key(&*text.id),
                "{locale} lacks {}",
                text.id
            );
        }
    }
    assert_eq!(
        texts[2].resolve("en-US"),
        "Ports 6738 to 6747 are in use; choose another port for the local server."
    );
    assert!(texts[0].resolve("es").contains("192.0.2.10"));
    assert!(texts[2].text().contains("6738 a 6747"));
    // Only the requested port was tried: no "6738 to 6738".
    assert_eq!(
        texts[8].resolve("en-US"),
        "Port 6738 is in use; choose another port for the local server."
    );
    assert_eq!(
        texts[8].text(),
        "A porta 6738 está em uso; escolha outra porta para o servidor local."
    );
}

/// A free loopback port: bound once by the system, then released.
async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    listener.local_addr().unwrap().port()
}

/// A listener on an ephemeral port that answers every connection with
/// `answer` and closes it.
async fn answering(answer: &'static str) -> u16 {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut head = [0u8; 1024];
            let _ = stream.read(&mut head).await;
            let _ = stream.write_all(answer.as_bytes()).await;
        }
    });
    port
}

#[tokio::test]
async fn probe_names_only_a_cuotai_server_on_the_port() {
    let open = start(ServeConfig::default(), fixture_source()).await;
    assert!(is_cuotai_server(open.addr().port()).await, "its /health");
    let guarded = start(
        ServeConfig {
            token: Some(Token::generate().unwrap()),
            ..ServeConfig::default()
        },
        fixture_source(),
    )
    .await;
    assert!(
        is_cuotai_server(guarded.addr().port()).await,
        "its 401 with Cuotai's realm, without knowing the token"
    );
    // Another program on the port, or nothing at all, is not Cuotai.
    let other = answering(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}",
    )
    .await;
    assert!(!is_cuotai_server(other).await, "health-like, not Cuotai's");
    let challenge = answering(
        "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"other\"\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(!is_cuotai_server(challenge).await, "another realm");
    let silent = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let started = Instant::now();
    assert!(!is_cuotai_server(silent.local_addr().unwrap().port()).await);
    assert!(started.elapsed() < PROBE_TIMEOUT * 2, "bounded wait");
    assert!(
        !is_cuotai_server(free_port().await).await,
        "nothing listens"
    );
    open.shutdown().await;
    guarded.shutdown().await;
}

/// INT-04: the default token is created once, privately, and reused; the
/// token sources resolve to the token and the notices the CLI prints.
#[test]
fn the_default_token_file_is_created_privately_and_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cuotai").join("serve").join("token");
    let first = ensure_token_file(&path).unwrap();
    assert!(first.expose().starts_with("cuotai_"));
    assert_eq!(ensure_token_file(&path).unwrap(), first, "reused");
    assert_eq!(read_token_file(&path).unwrap(), first);
    assert!(!token_file_exposed(&path));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        // A widened file is narrowed again on the next start.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(token_file_exposed(&path));
        assert_eq!(ensure_token_file(&path).unwrap(), first);
        assert_eq!(mode(&path), 0o600);
    }

    let resolved = resolve_token(&TokenSource::Default, Some(&path)).unwrap();
    assert_eq!(resolved.token, Some(first.clone()));
    assert_eq!(resolved.notices[0].id, "serve.token.default_file");
    let notice = resolved.notices[0].resolve("en-US");
    assert!(notice.contains(&path.display().to_string()), "{notice}");
    assert!(!notice.contains(first.expose()), "never the token");
    assert_eq!(
        resolve_token(&TokenSource::Default, None).unwrap_err(),
        ServeError::TokenFile
    );

    let none = resolve_token(&TokenSource::None, Some(&path)).unwrap();
    assert_eq!(none.token, None);
    assert_eq!(none.notices, [no_token_warning()]);
}

/// INT-04: a user token file is read as it is, warns when others can read
/// it, and errors never name the path nor echo the content.
#[test]
fn a_user_token_file_is_read_and_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("token");
    std::fs::write(&path, "cuotai_user-token-0123456789abcdef\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let resolved = resolve_token(&TokenSource::File(path.clone()), None).unwrap();
        assert_eq!(
            resolved.token.unwrap().expose(),
            "cuotai_user-token-0123456789abcdef"
        );
        assert_eq!(resolved.notices[0].id, "serve.token.file_exposed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let resolved = resolve_token(&TokenSource::File(path.clone()), None).unwrap();
    assert!(resolved.notices.is_empty());

    std::fs::write(&path, "short-secret-marker\n bad").unwrap();
    let error = resolve_token(&TokenSource::File(path.clone()), None).unwrap_err();
    assert_eq!(error, ServeError::InvalidToken);
    let missing = dir.path().join("missing");
    let error = resolve_token(&TokenSource::File(missing.clone()), None).unwrap_err();
    assert_eq!(error, ServeError::TokenFile);
    let text = format!("{error} {}", error.l10n().resolve("en-US"));
    assert!(!text.contains(&missing.display().to_string()), "{text}");
    // A folder is no token file, and never one to create a token in.
    assert_eq!(
        read_token_file(dir.path()).unwrap_err(),
        ServeError::TokenFile
    );
    assert_eq!(
        ensure_token_file(dir.path()).unwrap_err(),
        ServeError::TokenFile
    );
}

/// INT-04: a default token file that is a symbolic link is never trusted.
#[cfg(unix)]
#[test]
fn a_linked_default_token_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("elsewhere");
    std::fs::write(&target, "cuotai_linked-token-0123456789abcdef\n").unwrap();
    let serve = dir.path().join("serve");
    std::fs::create_dir(&serve).unwrap();
    let path = serve.join("token");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert_eq!(ensure_token_file(&path).unwrap_err(), ServeError::TokenFile);
}
