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
    export::v1::{Export, Options},
    incidents::ProviderStatus,
    refresh::RefreshCycle,
    serve::{self, LocalServer, ServeConfig, ServeError, SystemTokenVault, Token, TokenVault},
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
}

/// Where the backend publishes the [`Feed`]; empty while no configuration
/// is loaded or the server is off (data routes then answer `503`).
#[derive(Clone, Default)]
pub struct FeedSlot {
    feed: Arc<Mutex<Option<Feed>>>,
    /// PRD 0015 RF4.3: where `local_cost` comes from, set once at start.
    cost: Arc<OnceLock<Arc<LocalCost>>>,
}

impl FeedSlot {
    pub fn publish(&self, feed: Option<Feed>) {
        *self.feed.lock().unwrap_or_else(PoisonError::into_inner) = feed;
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
            Export::from_cycle(
                &feed.config,
                &feed.identity,
                &feed.cycle,
                &feed.incidents,
                Utc::now(),
                Options::default(),
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

    /// Stop the server (the port is closed when this returns) and, when
    /// `prefs` turn it on, start it again with them, reading `feed`.
    pub async fn apply(&mut self, prefs: ServePrefs, feed: &FeedSlot) {
        if let Some(server) = self.server.take() {
            server.shutdown().await;
        }
        self.prefs = prefs;
        self.error = None;
        self.cuotai_on_port = false;
        if !prefs.enabled {
            return;
        }
        match self.start(feed.clone()).await {
            Ok(server) => self.server = Some(server),
            Err(error) => {
                self.cuotai_on_port = matches!(error, ServeError::PortInUse { .. })
                    && serve::is_cuotai_server(prefs.port).await;
                self.error = Some(error);
            }
        }
    }

    /// A required token that is not in the vault keeps the server off: it
    /// never falls back to serving without one.
    async fn start(&self, feed: FeedSlot) -> Result<LocalServer, ServeError> {
        let token = if self.prefs.token {
            Some(self.stored_token().await?)
        } else {
            None
        };
        let config = ServeConfig {
            port: self.prefs.port,
            port_fallback: false,
            token,
            allow_browsers: self.prefs.allow_browsers,
            ..ServeConfig::default()
        };
        LocalServer::start(config, move || feed.export()).await
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

    fn on(port: u16) -> ServePrefs {
        ServePrefs {
            enabled: true,
            port,
            ..ServePrefs::default()
        }
    }

    /// The app's cycle after one answered query of its only account.
    fn feed() -> Feed {
        let config = Config {
            accounts: vec![Account {
                name: "Personal Max".into(),
                provider: Provider::Claude,
                ..Account::default()
            }],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let token = cycle.begin(0).unwrap();
        let report = Report {
            account: "Personal Max".into(),
            provider: Provider::Claude,
            checked_at: Utc::now(),
            status: Status::Ready,
            metrics: vec![],
            notes: vec![],
            notes_l10n: vec![],
            source: String::new(),
            session_expires_at: None,
        };
        cycle.finish_account(0, Some(report), token, Instant::now());
        Feed {
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
        let port = free_port();
        let slot = FeedSlot::default();
        let mut integration = Integration::new(on(port), Arc::new(MemoryVault::default()));
        assert_eq!(
            integration.stored_token().await,
            Err(ServeError::TokenMissing)
        );
        integration.store_new_token().await.unwrap();
        let guarded = ServePrefs {
            token: true,
            ..on(port)
        };
        integration.apply(guarded, &slot).await;
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
        integration.apply(on(port), &slot).await;
        assert_eq!(
            integration.stored_token().await,
            Err(ServeError::TokenMissing)
        );
        assert_eq!(status(port, "/health", &[]).await, Some(200));
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
}
