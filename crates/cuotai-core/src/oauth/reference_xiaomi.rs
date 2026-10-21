//! Xiaomi MiMo native login, ported from 9router `src/lib/oauth/providers/xiaomi-mimo.js`.
//!
//! This is NOT standard OAuth2: the platform runs an ECDH-encrypted loopback
//! callback flow against its fixed origin `https://platform.xiaomimimo.com`.
//!   1) We generate an ephemeral X25519 keypair; the browser opens
//!      `<platform>/authorize?pk=<base64 SPKI>&redirect_uri=http://127.0.0.1:<port>/<random>&kn=mimocode&key_name=...`
//!   2) The current platform displays a copyable code; a loopback callback
//!      may also supply it in `u`.
//!   3) Current payload: `[32-byte ephemeral X25519 pubkey][12-byte nonce]
//!      [ciphertext][16-byte tag]`, AES-256-GCM with SHA256(ECDH).
//!      The legacy 9router nonce-first layout remains accepted only if GCM verifies.
//!   4) Plaintext JSON: `{ uid, sk, url? }`.
//!
//! Current layout verified against the platform authorize bundle:
//! https://platform.xiaomimimo.com/static/690.2bb0a0bc9a7e83eb.chunk.js
//!
//! Safety properties:
//! - Only the fixed platform origin is ever placed in the prompt URL; the
//!   base URL returned in the decrypted payload is NEVER used for networking.
//! - The callback is authenticated end-to-end: GCM verification happens
//!   against our own ephemeral secret, low-order (non-contributory) shared
//!   secrets are rejected, and malformed payloads are refused. No secret
//!   material is ever placed in error values (all failures map to the fixed
//!   `OAuthError::InvalidResponse` variant).
//! - The loopback listener is bound to 127.0.0.1 with a random callback path,
//!   bounded header reads, per-connection and total deadlines, and is
//!   cancelled by dropping the pending login.
//!
//! The `sk` issued here is a permanent inference API key — it is NOT a Xiaomi
//! account session cookie and carries no cookie-backed quota surface. The
//! token metadata flags `auth_method = "native_key"` so the manager never
//! treats it as a cookie session. The previous manual cookie-import path
//! remains the only way to obtain cookie-backed access. Because the key does
//! not expire, `refresh` is a pass-through.

use super::{LoginPrompt, OAuthConfig, OAuthError, OAuthToken};
use crate::Provider;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::{
    engine::general_purpose::{STANDARD as B64, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
    Engine as _,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout_at, Instant};
use x25519_dalek::{PublicKey, StaticSecret};

// ---------------------------------------------------------------------------
// Endpoints (registry port: XIAOMI_MIMO_CONFIG). The platform origin is fixed
// and is the ONLY host this flow will ever name; nothing network-facing is
// taken from provider responses.
// ---------------------------------------------------------------------------

const PLATFORM_URL: &str = "https://platform.xiaomimimo.com";
const AUTHORIZE_PATH: &str = "/authorize";
const KEY_NAMESPACE: &str = "mimocode";

/// X25519 SPKI DER prefix (algorithm identifier) ahead of the raw 32-byte key.
const X25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x03, 0x21, 0x00,
];

const GCM_NONCE_BYTES: usize = 12;
const EPHEMERAL_PUB_BYTES: usize = 32;
const GCM_TAG_BYTES: usize = 16;
/// Minimum wire payload: nonce + ephemeral pubkey + tag + 1 ciphertext byte.
const MIN_PAYLOAD_BYTES: usize = GCM_NONCE_BYTES + EPHEMERAL_PUB_BYTES + GCM_TAG_BYTES + 1;

/// Loopback callback constraints (mirror flows.rs).
const CALLBACK_TOTAL_WAIT: Duration = Duration::from_secs(300);
const CALLBACK_CONN_DEADLINE: Duration = Duration::from_secs(15);
const MAX_HEADER_BYTES: usize = 8192;
const RESPONSE_DEADLINE: Duration = Duration::from_secs(5);
/// Maximum raw bytes accepted from the manual code channel (covers
/// screenshot pastes plus a generous wrapping/whitespace margin). The
/// decoded envelope is bounded separately by the existing MAX_HEADER_BYTES.
const MAX_MANUAL_CODE_BYTES: usize = 16 * 1024;

pub(crate) struct PendingLogin {
    prompt: LoginPrompt,
    listener: TcpListener,
    secret: StaticSecret,
    /// Exact callback path the browser is expected to land on. Requests
    /// landing on any other path are ignored — defending against local
    /// injection that would otherwise be silently authenticated by GCM
    /// against the ephemeral key (which, by design, anyone can use).
    callback_path: String,
    /// Region pulled from `OAuthConfig.options["region"]`, if present.
    region: Option<String>,
    deadline: Instant,
}

