//! PRD 0052 RF6.2: HTTPS requests of the core clients go through
//! `HTTPS_PROXY` as a `CONNECT` tunnel, `NO_PROXY` bypasses it, the loopback
//! clients never use it, and the proxy's userinfo never reaches
//! `network::proxy_state()`. One test in its own binary, because the proxy
//! variables are process-wide.

use std::time::Duration;

use cuotai_core::network::{client, proxy_state, ClientPurpose};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// The head a client sent to the fake proxy: its request line and whether
/// it carried `Proxy-Authorization`.
#[derive(Debug)]
struct Head {
    line: String,
    proxy_auth: bool,
}

/// A fake proxy on loopback that records each request head and refuses the
/// tunnel (no TLS is ever completed).
async fn fake_proxy() -> (u16, mpsc::UnboundedReceiver<Head>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&head).into_owned();
                let line = text.lines().next().unwrap_or_default().to_string();
                let proxy_auth = text
                    .lines()
                    .any(|l| l.to_ascii_lowercase().starts_with("proxy-authorization:"));
                let _ = tx.send(Head { line, proxy_auth });
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            });
        }
    });
    (port, rx)
}

async fn attempt(purpose: ClientPurpose, url: &str) {
    let result = client(purpose, Duration::from_secs(5))
        .expect("client")
        .get(url)
        .send()
        .await;
    assert!(result.is_err(), "{purpose:?}: the tunnel is refused");
}

#[tokio::test]
async fn https_goes_through_a_connect_tunnel_and_no_proxy_bypasses_it() {
    let (port, mut heads) = fake_proxy().await;
    // Edition 2021: plain calls; this binary has a single test.
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        std::env::remove_var(name);
    }
    std::env::set_var(
        "HTTPS_PROXY",
        format!("http://seeduser:seedpass@127.0.0.1:{port}"),
    );

    for purpose in [
        ClientPurpose::Provider,
        ClientPurpose::OAuth,
        ClientPurpose::UpdateCheck,
    ] {
        attempt(purpose, "https://quota.example.com/v1/usage?key=x").await;
        let head = heads.recv().await.expect("the proxy saw the request");
        assert_eq!(
            head.line, "CONNECT quota.example.com:443 HTTP/1.1",
            "{purpose:?}"
        );
        assert!(
            head.proxy_auth,
            "{purpose:?}: userinfo is used, as reqwest does"
        );
    }

    // Diagnostics show the endpoint without userinfo.
    let state = proxy_state();
    assert_eq!(state.state, "proxied");
    assert_eq!(state.source, Some("HTTPS_PROXY"));
    assert_eq!(
        state.endpoint.as_deref(),
        Some(format!("http://127.0.0.1:{port}").as_str())
    );
    let json = serde_json::to_string(&state).unwrap();
    assert!(!json.contains("seed"), "{json}");

    // A closed loopback port: a direct attempt fails at once without the
    // proxy hearing anything.
    let direct = "https://127.0.0.1:1/v1/usage";
    for purpose in [ClientPurpose::LocalProbe, ClientPurpose::LocalServer] {
        attempt(purpose, direct).await;
    }
    std::env::set_var("NO_PROXY", "127.0.0.1");
    attempt(ClientPurpose::Provider, direct).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        heads.try_recv().is_err(),
        "loopback and NO_PROXY bypass the proxy"
    );
    assert!(proxy_state().bypass_list);

    std::env::remove_var("HTTPS_PROXY");
    std::env::remove_var("NO_PROXY");
}
