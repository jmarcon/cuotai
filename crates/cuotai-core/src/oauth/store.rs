//! Secure storage seam for OAuth tokens.
//!
//! `TokenStore` abstracts where refresh/access tokens live so the same core
//! can back the TUI today and the native macOS app later. The provided
//! `SystemTokenStore` writes to the platform-native secure store via the
//! `keyring` crate (macOS Keychain, Windows Credential Manager, Linux Secret
//! Service). There is deliberately **no** automatic plaintext fallback.
//! An explicit SQLite development store is available only with debug assertions;
//! release builds reject it before filesystem access.
//!
//! All methods are synchronous and may block on OS IPC; async callers must
//! run them via `tokio::task::spawn_blocking` (the manager does this).
//!
//! Security notes:
//! - Tokens are serialized to JSON internally; that payload and any parse
//!   error are never logged or surfaced. Store errors use fixed strings.
//! - `OAuthToken` intentionally has no `Debug` impl.
//! - On macOS, the first save/access may trigger a Keychain access prompt.

use super::{OAuthError, OAuthToken};

#[cfg(debug_assertions)]
mod sqlite_dev;

/// Explicit plaintext development store; unavailable in release builds.
pub(crate) fn development_store(
    path: &std::path::Path,
) -> Result<std::sync::Arc<dyn TokenStore>, OAuthError> {
    #[cfg(debug_assertions)]
    {
        Ok(std::sync::Arc::new(sqlite_dev::SqliteDevStore::open(path)?))
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = path;
        Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.store.sqlite_dev_only", "SQLite de credenciais é permitido apenas em builds de desenvolvimento; release exige o cofre do sistema.",
        )))
    }
}

/// Keychain/credential-store service namespace for all Cuotai OAuth tokens.
const SERVICE_NAME: &str = "cuotai.oauth";

/// Maximum accepted length for a credential id. Ids are locally generated
/// random identifiers (UUID or hex), never user data and never a token.
const MAX_ID_LEN: usize = 128;

/// Synchronous, thread-safe storage for OAuth tokens.
///
/// Implementations must be usable from blocking threads (`Send + Sync`).
/// This seam lets the future macOS app share the same storage behavior.
pub trait TokenStore: Send + Sync {
    /// Load the token stored under `id`, or `Ok(None)` if no entry exists.
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError>;
    /// Persist `token` under `id`, replacing any existing entry.
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError>;
    /// Remove the entry for `id`. Deleting a missing entry succeeds.
    fn delete(&self, id: &str) -> Result<(), OAuthError>;
}

/// Validate that `id` is a locally generated opaque identifier:
/// nonempty, bounded, alphanumeric plus `-` and `_` only. This keeps
/// Keychain account names well-formed and prevents arbitrary user input
/// (or, worst case, token material) from becoming a key.
fn validate_id(id: &str) -> Result<&str, OAuthError> {
    let ok = !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(id)
    } else {
        Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.store.invalid_credential_id",
            "Identificador de credencial OAuth ausente ou inválido; vincule a conta novamente no assistente",
        )))
    }
}

/// Serialize a token for storage. Failure to serialize a well-formed struct
/// is an internal error, not user-recoverable.
fn encode(token: &OAuthToken) -> Result<String, OAuthError> {
    serde_json::to_string(token).map_err(|_| {
        OAuthError::Storage(crate::text::Localized::fixed(
            "oauth_error.store.token_encode_failed",
            "Falha ao codificar o token OAuth",
        ))
    })
}

/// Deserialize a stored token. The raw payload and the parser's error text
/// are intentionally discarded so no token material can leak via messages.
fn decode(raw: &str) -> Result<OAuthToken, OAuthError> {
    serde_json::from_str(raw).map_err(|_| {
        OAuthError::Storage(crate::text::Localized::fixed(
            "oauth_error.store.token_unreadable",
            "Token OAuth armazenado ilegível; entre novamente para revincular a conta",
        ))
    })
}

/// Token store backed by the platform-native secure credential store.
///
/// Compile-time platform gating: `SystemTokenStore` only exists on targets
/// where a real secure backend is compiled in. Other targets get no
/// implementation, so the failure surfaces at build time rather than as a
/// runtime plaintext fallback.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTokenStore;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl SystemTokenStore {
    pub fn new() -> Self {
        SystemTokenStore
    }

    /// Open the store entry for `id`, verifying the native credential store
    /// initialized successfully. Backend error text is never surfaced.
    fn entry(id: &str) -> Result<keyring::v1::Entry, OAuthError> {
        let id = validate_id(id)?;
        if keyring::v1::Entry::store_status().is_err() {
            return Err(storage_error());
        }
        keyring::v1::Entry::new(SERVICE_NAME, id).map_err(|_| storage_error())
    }
}

/// Fixed, platform-appropriate message (PT-BR, user-facing) for when secure
/// storage is unavailable or a backend operation fails. Contains no server
/// output, no payload, and no backend error text: backend error strings can
/// contain store-specific detail we do not control, so they are never
/// propagated.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn storage_error() -> OAuthError {
    #[cfg(target_os = "macos")]
    {
        OAuthError::Storage(crate::text::Localized::fixed(
            "oauth_error.store.macos_keychain_unavailable",
            "O Keychain do macOS está indisponível ou o acesso foi negado; desbloqueie o keychain de login e tente novamente",
        ))
    }
    #[cfg(target_os = "windows")]
    {
        OAuthError::Storage(crate::text::Localized::fixed(
            "oauth_error.store.windows_credential_manager_unavailable",
            "O Gerenciador de Credenciais do Windows está indisponível; verifique se o serviço está em execução e tente novamente",
        ))
    }
    #[cfg(target_os = "linux")]
    {
        OAuthError::Storage(crate::text::Localized::fixed(
            "oauth_error.store.linux_secret_service_unavailable",
            "Nenhum provedor Secret Service disponível; inicie o GNOME Keyring ou KWallet (com sessão D-Bus) e tente novamente",
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl TokenStore for SystemTokenStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        let entry = Self::entry(id)?;
        match entry.get_password() {
            Ok(raw) => decode(&raw).map(Some),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(_) => Err(storage_error()),
        }
    }

    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        let entry = Self::entry(id)?;
        let payload = encode(token)?;
        entry.set_password(&payload).map_err(|_| storage_error())
    }

    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        let entry = Self::entry(id)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()), // absent: idempotent success
            Err(_) => Err(storage_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_validation_accepts_uuid_and_hex_shapes() {
        assert!(validate_id("9b1d7c2e-4a3f-4e5b-8c6d-1a2b3c4d5e6f").is_ok());
        assert!(validate_id("deadBEEF0123").is_ok());
        assert!(validate_id("a").is_ok());
    }

    #[test]
    fn id_validation_rejects_empty_overlong_and_unsafe() {
        assert!(validate_id("").is_err());
        assert!(validate_id(&"x".repeat(MAX_ID_LEN + 1)).is_err());
        assert!(validate_id("has space").is_err());
        assert!(validate_id("slash/id").is_err());
        assert!(validate_id("unïcode").is_err());
        assert!(validate_id("with.dot").is_err());
    }

    #[test]
    fn decode_of_corrupt_payload_errs_without_echoing_payload() {
        let payload = "{\"access_token\":\"SECRET-DO-NOT-ECHO\"";
        let err = decode(payload).err().expect("corrupt payload must fail");
        let msg = err.to_string();
        assert!(!msg.contains("SECRET-DO-NOT-ECHO"));
        assert!(!msg.contains("access_token"));
    }
}