pub(crate) async fn begin(
    _http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
) -> Result<PendingLogin, OAuthError> {
    if provider != Provider::XiaomiMimo {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.xiaomi.provider_no_native_flow",
            "Este provedor não usa o fluxo nativo Xiaomi MiMo.",
        )));
    }

    // Bind before returning so the browser can be opened immediately after.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| OAuthError::Network)?;
    let port = listener
        .local_addr()
        .map_err(|_| OAuthError::Network)?
        .port();

    // Ephemeral X25519 keypair; public half travels as base64 SPKI in `pk`.
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);
    let public_raw: [u8; 32] = *public.as_bytes();
    let mut spki = Vec::with_capacity(X25519_SPKI_PREFIX.len() + EPHEMERAL_PUB_BYTES);
    spki.extend_from_slice(&X25519_SPKI_PREFIX);
    spki.extend_from_slice(&public_raw);
    let pk_b64 = B64.encode(spki);

    // Random callback path: stray loopback hits learn nothing, and a browser
    // redirect that preserves the redirect URI lands exactly where we expect.
    let callback_path = format!("/mimo-callback/{}", random_hex_16()?);
    let redirect_uri = format!("http://127.0.0.1:{port}{callback_path}");

    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("pk", pk_b64.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("kn", KEY_NAMESPACE),
            ("key_name", key_name().as_str()),
        ])
        .finish();

    let region = config
        .options
        .get("region")
        .map(|r| r.trim().to_owned())
        .filter(|r| !r.is_empty());

    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: format!("{PLATFORM_URL}{AUTHORIZE_PATH}?{query}"),
            user_code: None,
        },
        listener,
        secret,
        callback_path,
        region,
        deadline: Instant::now() + CALLBACK_TOTAL_WAIT,
    })
}

impl PendingLogin {
    pub(crate) fn prompt(&self) -> &LoginPrompt {
        &self.prompt
    }

    pub(crate) async fn finish(self) -> Result<OAuthToken, OAuthError> {
        let payload = self.wait_for_callback().await?;
        Ok(self.token_from_payload(payload))
    }

    /// Accept either the loopback callback OR a manually-pasted code from the
    /// caller. The two sources race under the original deadline (which is
    /// never extended). A code submission that fails authentication surfaces
    /// a fixed Portuguese message via `errors` and the loopback listener
    /// keeps waiting on the SAME ephemeral key — so retrying with a code
    /// from a later screenshot continues to work without reissuing a fresh
    /// `pk`/redirect_uri pair.
    ///
    /// `errors` is best-effort: if it is full or closed, the same code
    /// continues to be processed and the listener keeps running. The same
    /// goes for `codes`: if the receiver is dropped, the call falls back to
    /// the automatic loopback path (no busy-loop).
    pub(crate) async fn finish_with_codes(
        self,
        mut codes: tokio::sync::mpsc::Receiver<String>,
        errors: tokio::sync::mpsc::Sender<String>,
    ) -> Result<OAuthToken, OAuthError> {
        // Performs one manual-code attempt. Returns Some(token) on success
        // or None when the input was structurally rejected by the listener
        // (so we surface it via `errors`). Empty / oversized inputs are
        // treated as rejections with no error message — the user clearly
        // submitted nothing usable, so no TUI noise is warranted.
        fn try_manual(pending: &PendingLogin, raw: &str) -> Option<OAuthToken> {
            // Normalize: trim surrounding whitespace (screenshots may capture
            // an extra newline) and strip any URL wrapper the user might
            // have pasted along with the bare code.
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Bound the raw input before any decoding work.
            if trimmed.len() > MAX_MANUAL_CODE_BYTES {
                return None;
            }
            let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
            let candidate = extract_code_param(&compact);
            let encrypted = candidate.as_deref().unwrap_or(&compact);
            decrypt_callback(&pending.secret, encrypted)
                .ok()
                .map(|payload| pending.token_from_payload(payload))
        }

        let callback = self.wait_for_callback();
        tokio::pin!(callback);
        loop {
            // Race the next code against the loopback deadline. Errors
            // (decode/auth failures) do NOT reset the deadline — the same
            // deadline must protect the listener's lifetime.
            if Instant::now() >= self.deadline {
                return Err(OAuthError::Expired);
            }
            tokio::select! {
                biased;
                callback = &mut callback => {
                    let payload = callback?;
                    return Ok(self.token_from_payload(payload));
                }
                code = codes.recv() => {
                    match code {
                        Some(raw) => match try_manual(&self, &raw) {
                            Some(token) => return Ok(token),
                            None => {
                                // Code was structurally rejected: tell the
                                // TUI and keep listening for either a fresh
                                // code or the loopback.
                                let _ = errors.try_send(
                                    "Código inválido ou de outra sessão; copie o código desta tentativa.".to_owned(),
                                );
                                continue;
                            }
                        },
                        None => {
                            // Caller closed the code channel: fall back to
                            // the loopback callback without busy-looping.
                            // We must NOT drop `self` here because the
                            // listener still needs to be polled, so just
                            // call wait_for_callback to the end.
                            let payload = callback.await?;
                            return Ok(self.token_from_payload(payload));
                        }
                    }
                }
            }
        }
    }

