//! PRD 0012 RF5: the core's local server inside the desktop app.
//!
//! Off until the user turns it on in Settings → Integration. It serves the
//! `export::v1` of the app's own refresh cycle: on every state change the
//! backend publishes a [`Feed`] (the configuration, its identity, the cycle
//! and the provider statuses it just showed) and a request only builds the
//! export from it, so a read never queries a provider and no second cycle
//! exists; `local_cost` (PRD 0015 RF4.3) is read from the cost index per
//! request. It binds `127.0.0.1` on exactly the chosen port: a taken port is
//! an error the interface shows, naming another Cuotai server
//! (`cuotai --serve`) when it is one, never a silent move to another port.
//! A required token stays in the OS vault ([`TokenVault`]).

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use chrono::Utc;
use cuotai_core::{
    config_file::ConfigIdentity,
    export::v1::{Export, Options, StatsExport},
    incidents::ProviderStatus,
    refresh::RefreshCycle,
    serve::{
        self, LocalServer, ServeConfig, ServeError, StatsAnswer, SystemTokenVault, Token,
        TokenVault,
    },
    stats::StatsRange,
    text::Localized,
    Config,
};
use serde::Serialize;

use crate::local_cost::LocalCost;
use crate::settings::{ServePrefs, SERVE_PORTS};

/// What the server reads: the app's state as the backend last showed it.
pub struct Feed {
    pub config: Config,
    pub identity: ConfigIdentity,
    pub cycle: RefreshCycle,
    pub incidents: Vec<ProviderStatus>,
    /// Where the recorded points behind the pace come from (PRD 0031 RF13).
    pub monitor: Arc<cuotai_core::Monitor>,
}

/// Where the backend publishes the [`Feed`]; empty while no configuration
/// is loaded or the server is off (data routes then answer `503`).
#[derive(Clone, Default)]
pub struct FeedSlot {
    feed: Arc<Mutex<Option<Feed>>>,
    /// PRD 0015 RF4.3: where `local_cost` comes from, set once at start.
    cost: Arc<OnceLock<Arc<LocalCost>>>,
    /// PRD 0031 RF6: counts publications, one per `Backend::emit`, so the
    /// server reuses its export (and answers `304`) until the next one.
    generation: Arc<std::sync::atomic::AtomicU64>,
}

impl FeedSlot {
    pub fn publish(&self, feed: Option<Feed>) {
        *self.feed.lock().unwrap_or_else(PoisonError::into_inner) = feed;
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// What the server reads: this slot's export, tagged with its generation.
    fn source(&self) -> impl serve::Source {
        let (export, generation, stats) = (self.clone(), self.clone(), self.clone());
        serve::with_stats(
            serve::with_generation(
                move || export.export(),
                move || {
                    generation
                        .generation
                        .load(std::sync::atomic::Ordering::Relaxed)
                },
            ),
            move |range| stats.stats(range),
        )
    }

    /// PRD 0031 RF11.2.5: `/v1/stats` from the app's local history only
    /// (never a provider); `history_off` while nothing is being recorded.
    /// The route itself answers only with `ServeConfig::stats` and a token.
    fn stats(&self, range: StatsRange) -> StatsAnswer {
        let (monitor, accounts) = {
            let feed = self.feed.lock().unwrap_or_else(PoisonError::into_inner);
            match feed.as_ref() {
                Some(feed) => (Arc::clone(&feed.monitor), feed.config.accounts.clone()),
                None => return StatsAnswer::HistoryOff,
            }
        };
        let Some(store) = monitor.history().filter(|_| monitor.history_recording()) else {
            return StatsAnswer::HistoryOff;
        };
        let now = Utc::now();
        match cuotai_core::stats::summarize(&store, &accounts, range, now, &chrono::Local) {
            Ok(summary) => StatsAnswer::Ready(Box::new(StatsExport::from_summary(
                &summary, &accounts, now,
            ))),
            Err(_) => StatsAnswer::Failed,
        }
    }

    /// PRD 0015 RF4.3: export `cost`'s summary as `local_cost`.
    pub fn attach_cost(&self, cost: Arc<LocalCost>) {
        let _ = self.cost.set(cost);
    }

    /// The export of the published state at this instant (ages count from
    /// now); never a query.
    fn export(&self) -> Option<Export> {
        let local_cost = self.cost.get().and_then(|cost| cost.export());
        let feed = self.feed.lock().unwrap_or_else(PoisonError::into_inner);
        feed.as_ref().map(|feed| {
            Export::from_cycle_with_history(
                &feed.config,
                &feed.identity,
                &feed.cycle,
                &feed.incidents,
                Utc::now(),
                Options::default(),
                |index, metrics, at| feed.monitor.recent_points(index, metrics, at),
            )
            .with_local_cost(local_cost)
        })
    }
}

/// RF5.1/RF5.2: the server's choices and state, as Settings shows them.
/// Never the token itself, only whether one is required.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ServeView {
    /// `off`, `on` or `error` (turned on but not serving).
    pub state: &'static str,
    #[serde(flatten)]
    pub prefs: ServePrefs,
    /// The effective address while on (`http://127.0.0.1:6738`).
    pub url: Option<String>,
    /// Why it is not serving: the port taken, the vault, …
    pub error: Option<Localized>,
    /// The port is taken by another Cuotai server (`cuotai --serve`).
    pub cuotai_on_port: bool,
    pub default_port: u16,
    pub port_min: u16,
    pub port_max: u16,
    /// What "allow browsers" means, shown before turning it on.
    pub browsers_warning: Localized,
}

