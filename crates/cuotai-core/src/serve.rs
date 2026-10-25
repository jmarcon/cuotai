//! `serve::LocalServer` (PRD 0012 RF2/RF3): a read-only HTTP server that
//! publishes [`export::v1`](crate::export::v1) on loopback, behind the `serve`
//! feature (off by default in the core).
//!
//! The server never queries a provider: every data route asks a [`Source`]
//! for the [`Export`] the front end builds from the `RefreshCycle` it already
//! maintains, and answers `503` with `Retry-After` while there is no reading
//! yet. Routes are `GET` (and `HEAD`, without a body): `/health`, `/v1/snapshot`, `/v1/accounts/{key}`
//! and `/v1/limits`; any other method gets `405` with `Allow: GET, HEAD`, any other path `404`. Every
//! response is `application/json` with `Cache-Control: no-store`, and each
//! connection carries one request (`Connection: close`).
//!
//! Security (RF3): it binds to `127.0.0.1` (or `::1`) unless
//! [`ServeConfig::allow_non_loopback`] is set, which also requires a token;
//! the `Host` header must name loopback (DNS rebinding); a request carrying a
//! browser `Origin` (or a cross-site `Sec-Fetch-Site`) is refused and no
//! `Access-Control-Allow-Origin` is sent unless [`ServeConfig::allow_browsers`]
//! is on; an optional [`Token`] is required as `Authorization: Bearer`. The
//! access log ([`AccessEntry`]) never records headers or query parameters.
//! Without a token, **any local user or program** that can reach loopback
//! can read the data; headless front ends therefore require one by default
//! ([`TokenSource`], [`resolve_token`]). With a token, whoever can read it
//! (your own user's programs) can still read the data.

mod http;
mod token;

#[cfg(test)]
mod tests;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub use token::SystemTokenVault;
pub use token::{
    default_token_file, ensure_token, ensure_token_file, read_token_file, token_file_exposed,
    Token, TokenVault,
};

use std::fmt;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, RwLock, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::export::v1::{Export, Limits, SCHEMA};
use crate::text::{params, Localized, ParamValue};
use http::{Head, Read, Response};

/// Suggested port: unassigned in the IANA registry (6717–6766) and clear of
/// OpenUsage's 6736.
pub const DEFAULT_PORT: u16 = 6738;

/// Ports tried when the requested one is taken: it and the next nine.
pub const PORT_ATTEMPTS: u16 = 10;

/// `Retry-After` of a data route before the first reading.
pub const RETRY_AFTER_SECONDS: u64 = 5;

/// How long [`is_cuotai_server`] waits for an answer.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// Bytes of an answer [`is_cuotai_server`] reads at most.
const PROBE_MAX_BYTES: u64 = 4 * 1024;

/// The `WWW-Authenticate` of a `401`, which also tells [`is_cuotai_server`]
/// that a server needing a token is Cuotai's.
const BEARER_CHALLENGE: &str = "Bearer realm=\"cuotai\"";

/// The schema numbers `/health` lists (RF5.3): the current one, plus the
/// previous ones still served during their support window.
const SCHEMAS: &[u32] = &[SCHEMA];

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The body of `/health`, in its documented field order.
#[derive(serde::Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
    schema: u32,
    /// PRD 0031 RF5.3: every `export::v1`-style schema number served, so a
    /// client can tell that a newer one is available next to `schema`.
    schemas: &'static [u32],
}

/// Where the server reads the last known state. Building the export must
/// only read what the refresh cycle already holds (see
/// [`Export::from_cycle`]); `None` while there is no state at all.
///
/// Called on the runtime thread for every data request: keep it short.
pub trait Source: Send + Sync + 'static {
    fn export(&self) -> Option<Export>;
}

impl<F> Source for F
where
    F: Fn() -> Option<Export> + Send + Sync + 'static,
{
    fn export(&self) -> Option<Export> {
        self()
    }
}

