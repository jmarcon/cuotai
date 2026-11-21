//! The loopback listener of the browser sign-ins whose redirect names a
//! host (security review 2026-11-21, M-1).
//!
//! A browser resolves `localhost` to `::1` and `127.0.0.1` and usually
//! tries `::1` first. A listener on `127.0.0.1:P` alone leaves `[::1]:P` to
//! any other local program, which would then receive the callback (for
//! Cline, the token itself). So a `localhost` redirect binds the same port
//! on both loopback addresses, as the local server does (PRD 0012 RF3.1): a
//! taken `[::1]` side counts as a taken port, a fixed port fails closed and
//! an ephemeral one is drawn again. A host without IPv6 loopback keeps
//! `127.0.0.1` alone, since nobody can listen on `::1` there either. A
//! `127.0.0.1` redirect binds only that address.

use std::future::poll_fn;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::task::Poll;
use tokio::net::{TcpListener, TcpStream};

/// Ephemeral ports tried before giving up when the `[::1]` side is taken.
const EPHEMERAL_ATTEMPTS: usize = 8;

/// Why [`bind`] failed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum BindError {
    /// The fixed port is held on either loopback address.
    PortInUse,
    /// No socket could be bound (or the redirect host is not a loopback
    /// name this module knows).
    Failed,
}

/// The callback socket(s) of one sign-in; dropping it closes them all.
#[derive(Debug)]
pub(super) struct Listener {
    v4: TcpListener,
    v6: Option<TcpListener>,
}

impl Listener {
    pub(super) fn port(&self) -> Result<u16, BindError> {
        self.v4
            .local_addr()
            .map(|addr| addr.port())
            .map_err(|_| BindError::Failed)
    }

    /// The next connection on either address.
    pub(super) async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        poll_fn(|cx| {
            if let Poll::Ready(result) = self.v4.poll_accept(cx) {
                return Poll::Ready(result);
            }
            if let Some(v6) = &self.v6 {
                if let Poll::Ready(result) = v6.poll_accept(cx) {
                    return Poll::Ready(result);
                }
            }
            Poll::Pending
        })
        .await
    }

    #[cfg(test)]
    pub(super) fn has_v6(&self) -> bool {
        self.v6.is_some()
    }
}

/// Most callback connections [`serve`] reads at once; beyond that it stops
/// accepting until one finishes (each lasts at most its own deadline).
const MAX_OPEN: usize = 64;

/// Security review 2026-11-21, L-6: serve the connections of a sign-in's
/// callback listener side by side, each bounded by `per_connection` and
/// the sign-in's `deadline`, until `handle` decides. One connection held
/// idle by another local program no longer blocks the browser's.
///
/// `handle` reads one request, answers it and returns `None` to keep
/// listening (a stray request, a wrong state, a token that does not
/// decrypt) or the sign-in's outcome. `accept` yields the next connection;
/// it must be cancel-safe, as the Tokio listeners are.
pub(super) async fn serve<T, Accept, Accepted, Handle, Handled>(
    mut accept: Accept,
    deadline: tokio::time::Instant,
    per_connection: std::time::Duration,
    mut handle: Handle,
) -> Result<T, super::OAuthError>
where
    Accept: FnMut() -> Accepted,
    Accepted: std::future::Future<Output = std::io::Result<(TcpStream, SocketAddr)>>,
    Handle: FnMut(TcpStream) -> Handled,
    Handled: std::future::Future<Output = Option<Result<T, super::OAuthError>>>,
{
    use futures_util::stream::{FuturesUnordered, StreamExt};
    let mut open = FuturesUnordered::new();
    loop {
        tokio::select! {
            accepted = accept(), if open.len() < MAX_OPEN => {
                let (stream, _) = accepted.map_err(|_| super::OAuthError::Network)?;
                let until = std::cmp::min(deadline, tokio::time::Instant::now() + per_connection);
                open.push(tokio::time::timeout_at(until, handle(stream)));
            }
            Some(done) = open.next(), if !open.is_empty() => {
                // A connection past its own deadline is dropped unanswered.
                if let Ok(Some(outcome)) = done {
                    return outcome;
                }
            }
            () = tokio::time::sleep_until(deadline) => return Err(super::OAuthError::Expired),
        }
    }
}