impl Default for ServeView {
    fn default() -> Self {
        view_of(ServePrefs::default(), None, None, false)
    }
}

fn view_of(
    prefs: ServePrefs,
    url: Option<String>,
    error: Option<&ServeError>,
    cuotai_on_port: bool,
) -> ServeView {
    let state = match (&url, error) {
        (Some(_), _) => "on",
        (None, Some(_)) => "error",
        (None, None) => "off",
    };
    ServeView {
        state,
        prefs,
        url,
        error: error.map(ServeError::l10n),
        cuotai_on_port,
        default_port: serve::DEFAULT_PORT,
        port_min: *SERVE_PORTS.start(),
        port_max: *SERVE_PORTS.end(),
        browsers_warning: serve::allow_browsers_warning(),
    }
}

/// The running server (if any), its last failure and the vault of its token.
pub struct Integration {
    prefs: ServePrefs,
    vault: Arc<dyn TokenVault>,
    server: Option<LocalServer>,
    error: Option<ServeError>,
    cuotai_on_port: bool,
    /// Cold-restart counter. Increments exactly once per successful
    /// `LocalServer::start`; lets regression tests pin the structural
    /// invariant that a same-port hot swap does not bind a new socket.
    #[cfg(test)]
    listener_starts: usize,
}

impl Default for Integration {
    fn default() -> Self {
        Self::new(ServePrefs::default(), Arc::new(SystemTokenVault))
    }
}

impl Integration {
    /// Stopped, with the device's `prefs`; [`Self::apply`] starts it.
    pub fn new(prefs: ServePrefs, vault: Arc<dyn TokenVault>) -> Self {
        Self {
            prefs,
            vault,
            server: None,
            error: None,
            cuotai_on_port: false,
            #[cfg(test)]
            listener_starts: 0,
        }
    }

    pub fn prefs(&self) -> ServePrefs {
        self.prefs
    }

    /// Listening right now.
    pub fn is_on(&self) -> bool {
        self.server.is_some()
    }

    pub fn view(&self) -> ServeView {
        view_of(
            self.prefs,
            self.server.as_ref().map(LocalServer::url),
            self.error.as_ref(),
            self.cuotai_on_port,
        )
    }

    /// Cold starts since construction (test-only). Equals one after the
    /// first successful `apply`, unchanged across same-port policy/source
    /// swaps, and grows by one per binding-identity restart.
    #[cfg(test)]
    pub(crate) fn cold_starts(&self) -> usize {
        self.listener_starts
    }