/// Defensive limits of RF2.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Connections served at once; any other is closed as soon as accepted.
    pub max_connections: usize,
    /// Request line plus headers; more answers `431`.
    pub max_head_bytes: usize,
    /// Request target (path and query); longer answers `414`.
    pub max_url_bytes: usize,
    /// Header count; more answers `431`.
    pub max_headers: usize,
    /// Whole connection, from accepting to closing; a slower one is cut.
    pub request_timeout: Duration,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            max_connections: 16,
            max_head_bytes: 8 * 1024,
            max_url_bytes: 2 * 1024,
            max_headers: 32,
            request_timeout: Duration::from_secs(5),
        }
    }
}

/// One served request, for the access log. It never holds headers (so no
/// `Authorization`) nor the query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessEntry {
    pub at: DateTime<Utc>,
    pub peer: IpAddr,
    /// `-` when the request could not be read.
    pub method: String,
    /// The path without its query, printable ASCII only, at most
    /// [`AccessEntry::MAX_PATH`] bytes; `-` when the request could not be read.
    pub path: String,
    pub status: u16,
    pub elapsed: Duration,
}

impl AccessEntry {
    pub const MAX_PATH: usize = 256;
}

impl fmt::Display for AccessEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {} {}ms",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.peer,
            self.method,
            self.path,
            self.status,
            self.elapsed.as_millis()
        )
    }
}

/// Receives an [`AccessEntry`] per answered request, on the runtime thread.
pub type AccessLog = Arc<dyn Fn(&AccessEntry) + Send + Sync>;

/// How the server listens and whom it answers.
#[derive(Clone)]
pub struct ServeConfig {
    /// `127.0.0.1` by default; `::1` is also loopback.
    pub bind: IpAddr,
    /// [`DEFAULT_PORT`] by default; `0` lets the system pick one.
    pub port: u16,
    /// When the port is taken, try the next ones ([`PORT_ATTEMPTS`] in all).
    /// Off by default (PRD 0031 RF9.1): the address never moves silently.
    pub port_fallback: bool,
    /// RF3.1: the explicit flag a non-loopback `bind` needs; without it such
    /// an address is a configuration error. It also requires `token`.
    pub allow_non_loopback: bool,
    /// RF3.2: required as `Authorization: Bearer` on every route when set.
    pub token: Option<Token>,
    /// RF3.3: accept requests from web pages and let any of them read the
    /// answers (`Access-Control-Allow-Origin: *`); see
    /// [`allow_browsers_warning`].
    pub allow_browsers: bool,
    pub bounds: Bounds,
    pub access_log: Option<AccessLog>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: DEFAULT_PORT,
            port_fallback: false,
            allow_non_loopback: false,
            token: None,
            allow_browsers: false,
            bounds: Bounds::default(),
            access_log: None,
        }
    }
}

impl fmt::Debug for ServeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeConfig")
            .field("bind", &self.bind)
            .field("port", &self.port)
            .field("port_fallback", &self.port_fallback)
            .field("allow_non_loopback", &self.allow_non_loopback)
            .field("token", &self.token)
            .field("allow_browsers", &self.allow_browsers)
            .field("bounds", &self.bounds)
            .field("access_log", &self.access_log.is_some())
            .finish()
    }
}

impl ServeConfig {
    /// RF3.1: a non-loopback address only with the explicit flag, and then
    /// only with a token (without one such peers would be refused anyway).
    pub fn validate(&self) -> Result<(), ServeError> {
        if !self.bind.is_loopback() {
            if !self.allow_non_loopback {
                return Err(ServeError::NonLoopback { addr: self.bind });
            }
            if self.token.is_none() {
                return Err(ServeError::TokenRequired);
            }
        }
        Ok(())
    }
}

