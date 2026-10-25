//! PRD 0036 RF2.3: how Cuotai trusts a 9router instance served over HTTPS.
//!
//! - **Platform** (default): the client's built-in roots, as for every
//!   provider.
//! - **CA file** (`params.tls_ca_file`): a PEM bundle the user chose is
//!   trusted in addition to the built-in roots; the host name is still
//!   verified.
//! - **Pinned certificate** (`params.tls_pin_sha256`): only a leaf
//!   certificate whose SHA-256 (of its DER) equals the pin is accepted,
//!   with the handshake signatures still verified. This is how a
//!   self-signed instance is used. A pin is captured trust-on-first-use in
//!   the Test connection step ([`capture_fingerprint`]), shown to the user
//!   and saved only after their confirmation; a mismatch later is an
//!   authentication-class error and is never re-pinned silently.
//!
//! There is no "accept any certificate" mode. The only verifier that
//! accepts an unknown certificate is the capture one, which sends
//! `GET /api/health` (public, no credential) and nothing else.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use url::Url;

use crate::model::{Account, Status};
use crate::providers::QueryError;
use crate::text::Localized;

/// Largest CA file read (a PEM bundle of a few certificates is a few KiB).
const CA_FILE_MAX: u64 = 1024 * 1024;

/// Note id of a pin mismatch.
pub(crate) const CHANGED_NOTE: &str = "query.nine_router.certificate_changed";

/// The trust mode of one account, from its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TlsMode {
    Platform,
    CaFile(String),
    Pin([u8; 32]),
}

/// Why the TLS parameters cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TlsError {
    /// `tls_pin_sha256` is not 64 hexadecimal digits (colons allowed).
    PinFormat,
    /// Both a CA file and a pin, or either one on a plain-HTTP address.
    Conflict,
}

/// `AB:CD:…` or `abcd…` (64 hex digits) as bytes.
pub(crate) fn parse_pin(raw: &str) -> Option<[u8; 32]> {
    let hex: String = raw
        .trim()
        .chars()
        .filter(|c| *c != ':' && !c.is_whitespace())
        .collect();
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// The fingerprint shown to people: upper-case hex pairs joined by colons.
pub fn format_fingerprint(digest: &[u8; 32]) -> String {
    digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn param<'a>(account: &'a Account, key: &str) -> Option<&'a str> {
    account
        .params
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

/// The trust mode of `account` reaching `base`.
pub(crate) fn mode(account: &Account, base: &Url) -> Result<TlsMode, TlsError> {
    let ca = param(account, "tls_ca_file");
    let pin = param(account, "tls_pin_sha256");
    if (ca.is_some() || pin.is_some()) && base.scheme() != "https" {
        return Err(TlsError::Conflict);
    }
    match (ca, pin) {
        (Some(_), Some(_)) => Err(TlsError::Conflict),
        (Some(path), None) => Ok(TlsMode::CaFile(path.to_owned())),
        (None, Some(pin)) => parse_pin(pin).map(TlsMode::Pin).ok_or(TlsError::PinFormat),
        (None, None) => Ok(TlsMode::Platform),
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("cuotai/", env!("CARGO_PKG_VERSION")))
}

fn config_error(id: &'static str, message: &str) -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::NeedsConfiguration,
        note: Localized::fixed(id, message),
    }
}

/// A client of its own and, with a pin, the flag its verifier raises on a
/// mismatch.
pub(crate) type OwnClient = (reqwest::Client, Option<Arc<AtomicBool>>);