    /// Apply `prefs`. The token is resolved exactly once (via the vault)
    /// before any server path runs; a missing / vault error turns the
    /// server off and surfaces the error to the UI rather than falling
    /// back silently. When the server is already running and the change is
    /// policy-only (`allow_browsers`, token, access_log, request-only
    /// caps), the snapshot is hot-swapped on the same listener — there is
    /// no close/rebind and `listener_starts` does not change. Binding
    /// identity, `max_connections`, or `request_timeout` changes fall back
    /// to the cold path, which closes the existing server and binds a new
    /// one (the desktop forbids moving to another port by setting
    /// `port_fallback = false`).
    pub async fn apply(&mut self, prefs: ServePrefs, feed: &FeedSlot) {
        // Disabled: close if running, drop the error, do not start.
        if !prefs.enabled {
            if let Some(server) = self.server.take() {
                server.shutdown().await;
            }
            self.prefs = prefs;
            self.error = None;
            self.cuotai_on_port = false;
            return;
        }

        // Resolve the token once for this `apply`. The resulting
        // `ServeConfig` is used by both the hot-swap and cold paths so the
        // vault is touched at most one time per call.
        let new_config = match Self::config_for(&prefs, &self.vault).await {
            Ok(config) => config,
            Err(error) => {
                if let Some(server) = self.server.take() {
                    server.shutdown().await;
                }
                self.prefs = prefs;
                self.error = Some(error);
                self.cuotai_on_port = false;
                return;
            }
        };

        // Hot swap on the live listener.
        if let Some(server) = self.server.as_ref() {
            match server.reconfigure(new_config.clone(), feed.source()).await {
                Ok(serve::Reconfigure::Replaced) => {
                    self.prefs = prefs;
                    self.error = None;
                    self.cuotai_on_port = false;
                    return;
                }
                Ok(serve::Reconfigure::RestartRequired) => {
                    // Bind identity / max_connections / request_timeout
                    // changed: drop into the cold path. Close first so a
                    // half-reconfigured server cannot serve old policy if
                    // the bind fails.
                    if let Some(server) = self.server.take() {
                        server.shutdown().await;
                    }
                }
                Err(error) => {
                    if let Some(server) = self.server.take() {
                        server.shutdown().await;
                    }
                    self.prefs = prefs;
                    self.error = Some(error);
                    self.cuotai_on_port = false;
                    return;
                }
            }
        }

        // Cold path: listener is off, or we just closed a server whose bind
        // identity changed.
        self.prefs = prefs;
        self.error = None;
        self.cuotai_on_port = false;
        let config_for_start = new_config;
        match LocalServer::start(config_for_start, feed.source()).await {
            Ok(server) => {
                self.server = Some(server);
                #[cfg(test)]
                {
                    self.listener_starts += 1;
                }
            }
            Err(error) => {
                self.cuotai_on_port = matches!(error, ServeError::PortInUse { .. })
                    && serve::is_cuotai_server(prefs.port).await;
                self.error = Some(error);
            }
        }
    }

    /// Build the [`ServeConfig`] for `prefs`, fetching the token from the
    /// vault when one is required. A required token that is missing or
    /// rejected by the vault is a `ServeError` — the server must not fall
    /// back to serving without one. The vault call goes through
    /// `spawn_blocking` because the OS IPC may block the runtime thread.
    async fn config_for(
        prefs: &ServePrefs,
        vault: &Arc<dyn TokenVault>,
    ) -> Result<ServeConfig, ServeError> {
        let token = if prefs.token {
            let vault = Arc::clone(vault);
            let raw = blocking(move || vault.load())
                .await?
                .ok_or(ServeError::TokenMissing)?;
            Some(raw)
        } else {
            None
        };
        Ok(ServeConfig {
            port: prefs.port,
            port_fallback: false,
            token,
            allow_browsers: prefs.allow_browsers,
            stats: prefs.stats,
            ..ServeConfig::default()
        })
    }

    /// Generate a token and store it in the vault, replacing the previous
    /// one; the server uses it once [`Self::apply`]ed with `token` on.
    pub async fn store_new_token(&self) -> Result<(), ServeError> {
        let token = Token::generate()?;
        let vault = Arc::clone(&self.vault);
        blocking(move || vault.save(&token)).await
    }