/// Local failures to start or configure the server (exit code 1 for a CLI).
/// Messages carry addresses and ports only, never a token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeError {
    #[error(
        "O servidor local só escuta em loopback (127.0.0.1 ou ::1); o endereço {addr} exige a opção explícita para fora do loopback."
    )]
    NonLoopback { addr: IpAddr },
    #[error("Um endereço fora do loopback exige um token de acesso ao servidor local.")]
    TokenRequired,
    /// `first == last` when only the requested port was tried.
    #[error("{}", port_in_use_text(*.first, *.last))]
    PortInUse { first: u16, last: u16 },
    #[error("Não foi possível abrir a porta {port} do servidor local.")]
    Bind { port: u16 },
    #[error(
        "Token do servidor local inválido: use de 16 a 512 caracteres, sem espaços (letras, números e - . _ ~ + /)."
    )]
    InvalidToken,
    #[error("O cofre do sistema está indisponível ou negou acesso ao token do servidor local.")]
    Vault,
    #[error("O gerador aleatório do sistema está indisponível; o token não foi criado.")]
    Random,
    /// A front end expected its stored token and the vault has none.
    #[error(
        "O token do servidor local não está mais no cofre do sistema; gere um novo ou revogue o token."
    )]
    TokenMissing,
    /// INT-04: the token file could not be read, created or trusted (not a
    /// regular file, another user's, or not a token). Never names the path
    /// nor echoes the content.
    #[error(
        "Não foi possível ler ou criar o arquivo de token do servidor local (ele deve ser um arquivo comum seu, com um único token)."
    )]
    TokenFile,
}

impl ServeError {
    pub fn l10n(&self) -> Localized {
        let fallback = self.to_string();
        match *self {
            Self::NonLoopback { addr } => Localized::with_param(
                "serve.error.non_loopback",
                "addr",
                addr.to_string(),
                fallback,
            ),
            Self::TokenRequired => Localized::fixed("serve.error.token_required", fallback),
            Self::PortInUse { first, last } if first == last => {
                Localized::with_param("serve.error.port_taken", "port", first, fallback)
            }
            Self::PortInUse { first, last } => Localized::with_params(
                "serve.error.port_in_use",
                params([
                    ("first", ParamValue::from(first)),
                    ("last", ParamValue::from(last)),
                ]),
                fallback,
            ),
            Self::Bind { port } => {
                Localized::with_param("serve.error.bind", "port", port, fallback)
            }
            Self::InvalidToken => Localized::fixed("serve.error.invalid_token", fallback),
            Self::Vault => Localized::fixed("serve.error.vault", fallback),
            Self::Random => Localized::fixed("serve.error.random", fallback),
            Self::TokenMissing => Localized::fixed("serve.error.token_missing", fallback),
            Self::TokenFile => Localized::fixed("serve.error.token_file", fallback),
        }
    }
}

/// INT-04 (2026-10-21 review): where a headless front end such as
/// `cuotai --serve` takes the bearer token from. The token is required by
/// default; serving without one is an explicit opt-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    /// [`default_token_file`], created with a fresh token on first use and
    /// reused afterwards ([`ensure_token_file`]).
    Default,
    /// A file the user controls (`--token-file`), read as it is.
    File(std::path::PathBuf),
    /// No token (`--no-token`): only loopback peers that are not web pages,
    /// and **any local user or program** among them, can read.
    None,
}

/// The token a [`TokenSource`] yields, with the notices the front end must
/// show (on stderr for the CLI): where a default token lives, that a token
/// file is readable by other users, or that serving without a token lets
/// every local user read. Notices never contain the token.
#[derive(Debug)]
pub struct ResolvedToken {
    pub token: Option<Token>,
    pub notices: Vec<Localized>,
}

/// Resolve `source` (see [`TokenSource`]). `default_file` is
/// [`default_token_file`] in production; `None` (no local data directory)
/// makes [`TokenSource::Default`] a [`ServeError::TokenFile`].
pub fn resolve_token(
    source: &TokenSource,
    default_file: Option<&std::path::Path>,
) -> Result<ResolvedToken, ServeError> {
    match source {
        TokenSource::Default => {
            let path = default_file.ok_or(ServeError::TokenFile)?;
            let token = ensure_token_file(path)?;
            let path = path.display().to_string();
            Ok(ResolvedToken {
                token: Some(token),
                notices: vec![Localized::with_param(
                    "serve.token.default_file",
                    "path",
                    path.as_str(),
                    format!(
                        "Token de acesso em {path} (somente o seu usuário o lê): envie-o como Authorization: Bearer; --no-token desliga."
                    ),
                )],
            })
        }
        TokenSource::File(path) => {
            let token = read_token_file(path)?;
            let mut notices = Vec::new();
            if token_file_exposed(path) {
                notices.push(Localized::fixed(
                    "serve.token.file_exposed",
                    "Atenção: o arquivo de token pode ser lido por outros usuários deste computador; restrinja-o com chmod 600.",
                ));
            }
            Ok(ResolvedToken {
                token: Some(token),
                notices,
            })
        }
        TokenSource::None => Ok(ResolvedToken {
            token: None,
            notices: vec![no_token_warning()],
        }),
    }
}

