//! PRD 0050 `SEC-DEV-VAULT`: the one credential-store choice of a process.
//!
//! A front end decides once whether it runs on the system vault or on the
//! plaintext development store (`--dev-credentials-sqlite`,
//! `CUOTAI_DEV_CREDENTIALS_SQLITE`), and takes everything that reaches a
//! vault from that choice: the OAuth session, the vault erase-all clears
//! and the desktop's local-server token. The development choice never
//! calls the system factory, so it can never fall through to the OS vault;
//! a release build refuses it with the fixed `sqlite_dev_only` text before
//! any filesystem access.

use std::path::PathBuf;

use super::{OAuthError, OAuthSession};
use crate::privacy::erase;
use crate::text::Localized;

/// A vault erase-all can clear, shareable across an `await`.
pub type EraseVault = Box<dyn erase::Vault + Send + Sync>;

/// Where this process keeps its credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreChoice {
    /// The OS vault (Keychain, Credential Manager, Secret Service).
    System,
    /// The plaintext SQLite development store at this path (debug builds).
    Development(PathBuf),
}

impl StoreChoice {
    /// The development store when a path was given, else the system vault.
    pub fn from_flag(path: Option<PathBuf>) -> Self {
        match path {
            Some(path) => Self::Development(path),
            None => Self::System,
        }
    }

    /// Whether this is the development store (which, for example, a backup
    /// never includes).
    pub fn is_development(&self) -> bool {
        matches!(self, Self::Development(_))
    }

    /// The OAuth session over this store.
    pub fn session(&self) -> Result<OAuthSession, OAuthError> {
        match self {
            Self::System => OAuthSession::system(),
            Self::Development(path) => OAuthSession::development_sqlite(path),
        }
    }

    /// The vault erase-all clears ([`erase::SystemVault`] for the system
    /// choice).
    pub fn erase_vault(&self) -> Result<EraseVault, Localized> {
        self.erase_vault_with(|| Box::new(erase::SystemVault))
    }

    /// [`Self::erase_vault`] with the system vault built by `system`, which
    /// only the system choice calls (tests pass one that panics).
    pub fn erase_vault_with(
        &self,
        system: impl FnOnce() -> EraseVault,
    ) -> Result<EraseVault, Localized> {
        match self {
            Self::System => Ok(system()),
            Self::Development(path) => development_erase_vault(path),
        }
    }

    /// The vault of the local-server token
    /// ([`crate::serve::SystemTokenVault`] for the system choice).
    #[cfg(all(
        feature = "serve",
        any(target_os = "macos", target_os = "windows", target_os = "linux")
    ))]
    pub fn serve_vault(
        &self,
    ) -> Result<std::sync::Arc<dyn crate::serve::TokenVault>, crate::serve::ServeError> {
        self.serve_vault_with(|| std::sync::Arc::new(crate::serve::SystemTokenVault))
    }

    /// [`Self::serve_vault`] with the system vault built by `system`, which
    /// only the system choice calls.
    #[cfg(feature = "serve")]
    pub fn serve_vault_with(
        &self,
        system: impl FnOnce() -> std::sync::Arc<dyn crate::serve::TokenVault>,
    ) -> Result<std::sync::Arc<dyn crate::serve::TokenVault>, crate::serve::ServeError> {
        match self {
            Self::System => Ok(system()),
            Self::Development(path) => development_serve_vault(path),
        }
    }
}

#[cfg(debug_assertions)]
fn development_erase_vault(path: &std::path::Path) -> Result<EraseVault, Localized> {
    erase::DevelopmentVault::open(path).map(|vault| Box::new(vault) as EraseVault)
}

#[cfg(not(debug_assertions))]
fn development_erase_vault(path: &std::path::Path) -> Result<EraseVault, Localized> {
    let _ = path;
    Err(super::store::development_only())
}

#[cfg(all(feature = "serve", debug_assertions))]
fn development_serve_vault(
    path: &std::path::Path,
) -> Result<std::sync::Arc<dyn crate::serve::TokenVault>, crate::serve::ServeError> {
    crate::serve::DevelopmentTokenVault::open(path)
        .map(|vault| std::sync::Arc::new(vault) as std::sync::Arc<dyn crate::serve::TokenVault>)
}

/// A release build has no development serve vault: the server fails
/// closed with the vault error, never falling back to the system vault.
#[cfg(all(feature = "serve", not(debug_assertions)))]
fn development_serve_vault(
    path: &std::path::Path,
) -> Result<std::sync::Arc<dyn crate::serve::TokenVault>, crate::serve::ServeError> {
    let _ = path;
    Err(crate::serve::ServeError::Vault)
}