/// A client of its own for `mode` (`None` for the platform roots: the
/// shared client is used).
pub(crate) fn client(mode: &TlsMode, timeout: Duration) -> Result<Option<OwnClient>, QueryError> {
    match mode {
        TlsMode::Platform => Ok(None),
        TlsMode::CaFile(path) => {
            let pem = read_ca(Path::new(path))?;
            let certificates = reqwest::Certificate::from_pem_bundle(&pem).map_err(|_| {
                config_error(
                    "query.nine_router.ca_file",
                    "O arquivo de CA do 9router não pôde ser lido como PEM.",
                )
            })?;
            if certificates.is_empty() {
                return Err(config_error(
                    "query.nine_router.ca_file",
                    "O arquivo de CA do 9router não pôde ser lido como PEM.",
                ));
            }
            let mut builder = builder(timeout);
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
            let client = builder.build().map_err(|_| QueryError::transport())?;
            Ok(Some((client, None)))
        }
        TlsMode::Pin(pin) => {
            let mismatch = Arc::new(AtomicBool::new(false));
            let verifier = PinVerifier {
                pin: *pin,
                mismatch: Arc::clone(&mismatch),
                provider: provider(),
            };
            let config = tls_config(Arc::new(verifier))?;
            let client = builder(timeout)
                .use_preconfigured_tls(config)
                .build()
                .map_err(|_| QueryError::transport())?;
            Ok(Some((client, Some(mismatch))))
        }
    }
}

fn read_ca(path: &Path) -> Result<Vec<u8>, QueryError> {
    let unreadable = || {
        config_error(
            "query.nine_router.ca_file",
            "O arquivo de CA do 9router não pôde ser lido como PEM.",
        )
    };
    let metadata = std::fs::metadata(path).map_err(|_| unreadable())?;
    if !metadata.is_file() || metadata.len() > CA_FILE_MAX {
        return Err(unreadable());
    }
    std::fs::read(path).map_err(|_| unreadable())
}

fn tls_config(verifier: Arc<dyn ServerCertVerifier>) -> Result<rustls::ClientConfig, QueryError> {
    Ok(rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|_| QueryError::transport())?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// The error of a pin mismatch: the instance presented another
/// certificate. Authentication-class: the user must look at it.
pub(crate) fn changed() -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::AuthenticationError,
        note: Localized::fixed(
            CHANGED_NOTE,
            "O certificado da instância do 9router mudou; confira-o e teste a conexão de novo antes de confiar nele.",
        ),
    }
}

/// Accepts exactly the pinned leaf certificate; signatures are verified.
#[derive(Debug)]
struct PinVerifier {
    pin: [u8; 32],
    mismatch: Arc<AtomicBool>,
    provider: Arc<CryptoProvider>,
}