/// What serving without a token means: shown by `cuotai --serve
/// --no-token` and before a front end serves tokenless.
pub fn no_token_warning() -> Localized {
    Localized::fixed(
        "serve.token.none_warning",
        "Atenção: sem token, qualquer usuário ou programa deste computador pode ler o seu uso no servidor local (somente páginas web e outros computadores continuam recusados).",
    )
}

fn port_in_use_text(first: u16, last: u16) -> String {
    if first == last {
        format!("A porta {first} está em uso; escolha outra porta para o servidor local.")
    } else {
        format!(
            "As portas {first} a {last} estão em uso; escolha outra porta para o servidor local."
        )
    }
}

/// RF3.3: what a front end shows before turning on
/// [`ServeConfig::allow_browsers`].
pub fn allow_browsers_warning() -> Localized {
    Localized::fixed(
        "serve.allow_browsers.warning",
        "Com navegadores permitidos, qualquer página aberta no navegador pode ler seu uso no servidor local (com token, qualquer página que o conheça). Ative só se uma página sua precisa desses dados.",
    )
}

/// A running server. Dropping it stops accepting; [`LocalServer::shutdown`]
/// also waits until the port is closed.
pub struct LocalServer {
    addr: SocketAddr,
    requested_port: u16,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
    state: Arc<State>,
}

/// What [`LocalServer::reconfigure`] did. Either the policy and source are
/// live on the bound listener ([`Reconfigure::Replaced`]) or the caller must
/// stop the server and start a new one with the requested port/binding
/// ([`Reconfigure::RestartRequired`]). The accept loop owns the
/// `max_connections` semaphore and drain `request_timeout`; the bind identity
/// and `allow_non_loopback` pin listener security. Any change there forces a
/// restart. Token, browser access, logging, and request-size caps are applied
/// in place.
#[derive(Debug, PartialEq, Eq)]
pub enum Reconfigure {
    /// The snapshot was swapped in place under a single write lock; every
    /// request accepted after this point runs against the new [`ServeConfig`]
    /// and [`Source`], in-flight requests keep the snapshot they captured.
    Replaced,
    /// A field whose live semantics depends on `accept_loop` changed (bind
    /// identity, `max_connections`, or `request_timeout`). The state is
    /// untouched; the caller must stop this `LocalServer` and start a new
    /// one with the requested config.
    RestartRequired,
}

impl LocalServer {
    /// Validate `config`, bind (falling back to the next ports when allowed)
    /// and serve `source` on the current Tokio runtime. The listener stays
    /// open for the lifetime of the returned [`LocalServer`]; the policy in
    /// effect for any newly accepted connection can be exchanged in place
    /// with [`LocalServer::reconfigure`] without dropping the bound socket.
    pub async fn start(config: ServeConfig, source: impl Source) -> Result<Self, ServeError> {
        config.validate()?;
        let requested_port = config.port;
        let listener = bind(config.bind, config.port, config.port_fallback).await?;
        let addr = listener
            .local_addr()
            .map_err(|_| ServeError::Bind { port: config.port })?;
        let initial = Arc::new(Snapshot::new(config, source));
        Ok(Self::from_listener(listener, addr, requested_port, initial))
    }

    /// Build a server around an already-bound, validated listener.
    fn from_listener(
        listener: TcpListener,
        addr: SocketAddr,
        requested_port: u16,
        initial: Arc<Snapshot>,
    ) -> Self {
        let (stop, stopped) = watch::channel(false);
        let state = Arc::new(State {
            snapshot: RwLock::new(initial),
        });
        let task = tokio::spawn(accept_loop(listener, state.clone(), stopped));
        Self {
            addr,
            requested_port,
            stop,
            task: Some(task),
            state,
        }
    }

