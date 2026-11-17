//! PRD 0052 RF6/RF7 — the HTTP client contract and the server-clock check.
//!
//! - [`builder`] is where every core client starts: redirects off (the
//!   update check follows HTTPS only), connect and request timeouts, the
//!   `cuotai/<version>` user agent, the environment's proxies
//!   (`HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY` and `NO_PROXY`, upper or lower
//!   case, as reqwest reads them; the loopback probe never uses one) and
//!   **only the operating system's trust store**: the bundled webpki roots
//!   are never loaded, so an administrator-installed corporate root works
//!   and no machine silently trusts a different set.
//! - [`Route::for_url`] (PRD 0036 re-validation S-02): a request to a
//!   plain-HTTP address, or to HTTPS on a loopback, private-network,
//!   `localhost`, `.local` or `.ts.net` host, never uses the environment's
//!   proxy; [`builder_for`], [`builder_with_own_roots_for`] and
//!   [`direct_client`] apply it. Public HTTPS keeps the proxy.
//! - [`proxy_state`] describes the effective proxy for diagnostics without
//!   userinfo or the raw `NO_PROXY` list.
//! - [`ClockSkew`] compares a response's `Date` with the local clock and
//!   warns when they differ by more than five minutes; it never changes a
//!   reading, a schedule or the clock.

use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::diagnostics::logging::{self, Field, Level};
use crate::text::{params, Localized};

/// What a client is for (the network rows of `docs/privacy.md` name the
/// same flows, see [`crate::privacy::Purpose`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPurpose {
    /// Provider APIs and model lists.
    Provider,
    /// Public provider status pages (opt-in incident status, PRD 0011). The
    /// requests themselves go through the Monitor's shared client (RF2.1);
    /// this purpose labels their observations and probes.
    PublicStatus,
    /// Sign-in and token refresh.
    OAuth,
    /// The terminal's update notice.
    UpdateCheck,
    /// A 9router instance on loopback (never through a proxy).
    LocalProbe,
    /// PRD 0044: the notification channels the person added.
    Channels,
    /// A running Cuotai local server read back on loopback (the status line
    /// and the feed reader; never through a proxy).
    LocalServer,
}

impl ClientPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::PublicStatus => "public_status",
            Self::OAuth => "oauth",
            Self::UpdateCheck => "update_check",
            Self::LocalProbe => "local_probe",
            Self::Channels => "channels",
            Self::LocalServer => "local_server",
        }
    }

    /// The inventory row this client belongs to.
    pub fn privacy_purpose(self) -> crate::privacy::Purpose {
        match self {
            Self::Provider => crate::privacy::Purpose::ProviderApi,
            Self::PublicStatus => crate::privacy::Purpose::PublicStatus,
            Self::OAuth => crate::privacy::Purpose::OAuth,
            Self::UpdateCheck => crate::privacy::Purpose::UpdateCheck,
            Self::LocalProbe => crate::privacy::Purpose::LocalProbe,
            Self::Channels => crate::privacy::Purpose::NotificationChannels,
            Self::LocalServer => crate::privacy::Purpose::LocalServer,
        }
    }

    pub const ALL: [ClientPurpose; 7] = [
        Self::Provider,
        Self::PublicStatus,
        Self::OAuth,
        Self::UpdateCheck,
        Self::LocalProbe,
        Self::Channels,
        Self::LocalServer,
    ];
}

/// The OS trust store could not provide any root certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustStoreError;

impl TrustStoreError {
    pub fn l10n(self) -> Localized {
        Localized::fixed(
            "network.trust_store_empty",
            "No trusted root certificate could be loaded from the system's certificate store, so no secure connection can be made. Repair or update the system certificates and start Cuotai again.",
        )
    }
}

/// The OS root certificates, loaded once per process. Certificates the
/// platform cannot parse are skipped; none at all is an error (no
/// fallback to bundled roots).
pub fn native_roots() -> Result<&'static [reqwest::Certificate], TrustStoreError> {
    static ROOTS: OnceLock<Vec<reqwest::Certificate>> = OnceLock::new();
    let roots = ROOTS.get_or_init(|| {
        let loaded = rustls_native_certs::load_native_certs();
        loaded
            .certs
            .iter()
            .filter_map(|der| reqwest::Certificate::from_der(der.as_ref()).ok())
            .collect()
    });
    if roots.is_empty() {
        logging::event(Level::Error, "network.trust_store_empty", &[]);
        Err(TrustStoreError)
    } else {
        Ok(roots)
    }
}

