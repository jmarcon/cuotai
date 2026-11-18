//! PRD 0050 `SEC-DEV-VAULT`: the credential store this process started
//! with, read once from `CUOTAI_DEV_CREDENTIALS_SQLITE` (the development
//! store of `run.sh gui`, refused by release builds in the core). Every
//! part of the desktop that reaches a vault (the OAuth session, erase-all
//! and the local-server token) takes it from [`choice`], so a development
//! process never falls through to the system vault.

use std::sync::OnceLock;

use cuotai_core::oauth::StoreChoice;

/// The store of this process (the variable is read once).
pub fn choice() -> &'static StoreChoice {
    static CHOICE: OnceLock<StoreChoice> = OnceLock::new();
    CHOICE.get_or_init(|| {
        StoreChoice::from_flag(
            std::env::var_os("CUOTAI_DEV_CREDENTIALS_SQLITE").map(std::path::PathBuf::from),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The source of `file`, without its `#[cfg(test)] mod tests` part.
    fn non_test(source: &str) -> &str {
        source
            .find("#[cfg(test)]\nmod tests")
            .map_or(source, |end| &source[..end])
    }

    /// PRD 0050 `SEC-DEV-VAULT`: the system vaults are reached only through
    /// the store choice: the command, backend and server code never name
    /// them, and this module holds the only reader of the development
    /// variable.
    // security-invariant: SEC-DEV-VAULT
    #[test]
    fn the_os_vault_is_reached_only_through_the_store_choice() {
        let system_vault = concat!("System", "Vault");
        let system_token_vault = concat!("System", "TokenVault");
        for (name, source) in [
            ("lib.rs", include_str!("lib.rs")),
            ("backend.rs", include_str!("backend.rs")),
            ("local_server.rs", include_str!("local_server.rs")),
        ] {
            let code = non_test(source);
            assert!(!code.contains(system_vault), "{name} names the OS vault");
            assert!(
                !code.contains(system_token_vault),
                "{name} names the OS token vault"
            );
        }
        let variable = concat!("CUOTAI_DEV_", "CREDENTIALS_SQLITE");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        let mut readers = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && std::fs::read_to_string(&path)
                        .unwrap()
                        .contains(&format!("\"{variable}\""))
                {
                    readers.push(path.file_name().unwrap().to_string_lossy().into_owned());
                }
            }
        }
        assert_eq!(readers, vec!["credential_store.rs".to_owned()]);
    }
}