    /// Same-port hot swap: keeps the bound listener open and atomically
    /// replaces the [`ServeConfig`] + [`Source`] used by every connection
    /// accepted from this point on. Used by Settings → Integration on the
    /// desktop when only policy that does not depend on the socket changes
    /// (`allow_browsers`, token, access_log, request-only caps on head / url
    /// / header size). The bind identity, the `max_connections` semaphore,
    /// and `bounds.request_timeout` (which the accept loop also uses for
    /// connection draining) own live resources; changing any of them returns
    /// `RestartRequired` and leaves the server untouched.
    ///
    /// Holding one write lock across the compatibility check and swap makes
    /// concurrent calls linearizable: each reconfigure sees the result of any
    /// swap serialized before it. The replacement snapshot is allocated only
    /// after the compatibility check passes.
    pub async fn reconfigure(
        &self,
        config: ServeConfig,
        source: impl Source,
    ) -> Result<Reconfigure, ServeError> {
        // Validation is a pure config read; a bad input never mutates state.
        config.validate()?;
        let mut guard = self.state.snapshot.write().await;
        let live = &guard.config;
        let restart_required = live.bind != config.bind
            || live.port != config.port
            || live.port_fallback != config.port_fallback
            || live.allow_non_loopback != config.allow_non_loopback
            || live.bounds.max_connections != config.bounds.max_connections
            || live.bounds.request_timeout != config.bounds.request_timeout;
        if restart_required {
            return Ok(Reconfigure::RestartRequired);
        }
        *guard = Arc::new(Snapshot::new(config, source));
        Ok(Reconfigure::Replaced)
    }

    /// The address actually bound.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:6738` (IPv6 in brackets).
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The requested port was taken and another one was bound.
    pub fn fell_back(&self) -> bool {
        self.requested_port != 0 && self.addr.port() != self.requested_port
    }

    /// Stop accepting, let requests in flight finish (at most
    /// [`Bounds::request_timeout`]) and return once the port is closed.
    pub async fn shutdown(mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

async fn bind(ip: IpAddr, port: u16, fallback: bool) -> Result<TcpListener, ServeError> {
    let last = if port == 0 || !fallback {
        port
    } else {
        port.saturating_add(PORT_ATTEMPTS - 1)
    };
    for candidate in port..=last {
        match TcpListener::bind(SocketAddr::new(ip, candidate)).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == ErrorKind::AddrInUse => continue,
            Err(_) => return Err(ServeError::Bind { port: candidate }),
        }
    }
    Err(ServeError::PortInUse { first: port, last })
}

/// PRD 0012 RF5.2: whether the program listening on `127.0.0.1:port` is a
/// Cuotai local server (`cuotai --serve` or the desktop app), so a front end
/// whose port is taken can name the conflict. It asks `/health` once,
/// without a token, waits at most [`PROBE_TIMEOUT`] and recognises the
/// documented answer or, when that server needs a token, its `401` with
/// Cuotai's bearer realm. Nothing else is read.
pub async fn is_cuotai_server(port: u16) -> bool {
    tokio::time::timeout(PROBE_TIMEOUT, probe(port))
        .await
        .unwrap_or(false)
}

async fn probe(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await else {
        return false;
    };
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    let mut answer = Vec::new();
    // The server closes after answering; a partial read is judged as is.
    let _ = stream.take(PROBE_MAX_BYTES).read_to_end(&mut answer).await;
    http::is_cuotai_health(&answer, BEARER_CHALLENGE)
}

/// The mutable live state shared by every connection an accept loop serves.
/// Each newly accepted connection takes a clone of the current `Arc<Snapshot>`,
/// so an in-place reconfigure is observed exactly once per connection with no
/// per-request lock. `max_connections` is not part of the snapshot because it
/// owns the semaphore that gates acceptance; only a restart can change it.
struct State {
    snapshot: RwLock<Arc<Snapshot>>,
}

/// One immutable pair of policy and source. Replacement is atomic and lock-free
/// for serving connections: a request either runs against the snapshot it
/// captured or the snapshot a [`LocalServer::reconfigure`] stored before its
/// accept.
struct Snapshot {
    config: ServeConfig,
    source: Box<dyn Source>,
}

impl Snapshot {
    fn new(config: ServeConfig, source: impl Source) -> Self {
        Self {
            config,
            source: Box::new(source),
        }
    }
}

async fn accept_loop(listener: TcpListener, state: Arc<State>, mut stopped: watch::Receiver<bool>) {
    // `max_connections` cannot change live; the accept_loop owns the
    // semaphore for the server's lifetime. Reconfigure forces a restart when
    // this field differs.
    let max_connections = state.snapshot.read().await.config.bounds.max_connections;
    let drain_timeout = state.snapshot.read().await.config.bounds.request_timeout;
    let slots = Arc::new(Semaphore::new(max_connections));
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    // RF2.4: over the limit, the connection is closed at once.
                    let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                        continue;
                    };
                    let snapshot = state.snapshot.read().await.clone();
                    let state = Arc::clone(&state);
                    connections.spawn(async move {
                        let _slot = slot;
                        serve_connection(stream, peer.ip(), &state, &snapshot).await;
                    });
                }
                // Out of descriptors and the like: do not spin.
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            },
        }
    }
    drop(listener);
    let drain = async { while connections.join_next().await.is_some() {} };
    if tokio::time::timeout(drain_timeout, drain).await.is_err() {
        connections.shutdown().await;
    }
}

