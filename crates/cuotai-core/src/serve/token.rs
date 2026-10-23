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

/// INT-04: `<OS local data dir>/cuotai/serve/token`, the token
/// `cuotai --serve` creates and requires by default; `None` without a local
/// data directory.
pub fn default_token_file() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai").join("serve").join("token"))
}

/// The token in the file at `path` (trimmed; a trailing newline is fine).
/// Only a regular file is read, so a FIFO never blocks; anything that is
/// not one token is [`ServeError::TokenFile`], never echoed.
pub fn read_token_file(path: &std::path::Path) -> Result<Token, ServeError> {
    use std::io::Read;
    let meta = std::fs::metadata(path).map_err(|_| ServeError::TokenFile)?;
    if !meta.is_file() {
        return Err(ServeError::TokenFile);
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .map_err(|_| ServeError::TokenFile)?
        .take(MAX_LEN as u64 + 2)
        .read_to_string(&mut text)
        .map_err(|_| ServeError::TokenFile)?;
    Token::parse(&text)
}

/// On Unix, whether users other than the owner may read (or write) the
/// file at `path`; always `false` elsewhere and for a file that cannot be
/// inspected (reading it fails on its own).
pub fn token_file_exposed(path: &std::path::Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| crate::private_fs::open_to_others(&meta))
}

/// INT-04: the token in `path`, created on first use. A missing file gets a
/// fresh [`Token::generate`] written exclusively (`create_new`, so two
/// servers starting together agree on one token) as a `0600` file in a
/// `0700` folder on Unix. An existing file must be a regular file of the
/// current user (not a symbolic link); it is narrowed to `0600` when it was
/// wider, then read. Errors never name the path nor echo the content.
pub fn ensure_token_file(path: &std::path::Path) -> Result<Token, ServeError> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or(ServeError::TokenFile)?;
    crate::private_fs::ensure_dir(dir).map_err(|_| ServeError::TokenFile)?;
    loop {
        match std::fs::symlink_metadata(path) {
            Ok(meta) => {
                if !meta.is_file() || crate::private_fs::foreign_owner(&meta) {
                    return Err(ServeError::TokenFile);
                }
                #[cfg(unix)]
                if crate::private_fs::open_to_others(&meta) {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                        .map_err(|_| ServeError::TokenFile)?;
                }
                return read_token_file(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ServeError::TokenFile),
        }
        let token = Token::generate()?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(path) {
            Ok(mut file) => {
                let written = file
                    .write_all(format!("{}\n", token.expose()).as_bytes())
                    .and_then(|()| file.sync_all());
                if written.is_err() {
                    let _ = std::fs::remove_file(path);
                    return Err(ServeError::TokenFile);
                }
                return Ok(token);
            }
            // Another server created it meanwhile: read that one.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ServeError::TokenFile),
        }
    }
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