/// The user agent of every core client.
pub const USER_AGENT: &str = concat!("cuotai/", env!("CARGO_PKG_VERSION"));

/// A client builder for `purpose` with `timeout` (see the module
/// documentation). Callers may add purpose-specific settings (a pinned
/// TLS configuration replaces the roots on its own).
///
/// PRD 0050 RF7.2: this and [`builder_with_own_roots`] are the only places
/// a non-test client may be built (`cargo xtask security-baseline` refuses
/// any other `reqwest::Client::builder()`), so the redirect policy below
/// covers every credential-bearing request.
pub fn builder(purpose: ClientPurpose, timeout: Duration) -> reqwest::ClientBuilder {
    let mut builder = builder_with_own_roots(purpose, timeout);
    for root in native_roots().unwrap_or_default() {
        builder = builder.add_root_certificate(root.clone());
    }
    builder
}

/// [`builder`] without the OS roots, for a client that brings its own trust
/// (the 9router CA file or certificate pin) or speaks plain HTTP on
/// loopback. Everything else (redirects, timeouts, user agent, proxies) is
/// the same.
pub fn builder_with_own_roots(purpose: ClientPurpose, timeout: Duration) -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout.min(Duration::from_secs(10)))
        .user_agent(USER_AGENT)
        .tls_built_in_root_certs(false);
    builder = match purpose {
        ClientPurpose::UpdateCheck => {
            builder.redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() < 4 && attempt.url().scheme() == "https" {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
        }
        _ => builder.redirect(reqwest::redirect::Policy::none()),
    };
    if matches!(
        purpose,
        ClientPurpose::LocalProbe | ClientPurpose::LocalServer
    ) {
        builder = builder.no_proxy();
    }
    builder
}

/// PRD 0036 re-validation S-02: how a request to a user-set address may
/// leave this computer.
///
/// - [`Route::Direct`]: never through an environment proxy. Any plain-HTTP
///   address (the secret would reach the proxy unencrypted, and the proxy
///   would connect to *its own* loopback or private network), and HTTPS to
///   a loopback or private host (RFC 1918, link-local, `100.64.0.0/10`,
///   ULA) or a `localhost`, `.local` or `.ts.net` name, which a corporate
///   proxy cannot reach anyway.
/// - [`Route::Environment`]: public HTTPS keeps the environment's proxy
///   (`HTTPS_PROXY`/`ALL_PROXY`, `NO_PROXY`): reqwest tunnels it with
///   `CONNECT`, so TLS stays end to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Direct,
    Environment,
}

impl Route {
    /// The route of `url` (see the type).
    pub fn for_url(url: &url::Url) -> Self {
        if url.scheme() != "https" {
            return Self::Direct;
        }
        match url.host() {
            Some(host)
                if crate::transport::is_loopback_host(&host)
                    || crate::transport::is_private_network(&host) =>
            {
                Self::Direct
            }
            Some(_) => Self::Environment,
            None => Self::Direct,
        }
    }
}

/// Apply [`Route::for_url`] to a builder: `proxy` (tests: a stand-in for
/// the environment's proxy) is added first, then a direct route clears
/// every proxy, the environment's included.
fn routed(
    mut builder: reqwest::ClientBuilder,
    url: &url::Url,
    proxy: Option<reqwest::Proxy>,
) -> reqwest::ClientBuilder {
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    }
    if Route::for_url(url) == Route::Direct {
        builder = builder.no_proxy();
    }
    builder
}

/// [`builder`] for a client that reaches `url` (S-02 proxy rule).
pub fn builder_for(
    purpose: ClientPurpose,
    timeout: Duration,
    url: &url::Url,
) -> reqwest::ClientBuilder {
    builder_for_with(purpose, timeout, url, None)
}

/// [`builder_for`] with an injected proxy (tests), applied before the rule.
pub fn builder_for_with(
    purpose: ClientPurpose,
    timeout: Duration,
    url: &url::Url,
    proxy: Option<reqwest::Proxy>,
) -> reqwest::ClientBuilder {
    routed(builder(purpose, timeout), url, proxy)
}