    /// Build an OAuthToken from a validated callback payload, applying the
    /// shared metadata contract (auth_method = native_key, optional region).
    fn token_from_payload(&self, payload: CallbackPayload) -> OAuthToken {
        let mut metadata: BTreeMap<String, String> = BTreeMap::new();
        metadata.insert("user_id".to_owned(), payload.uid);
        // Permanent inference key, not an account session cookie: the manager
        // must not treat this credential as cookie-backed (no cookie quota).
        metadata.insert("auth_method".to_owned(), "native_key".to_owned());
        if let Some(region) = &self.region {
            metadata.insert("region".to_owned(), region.clone());
        }
        OAuthToken {
            access_token: payload.sk,
            refresh_token: None,
            expires_at: None,
            metadata,
        }
    }

    /// Accept loopback connections until a callback carrying `u` arrives or
    /// the deadline passes. Requests without exactly one `u` parameter are
    /// ignored with a bare 404; a `u` that fails authentication is fatal.
    async fn wait_for_callback(&self) -> Result<CallbackPayload, OAuthError> {
        loop {
            let accepted = timeout_at(self.deadline, self.listener.accept())
                .await
                .map_err(|_| OAuthError::Expired)?
                .map_err(|_| OAuthError::Network)?;
            let (mut stream, _) = accepted;
            let conn_deadline =
                std::cmp::min(self.deadline, Instant::now() + CALLBACK_CONN_DEADLINE);
            let outcome = match timeout_at(
                conn_deadline,
                read_callback(&mut stream, &self.callback_path),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    if Instant::now() >= self.deadline {
                        return Err(OAuthError::Expired);
                    }
                    CallbackOutcome::Ignored
                }
            };
            match outcome {
                CallbackOutcome::Payload(encrypted) => {
                    let payload = match decrypt_callback(&self.secret, &encrypted) {
                        Ok(payload) => payload,
                        Err(error) => {
                            respond(&mut stream, "400 Bad Request", "Autorização inválida.").await;
                            return Err(error);
                        }
                    };
                    respond(
                        &mut stream,
                        "200 OK",
                        "<!doctype html><html><body><p>Autorização recebida. \
                         Você pode fechar esta janela e voltar ao Cuotai.</p></body></html>",
                    )
                    .await;
                    return Ok(payload);
                }
                CallbackOutcome::Denied => {
                    respond(
                        &mut stream,
                        "200 OK",
                        "<!doctype html><html><body><p>A autorização foi negada. \
                         Você pode fechar esta janela.</p></body></html>",
                    )
                    .await;
                    return Err(OAuthError::Denied);
                }
                CallbackOutcome::Malformed => {
                    // Generic body: no detail about what was rejected.
                    respond(
                        &mut stream,
                        "400 Bad Request",
                        "<!doctype html><html><body><p>Não foi possível concluir \
                         a autorização. Feche esta janela e tente novamente.</p></body></html>",
                    )
                    .await;
                    return Err(OAuthError::InvalidResponse);
                }
                CallbackOutcome::Ignored => {
                    // No details: a stray request learns nothing from us.
                    respond(&mut stream, "404 Not Found", "").await;
                }
            }
        }
    }
}

/// The MiMo key never expires and cannot be rotated headlessly: there is no
/// refresh endpoint in this flow, so the stored key is returned unchanged.
pub(crate) async fn refresh(
    _http: &reqwest::Client,
    provider: Provider,
    _config: &OAuthConfig,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    if provider != Provider::XiaomiMimo {
        return Err(OAuthError::Expired);
    }
    Ok(token.clone())
}

// ---------------------------------------------------------------------------
// Crypto (port of decryptCallback in xiaomi-mimo.js)
// ---------------------------------------------------------------------------

struct CallbackPayload {
    uid: String,
    sk: String,
}

/// Try the three encodings a screenshot paste or clipboard may produce:
/// URL-safe no-pad (underscores/hyphens), standard no-pad, and the original
/// standard padded form. The result is identical bytes either way; the
/// caller still performs every authentication check (length, low-order
/// rejection, GCM tag, JSON shape) so broadening the decoder cannot weaken
/// the security guarantees.
fn decode_b64_envelope(encoded: &str) -> Result<Vec<u8>, OAuthError> {
    // Quick reject: any non-base64 alphabet character (including URL-unsafe
    // chars like spaces that survived whitespace stripping) makes a decode
    // attempt useless. The decoders below would otherwise treat them as
    // truncated input and silently truncate.
    let alphabet_ok = encoded.bytes().all(|b| {
        matches!(
            b,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'+' | b'/'
                | b'-' | b'_'
                | b'='
        )
    });
    if !alphabet_ok {
        return Err(OAuthError::InvalidResponse);
    }

    // Prefer the tightest match first: unpadded URL-safe is what the
    // platform actually emits when the code travels as a query parameter,
    // so it succeeds without padding repair.
    if let Ok(bytes) = URL_SAFE_NO_PAD.decode(encoded) {
        return Ok(bytes);
    }
    if let Ok(bytes) = STANDARD_NO_PAD.decode(encoded) {
        return Ok(bytes);
    }
    B64.decode(encoded).map_err(|_| OAuthError::InvalidResponse)
}