fn digest(certificate: &CertificateDer<'_>) -> [u8; 32] {
    Sha256::digest(certificate.as_ref()).into()
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if digest(end_entity) == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            self.mismatch.store(true, Ordering::Relaxed);
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Records the leaf certificate's digest and accepts it: used only by
/// [`capture_fingerprint`], whose one request carries no credential.
#[derive(Debug)]
struct CaptureVerifier {
    seen: Arc<Mutex<Option<[u8; 32]>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for CaptureVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(digest(end_entity));
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// PRD 0036 RF2.3 (trust on first use): the SHA-256 of the certificate
/// `health` (an `https` URL of the instance's public health route) presents,
/// read with a verifier that records it. The single request carries no
/// cookie, password or token, and its answer is discarded.
pub(crate) async fn capture_fingerprint(health: Url, timeout: Duration) -> Option<[u8; 32]> {
    if health.scheme() != "https" {
        return None;
    }
    let seen = Arc::new(Mutex::new(None));
    let verifier = CaptureVerifier {
        seen: Arc::clone(&seen),
        provider: provider(),
    };
    let client = builder(timeout)
        .use_preconfigured_tls(tls_config(Arc::new(verifier)).ok()?)
        .build()
        .ok()?;
    let _ = client.get(health).send().await;
    let digest = *seen.lock().unwrap_or_else(PoisonError::into_inner);
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(params: &[(&str, &str)]) -> Account {
        Account {
            name: "home".into(),
            provider: crate::Provider::NineRouter,
            params: params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            ..Account::default()
        }
    }

    const PIN: &str = "AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89";

    #[test]
    fn pins_read_with_or_without_colons_and_print_with_them() {
        let pin = parse_pin(PIN).unwrap();
        assert_eq!(parse_pin(&PIN.replace(':', "").to_lowercase()), Some(pin));
        assert_eq!(format_fingerprint(&pin), PIN);
        assert!(parse_pin("ABCD").is_none());
        assert!(parse_pin(&"G".repeat(64)).is_none());
    }

    #[test]
    fn the_mode_follows_the_parameters_and_refuses_conflicts() {
        let https = Url::parse("https://router.example.com/").unwrap();
        let http = Url::parse("http://127.0.0.1:20128/").unwrap();
        assert_eq!(mode(&account(&[]), &https), Ok(TlsMode::Platform));
        assert_eq!(mode(&account(&[]), &http), Ok(TlsMode::Platform));
        assert_eq!(
            mode(&account(&[("tls_ca_file", "/etc/ca.pem")]), &https),
            Ok(TlsMode::CaFile("/etc/ca.pem".into()))
        );
        assert!(matches!(
            mode(&account(&[("tls_pin_sha256", PIN)]), &https),
            Ok(TlsMode::Pin(_))
        ));
        assert_eq!(
            mode(&account(&[("tls_pin_sha256", "nope")]), &https),
            Err(TlsError::PinFormat)
        );
        assert_eq!(
            mode(
                &account(&[("tls_pin_sha256", PIN), ("tls_ca_file", "/etc/ca.pem")]),
                &https
            ),
            Err(TlsError::Conflict)
        );
        // TLS options make no sense over plain HTTP.
        assert_eq!(
            mode(&account(&[("tls_pin_sha256", PIN)]), &http),
            Err(TlsError::Conflict)
        );
        // Blank values are no option.
        assert_eq!(
            mode(&account(&[("tls_pin_sha256", "  ")]), &https),
            Ok(TlsMode::Platform)
        );
    }

    #[test]
    fn a_missing_or_non_pem_ca_file_needs_configuration() {
        let error = client(
            &TlsMode::CaFile("/nonexistent/ca.pem".into()),
            Duration::from_secs(1),
        )
        .err()
        .unwrap();
        assert_eq!(error.status, Status::NeedsConfiguration);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ca.pem");
        std::fs::write(&path, "not a certificate").unwrap();
        let error = client(
            &TlsMode::CaFile(path.to_string_lossy().into_owned()),
            Duration::from_secs(1),
        )
        .err()
        .unwrap();
        assert_eq!(error.note.id, "query.nine_router.ca_file");
    }

    // Throwaway P-256 test material generated for these tests only (100-year
    // validity, `localhost` / `127.0.0.1`); it trusts and protects nothing.
    // Kept as base64 DER so it is never mistaken for a deployed key.
    const LEAF: &str = "MIIBfDCCASKgAwIBAgIUGgs6OPyscnxLLRFCV5jO/S04xiUwCgYIKoZIzj0EAwIwGTEXMBUGA1UEAwwOQ3VvdGFpIHRlc3QgQ0EwIBcNMjYxMDI1MTYwNDU5WhgPMjEyNjEwMDExNjA0NTlaMBQxEjAQBgNVBAMMCWxvY2FsaG9zdDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABCNpfb8Kri6gapkCtg0wODB5EtjE8yDhXT6YvHZLv+sBxh+2fJv0seV6+NLVOc+sdasiIbKynub1LaJKTnwD+OijSzBJMAkGA1UdEwQCMAAwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMBMGA1UdJQQMMAoGCCsGAQUFBwMBMAsGA1UdDwQEAwIHgDAKBggqhkjOPQQDAgNIADBFAiEAzV3tPuPtdwL02uOzBZI9KSwA21acWQcOGblhNpj2W4ECID/4CHuy9MToJ6r/KbpK38iMPx833oU2QJ0rGfiQH7uM";
    const LEAF_KEY: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg5F7FXqa0l//o6hUXCus07ATsN8hdHIgKOILaL97VL8ehRANCAAQjaX2/Cq4uoGqZArYNMDgweRLYxPMg4V0+mLx2S7/rAcYftnyb9LHlevjS1TnPrHWrIiGysp7m9S2iSk58A/jo";
    const CA: &str = "MIIBZzCCAQ6gAwIBAgIUFmAhiAFdC/DMHdsLlVzueuZns4kwCgYIKoZIzj0EAwIwGTEXMBUGA1UEAwwOQ3VvdGFpIHRlc3QgQ0EwIBcNMjYxMDI1MTYwNDU5WhgPMjEyNjEwMDExNjA0NTlaMBkxFzAVBgNVBAMMDkN1b3RhaSB0ZXN0IENBMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEJTIUNqZvVDp0DBmsZV+Xiab7yEeIWosNZgk+wf6u5M31AoiQrPWqST2MKih0nFUbuwiI1spjXKFaVWnJ38xlraMyMDAwHQYDVR0OBBYEFJEPF/T8dmWwMqOF6nHRL/VVgAedMA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDRwAwRAIgeioIVyQDiAUy717r/1axmyNU4KncdiCrPGCs5EYQQC4CIDOdR694iT54k17Y+cLI1lH+eIsQben9cLaSzQTHncct";
    const OTHER: &str = "MIIBejCCASCgAwIBAgIUF1yHqc8fY1xbTlKIvd7vUS/wBaEwCgYIKoZIzj0EAwIwFDESMBAGA1UEAwwJbG9jYWxob3N0MCAXDTI2MTAyNTE2MDQ0MFoYDzIxMjYxMDAxMTYwNDQwWjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAARzQ5N2mrBmvO0bBEe4Wlu9TLg/wXW//2NKaEKxGxdpre3rtCCuaT9g4opevWoLTQiKoVUn8VMtVHVmXvU0Y6Zro04wTDAdBgNVHQ4EFgQUKBgOj++7DFTLlJmv8ftdZ49r5CcwDwYDVR0TAQH/BAUwAwEB/zAaBgNVHREEEzARgglsb2NhbGhvc3SHBH8AAAEwCgYIKoZIzj0EAwIDSAAwRQIhAMY0UfZxlzSSpGo1qH9mrSQkcyA7f3TR+vF+g5yz8P0ZAiBejN0SbpiWFQAo6urE9Wu19phyIDUPoZOZOIljZeP45A==";
    const OTHER_KEY: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgny6saazqC7a0MJ0tV/1OYrTFYFMQxo8vCgQStb8WvoChRANCAARzQ5N2mrBmvO0bBEe4Wlu9TLg/wXW//2NKaEKxGxdpre3rtCCuaT9g4opevWoLTQiKoVUn8VMtVHVmXvU0Y6Zr";

    fn der(text: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .unwrap()
    }

    fn pem(label: &str, text: &str) -> String {
        let lines: Vec<String> = text
            .as_bytes()
            .chunks(64)
            .map(|chunk| String::from_utf8(chunk.to_vec()).unwrap())
            .collect();
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            lines.join("\n")
        )
    }

    /// A tiny HTTPS 9router on 127.0.0.1 presenting `cert`: health, version
    /// and an empty connection list, without a password.
    async fn https_router(cert: &str, key: &str) -> String {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(der(cert))],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der(key))),
            )
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(stream).await else {
                        return;
                    };
                    let mut head = vec![0u8; 4096];
                    let read = tls.read(&mut head).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&head[..read]).to_string();
                    let body = if request.starts_with("GET /api/health") {
                        r#"{"ok":true}"#.to_string()
                    } else if request.starts_with("GET /api/version") {
                        r#"{"currentVersion":"0.5.109"}"#.to_string()
                    } else if request.starts_with("GET /api/providers/client") {
                        r#"{"connections":[],"pagination":{"totalPages":1}}"#.to_string()
                    } else {
                        "{}".to_string()
                    };
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = tls.write_all(answer.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        format!("https://127.0.0.1:{}", address.port())
    }

    struct NoSecrets;
    impl crate::Credentials for NoSecrets {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    async fn test_with(
        base: &str,
        params: &[(&str, &str)],
    ) -> crate::providers::nine_router::TestOutcome {
        let mut account = account(params);
        account.params.insert("base_url".into(), base.into());
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ctx = crate::providers::Context {
            http: &http,
            credentials: &NoSecrets,
            timeout: Duration::from_secs(5),
        };
        crate::providers::nine_router::test_connection_at(
            &account,
            &ctx,
            crate::providers::nine_router::Limits::DEFAULT,
            chrono::Utc::now(),
        )
        .await
    }

    /// RF2.3 acceptance: a self-signed instance is refused by the platform
    /// roots (its fingerprint offered for trust on first use), accepted
    /// with the right pin or CA file, and refused after its certificate
    /// changed, with the new fingerprint and never re-pinned.
    #[tokio::test]
    async fn self_signed_instances_need_a_pin_or_a_ca_file() {
        use crate::providers::nine_router::TestOutcome;
        let leaf_pin = format_fingerprint(&digest(&CertificateDer::from(der(LEAF))));
        let base = https_router(LEAF, LEAF_KEY).await;

        assert_eq!(
            test_with(&base, &[]).await,
            TestOutcome::UntrustedCertificate {
                fingerprint: leaf_pin.clone()
            }
        );
        let connected = |outcome: &TestOutcome| matches!(outcome, TestOutcome::Connected { .. });
        let pinned = test_with(&base, &[("tls_pin_sha256", &leaf_pin)]).await;
        assert!(connected(&pinned), "{pinned:?}");

        let dir = tempfile::tempdir().unwrap();
        let ca_file = dir.path().join("ca.pem");
        std::fs::write(&ca_file, pem("CERTIFICATE", CA)).unwrap();
        let ca_path = ca_file.to_string_lossy().into_owned();
        let with_ca = test_with(&base, &[("tls_ca_file", &ca_path)]).await;
        assert!(connected(&with_ca), "{with_ca:?}");

        // The instance now presents another certificate.
        let changed = https_router(OTHER, OTHER_KEY).await;
        let other_pin = format_fingerprint(&digest(&CertificateDer::from(der(OTHER))));
        assert_eq!(
            test_with(&changed, &[("tls_pin_sha256", &leaf_pin)]).await,
            TestOutcome::CertificateChanged {
                fingerprint: other_pin
            }
        );
        // A CA that did not sign it does not help either.
        assert!(matches!(
            test_with(&changed, &[("tls_ca_file", &ca_path)]).await,
            TestOutcome::UntrustedCertificate { .. }
        ));
        // A refresh against a changed pin is an authentication error.
        let mut account = account(&[("tls_pin_sha256", &leaf_pin)]);
        account.params.insert("base_url".into(), changed.clone());
        let http = reqwest::Client::new();
        let ctx = crate::providers::Context {
            http: &http,
            credentials: &NoSecrets,
            timeout: Duration::from_secs(5),
        };
        let error = crate::providers::nine_router::fetch_at(
            &account,
            &ctx,
            crate::providers::nine_router::Limits::DEFAULT,
            chrono::Utc::now(),
        )
        .await
        .map(|_| ())
        .expect_err("a changed certificate fails");
        assert_eq!(error.status, Status::AuthenticationError);
        assert_eq!(error.note.id, CHANGED_NOTE);
    }

    #[test]
    fn a_pinned_client_builds() {
        let (client, mismatch) = client(&TlsMode::Pin([7; 32]), Duration::from_secs(1))
            .unwrap()
            .unwrap();
        drop(client);
        assert!(!mismatch.unwrap().load(Ordering::Relaxed));
    }
}