/// [`builder_with_own_roots`] for a client that reaches `url` (the 9router
/// CA-file, pinned and capture clients), with an optional injected proxy
/// (tests) applied before the rule.
pub fn builder_with_own_roots_for(
    purpose: ClientPurpose,
    timeout: Duration,
    url: &url::Url,
    proxy: Option<reqwest::Proxy>,
) -> reqwest::ClientBuilder {
    routed(builder_with_own_roots(purpose, timeout), url, proxy)
}

/// A client for `purpose` that never uses a proxy: what a [`Route::Direct`]
/// request goes through when the caller keeps one shared client per route.
pub fn direct_client(purpose: ClientPurpose, timeout: Duration) -> Option<reqwest::Client> {
    builder(purpose, timeout).no_proxy().build().ok()
}

/// A built client for `purpose`; `None` when the TLS backend cannot start.
/// An empty trust store still builds one (logged by [`native_roots`]): it
/// trusts no server, so every HTTPS request fails as a per-account transport
/// error instead of the whole process stopping before it can classify the
/// accounts.
pub fn client(purpose: ClientPurpose, timeout: Duration) -> Option<reqwest::Client> {
    builder(purpose, timeout).build().ok()
}

/// The effective proxy of the HTTPS clients, as diagnostics show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProxyState {
    /// `none`, `proxied` or `invalid`.
    pub state: &'static str,
    /// The variable that selected it (`HTTPS_PROXY`, `https_proxy`, …).
    pub source: Option<&'static str>,
    /// `scheme://host:port` of the proxy, without userinfo, path or query.
    pub endpoint: Option<String>,
    /// Whether a `NO_PROXY` list is set (its entries are never shown).
    pub bypass_list: bool,
}

const PROXY_VARS: [&str; 4] = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

/// [`ProxyState`] from the process environment.
pub fn proxy_state() -> ProxyState {
    proxy_state_from(|name| std::env::var(name).ok())
}

/// [`ProxyState`] from any variable lookup (tests).
pub fn proxy_state_from(var: impl Fn(&str) -> Option<String>) -> ProxyState {
    let bypass_list = ["NO_PROXY", "no_proxy"]
        .iter()
        .any(|name| var(name).is_some_and(|v| !v.trim().is_empty()));
    let found = PROXY_VARS.iter().find_map(|name| {
        var(name)
            .filter(|v| !v.trim().is_empty())
            .map(|v| (*name, v))
    });
    let Some((source, raw)) = found else {
        return ProxyState {
            state: "none",
            source: None,
            endpoint: None,
            bypass_list,
        };
    };
    let with_scheme = if raw.contains("://") {
        raw.clone()
    } else {
        format!("http://{raw}")
    };
    match url::Url::parse(&with_scheme) {
        Ok(url) if url.host_str().is_some() => ProxyState {
            state: "proxied",
            source: Some(source),
            endpoint: Some(format!(
                "{}://{}:{}",
                url.scheme(),
                url.host_str().unwrap_or_default(),
                url.port_or_known_default().unwrap_or(0)
            )),
            bypass_list,
        },
        _ => ProxyState {
            state: "invalid",
            source: Some(source),
            endpoint: None,
            bypass_list,
        },
    }
}

/// The threshold above which a server's clock disagrees (RF7.1).
pub const SKEW_THRESHOLD: Duration = Duration::from_secs(5 * 60);

/// Agreeing samples that clear a warning.
const CLEARING_SAMPLES: u8 = 2;

/// Which way the local clock is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkewDirection {
    /// The local clock is ahead of the servers.
    Ahead,
    /// The local clock is behind the servers.
    Behind,
}

/// A process-level clock warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Skew {
    pub direction: SkewDirection,
    /// Magnitude rounded to whole minutes.
    pub minutes: u64,
}

impl Skew {
    /// The warning text (RF7.2/RF7.3): advice, never a claim about which
    /// clock is right.
    pub fn l10n(&self) -> Localized {
        let p = params([("minutes", self.minutes.into())]);
        match self.direction {
            SkewDirection::Ahead => Localized::with_params(
                "network.clock_ahead",
                p,
                "This computer's clock is about {minutes} min ahead of the providers' servers. Times, ages and resets may look wrong: turn on automatic date and time, then refresh.",
            ),
            SkewDirection::Behind => Localized::with_params(
                "network.clock_behind",
                p,
                "This computer's clock is about {minutes} min behind the providers' servers. Times, ages and resets may look wrong: turn on automatic date and time, then refresh.",
            ),
        }
    }
}