    /// Revoke: remove the token from the vault.
    pub async fn forget_token(&self) -> Result<(), ServeError> {
        let vault = Arc::clone(&self.vault);
        blocking(move || vault.delete()).await
    }

    /// The stored token, for the user to copy into a client.
    pub async fn stored_token(&self) -> Result<Token, ServeError> {
        let vault = Arc::clone(&self.vault);
        blocking(move || vault.load())
            .await?
            .ok_or(ServeError::TokenMissing)
    }
}

/// Vault calls may block on OS IPC.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ServeError> + Send + 'static,
) -> Result<T, ServeError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| ServeError::Vault)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::{Account, Provider, Report, Status};
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::time::Instant;

    #[derive(Default)]
    struct MemoryVault(parking_lot::Mutex<Option<Token>>);

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
            Err(ServeError::Vault)
        }
        fn save(&self, _: &Token) -> Result<(), ServeError> {
            Err(ServeError::Vault)
        }
        fn delete(&self) -> Result<(), ServeError> {
            Err(ServeError::Vault)
        }
    }

    fn free_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn bound_port(integration: &Integration) -> u16 {
        integration.server.as_ref().unwrap().addr().port()
    }

    fn on(port: u16) -> ServePrefs {
        ServePrefs {
            enabled: true,
            port,
            ..ServePrefs::default()
        }
    }

    /// The app's cycle after one answered query of its only account.
    fn feed() -> Feed {
        feed_named("Personal Max")
    }

    /// A test source with a single account of `name`. Distinct names let
    /// the regression test prove the running `Source` was actually swapped
    /// during an in-place reconfigure.
    fn feed_named(name: &str) -> Feed {
        let config = Config {
            accounts: vec![Account {
                id: None,
                name: name.into(),
                provider: Provider::Claude,
                ..Account::default()
            }],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let token = cycle.begin(0).unwrap();
        let report = Report {
            account_id: None,
            account: name.into(),
            provider: Provider::Claude,
            checked_at: Utc::now(),
            status: Status::Ready,
            metrics: vec![],
            notes: vec![],
            notes_l10n: vec![],
            source: String::new(),
            session_expires_at: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        };
        cycle.finish_account(0, Some(report), token, Instant::now());
        Feed {
            monitor: Arc::new(cuotai_core::Monitor::new(config.clone()).unwrap()),
            config,
            identity: ConfigIdentity::Explicit("desktop".into()),
            cycle,
            incidents: vec![],
        }
    }

    /// One `GET` the way `curl` sends it, plus `headers`: the status and the
    /// whole answer, or `None` when nothing listens on `port`.
    async fn get(port: u16, path: &str, headers: &[(&str, &str)]) -> Option<(u16, String)> {
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{}Connection: close\r\n\r\n",
            headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect::<String>()
        );
        tokio::task::spawn_blocking(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
            stream.write_all(request.as_bytes()).ok()?;
            let mut answer = String::new();
            stream.read_to_string(&mut answer).ok()?;
            let status = answer.split(' ').nth(1)?.parse().ok()?;
            Some((status, answer))
        })
        .await
        .unwrap()
    }

    async fn status(port: u16, path: &str, headers: &[(&str, &str)]) -> Option<u16> {
        get(port, path, headers).await.map(|(status, _)| status)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn off_by_default_and_turning_it_off_closes_the_port() {
        let port = free_port();
        let slot = FeedSlot::default();
        let mut integration = Integration::new(
            ServePrefs {
                port,
                ..ServePrefs::default()
            },
            Arc::new(MemoryVault::default()),
        );
        integration.apply(integration.prefs(), &slot).await;
        assert_eq!(integration.view().state, "off");
        assert_eq!(status(port, "/health", &[]).await, None, "nothing listens");

        integration.apply(on(port), &slot).await;
        let view = integration.view();
        assert_eq!(view.state, "on");
        assert_eq!(
            view.url.as_deref(),
            Some(&*format!("http://127.0.0.1:{port}"))
        );
        assert_eq!(status(port, "/health", &[]).await, Some(200));

        integration
            .apply(
                ServePrefs {
                    enabled: false,
                    ..on(port)
                },
                &slot,
            )
            .await;
        assert_eq!(integration.view().state, "off");
        assert_eq!(status(port, "/health", &[]).await, None, "port closed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn serves_what_the_app_published_and_refuses_pages_unless_allowed() {
        let port = free_port();
        let slot = FeedSlot::default();
        let mut integration = Integration::new(on(port), Arc::new(MemoryVault::default()));
        integration.apply(on(port), &slot).await;
        // Nothing published yet (no configuration loaded): 503, no query.
        assert_eq!(status(port, "/v1/snapshot", &[]).await, Some(503));
        slot.publish(Some(feed()));
        let (code, answer) = get(port, "/v1/snapshot", &[]).await.unwrap();
        assert_eq!(code, 200);
        assert!(answer.contains("\"name\":\"Personal Max\""), "{answer}");
        assert_eq!(
            status(port, "/v1/accounts/claude:Personal%20Max", &[]).await,
            Some(200)
        );
        let page = [("Origin", "https://example.com")];
        assert_eq!(status(port, "/v1/limits", &page).await, Some(403));

        integration
            .apply(
                ServePrefs {
                    allow_browsers: true,
                    ..on(port)
                },
                &slot,
            )
            .await;
        let (code, answer) = get(port, "/v1/limits", &page).await.unwrap();
        assert_eq!(code, 200);
        assert!(
            answer.contains("Access-Control-Allow-Origin: *"),
            "{answer}"
        );
        integration.apply(ServePrefs::default(), &slot).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_token_is_generated_copied_rotated_and_revoked() {
        let slot = FeedSlot::default();
        let mut integration = Integration::new(on(0), Arc::new(MemoryVault::default()));
        assert_eq!(
            integration.stored_token().await,
            Err(ServeError::TokenMissing)
        );
        integration.store_new_token().await.unwrap();
        let guarded = ServePrefs {
            token: true,
            ..on(0)
        };
        integration.apply(guarded, &slot).await;
        let mut port = bound_port(&integration);
        let first = integration.stored_token().await.unwrap();
        let bearer = |token: &Token| format!("Bearer {}", token.expose());
        assert_eq!(status(port, "/health", &[]).await, Some(401));
        assert_eq!(
            status(port, "/health", &[("Authorization", &bearer(&first))]).await,
            Some(200)
        );
        // A new token replaces the old one at once.
        integration.store_new_token().await.unwrap();
        integration.apply(guarded, &slot).await;
        port = bound_port(&integration);
        let second = integration.stored_token().await.unwrap();
        assert_ne!(first, second);
        assert_eq!(
            status(port, "/health", &[("Authorization", &bearer(&first))]).await,
            Some(401)
        );
        assert_eq!(
            status(port, "/health", &[("Authorization", &bearer(&second))]).await,
            Some(200)
        );
        // Revoked: gone from the vault, and the server no longer asks for one.
        integration.forget_token().await.unwrap();
        integration.apply(on(0), &slot).await;
        port = bound_port(&integration);
        assert_eq!(
            integration.stored_token().await,
            Err(ServeError::TokenMissing)
        );
        assert_eq!(status(port, "/health", &[]).await, Some(200));
        integration.apply(ServePrefs::default(), &slot).await;
    }

    /// PRD 0031 RF11.2.5: "Share usage statistics" turns `/v1/stats` on,
    /// still behind the token; without the setting the route is `404`.
    #[tokio::test(flavor = "multi_thread")]
    async fn share_usage_statistics_serves_v1_stats_behind_the_token() {
        let slot = FeedSlot::default();
        slot.publish(Some(feed()));
        let mut integration = Integration::new(on(0), Arc::new(MemoryVault::default()));
        integration.store_new_token().await.unwrap();
        let token = integration.stored_token().await.unwrap();
        let bearer = format!("Bearer {}", token.expose());
        let auth = [("Authorization", bearer.as_str())];
        let guarded = ServePrefs {
            token: true,
            ..on(0)
        };
        integration.apply(guarded, &slot).await;
        let port = bound_port(&integration);
        assert_eq!(status(port, "/v1/stats", &auth).await, Some(404));
        integration
            .apply(
                ServePrefs {
                    stats: true,
                    ..guarded
                },
                &slot,
            )
            .await;
        let port = bound_port(&integration);
        assert!(integration.view().prefs.stats);
        // The feed's monitor records no history: the honest `history_off`.
        assert_eq!(status(port, "/v1/stats", &auth).await, Some(503));
        assert_eq!(status(port, "/v1/stats", &[]).await, Some(401));
        integration.apply(ServePrefs::default(), &slot).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_required_token_the_vault_lacks_keeps_the_server_off() {
        let port = free_port();
        let slot = FeedSlot::default();
        let guarded = ServePrefs {
            token: true,
            ..on(port)
        };
        for (vault, error) in [
            (
                Arc::new(MemoryVault::default()) as Arc<dyn TokenVault>,
                "serve.error.token_missing",
            ),
            (Arc::new(BrokenVault), "serve.error.vault"),
        ] {
            let mut integration = Integration::new(guarded, vault);
            integration.apply(guarded, &slot).await;
            let view = integration.view();
            assert_eq!(view.state, "error");
            assert_eq!(
                view.error.map(|e| e.id.into_owned()).as_deref(),
                Some(error)
            );
            assert_eq!(status(port, "/health", &[]).await, None, "never tokenless");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_port_taken_by_cuotai_serve_is_named_and_another_program_is_not() {
        // `cuotai --serve --port N` already holds the port.
        let cli = LocalServer::start(
            ServeConfig {
                port: 0,
                ..ServeConfig::default()
            },
            || None,
        )
        .await
        .unwrap();
        let port = cli.addr().port();
        let slot = FeedSlot::default();
        let mut integration = Integration::new(on(port), Arc::new(MemoryVault::default()));
        integration.apply(on(port), &slot).await;
        let view = integration.view();
        assert_eq!(view.state, "error");
        assert_eq!(view.url, None, "never moved to another port");
        assert!(view.cuotai_on_port);
        let error = view.error.unwrap();
        assert_eq!(error.id, "serve.error.port_taken");
        assert!(error.resolve("en-US").contains(&port.to_string()));
        cli.shutdown().await;

        let other = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = other.local_addr().unwrap().port();
        integration.apply(on(port), &slot).await;
        let view = integration.view();
        assert_eq!(view.state, "error");
        assert!(!view.cuotai_on_port);
        // Once the port is free again, turning it on works.
        drop(other);
        integration.apply(on(port), &slot).await;
        assert_eq!(integration.view().state, "on");
        integration.apply(ServePrefs::default(), &slot).await;
    }

    /// `Integration::apply` is the lifecycle boundary Settings → Integration
    /// uses when the user flips `allow_browsers` or `token` on the same
    /// port. The contract is that the listener is **kept open** for in-place
    /// swaps (only policy and `Source` change); the macOS CI hit `ECONNREFUSED`
    /// when the implementation closed and rebound the socket every time, so
    /// this test pins two things at once:
    ///
    /// - `Integration::cold_starts()` stays `1` across same-port policy and
    ///   `Source` swaps (the structural invariant — no new bind was opened).
    /// - The next live request after each `apply` reflects the new policy
    ///   (CORS on / off, browser-origin allow / reject) and the new `Source`
    ///   (the JSON the server returns contains the new account name, not
    ///   the old one).
    ///
    /// A binding-identity change increments the counter to `2`, proving the
    /// cold path runs exactly when reconfigure says it must.
    #[tokio::test(flavor = "multi_thread")]
    async fn apply_hot_swaps_same_port_policy_and_source_and_cold_restarts_only_on_bind_change() {
        let port = free_port();
        let slot = FeedSlot::default();
        let mut integration = Integration::new(on(port), Arc::new(MemoryVault::default()));
        slot.publish(Some(feed_named("Personal Max")));

        // First cold start: account "Personal Max".
        integration.apply(on(port), &slot).await;
        assert_eq!(integration.view().state, "on");
        assert_eq!(integration.cold_starts(), 1, "first apply binds once");
        let (_, body) = get(port, "/v1/snapshot", &[]).await.unwrap();
        assert!(body.contains("Personal Max"), "{body}");

        // Same-port CORS on: hot swap. Counter stays at 1.
        integration
            .apply(
                ServePrefs {
                    allow_browsers: true,
                    ..on(port)
                },
                &slot,
            )
            .await;
        assert_eq!(
            integration.cold_starts(),
            1,
            "CORS flip must not bind a new socket"
        );
        let browser = [("Origin", "https://example.com")];
        let (code, answer) = get(port, "/v1/limits", &browser).await.unwrap();
        assert_eq!(code, 200, "browser-origin limits reachable");
        assert!(
            answer.contains("Access-Control-Allow-Origin: *"),
            "{answer}"
        );

        // Source swap: a different `FeedSlot` with a distinct account name.
        // The source the running server reads must change without rebinding.
        let other_slot = FeedSlot::default();
        other_slot.publish(Some(feed_named("Work Pro")));
        integration.apply(on(port), &other_slot).await;
        assert_eq!(
            integration.cold_starts(),
            1,
            "source swap must not bind a new socket"
        );
        let (_, body) = get(port, "/v1/snapshot", &[]).await.unwrap();
        assert!(
            body.contains("Work Pro"),
            "snapshot reflects new source: {body}"
        );
        assert!(
            !body.contains("Personal Max"),
            "snapshot must not contain old source data: {body}"
        );

        // CORS off: another hot swap. Counter still 1.
        integration.apply(on(port), &slot).await;
        assert_eq!(integration.cold_starts(), 1);
        let (code, _) = get(port, "/v1/limits", &browser).await.unwrap();
        assert_eq!(code, 403, "browser origin refused when allow_browsers off");

        // Binding-identity change: counter must move to 2 (close + reopen).
        let other_port = free_port();
        integration
            .apply(
                ServePrefs {
                    port: other_port,
                    ..on(other_port)
                },
                &slot,
            )
            .await;
        assert_eq!(integration.view().state, "on");
        assert_eq!(
            integration.cold_starts(),
            2,
            "port change must cold-restart exactly once"
        );
        let (_, body) = get(other_port, "/v1/snapshot", &[]).await.unwrap();
        assert!(body.contains("Personal Max"), "{body}");

        integration.apply(ServePrefs::default(), &slot).await;
    }

    /// When a binding-identity change fails (port in use by an external
    /// listener), the apply path must turn the server off and report the
    /// error to the UI rather than silently keeping the old listener open.
    /// No new bind succeeded, so `cold_starts()` does not advance beyond
    /// the count that existed before the failed apply.
    #[tokio::test(flavor = "multi_thread")]
    async fn apply_reports_a_failed_binding_change_and_does_not_advance_cold_starts() {
        let port = free_port();
        let slot = FeedSlot::default();
        let mut integration = Integration::new(on(port), Arc::new(MemoryVault::default()));
        slot.publish(Some(feed_named("Personal Max")));
        integration.apply(on(port), &slot).await;
        assert_eq!(integration.cold_starts(), 1);

        // External listener holds the requested new port: the cold restart
        // cannot bind, so the apply must surface a `PortInUse` error and
        // turn off the server (no silent retention of a now-out-of-policy
        // listener that points at a port the user's port selection no
        // longer matches).
        let holder = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let taken_port = holder.local_addr().unwrap().port();
        integration
            .apply(
                ServePrefs {
                    port: taken_port,
                    ..on(taken_port)
                },
                &slot,
            )
            .await;
        assert_eq!(integration.view().state, "error");
        assert_eq!(
            integration.view().url,
            None,
            "a failed bind must not advertise a URL"
        );
        assert_eq!(
            integration.cold_starts(),
            1,
            "no successful LocalServer::start happened during the failure"
        );

        drop(holder);
        integration.apply(ServePrefs::default(), &slot).await;
    }
}