async fn serve_connection(mut stream: TcpStream, peer: IpAddr, state: &State, snapshot: &Snapshot) {
    let bounds = snapshot.config.bounds;
    let started = Instant::now();
    let _ = tokio::time::timeout(bounds.request_timeout, async {
        let (response, method, path) =
            match http::read_head(&mut stream, bounds.max_head_bytes, bounds.max_headers).await {
                Read::Closed => return,
                Read::TooLarge => (Response::error(431, "headers_too_large"), None, None),
                Read::Malformed => (Response::error(400, "bad_request"), None, None),
                Read::Head(head) => {
                    let method = head.method.clone();
                    let target = head.target.clone();
                    let response = state.respond(&head, peer, snapshot);
                    (response, Some(method), Some(target))
                }
            };
        let response = state.finish(response, snapshot);
        let status = response.status;
        let include_body = method.as_deref() != Some("HEAD");
        let _ = http::write(&mut stream, response, include_body).await;
        if let Some(log) = &snapshot.config.access_log {
            log(&AccessEntry {
                at: Utc::now(),
                peer,
                method: method.unwrap_or_else(|| "-".into()),
                path: path.as_deref().map_or_else(|| "-".into(), log_path),
                status,
                elapsed: started.elapsed(),
            });
        }
        http::linger(&mut stream).await;
    })
    .await;
}

/// The path of `target` for the log: no query, printable ASCII, bounded.
fn log_path(target: &str) -> String {
    let path = target.split(['?', '#']).next().unwrap_or_default();
    path.chars()
        .map(|c| if c.is_ascii_graphic() { c } else { '?' })
        .take(AccessEntry::MAX_PATH)
        .collect()
}