#[derive(Debug, Default)]
struct SkewState {
    warning: Option<Skew>,
    agreeing: u8,
    /// A valid server date was compared at least once.
    observed: bool,
}

/// RF7: the server-clock observer. One per process ([`clock_skew`]);
/// tests build their own.
#[derive(Debug, Default)]
pub struct ClockSkew {
    state: Mutex<SkewState>,
}

impl ClockSkew {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe the `Date` header of a response received at `received_at`.
    pub fn observe(
        &self,
        headers: &reqwest::header::HeaderMap,
        received_at: DateTime<Utc>,
        purpose: ClientPurpose,
    ) {
        let date = headers
            .get(reqwest::header::DATE)
            .and_then(|value| value.to_str().ok());
        self.observe_date(date, received_at, purpose);
    }

    /// [`Self::observe`] over the header's text. An absent or invalid date
    /// is ignored; a sample more than five minutes off raises (or updates)
    /// the warning; two samples within five minutes clear it.
    pub fn observe_date(
        &self,
        date: Option<&str>,
        received_at: DateTime<Utc>,
        purpose: ClientPurpose,
    ) {
        let Some(server) = date.and_then(parse_http_date) else {
            return;
        };
        let offset = (received_at - server).num_seconds();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.observed = true;
        if offset.unsigned_abs() > SKEW_THRESHOLD.as_secs() {
            let skew = Skew {
                direction: if offset > 0 {
                    SkewDirection::Ahead
                } else {
                    SkewDirection::Behind
                },
                minutes: (offset.unsigned_abs() + 30) / 60,
            };
            if state.warning.is_none() {
                logging::event(
                    Level::Warn,
                    "network.clock_skew",
                    &[
                        ("purpose", Field::Text(purpose.as_str())),
                        (
                            "direction",
                            Field::Text(match skew.direction {
                                SkewDirection::Ahead => "ahead",
                                SkewDirection::Behind => "behind",
                            }),
                        ),
                        ("seconds", Field::Count(offset.unsigned_abs() / 60 * 60)),
                    ],
                );
            }
            state.warning = Some(skew);
            state.agreeing = 0;
        } else if state.warning.is_some() {
            state.agreeing += 1;
            if state.agreeing >= CLEARING_SAMPLES {
                state.warning = None;
                state.agreeing = 0;
                logging::event(Level::Info, "network.clock_skew_cleared", &[]);
            }
        }
    }

    /// PRD 0046 RF4.1: whether any valid server date was compared yet (so
    /// no warning means the clocks agree, not that nothing was seen).
    pub fn observed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observed
    }

    /// The current warning, if any.
    pub fn warning(&self) -> Option<Skew> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .warning
    }
}

/// The process-wide observer every core response reports to.
pub fn clock_skew() -> &'static ClockSkew {
    static SKEW: OnceLock<ClockSkew> = OnceLock::new();
    SKEW.get_or_init(ClockSkew::new)
}

/// Report a response's `Date` to [`clock_skew`] (provider, OAuth and status
/// responses call this as they arrive).
pub fn observe(response: &reqwest::Response, purpose: ClientPurpose) {
    clock_skew().observe(response.headers(), Utc::now(), purpose);
}

