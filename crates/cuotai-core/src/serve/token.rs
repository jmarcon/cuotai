//! RF3.2: the optional access token of the local server. Cuotai generates
//! it and keeps it in the OS vault ([`SystemTokenVault`]), never in the
//! TOML; a front end may also read one from a file the user controls
//! ([`Token::parse`]). It has no `Display`/`Serialize` and its `Debug` is
//! redacted, so it cannot slip into a log or an answer.

use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use super::ServeError;

/// Marks a generated token for secret scanners.
const PREFIX: &str = "cuotai_";
const MIN_LEN: usize = 16;
const MAX_LEN: usize = 512;

#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// `cuotai_` and 32 random bytes from the OS, URL-safe base64.
    pub fn generate() -> Result<Self, ServeError> {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| ServeError::Random)?;
        Ok(Self(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))))
    }

    /// A token given by the user (a token file, trimmed): 16 to 512
    /// characters of the RFC 6750 `b64token` alphabet.
    pub fn parse(text: &str) -> Result<Self, ServeError> {
        let text = text.trim();
        let body = text.trim_end_matches('=');
        let valid = (MIN_LEN..=MAX_LEN).contains(&text.len())
            && !body.is_empty()
            && body
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~+/".contains(&b));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ServeError::InvalidToken)
        }
    }

    /// The secret itself, for the user to copy into a client.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Constant-time comparison with what a request presented.
    pub(super) fn matches(&self, presented: &[u8]) -> bool {
        let expected = self.0.as_bytes();
        expected.len() == presented.len()
            && expected
                .iter()
                .zip(presented)
                .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([redacted])")
    }
}

/// Where the token is kept. Methods may block on OS IPC: async callers run
/// them through `tokio::task::spawn_blocking`.
pub trait TokenVault: Send + Sync {
    fn load(&self) -> Result<Option<Token>, ServeError>;
    /// Replace the stored token.
    fn save(&self, token: &Token) -> Result<(), ServeError>;
    /// Revoke: remove the stored token; removing a missing one succeeds.
    fn delete(&self) -> Result<(), ServeError>;
}

/// The stored token, or a new one generated and stored when there is none.
pub fn ensure_token(vault: &dyn TokenVault) -> Result<Token, ServeError> {
    if let Some(token) = vault.load()? {
        return Ok(token);
    }
    let token = Token::generate()?;
    vault.save(&token)?;
    Ok(token)
}

/// The token in the platform's secure store (macOS Keychain, Windows
/// Credential Manager, Linux Secret Service), apart from OAuth tokens.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTokenVault;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl SystemTokenVault {
    const SERVICE: &'static str = "cuotai.serve";
    const ACCOUNT: &'static str = "local-server-token";

    /// Backend error text is never surfaced.
    fn entry() -> Result<keyring::v1::Entry, ServeError> {
        if keyring::v1::Entry::store_status().is_err() {
            return Err(ServeError::Vault);
        }
        keyring::v1::Entry::new(Self::SERVICE, Self::ACCOUNT).map_err(|_| ServeError::Vault)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl TokenVault for SystemTokenVault {
    fn load(&self) -> Result<Option<Token>, ServeError> {
        match Self::entry()?.get_password() {
            // A stored value that is not a token is a vault problem, never
            // something to echo.
            Ok(raw) => Token::parse(&raw).map(Some).map_err(|_| ServeError::Vault),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(_) => Err(ServeError::Vault),
        }
    }

    fn save(&self, token: &Token) -> Result<(), ServeError> {
        Self::entry()?
            .set_password(token.expose())
            .map_err(|_| ServeError::Vault)
    }

    fn delete(&self) -> Result<(), ServeError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
            Err(_) => Err(ServeError::Vault),
        }
    }
}