impl State {
    /// RF3 checks, in order, then the route.
    fn respond(&self, head: &Head, peer: IpAddr, snapshot: &Snapshot) -> Response {
        let config = &snapshot.config;
        if head.target.len() > config.bounds.max_url_bytes {
            return Response::error(414, "uri_too_long");
        }
        if !self.host_allowed(head, snapshot) {
            return Response::error(403, "host_not_allowed");
        }
        if !config.allow_browsers && head.sent_by_page() {
            return Response::error(403, "browser_not_allowed");
        }
        if config.token.is_none() && !is_loopback(peer) {
            return Response::error(403, "peer_not_allowed");
        }
        if config.allow_browsers && head.method == "OPTIONS" {
            // A CORS preflight carries no credentials.
            return Response::empty(204)
                .header("Access-Control-Allow-Methods", "GET, HEAD")
                .header("Access-Control-Allow-Headers", "Authorization")
                .header("Access-Control-Max-Age", "600");
        }
        if let Some(token) = &config.token {
            if !head
                .bearer()
                .is_some_and(|presented| token.matches(presented))
            {
                return Response::error(401, "unauthorized")
                    .header("WWW-Authenticate", BEARER_CHALLENGE);
            }
        }
        // PRD 0031 RF9.2: `HEAD` answers like `GET`, without the body
        // (`serve_connection` drops it when writing).
        if head.method != "GET" && head.method != "HEAD" {
            return Response::error(405, "method_not_allowed").header("Allow", "GET, HEAD");
        }
        let Some(path) = head.target.split('?').next().filter(|p| p.starts_with('/')) else {
            return Response::error(400, "bad_request");
        };
        match path {
            "/health" => Response::json(
                200,
                &Health {
                    status: "ok",
                    version: VERSION,
                    schema: SCHEMA,
                    schemas: SCHEMAS,
                },
            ),
            "/v1/snapshot" => self.with_export(|export| Response::json(200, &export), snapshot),
            "/v1/limits" => self.with_export(
                |export| Response::json(200, &Limits::from_export(&export)),
                snapshot,
            ),
            _ => match path
                .strip_prefix("/v1/accounts/")
                .filter(|key| !key.is_empty() && !key.contains('/'))
                .and_then(http::percent_decode)
            {
                Some(key) => self.with_export(
                    // PRD 0029 RF4.2: an id first, then the legacy key.
                    |export| match export
                        .accounts
                        .iter()
                        .find(|account| account.id == key)
                        .or_else(|| export.accounts.iter().find(|account| account.key == key))
                    {
                        Some(account) => Response::json(200, account),
                        None => Response::error(404, "not_found"),
                    },
                    snapshot,
                ),
                None => Response::error(404, "not_found"),
            },
        }
    }

    /// RF2.3: data comes only from the source, and `503` until any
    /// account has a reading (current or preserved).
    fn with_export(
        &self,
        answer: impl FnOnce(Export) -> Response,
        snapshot: &Snapshot,
    ) -> Response {
        match snapshot.source.export().filter(|export| {
            export
                .accounts
                .iter()
                .any(|account| account.report.is_some() || account.last_good.is_some())
        }) {
            Some(export) => answer(export),
            None => Response::error(503, "no_data")
                .header("Retry-After", RETRY_AFTER_SECONDS.to_string()),
        }
    }

    /// RF3.3: what every answer carries in "allow browsers" mode.
    fn finish(&self, response: Response, snapshot: &Snapshot) -> Response {
        if snapshot.config.allow_browsers {
            response.header("Access-Control-Allow-Origin", "*")
        } else {
            response
        }
    }

    /// RF3.3: exactly one `Host`, naming loopback (or the address bound by
    /// the explicit flag); a rebound domain never matches.
    fn host_allowed(&self, head: &Head, snapshot: &Snapshot) -> bool {
        let mut hosts = head.values("host");
        let (Some(host), None) = (hosts.next(), hosts.next()) else {
            return false;
        };
        let Some(name) = std::str::from_utf8(host).ok().and_then(host_name) else {
            return false;
        };
        name.eq_ignore_ascii_case("localhost")
            || name.parse::<IpAddr>().is_ok_and(|ip| {
                ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
                    || ip == IpAddr::V6(Ipv6Addr::LOCALHOST)
                    || ip == snapshot.config.bind
            })
    }
}

/// The name of a `Host` value without its port (`[::1]:6738` → `::1`).
fn host_name(host: &str) -> Option<&str> {
    let (name, port) = match host.strip_prefix('[') {
        Some(rest) => match rest.split_once(']')? {
            (name, "") => (name, None),
            (name, after) => (name, Some(after.strip_prefix(':')?)),
        },
        None => match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        },
    };
    let port_ok = port.is_none_or(|port| {
        (1..=5).contains(&port.len()) && port.bytes().all(|b| b.is_ascii_digit())
    });
    (port_ok && !name.is_empty()).then_some(name)
}

fn is_loopback(ip: IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}