/// An RFC 7231 `Date` (IMF-fixdate; RFC 2822 forms also parse).
fn parse_http_date(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc2822(text.trim())
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

/// Tests: a stand-in for an environment proxy that records the first line
/// of every request it receives (`CONNECT host:443 …` or `GET http://…`)
/// and answers 502, so a test can prove which requests went through it.
#[cfg(test)]
pub(crate) mod recording_proxy {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    pub(crate) struct RecordingProxy {
        pub url: String,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingProxy {
        pub(crate) async fn start() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let seen = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&seen);
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let log = Arc::clone(&log);
                    tokio::spawn(async move {
                        let mut buffer = vec![0u8; 4096];
                        let read = socket.read(&mut buffer).await.unwrap_or(0);
                        let text = String::from_utf8_lossy(&buffer[..read]);
                        let line = text.lines().next().unwrap_or_default().to_string();
                        log.lock().unwrap().push(line);
                        let _ = socket
                            .write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                            .await;
                    });
                }
            });
            Self { url, seen }
        }

        /// The proxy as reqwest takes it (for every scheme).
        pub(crate) fn proxy(&self) -> reqwest::Proxy {
            reqwest::Proxy::all(&self.url).unwrap()
        }

        /// The first line of each request received so far.
        pub(crate) fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap() + chrono::TimeDelta::seconds(secs)
    }

    const SERVER: &str = "Wed, 04 Nov 2026 12:00:00 GMT";

    /// PRD 0036 re-validation S-02: plain HTTP and local or private hosts go
    /// direct; public HTTPS keeps the environment's proxy.
    #[test]
    fn local_and_private_addresses_never_take_the_environment_proxy() {
        let route = |raw: &str| Route::for_url(&url::Url::parse(raw).unwrap());
        for direct in [
            "http://127.0.0.1:20128/",
            "http://localhost:4000/",
            "http://192.168.1.10:20128/",
            "http://api.example.com/",
            "https://127.0.0.1:20128/",
            "https://[::1]:8443/",
            "https://LOCALHOST/",
            "https://10.1.2.3/",
            "https://172.16.0.9/",
            "https://169.254.1.1/",
            "https://100.101.102.103/",
            "https://[fd12::1]/",
            "https://router.local/",
            "https://box.tail1234.ts.net/",
        ] {
            assert_eq!(route(direct), Route::Direct, "{direct}");
        }
        for proxied in [
            "https://api.example.com/v1",
            "https://8.8.8.8/",
            "https://example.localhost.example.com/",
            "https://ts.net.example.com/",
        ] {
            assert_eq!(route(proxied), Route::Environment, "{proxied}");
        }
    }

    /// The builder applies the rule after an injected proxy: a public HTTPS
    /// URL is tunnelled through it (`CONNECT`), a private HTTPS one is not.
    #[tokio::test]
    async fn the_routed_builder_keeps_the_proxy_only_for_public_https() {
        let proxy = recording_proxy::RecordingProxy::start().await;
        let public = url::Url::parse("https://api.example.com/v1/models").unwrap();
        let client = builder_for_with(
            ClientPurpose::Provider,
            Duration::from_secs(5),
            &public,
            Some(proxy.proxy()),
        )
        .build()
        .unwrap();
        assert!(client.get(public.clone()).send().await.is_err());
        assert_eq!(
            proxy.seen(),
            ["CONNECT api.example.com:443 HTTP/1.1".to_string()]
        );

        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = target.local_addr().unwrap().port();
        let accepted = tokio::spawn(async move { target.accept().await.is_ok() });
        let private = url::Url::parse(&format!("https://127.0.0.1:{port}/x")).unwrap();
        let client = builder_for_with(
            ClientPurpose::Provider,
            Duration::from_secs(2),
            &private,
            Some(proxy.proxy()),
        )
        .build()
        .unwrap();
        let _ = client.get(private).send().await;
        assert!(
            accepted.await.unwrap(),
            "the request went to the host itself"
        );
        assert_eq!(proxy.seen().len(), 1, "{:?}", proxy.seen());
    }

    #[test]
    fn exactly_five_minutes_is_not_a_warning_and_one_second_more_is() {
        for (offset, warns) in [(300, false), (-300, false), (301, true), (-301, true)] {
            let skew = ClockSkew::new();
            skew.observe_date(Some(SERVER), at(offset), ClientPurpose::Provider);
            assert_eq!(skew.warning().is_some(), warns, "offset {offset}");
        }
        let skew = ClockSkew::new();
        skew.observe_date(Some(SERVER), at(-600), ClientPurpose::OAuth);
        assert_eq!(
            skew.warning(),
            Some(Skew {
                direction: SkewDirection::Behind,
                minutes: 10
            })
        );
    }

    #[test]
    fn missing_or_malformed_dates_are_ignored() {
        let skew = ClockSkew::new();
        for date in [
            None,
            Some(""),
            Some("yesterday"),
            Some("Wed, 99 Nov 2026 12:00:00 GMT"),
        ] {
            skew.observe_date(date, at(10_000), ClientPurpose::Provider);
        }
        assert_eq!(skew.warning(), None);
    }

    #[test]
    fn hosts_coalesce_and_two_agreeing_samples_clear() {
        let skew = ClockSkew::new();
        skew.observe_date(Some(SERVER), at(900), ClientPurpose::Provider);
        skew.observe_date(Some(SERVER), at(1200), ClientPurpose::OAuth);
        assert_eq!(
            skew.warning().unwrap().minutes,
            20,
            "one warning, latest sample"
        );
        skew.observe_date(Some(SERVER), at(5), ClientPurpose::Provider);
        assert!(
            skew.warning().is_some(),
            "one agreeing sample is not enough"
        );
        skew.observe_date(Some(SERVER), at(900), ClientPurpose::Provider);
        skew.observe_date(Some(SERVER), at(0), ClientPurpose::Provider);
        assert!(skew.warning().is_some(), "a new skew restarts the count");
        skew.observe_date(Some(SERVER), at(0), ClientPurpose::Provider);
        assert_eq!(skew.warning(), None);
    }

    #[test]
    fn the_warning_text_names_the_direction_and_advises() {
        let text = Skew {
            direction: SkewDirection::Ahead,
            minutes: 12,
        }
        .l10n()
        .resolve("en-US");
        assert!(text.contains("12 min ahead") && text.contains("automatic date and time"));
    }

    #[test]
    fn proxy_state_hides_userinfo_and_the_bypass_list() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        let state = proxy_state_from(env(&[
            (
                "https_proxy",
                "http://seeduser:seedpass@proxy.example.com:3128/x?y=1",
            ),
            ("NO_PROXY", "internal.example.com,10.0.0.0/8"),
        ]));
        assert_eq!(state.state, "proxied");
        assert_eq!(state.source, Some("https_proxy"));
        assert_eq!(
            state.endpoint.as_deref(),
            Some("http://proxy.example.com:3128")
        );
        assert!(state.bypass_list);
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("seed") && !json.contains("internal"));
        assert_eq!(proxy_state_from(env(&[])).state, "none");
        assert_eq!(
            proxy_state_from(env(&[("HTTPS_PROXY", "http://[::bad")])).state,
            "invalid"
        );
    }

    #[test]
    fn every_client_purpose_has_a_privacy_row_and_builds() {
        for purpose in ClientPurpose::ALL {
            let row = crate::privacy::flow(purpose.privacy_purpose());
            assert!(!row.host.is_empty());
            assert!(builder(purpose, Duration::from_secs(5)).build().is_ok());
        }
        // PRD 0011 re-validation (S-02): status pages have their own row.
        assert_eq!(
            ClientPurpose::PublicStatus.privacy_purpose(),
            crate::privacy::Purpose::PublicStatus
        );
        assert_eq!(ClientPurpose::PublicStatus.as_str(), "public_status");
    }

    /// PRD 0050 RF7.2: through the seam, a 3xx from the first host never
    /// takes the `Authorization` or `Cookie` header to a second host, for
    /// every purpose (the update check follows only HTTPS), and the error a
    /// provider or OAuth caller returns names neither URL nor body.
    // security-invariant: SEC-REDIRECT
    #[tokio::test]
    async fn a_redirect_never_carries_credentials_to_a_second_host() {
        use wiremock::matchers::any;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        const TOKEN: &str = "sentinel-token-7f3a";
        const COOKIE: &str = "sid=sentinel-cookie-7f3a";
        const BODY: &str = "sentinel-body-7f3a";
        let second = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&second)
            .await;
        let first = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/steal", second.uri()).as_str())
                    .set_body_string(BODY),
            )
            .mount(&first)
            .await;
        let url = format!("{}/v1/usage?key=sentinel-query-7f3a", first.uri());
        let leaks = |text: &str| {
            ["sentinel", "127.0.0.1", "localhost", "/steal", "/v1/usage"]
                .iter()
                .any(|needle| text.contains(needle))
        };
        let seeded_request = |purpose: ClientPurpose| {
            client(purpose, Duration::from_secs(5))
                .unwrap()
                .get(&url)
                .bearer_auth(TOKEN)
                .header(reqwest::header::COOKIE, COOKIE)
        };

        let error =
            crate::providers::json::<serde_json::Value>(seeded_request(ClientPurpose::Provider))
                .await
                .unwrap_err();
        assert!(!leaks(&format!("{error:?}")), "{error:?}");
        assert!(!leaks(&error.note.to_string()), "{}", error.note);

        let response = crate::oauth::send_secure(seeded_request(ClientPurpose::OAuth))
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 302);
        let error = crate::oauth::status_failure(response.status());
        assert!(!leaks(&format!("{error} {error:?}")), "{error:?}");

        for purpose in ClientPurpose::ALL {
            let response = seeded_request(purpose).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 302, "{purpose:?}");
        }
        assert!(
            second.received_requests().await.unwrap().is_empty(),
            "the second host must receive nothing"
        );
    }
}
