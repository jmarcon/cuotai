//! PRD 0052 RF6.2: the core clients follow the environment's proxies and
//! `NO_PROXY`, except the loopback probe. One test in its own binary,
//! because the proxy variables are process-wide.

use std::time::Duration;

use cuotai_core::network::{client, ClientPurpose};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn answering(body: &'static str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/probe"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    server
}

async fn body(purpose: ClientPurpose, url: &str) -> String {
    client(purpose, Duration::from_secs(5))
        .expect("client")
        .get(url)
        .send()
        .await
        .expect("answered")
        .text()
        .await
        .unwrap()
}

#[tokio::test]
async fn clients_use_the_environment_proxy_and_honour_no_proxy() {
    // Both are local mock servers: the proxy answers "via-proxy" for any
    // request it relays, the target "direct".
    let proxy = answering("via-proxy").await;
    let target = answering("direct").await;
    let url = format!("{}/probe", target.uri());
    // Edition 2021: plain calls; this binary has a single test.
    std::env::set_var("HTTP_PROXY", proxy.uri());
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");

    for purpose in [
        ClientPurpose::Provider,
        ClientPurpose::OAuth,
        ClientPurpose::UpdateCheck,
    ] {
        assert_eq!(body(purpose, &url).await, "via-proxy", "{purpose:?}");
    }
    // The loopback probe never uses a proxy.
    assert_eq!(body(ClientPurpose::LocalProbe, &url).await, "direct");

    // A matching NO_PROXY entry bypasses the proxy.
    std::env::set_var("NO_PROXY", "127.0.0.1");
    assert_eq!(body(ClientPurpose::Provider, &url).await, "direct");
    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("NO_PROXY");
}