/// Bind the callback listener for a redirect to `http://<redirect_host>:P`,
/// on `fixed_port` or an ephemeral port.
pub(super) async fn bind(
    redirect_host: &str,
    fixed_port: Option<u16>,
) -> Result<Listener, BindError> {
    let both = match redirect_host {
        "localhost" => true,
        "127.0.0.1" => false,
        _ => return Err(BindError::Failed),
    };
    let attempts = if fixed_port.is_some() {
        1
    } else {
        EPHEMERAL_ATTEMPTS
    };
    for _ in 0..attempts {
        let v4 = match TcpListener::bind((Ipv4Addr::LOCALHOST, fixed_port.unwrap_or(0))).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == ErrorKind::AddrInUse && fixed_port.is_some() => {
                return Err(BindError::PortInUse)
            }
            Err(_) => return Err(BindError::Failed),
        };
        if !both {
            return Ok(Listener { v4, v6: None });
        }
        let port = v4.local_addr().map_err(|_| BindError::Failed)?.port();
        match TcpListener::bind((Ipv6Addr::LOCALHOST, port)).await {
            Ok(v6) => return Ok(Listener { v4, v6: Some(v6) }),
            // Another program holds `[::1]:port`: never answer beside it.
            Err(error) if error.kind() == ErrorKind::AddrInUse => continue,
            // EADDRNOTAVAIL, EAFNOSUPPORT and the like: this host has no IPv6
            // loopback, so no browser or other program can use it either.
            Err(_) => return Ok(Listener { v4, v6: None }),
        }
    }
    Err(if fixed_port.is_some() {
        BindError::PortInUse
    } else {
        BindError::Failed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Whether this host can bind the IPv6 loopback at all.
    fn ipv6_loopback() -> bool {
        std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok()
    }

    async fn round_trip(listener: &Listener, target: SocketAddr) {
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(target).await.unwrap();
            stream.write_all(b"ping").await.unwrap();
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        client.await.unwrap();
    }

    /// L-6: idle connections held open by another local program do not
    /// delay the callback that arrives after them.
    #[tokio::test]
    async fn idle_connections_do_not_stall_the_callback() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target = listener.local_addr().unwrap();
        let mut idle = Vec::new();
        for _ in 0..8 {
            idle.push(TcpStream::connect(target).await.unwrap());
        }
        let client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let mut stream = TcpStream::connect(target).await.unwrap();
            stream.write_all(b"code").await.unwrap();
        });
        let started = tokio::time::Instant::now();
        let outcome = serve(
            || listener.accept(),
            started + std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(15),
            |mut stream| async move {
                let mut buf = [0u8; 4];
                stream.read_exact(&mut buf).await.ok()?;
                (&buf == b"code").then_some(Ok::<_, super::super::OAuthError>(42))
            },
        )
        .await;
        assert_eq!(outcome.unwrap(), 42);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        client.await.unwrap();
        drop(idle);
    }

    #[tokio::test]
    async fn a_localhost_redirect_holds_both_loopback_addresses_on_one_port() {
        let listener = bind("localhost", None).await.unwrap();
        let port = listener.port().unwrap();
        round_trip(&listener, SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await;
        if !ipv6_loopback() {
            assert!(!listener.has_v6());
            return;
        }
        assert!(listener.has_v6());
        // Nobody else can take the `[::1]` side while the sign-in waits.
        assert!(std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, port)).is_err());
        round_trip(&listener, SocketAddr::from((Ipv6Addr::LOCALHOST, port))).await;
    }

    #[tokio::test]
    async fn a_taken_ipv6_side_fails_a_fixed_port_closed() {
        if !ipv6_loopback() {
            return;
        }
        // Another program already holds `[::1]:P`; `127.0.0.1:P` is free.
        let squatter = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let port = squatter.local_addr().unwrap().port();
        if std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err() {
            return; // the IPv4 side happens to be busy too: nothing to show
        }
        assert_eq!(
            bind("localhost", Some(port)).await.unwrap_err(),
            BindError::PortInUse
        );
        // A `127.0.0.1` redirect does not depend on the `[::1]` side.
        let v4_only = bind("127.0.0.1", Some(port)).await.unwrap();
        assert!(!v4_only.has_v6());
    }

    #[tokio::test]
    async fn a_taken_fixed_ipv4_port_is_in_use_and_unknown_hosts_are_refused() {
        let busy = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = busy.local_addr().unwrap().port();
        assert_eq!(
            bind("localhost", Some(port)).await.unwrap_err(),
            BindError::PortInUse
        );
        assert_eq!(
            bind("example.com", None).await.unwrap_err(),
            BindError::Failed
        );
        assert_eq!(bind("::1", None).await.unwrap_err(), BindError::Failed);
    }
}
