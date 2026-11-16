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

/// Re-validation S-03: what a `GET /health` with `Cuotai-Challenge` binds the
/// proof to. Versioned so a later scheme can never be confused with this one.
const PROOF_LABEL: &[u8] = b"cuotai-serve-proof/1";

/// The bytes of a `Cuotai-Challenge` nonce: 32 random bytes as 64
/// lowercase hex characters.
pub const CHALLENGE_LEN: usize = 64;

/// Whether `nonce` is a well-formed challenge (64 lowercase hex characters).
pub fn challenge_is_valid(nonce: &[u8]) -> bool {
    nonce.len() == CHALLENGE_LEN && nonce.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A fresh challenge nonce from the OS random generator.
pub fn new_challenge() -> Result<String, ServeError> {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| ServeError::Random)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl Token {
    /// HMAC-SHA256 under this token over the label, `nonce` and the server's
    /// bound `port`, each separated by a zero byte.
    fn proof_mac(&self, nonce: &str, port: u16) -> hmac::Hmac<sha2::Sha256> {
        use hmac::Mac;
        let mut mac = <hmac::Hmac<sha2::Sha256> as Mac>::new_from_slice(self.0.as_bytes())
            .expect("HMAC takes a key of any length");
        mac.update(PROOF_LABEL);
        mac.update(&[0]);
        mac.update(nonce.as_bytes());
        mac.update(&[0]);
        mac.update(port.to_string().as_bytes());
        mac
    }

    /// The proof a server answers a challenge with: 64 lowercase hex characters.
    pub fn proof(&self, nonce: &str, port: u16) -> String {
        use hmac::Mac;
        let digest = self.proof_mac(nonce, port).finalize().into_bytes();
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Whether `proof` (hex) is the proof for `nonce` and `port`, compared in
    /// constant time.
    pub(super) fn proof_matches(&self, nonce: &str, port: u16, proof: &str) -> bool {
        use hmac::Mac;
        let Some(bytes) = decode_hex(proof) else {
            return false;
        };
        self.proof_mac(nonce, port).verify_slice(&bytes).is_ok()
    }
}

/// Lowercase or uppercase hex to bytes; `None` for anything else.
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
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
    crate::data_dir::root().map(|dir| dir.join("serve").join("token"))
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

/// Whether users other than the owner may read (or write) the file at
/// `path`: Unix group/other bits, or a Windows DACL granting a broad
/// principal access; `false` for a file that cannot be found (reading it
/// fails on its own).
pub fn token_file_exposed(path: &std::path::Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| crate::private_fs::open_to_others(path, &meta))
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
                if crate::private_fs::open_to_others(path, &meta) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                            .map_err(|_| ServeError::TokenFile)?;
                    }
                    crate::private_fs::make_private(path, false)
                        .map_err(|_| ServeError::TokenFile)?;
                    #[cfg(windows)]
                    if crate::private_fs::open_to_others(path, &meta) {
                        return Err(ServeError::TokenFile);
                    }
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
                // Windows: the private DACL before the secret is written
                // (the folder's inherited one already is, when it was made
                // by `ensure_dir`); best effort for volumes without ACLs.
                let _ = crate::private_fs::make_private(path, false);
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