/// If the user pastes a full callback URL instead of the bare base64, pluck
/// the `u` parameter from its query string so the manual channel accepts
/// either shape. Returns `None` when no `u=` is present (the caller treats
/// the trimmed raw string as the candidate in that case).
fn extract_code_param(raw: &str) -> Option<String> {
    let query_start = raw.find('?')?;
    for (key, value) in url::form_urlencoded::parse(&raw.as_bytes()[query_start + 1..]) {
        if key == "u" {
            let value = value.into_owned();
            if value.len() <= MAX_MANUAL_CODE_BYTES {
                return Some(value);
            }
            return None;
        }
    }
    None
}

/// Decrypt and authenticate the `u` callback parameter against our own
/// ephemeral secret. Every failure — short buffer, low-order shared secret,
/// GCM tag mismatch, malformed JSON, missing fields — collapses to
/// `OAuthError::InvalidResponse` so no secret-derived detail leaks.
///
/// Accepts the standard padded base64 the platform emits, plus the
/// URL-safe no-pad form a screenshot paste or copy-to-clipboard may strip
/// (underscores/hyphens, no padding) and a plain standard no-pad form.
/// All three are equally authenticated; the key is fixed and the GCM tag
/// is verified identically.
fn decrypt_callback(
    secret: &StaticSecret,
    encrypted_b64: &str,
) -> Result<CallbackPayload, OAuthError> {
    let raw = decode_b64_envelope(encrypted_b64)?;
    if raw.len() < MIN_PAYLOAD_BYTES || raw.len() > MAX_HEADER_BYTES {
        return Err(OAuthError::InvalidResponse);
    }
    // Current platform authorizer serializes public key (32), nonce (12), then
    // ciphertext+tag. Older 9router callbacks used nonce first. Authenticate
    // each layout with GCM; never accept unauthenticated/decrypted garbage.
    let plaintext = decrypt_envelope(secret, &raw[..32], &raw[32..44], &raw[44..])
        .or_else(|_| decrypt_envelope(secret, &raw[12..44], &raw[..12], &raw[44..]))?;

    let parsed: serde_json::Value =
        serde_json::from_slice(&plaintext).map_err(|_| OAuthError::InvalidResponse)?;
    let uid = parsed
        .get("uid")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| (0x20..=0x7e).contains(&b)))
        .ok_or(OAuthError::InvalidResponse)?;
    let sk = parsed
        .get("sk")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| (0x20..=0x7e).contains(&b)))
        .ok_or(OAuthError::InvalidResponse)?;
    Ok(CallbackPayload {
        uid: uid.to_owned(),
        sk: sk.to_owned(),
    })
}

fn decrypt_envelope(
    secret: &StaticSecret,
    ephemeral_raw: &[u8],
    nonce: &[u8],
    ciphertext_and_tag: &[u8],
) -> Result<Vec<u8>, OAuthError> {
    let ephemeral: [u8; 32] = ephemeral_raw
        .try_into()
        .map_err(|_| OAuthError::InvalidResponse)?;
    let ephemeral_pub = PublicKey::from(ephemeral);
    let shared = secret.diffie_hellman(&ephemeral_pub);
    // Reject low-order (non-contributory) server keys: X25519 with a
    // low-order input yields an all-zero shared secret.
    if shared.as_bytes() == &[0u8; 32] {
        return Err(OAuthError::InvalidResponse);
    }
    let derived_key = Sha256::digest(shared.as_bytes());

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived_key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext_and_tag)
        .map_err(|_| OAuthError::InvalidResponse)
}

// ---------------------------------------------------------------------------
// Loopback HTTP plumbing (bounded, mirrors flows.rs)
// ---------------------------------------------------------------------------

enum CallbackOutcome {
    Payload(String),
    Denied,
    /// A `u` parameter was present but structurally unusable.
    Malformed,
    /// Wrong shape entirely: no token material is touched.
    Ignored,
}

async fn read_callback(stream: &mut TcpStream, expected_path: &str) -> CallbackOutcome {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if head.len() >= MAX_HEADER_BYTES {
            return CallbackOutcome::Ignored;
        }
        let read = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return CallbackOutcome::Ignored,
            Ok(n) => n,
        };
        head.extend_from_slice(&chunk[..read]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.split("\r\n");
    let request_line = match lines.next() {
        Some(line) => line,
        None => return CallbackOutcome::Ignored,
    };
    let mut parts = request_line.split_whitespace();
    let (method, target) = match (parts.next(), parts.next()) {
        (Some(m), Some(t)) => (m, t),
        _ => return CallbackOutcome::Ignored,
    };
    if method != "GET" {
        return CallbackOutcome::Ignored;
    }
    // The path must match exactly. Requests to other loopback paths are
    // silently ignored — they could be local noise (favicon, debug probes)
    // or a callback forgery that wouldn't reach the legitimate `u` payload.
    let (path, query) = match target.find('?') {
        Some(idx) => (&target[..idx], &target[idx + 1..]),
        None => (target, ""),
    };
    if path != expected_path {
        return CallbackOutcome::Ignored;
    }
    let mut payload: Option<String> = None;
    let mut denied = false;
    // Duplicate `u` parameters are ignored: a single authoritative payload
    // is required so we can't be tricked into combining attacker-supplied
    // fragments with a platform-issued nonce/ephemeral.
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "u" => {
                if payload.is_some() {
                    return CallbackOutcome::Malformed;
                }
                let value = value.into_owned();
                // Bound the per-request payload before any further work.
                if value.len() > MAX_HEADER_BYTES {
                    return CallbackOutcome::Malformed;
                }
                payload = Some(value);
            }
            "error" => denied = true,
            _ => {}
        }
    }
    match payload {
        Some(p) => CallbackOutcome::Payload(p),
        None if denied => CallbackOutcome::Denied,
        None => CallbackOutcome::Ignored,
    }
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = timeout_at(Instant::now() + RESPONSE_DEADLINE, async {
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    })
    .await;
}

fn random_hex_16() -> Result<String, OAuthError> {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .map_err(|_| OAuthError::InvalidResponse)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Stable per-installation key label (port of getKeyName): the platform
/// treats `key_name` as a label only, so re-auth reuses the same name.
fn key_name() -> String {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_owned());
    let digest = Sha256::digest(format!("cuotai-{host}").as_bytes());
    let short: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("cuotai-xmd-{short}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::Aead;
    use tokio::io::AsyncWriteExt;

    fn test_config() -> OAuthConfig {
        OAuthConfig {
            client_id: String::new(),
            tenant: None,
            client_secret_env: None,
            credential_id: "xiaomi-mimo".to_owned(),
            options: BTreeMap::from([("region".to_owned(), "sgp".to_owned())]),
        }
    }

    /// Platform-side encryption: build the `u` payload for a client whose raw
    /// public key is `client_pub_raw`, carrying `json` as the plaintext.
    fn encrypt_payload(client_pub_raw: &[u8; 32], json: &str) -> String {
        encrypt_payload_with(client_pub_raw, json, None)
    }

    fn encrypt_payload_with(
        client_pub_raw: &[u8; 32],
        json: &str,
        force_ephemeral: Option<[u8; 32]>,
    ) -> String {
        let server_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let server_pub_raw =
            force_ephemeral.unwrap_or_else(|| *PublicKey::from(&server_secret).as_bytes());
        let shared = match force_ephemeral {
            // Low-order case: the ECDH output is defined to be all zero.
            Some(_) => [0u8; 32],
            None => *server_secret
                .diffie_hellman(&PublicKey::from(*client_pub_raw))
                .as_bytes(),
        };
        let key = Sha256::digest(shared);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        use rand::Rng;
        let iv: [u8; GCM_NONCE_BYTES] = rand::rngs::OsRng.gen();
        let ciphertext_and_tag = cipher
            .encrypt(Nonce::from_slice(&iv), json.as_bytes())
            .expect("test encryption");
        let mut raw = Vec::new();
        raw.extend_from_slice(&iv);
        raw.extend_from_slice(&server_pub_raw);
        raw.extend_from_slice(&ciphertext_and_tag);
        B64.encode(raw)
    }

    fn fresh_secret() -> (StaticSecret, [u8; 32]) {
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let raw = *PublicKey::from(&secret).as_bytes();
        (secret, raw)
    }

    #[test]
    fn decrypt_roundtrip() {
        let (secret, pub_raw) = fresh_secret();
        let encrypted = encrypt_payload(
            &pub_raw,
            r#"{"uid":"u-1","sk":"sk-test","url":"https://evil.example"}"#,
        );
        let payload = decrypt_callback(&secret, &encrypted).expect("valid payload");
        assert_eq!(payload.uid, "u-1");
        assert_eq!(payload.sk, "sk-test");
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let (secret, pub_raw) = fresh_secret();
        let encrypted = encrypt_payload(&pub_raw, r#"{"uid":"u-1","sk":"sk-test"}"#);
        let mut raw = B64.decode(&encrypted).unwrap();
        // Flip a bit inside the ciphertext body (after nonce + ephemeral pub).
        raw[MIN_PAYLOAD_BYTES - 1] ^= 0x01;
        let tampered = B64.encode(raw);
        assert!(matches!(
            decrypt_callback(&secret, &tampered),
            Err(OAuthError::InvalidResponse)
        ));
    }

    #[test]
    fn decrypt_rejects_wrong_key() {
        let (_right, pub_raw) = fresh_secret();
        let (wrong_secret, _) = fresh_secret();
        let encrypted = encrypt_payload(&pub_raw, r#"{"uid":"u-1","sk":"sk-test"}"#);
        assert!(matches!(
            decrypt_callback(&wrong_secret, &encrypted),
            Err(OAuthError::InvalidResponse)
        ));
    }

    #[test]
    fn decrypt_rejects_low_order_ephemeral_key() {
        let (secret, pub_raw) = fresh_secret();
        // All-zero ephemeral public key: classic low-order point.
        let encrypted =
            encrypt_payload_with(&pub_raw, r#"{"uid":"u-1","sk":"sk-test"}"#, Some([0u8; 32]));
        assert!(matches!(
            decrypt_callback(&secret, &encrypted),
            Err(OAuthError::InvalidResponse)
        ));
    }

    #[test]
    fn decrypt_rejects_short_or_missing_fields() {
        let (secret, pub_raw) = fresh_secret();
        assert!(matches!(
            decrypt_callback(&secret, &B64.encode([0u8; 10])),
            Err(OAuthError::InvalidResponse)
        ));
        assert!(matches!(
            decrypt_callback(&secret, "!!!not-base64!!!"),
            Err(OAuthError::InvalidResponse)
        ));
        // Well-formed envelope, but the plaintext lacks the required `sk`.
        let missing_sk = encrypt_payload(&pub_raw, r#"{"uid":"u-1"}"#);
        assert!(matches!(
            decrypt_callback(&secret, &missing_sk),
            Err(OAuthError::InvalidResponse)
        ));
    }

    #[tokio::test]
    async fn authorize_url_uses_fixed_platform_origin() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let url = &pending.prompt().url;
        assert!(url.starts_with("https://platform.xiaomimimo.com/authorize?"));
        assert!(url.contains("kn=mimocode"));
        assert!(url.contains("key_name=cuotai-xmd-"));
        assert!(url.contains("pk="));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"));
    }

    #[tokio::test]
    async fn loopback_callback_yields_native_key_token() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let addr = pending.listener.local_addr().unwrap();
        let callback_path = pending.callback_path.clone();
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let encrypted = encrypt_payload(
            &client_pub,
            r#"{"uid":"uid-42","sk":"sk-permanent","url":"https://attacker.invalid"}"#,
        );

        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let request = format!(
                "GET {callback_path}?u={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                url::form_urlencoded::byte_serialize(encrypted.as_bytes()).collect::<String>()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });

        let token = pending.finish().await.expect("finish");
        assert_eq!(token.access_token, "sk-permanent");
        assert_eq!(token.refresh_token, None);
        assert_eq!(token.expires_at, None);
        assert_eq!(
            token.metadata.get("user_id").map(String::as_str),
            Some("uid-42")
        );
        assert_eq!(
            token.metadata.get("auth_method").map(String::as_str),
            Some("native_key")
        );
        assert_eq!(
            token.metadata.get("region").map(String::as_str),
            Some("sgp")
        );

        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(!response.contains("sk-permanent"));
    }

    #[tokio::test]
    async fn loopback_tampered_callback_is_rejected() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let addr = pending.listener.local_addr().unwrap();
        let callback_path = pending.callback_path.clone();
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let mut raw = B64
            .decode(encrypt_payload(&client_pub, r#"{"uid":"u","sk":"sk"}"#))
            .unwrap();
        raw[MIN_PAYLOAD_BYTES - 1] ^= 0x40;
        let tampered = B64.encode(raw);

        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let request = format!(
                "GET {callback_path}?u={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                url::form_urlencoded::byte_serialize(tampered.as_bytes()).collect::<String>()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });

        let result = pending.finish().await;
        assert!(matches!(result, Err(OAuthError::InvalidResponse)));
        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    }

    #[tokio::test]
    async fn loopback_wrong_path_is_ignored_and_real_callback_succeeds() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let addr = pending.listener.local_addr().unwrap();
        let real_path = pending.callback_path.clone();
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let encrypted = encrypt_payload(&client_pub, r#"{"uid":"uid-99","sk":"sk-real"}"#);
        let wrong_payload = encrypted.clone();
        let finish = tokio::spawn(pending.finish());

        let wrong_path_client = tokio::spawn(async move {
            // Bogus path with the same `u` payload: must be ignored, not
            // accepted (otherwise a local attacker could ship a forged u).
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let request = format!(
                "GET /attacker?u={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                url::form_urlencoded::byte_serialize(wrong_payload.as_bytes()).collect::<String>()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });

        let wrong_response = wrong_path_client.await.unwrap();
        assert!(wrong_response.starts_with("HTTP/1.1 404 Not Found"));

        let real_client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let request = format!(
                "GET {real_path}?u={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                url::form_urlencoded::byte_serialize(encrypted.as_bytes()).collect::<String>()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });

        let token = finish.await.unwrap().expect("finish");
        assert_eq!(token.access_token, "sk-real");
        assert_eq!(
            token.metadata.get("user_id").map(String::as_str),
            Some("uid-99")
        );
        let response = real_client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }

    #[tokio::test]
    async fn refresh_returns_permanent_key_unchanged() {
        let http = reqwest::Client::new();
        let token = OAuthToken {
            access_token: "sk-permanent".to_owned(),
            refresh_token: None,
            expires_at: None,
            metadata: BTreeMap::from([
                ("user_id".to_owned(), "uid-42".to_owned()),
                ("auth_method".to_owned(), "native_key".to_owned()),
            ]),
        };
        let refreshed = refresh(&http, Provider::XiaomiMimo, &test_config(), &token)
            .await
            .expect("refresh");
        assert_eq!(refreshed.access_token, "sk-permanent");
        assert_eq!(refreshed.expires_at, None);
    }

    // -----------------------------------------------------------------
    // Manual-code acceptance: covers the screenshot paste / typed-code
    // path added for finish_with_codes. No real network, no real browser.
    // -----------------------------------------------------------------

    /// Build a code string using a configurable encoder so we can
    /// exercise every flavour a screenshot paste may produce.
    fn build_code(client_pub_raw: &[u8; 32], json: &str, flavor: CodeFlavor) -> String {
        let standard = encrypt_payload(client_pub_raw, json);
        match flavor {
            CodeFlavor::Standard => standard,
            CodeFlavor::UrlSafeNoPad => {
                URL_SAFE_NO_PAD.encode(B64.decode(&standard).expect("decode standard"))
            }
        }
    }

    enum CodeFlavor {
        Standard,
        UrlSafeNoPad,
    }

    #[tokio::test]
    async fn manual_code_url_safe_no_pad_succeeds() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let code = build_code(
            &client_pub,
            r#"{"uid":"uid-url","sk":"sk-url"}"#,
            CodeFlavor::UrlSafeNoPad,
        );

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        let (etx, mut erx) = tokio::sync::mpsc::channel::<String>(4);
        tx.send(code).await.expect("send");
        drop(tx);

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let token = finish.await.unwrap().expect("finish_with_codes");
        assert_eq!(token.access_token, "sk-url");
        assert_eq!(
            token.metadata.get("user_id").map(String::as_str),
            Some("uid-url")
        );
        assert_eq!(
            token.metadata.get("auth_method").map(String::as_str),
            Some("native_key")
        );
        // No errors emitted on success.
        assert!(erx.try_recv().is_err());
    }

    #[tokio::test]
    async fn manual_code_with_wrapped_whitespace_and_newlines_succeeds() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let code = build_code(
            &client_pub,
            r#"{"uid":"uid-ws","sk":"sk-ws"}"#,
            CodeFlavor::Standard,
        );

        // Surround with whitespace, embedded newlines and tabs to mimic a
        // screenshot OCR capture or terminal paste.
        let wrapped = format!("\n  \t{code}\n   \r\n");
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        let (etx, _erx) = tokio::sync::mpsc::channel::<String>(4);
        tx.send(wrapped).await.expect("send");
        drop(tx);

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let token = finish.await.unwrap().expect("finish_with_codes");
        assert_eq!(token.access_token, "sk-ws");
    }

    #[tokio::test]
    async fn manual_code_wrong_session_then_valid_succeeds() {
        let http = reqwest::Client::new();
        // Two independent pending logins: a "previous" session whose code
        // must NOT be honoured, and the live one whose code must succeed.
        let previous = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("previous");
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("live");

        let previous_pub = *PublicKey::from(&previous.secret).as_bytes();
        let live_pub = *PublicKey::from(&pending.secret).as_bytes();
        let stale_code = build_code(
            &previous_pub,
            r#"{"uid":"stale","sk":"sk-stale"}"#,
            CodeFlavor::Standard,
        );
        let valid_code = build_code(
            &live_pub,
            r#"{"uid":"uid-ok","sk":"sk-ok"}"#,
            CodeFlavor::Standard,
        );
        // Drain the previous listener so its socket does not linger.
        drop(previous);

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        let (etx, mut erx) = tokio::sync::mpsc::channel::<String>(8);
        tx.send(stale_code).await.expect("send stale");
        tx.send(valid_code).await.expect("send valid");
        drop(tx);

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let token = finish.await.unwrap().expect("finish_with_codes");
        assert_eq!(token.access_token, "sk-ok");
        assert_eq!(
            token.metadata.get("user_id").map(String::as_str),
            Some("uid-ok")
        );
        // The stale code must surface exactly one error message in the
        // fixed Portuguese wording, no more.
        let msg = erx.recv().await.expect("error message");
        assert_eq!(
            msg,
            "Código inválido ou de outra sessão; copie o código desta tentativa."
        );
        assert!(erx.recv().await.is_none());
    }

    #[tokio::test]
    async fn manual_code_cancelled_receiver_falls_back_to_loopback() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let addr = pending.listener.local_addr().unwrap();
        let callback_path = pending.callback_path.clone();
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let encrypted = encrypt_payload(&client_pub, r#"{"uid":"uid-cb","sk":"sk-cb"}"#);

        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let request = format!(
                "GET {callback_path}?u={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                url::form_urlencoded::byte_serialize(encrypted.as_bytes()).collect::<String>()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });

        // Drop the codes receiver before sending anything: finish_with_codes
        // must fall back to the loopback callback without busy-looping.
        let (_tx, rx) = tokio::sync::mpsc::channel::<String>(1);
        drop(_tx);
        let (etx, _erx) = tokio::sync::mpsc::channel::<String>(1);

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let token = finish.await.unwrap().expect("finish_with_codes");
        assert_eq!(token.access_token, "sk-cb");
        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }

    #[tokio::test]
    async fn manual_code_does_not_extend_original_deadline() {
        // Build a PendingLogin with a near-future deadline so the
        // deadline-honouring contract is observable in a few hundred ms.
        // If finish_with_codes reset the deadline on any code attempt,
        // this test would either hang for CALLBACK_TOTAL_WAIT (300s) or
        // return a different error class. We assert Expired is returned
        // after a short window, proving the deadline is honored.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let callback_path = "/mimo-callback/testdeadline".to_owned();
        let redirect_uri = format!("http://127.0.0.1:{port}{callback_path}");
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let pending = PendingLogin {
            prompt: LoginPrompt {
                url: format!("{PLATFORM_URL}{AUTHORIZE_PATH}?redirect_uri={redirect_uri}"),
                user_code: None,
            },
            listener,
            secret,
            callback_path,
            region: None,
            deadline: Instant::now() + std::time::Duration::from_millis(150),
        };

        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let bad = build_code(&client_pub, r#"{"uid":"x","sk":"x"}"#, CodeFlavor::Standard);
        // Tamper so the envelope fails to authenticate: flip a bit in
        // the ciphertext.
        let mut raw = B64.decode(&bad).unwrap();
        raw[MIN_PAYLOAD_BYTES - 1] ^= 0x10;
        let tampered = B64.encode(raw);

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        let (etx, mut erx) = tokio::sync::mpsc::channel::<String>(8);
        for _ in 0..3 {
            tx.send(tampered.clone()).await.expect("send tampered");
        }
        drop(tx);

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let started = std::time::Instant::now();
        let result = finish.await.unwrap();
        let elapsed = started.elapsed();
        // Deadline was ~150ms; full timeout window is well under 5s.
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "finish_with_codes took {elapsed:?} (deadline not honored)"
        );
        assert!(matches!(result, Err(OAuthError::Expired)));
        // At least one rejection message is surfaced per bad code; we
        // just check the wording matches the contract.
        let msg = erx.recv().await.expect("error message");
        assert_eq!(
            msg,
            "Código inválido ou de outra sessão; copie o código desta tentativa."
        );
    }

    #[tokio::test]
    async fn manual_code_callback_path_still_works_after_reject() {
        // After a manual code is rejected, the loopback callback must
        // still work using the SAME pending key.
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let addr = pending.listener.local_addr().unwrap();
        let callback_path = pending.callback_path.clone();
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let encrypted = encrypt_payload(&client_pub, r#"{"uid":"uid-after","sk":"sk-after"}"#);

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        let (etx, _erx) = tokio::sync::mpsc::channel::<String>(4);
        // Bad code first.
        tx.send("not-base64-at-all".to_owned())
            .await
            .expect("send bad");
        drop(tx);

        let client = tokio::spawn(async move {
            // Tiny delay to let the bad code be processed first.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let request = format!(
                "GET {callback_path}?u={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                url::form_urlencoded::byte_serialize(encrypted.as_bytes()).collect::<String>()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let token = finish.await.unwrap().expect("finish_with_codes");
        assert_eq!(token.access_token, "sk-after");
        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }

    #[tokio::test]
    async fn manual_code_accepts_pasted_full_callback_url() {
        let http = reqwest::Client::new();
        let pending = begin(&http, Provider::XiaomiMimo, &test_config())
            .await
            .expect("begin");
        let client_pub = *PublicKey::from(&pending.secret).as_bytes();
        let code = build_code(
            &client_pub,
            r#"{"uid":"uid-url","sk":"sk-url"}"#,
            CodeFlavor::Standard,
        );
        // The user pasted the full callback URL instead of the bare code.
        let pasted = format!(
            "http://127.0.0.1:1234/mimo-callback/deadbeef?u={}",
            url::form_urlencoded::byte_serialize(code.as_bytes()).collect::<String>()
        );

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(1);
        let (etx, _erx) = tokio::sync::mpsc::channel::<String>(1);
        tx.send(pasted).await.expect("send");
        drop(tx);

        let finish = tokio::spawn(pending.finish_with_codes(rx, etx));
        let token = finish.await.unwrap().expect("finish_with_codes");
        assert_eq!(token.access_token, "sk-url");
    }
    #[tokio::test]
    async fn manual_current_mimo_public_key_first_251_chars() {
        let pending = begin(
            &reqwest::Client::new(),
            Provider::XiaomiMimo,
            &test_config(),
        )
        .await
        .unwrap();
        let public = *PublicKey::from(&pending.secret).as_bytes();
        let payload = serde_json::json!({"uid":"user-123", "sk":"s".repeat(102)}).to_string();
        assert_eq!(payload.len(), 128);
        let legacy = B64.decode(encrypt_payload(&public, &payload)).unwrap();
        let mut current = Vec::new();
        current.extend_from_slice(&legacy[12..44]);
        current.extend_from_slice(&legacy[..12]);
        current.extend_from_slice(&legacy[44..]);
        let code = URL_SAFE_NO_PAD.encode(current);
        assert_eq!(code.len(), 251);
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let (errors, _messages) = tokio::sync::mpsc::channel(1);
        tx.send(code).await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            pending.finish_with_codes(rx, errors),
        )
        .await;
        assert!(
            result.is_ok(),
            "Current MiMo public-key-first envelope was not accepted"
        );
        let token = result.unwrap().unwrap();
        assert_eq!(token.access_token, "s".repeat(102));
    }
}
